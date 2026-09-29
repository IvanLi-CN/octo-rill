# SQLite 单写入调度层

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- OctoRill 继续使用单个 SQLite 数据库承载 HTTP 请求、后台任务、repo release worker、translation worker 与 LLM 调度状态。
- SQLite WAL 允许读写并发，但仍只有一个 writer；当高并发 worker 直接争抢写事务时，`database is locked` 会外溢到用户请求并造成 500。
- 既有修复已把部分 read-then-write 事务改为 `BEGIN IMMEDIATE`，但如果写协调只覆盖少数后台 claim/finalize 路径，登录/session、job enqueue、LLM lifecycle、translation batch 启动状态切换、repo release reaction refresh 持久化与 repo release recovery 等路径仍会在高 worker 并发下直接争抢 SQLite writer。

## Context and Scope

- Context: OctoRill 的 HTTP 请求、后台 worker 与调度状态共享 SQLite；应用内 writer coordinator 负责把短 SQLite 写段排队，同时保留网络和 AI 阶段的并发。
- In scope: SQLite writer permit、`BEGIN IMMEDIATE` bounded retry、session/job enqueue 热路径、social activity snapshot 分块持久化与并发回归验证。
- Out of scope: 数据库迁移到其他引擎、降低业务 worker 并发、生产部署和 API 响应结构变更。

## 目标 / 非目标

### Goals

- 保留高并发 worker 能力；只在进入 SQLite 写入段时协调单 writer。
- 读请求继续使用现有 `SqlitePool`，不得因为写协调退化为全局单连接。
- 高竞争写路径必须通过统一 coordinator 获取 writer permit，并记录 lane、priority、等待时长、attempt 与写入耗时。
- 大集合重建类写入不得把全量 delete/upsert/reconcile 包在单个 writer permit 内；必须先完成读侧候选聚合，再用固定 chunk 的短事务提交写入，并记录 chunk count 与最大 chunk elapsed。
- coordinator 必须区分 `foreground`、`background`、`best_effort` 语义，保证用户可见写入不会长期排在后台 heartbeat/finalize 后面。
- SQLite busy/locked 必须作为可恢复背压处理，经过有界退避重试后再决定是否失败。
- `last_active_at` 等用户热路径 best-effort 写入不得等待后台 writer 排队，也不得把 `/api/me` 类请求打成 500。
- 对已有 pending 合同的读取接口，若结果表已经存在当前 source hash 的 `queued/running` 状态，则允许在 writer 压力下直接复用该快照，不得为了重复 resolve 再强制进入新的写事务。

### Non-goals

- 不迁移到 PostgreSQL。
- 不降低 `repo_release_worker_concurrency`、translation worker 或 LLM 并发作为修复方案。
- 不修改 101 线上 compose、secrets、容器或生产数据库。
- 不新增前端 UI 或视觉交付面。

## 范围（Scope）

### In scope

- 后端 runtime 内的 SQLite write coordinator。
- `job_tasks` enqueue/event/cancel/claim/heartbeat/finalize、session create/save/delete、repo release attach/claim/finalize/heartbeat/recovery/sync-state、translation request/batch/recovery/finalize、LLM call lifecycle、`touch_user_last_active_at` 等热写路径。
- `translation_batches queued -> running`、`translation_work_items batched -> running` 与 feed reaction refresh counts 持久化等短写段。
- 针对 SQLite WAL + 多连接 pool 的并发回归测试。

### Out of scope

- 外部数据库迁移。
- 改变现有 API 响应结构。
- 生产部署操作。

## Requirements

### MUST

### REQ-SQLITE-WRITER-001

- 写协调层必须以应用内单 writer permit 串行化 SQLite 写入段。

### REQ-SQLITE-WRITER-002

- 前台写入必须能在当前 writer 释放后优先于已排队后台写入运行。

### REQ-SQLITE-WRITER-003

- best-effort 写入不得因为后台 writer backlog 破坏用户主要流程。

### REQ-SQLITE-WRITER-004

- 网络请求、GitHub API、AI 调用与长耗时计算不得在 writer permit 内执行。

### REQ-SQLITE-WRITER-005

- busy/locked retry 必须有上限，避免无限等待。

### REQ-SQLITE-WRITER-006

- 关键写入 lane 必须有结构化 tracing 字段。

### REQ-SQLITE-WRITER-007

- 生产量级 social activity snapshot 必须在 writer permit 外聚合候选，并以固定 64 行 chunk 独立提交；chunk 之间必须释放 permit，且 stale cleanup 必须保留显式 follow 选择与可中断恢复语义。

### SHOULD

- 事务仍应尽量短小；小批量写可以在单次 permit 内完成，生产量级全量重建必须拆成多个短 permit。
- 对 best-effort 写入失败路径记录 warning/debug，但不破坏用户主要读流程。

### COULD

- 后续可按 lane 增加指标导出或管理端展示。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 后台 worker 可以继续高并发执行网络/AI 阶段；进入 SQLite 写入时通过 coordinator 排队。
- 登录/session、手动触发任务、job enqueue 与取消任务走 foreground lane；后台 claim/heartbeat/finalize/recovery 走 background lane；非关键活跃时间和清理类写入走 best-effort lane。
- translation batch 的启动状态切换必须在单个短事务内完成：`translation_batches` 的 `queued -> running`、关联 `translation_work_items` 的 `running` 标记与 lease 元数据更新都要在 writer permit 内提交，AI 调用与后续长计算继续留在 permit 外。
- feed reaction refresh 的 counts 持久化属于非关键后台写入；当 writer permit 不可得或 SQLite busy 时允许跳过持久化，但必须保留 live payload 返回与结构化降级证据。
- repo refresh governance rebuild 属于生产量级集合重建路径：候选聚合留在 writer permit 外，stale cleanup、snapshot upsert、member reconciliation、snapshot completion 与 cycle reconciliation 分阶段提交；snapshot/member 写入固定 500 行 chunk。
- social activity snapshot 属于生产量级集合重建路径：follower、owned-repo/member、history 与 association cleanup 的候选必须在 writer permit 外聚合；持久化使用固定 64 行 chunk，每个 chunk 独立取得并释放 writer permit，且 chunk 之间允许前台写入插入。snapshot baseline 与幂等 history materialization 必须保持可中断恢复，stale cleanup 不得删除当前 target 之外的显式 follow 选择。
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
  Then 能看到 lane、priority、wait、attempt、elapsed 或 retry_after 字段。

- Given 多个后台 repo release/translation/LLM worker 正在高并发执行网络任务并产生 heartbeat/finalize 写入
  When 前台请求创建 session 或调用 `jobs::enqueue_task`
  Then 前台写入通过 coordinator 排队并优先进入写段，不直接把 SQLite busy/locked 冒泡成 500。

- Given 翻译调度器已经 claim 到 batch 且另一个 SQLite 写事务暂时持有 writer
  When worker 尝试把 batch 与 work items 标记为 `running`
  Then worker 会在 coordinator 内等待短写段串行提交，而不是直接把 `translation_batches ... database is locked` 冒泡成失败。

- Given feed reaction refresh 已经拿到 GitHub live payload
  When counts 持久化阶段遇到 writer backlog 或 SQLite busy
  Then 非关键持久化允许跳过，但接口仍返回 live payload，且日志能区分 `sqlite_writer_busy` 或 `sqlite_busy` 降级原因。

- Given repo refresh governance rebuild 需要处理生产量级 candidate repo 与 active cycle members
  When 重建 governance snapshots 与 reconciliation
  Then 单个 writer permit 只覆盖一个短阶段或一个 500 行 chunk，日志包含 `snapshot_upsert_chunks`、member reconciliation chunk count 与 `max_writer_chunk_elapsed_ms`。

- Given social activity snapshot 需要处理数百个 owned repo/member association 与至少 100000 条 search documents 共存的数据库
  When snapshot 写入运行并暂停在第一个 chunk 之后
  Then `GET /api/dashboard/updates`、session save 与 `jobs::enqueue_task` 仍能完成；日志或测试证据分别报告候选读取耗时、writer wait、query/write chunk 耗时及 busy/500 结果，且每个 social snapshot writer chunk 不超过固定 64 行。

## Verification

### VER-SQLITE-WRITER-001

- Method: SQLite WAL tests with multiple pool connections, competing foreground/background writes, busy retry fixtures, and coordinator telemetry assertions.
- covers: REQ-SQLITE-WRITER-001, REQ-SQLITE-WRITER-002, REQ-SQLITE-WRITER-003, REQ-SQLITE-WRITER-004, REQ-SQLITE-WRITER-005, REQ-SQLITE-WRITER-006
- Pass condition: writer sections are serialized, foreground work is admitted ahead of queued background work, best-effort work may skip without breaking the main request, retry is bounded, and logs expose lane, priority, wait, attempt and elapsed fields.

### VER-SQLITE-WRITER-002

- Method: social activity snapshot regression and production-shaped concurrency test with 397 owned-repo associations, at least 100000 search documents, dashboard updates, persisted session save and task enqueue.
- covers: REQ-SQLITE-WRITER-001, REQ-SQLITE-WRITER-002, REQ-SQLITE-WRITER-004, REQ-SQLITE-WRITER-006, REQ-SQLITE-WRITER-007
- Pass condition: candidate reads happen outside the writer permit, every write transaction is bounded to 64 candidates or a single baseline row, the first chunk releases the permit before the foreground operations run, no busy/500 result occurs, explicit follow state remains intact, and the snapshot can finish after resumption.

## 验收清单（Acceptance checklist）

- 核心路径的长期行为已被明确描述。
- 关键边界/错误场景已被覆盖。
- 涉及的接口/契约已写清楚或明确为 `None`。
- 相关验收条件已经可以用于实现与 review 对齐。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `SqliteWriteCoordinator` 并发串行化、foreground 优先级与 busy 分类。
- Integration tests: SQLite WAL + 多连接 pool 下并发写入热路径不产生应用内 writer 竞争；后台 writer 压力下 `enqueue_task` 不绕过 coordinator。
- E2E tests: None。

### UI / Storybook (if applicable)

- Not applicable。

### Quality checks

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --locked --all-features`

## Visual Evidence

- None

Not applicable。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：单 writer permit 内如果保留长事务，会把锁竞争从 SQLite 转移成应用排队长尾；实现必须保持写入段短小。
- 假设：SQLite 继续作为当前主数据库，生产部署另行确认。

## Related ADRs

None

## 参考（References）

- `docs/solutions/backend/sqlite-wal-write-transactions.md`
- `docs/specs/subscription-sync/SPEC.md`
- `docs/specs/translation-scheduler/SPEC.md`
