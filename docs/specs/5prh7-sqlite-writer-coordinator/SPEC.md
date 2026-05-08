# SQLite 单写入调度层（#5prh7）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- OctoRill 继续使用单个 SQLite 数据库承载 HTTP 请求、后台任务、repo release worker、translation worker 与 LLM 调度状态。
- SQLite WAL 允许读写并发，但仍只有一个 writer；当高并发 worker 直接争抢写事务时，`database is locked` 会外溢到用户请求并造成 500。
- 既有修复已把部分 read-then-write 事务改为 `BEGIN IMMEDIATE`，但缺少应用层写入协调，无法在业务高并发下把 SQLite 写锁竞争转换成可观测的有界排队。

## 目标 / 非目标

### Goals

- 保留高并发 worker 能力；只在进入 SQLite 写入段时协调单 writer。
- 读请求继续使用现有 `SqlitePool`，不得因为写协调退化为全局单连接。
- 高竞争写路径必须通过统一 coordinator 获取 writer permit，并记录 lane、等待时长、attempt 与写入耗时。
- SQLite busy/locked 必须作为可恢复背压处理，经过有界退避重试后再决定是否失败。
- `last_active_at` 等用户热路径 best-effort 写入不得等待后台 writer 排队，也不得把 `/api/me` 类请求打成 500。

### Non-goals

- 不迁移到 PostgreSQL。
- 不降低 `repo_release_worker_concurrency`、translation worker 或 LLM 并发作为修复方案。
- 不修改 101 线上 compose、secrets、容器或生产数据库。
- 不新增前端 UI 或视觉交付面。

## 范围（Scope）

### In scope

- 后端 runtime 内的 SQLite write coordinator。
- `job_tasks` claim/heartbeat、repo release attach/claim/finalize/heartbeat、translation request/batch/recovery/finalize、`touch_user_last_active_at` 等热写路径。
- 针对 SQLite WAL + 多连接 pool 的并发回归测试。

### Out of scope

- 外部数据库迁移。
- 改变现有 API 响应结构。
- 生产部署操作。

## 需求（Requirements）

### MUST

- 写协调层必须以应用内单 writer permit 串行化 SQLite 写入段。
- 网络请求、GitHub API、AI 调用与长耗时计算不得在 writer permit 内执行。
- busy/locked retry 必须有上限，避免无限等待。
- 关键写入 lane 必须有结构化 tracing 字段。

### SHOULD

- 事务仍应尽量短小，批量写优先在单次 permit 内完成。
- 对 best-effort 写入失败路径记录 warning/debug，但不破坏用户主要读流程。

### COULD

- 后续可按 lane 增加指标导出或管理端展示。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 后台 worker 可以继续高并发执行网络/AI 阶段；进入 SQLite 写入时通过 coordinator 排队。
- HTTP 请求读取数据时不需要 writer permit；更新用户活跃时间等 best-effort 写入使用非阻塞 writer 尝试，拿不到 permit 时直接跳过。
- 如果 SQLite 返回 busy/locked，coordinator 使用短退避重试，并在耗尽后返回原始错误上下文。

### Edge cases / errors

- coordinator 自身 permit 不可用时，写入返回内部错误并带上下文。
- `last_active_at` best-effort 写入拿不到 writer permit 时只记录 debug 并跳过；若已取得 permit 但 SQLite busy/locked，则记录 warning，请求继续返回。
- 外部进程持有 SQLite writer lock 时，coordinator retry 后仍可失败，但失败必须可观测。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `SqliteWriteCoordinator` | Rust runtime API | internal | New | None | backend | backend runtime | 单 writer permit、busy retry、tracing |

### 契约文档（按 Kind 拆分）

- None

## 验收标准（Acceptance Criteria）

- Given SQLite WAL + 多连接 pool + 多个后台写任务并发运行
  When 写任务同时 claim/heartbeat/finalize
  Then 写入通过 coordinator 排队，不因应用内 writer 竞争产生 `database is locked`。

- Given `/api/me` 读取用户状态时需要更新 `last_active_at`
  When SQLite 写入暂时 busy
  Then 请求不因为 best-effort 活跃时间写入失败返回 500。

- Given writer lane 出现等待或 retry
  When 查看 tracing 日志
  Then 能看到 lane、wait、attempt、elapsed 或 retry_after 字段。

## 验收清单（Acceptance checklist）

- [ ] 核心路径的长期行为已被明确描述。
- [ ] 关键边界/错误场景已被覆盖。
- [ ] 涉及的接口/契约已写清楚或明确为 `None`。
- [ ] 相关验收条件已经可以用于实现与 review 对齐。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `SqliteWriteCoordinator` 并发串行化与 busy 分类。
- Integration tests: SQLite WAL + 多连接 pool 下并发写入热路径不产生应用内 writer 竞争。
- E2E tests: None。

### UI / Storybook (if applicable)

- Not applicable。

### Quality checks

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --locked --all-features`

## Visual Evidence

Not applicable。

## Related PRs

- None

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：单 writer permit 内如果保留长事务，会把锁竞争从 SQLite 转移成应用排队长尾；实现必须保持写入段短小。
- 假设：SQLite 继续作为当前主数据库，生产部署另行确认。

## 参考（References）

- `docs/solutions/backend/sqlite-wal-write-transactions.md`
- `docs/specs/s8qkn-subscription-sync/SPEC.md`
- `docs/specs/35r55-translation-scheduler/SPEC.md`
