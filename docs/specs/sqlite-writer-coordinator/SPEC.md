# SQLite 单写入调度层

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- OctoRill 继续使用单个 SQLite 数据库承载 HTTP 请求、后台任务、repo release worker、translation worker 与 LLM 调度状态。
- SQLite WAL 允许读写并发，但仍只有一个 writer；当高并发 worker 直接争抢写事务时，`database is locked` 会外溢到用户请求并造成 500。
- 既有修复已把部分 read-then-write 事务改为 `BEGIN IMMEDIATE`，但如果写协调只覆盖少数后台 claim/finalize 路径，登录/session、job enqueue、LLM lifecycle、translation batch 启动状态切换、translation worker runtime slot、admin runtime settings、repo release reaction refresh 持久化与 repo release recovery 等路径仍会在高 worker 并发下直接争抢 SQLite writer。

## Context and Scope

- Context: OctoRill 的 HTTP 请求、后台 worker 与调度状态共享 SQLite；应用内 writer coordinator 负责把短 SQLite 写段排队，同时保留网络和 AI 阶段的并发。
- In scope: SQLite writer permit、`BEGIN IMMEDIATE` bounded retry、session/job enqueue 热路径、admin/translation/LLM runtime state、social activity snapshot 分块持久化、源代码守门与并发回归验证。
- Out of scope: 数据库迁移到其他引擎、降低业务 worker 并发、生产部署和 API 响应结构变更。

## 目标 / 非目标

### Goals

- 保留高并发 worker 能力；只在进入 SQLite 写入段时协调单 writer。
- 读请求继续使用现有 `SqlitePool`，不得因为写协调退化为全局单连接。
- 高竞争写路径必须通过统一 coordinator 获取 writer permit，并记录 lane、priority、等待时长、attempt 与写入耗时。
- 大集合重建类写入不得把全量 delete/upsert/reconcile 包在单个 writer permit 内；必须先完成读侧候选聚合，再用固定 chunk 的短事务提交写入，并记录 chunk count 与最大 chunk elapsed。
- coordinator 必须区分 `foreground`、`background`、`best_effort` 语义，保证用户可见写入不会长期排在后台 heartbeat/finalize 后面。
- 生产高竞争写入必须通过共享 `SqliteWriteCoordinator` 或明确的内部协调 facade；运行时配置、LLM health/recovery、translation worker runtime slot、jobs lifecycle、repo release sync state 与高频 sync metadata 不得保留直接池写入或可选 writer fallback。
- 源代码质量检查必须在已审计生产模块中拒绝绕过 coordinator 的 `SqlitePool::execute` 与 raw `BEGIN IMMEDIATE`；测试专用、迁移/bootstrap 与纯读代码不属于该写入守门范围，coordinator facade callback 只有在声明 marker 且实现被 AST 验证确实调用 coordinator 时才可放行。
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
- admin runtime settings seed/backfill/update、LLM recovery flags/model health、translation worker runtime slots、reaction PAT state、dashboard rollup、scheduled slot 与 release usage metadata 等高竞争运行时写入。
- `translation_batches queued -> running`、`translation_work_items batched -> running` 与 feed reaction refresh counts 持久化等短写段。
- 针对 SQLite WAL + 多连接 pool 的并发回归测试。
- `tools/rust-source-check` 中针对受保护生产模块的 coordinator bypass guard。

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

- `BEGIN IMMEDIATE` 遇到连接恢复期间的 busy/locked 时，重试必须继续共享同一个单调 deadline；固定尝试次数不得在 deadline 尚未到达时提前放弃恢复窗口。

### REQ-SQLITE-WRITER-006

- 关键写入 lane 必须有结构化 tracing 字段。

### REQ-SQLITE-WRITER-007

- 生产量级 social activity snapshot 必须在 writer permit 外聚合候选，并以固定 64 行 chunk 独立提交；chunk 之间必须释放 permit，且 stale cleanup 必须保留显式 follow 选择与可中断恢复语义。

### REQ-SQLITE-WRITER-008

- admin runtime settings、translation worker runtime slot、LLM recovery/model health、jobs lifecycle、repo release sync-state 与高频 sync metadata 的生产写入必须通过共享 `SqliteWriteCoordinator` 或其内部协调 facade；调用方不得通过 `Option<&SqliteWriteCoordinator>` 在生产路径回退到 raw pool transaction。

### REQ-SQLITE-WRITER-009

- 高竞争运行时写入必须选择并保留明确 lane：用户可见状态使用 `foreground`，worker lifecycle 与 runtime metadata 使用 `background`，非关键 cleanup/touch 使用 `best_effort`；每个 lane 的降级结果必须通过结构化 `sqlite.write` telemetry 可区分。

### REQ-SQLITE-WRITER-010

- `scripts/check-rust-source-quality.sh` 使用 AST source guard 检查受保护生产模块；新增 direct pool write 或未协调的 pool transaction 必须失败，除非属于 test-only/bootstrap，带有明确且受审计的 read-only transaction marker，或位于经过 AST 验证并转发 callback 的 coordinator facade 内。

### REQ-SQLITE-WRITER-011

- 生产写入必须使用隔离于普通读池的专用 SQLite write pool，连接数为 1；普通读池维持现有可配置容量。
- coordinator 对 writer permit、write-pool acquisition、`BEGIN IMMEDIATE`、事务 setup、callback 启动与重试使用同一个单调时钟总 deadline：foreground 为 900 ms，background 为 2500 ms；best-effort 不排队，取得 permit 后沿用 2500 ms 操作预算。deadline 到期后不得启动新的 callback 或重试。
- SQLite write connection 的 `busy_timeout` 不得超过 100 ms；普通 coordinator callback 最多 4 次尝试，退避为 25/50/100 ms。`BEGIN IMMEDIATE` 在连接恢复期间可在同一个总 deadline 内继续使用 25/50/100 ms 退避，不得被固定尝试次数提前截断。busy timeout 与退避、队列、连接池、事务启动和重试开始都受总 deadline 限制，不得在重试后重新开始 deadline。
- callback 在 deadline 前启动后不得由 coordinator 的异步 timeout 取消；它必须持有 writer permit 直到返回 SQLite 的实际结果。事务中的 SQLite 语句仍可由 progress handler 在 deadline 后中断。
- transaction deadline 到期且 COMMIT 尚未派发时，必须在 150 ms cleanup budget 内尝试回滚，不得继续提交。COMMIT 派发前关闭 progress handler；COMMIT 一旦派发，不得因 deadline 取消或中断，必须等待 SQLite 的实际成功或错误结果，即使耗时超过 deadline。已知 `SQLITE_BUSY` 在 deadline 后返回时先回滚并报告 deadline，不得开始重试；其他 COMMIT 错误按实际错误返回，不得仅因当前时间已过 deadline 将其改报为可重试超时。事务 future 被取消时必须中断活动 SQLite 语句；专用 writer pool 连接归还时若 transaction depth 仍非零，必须立即硬驱逐不确定连接；普通读池/内存 fallback 则可在同一个 150 ms budget 内完成 queued rollback，只有 transaction depth 已清零且 progress handler 已移除并确认成功的连接才可复用。回滚或连接清理失败/超时时不得复用不确定连接，必须由对应 pool 补建。

### REQ-SQLITE-WRITER-012

- deadline 到期必须按 lane 语义处理：foreground 返回可识别的 retryable 503；background 将工作延后或持久化重试后结束本次尝试，不得立即自旋；best-effort 跳过写入且不得改变主要用户请求结果。
- content processing recovery/claim、普通 job claim 与 repo release claim worker 在数据库尝试失败时必须使用有界内存退避，基础等待依次为 1/2/4/8/16/30 秒并封顶 30 秒；抖动只能增加当次等待。每个 worker 同时最多执行一笔同类 claim，成功完成对应 recovery/claim DB 尝试后复位退避；等待期间不得由 tick、notify 或其他循环信号提前绕过。
- 结构化 `sqlite.write` telemetry 必须包含 `writer_wait_ms`、`pool_wait_ms`、`begin_ms`、`transaction_ms` 与 `deadline_ms`，并可区分 `writer_queue_timeout`、`write_pool_timeout` 与 `sqlite_busy`。

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
- admin runtime seed/backfill/update、LLM recovery/model health 与 translation worker slot reconciliation 只在短 SQLite 段内持有 writer permit；配置加载和 worker/AI/network 阶段留在 permit 外。
- translation runtime 的 production internal helper 必须接收非可选 `&SqliteWriteCoordinator`；仅 `#[cfg(test)]` wrapper 可以为独立 unit fixture 创建局部 coordinator。
- reaction PAT、dashboard rollup、scheduled slot 与 public release usage metadata 等高频 API/sync metadata 写入使用明确 foreground/background lane，不能因为写入对象较小而直接调用 pool。
- 如果 SQLite 返回 busy/locked，coordinator 使用短退避重试，并在耗尽后返回原始错误上下文。
- foreground 与 background 使用各自有界总 deadline；deadline 限制 writer permit、专用 write pool 获取、`BEGIN IMMEDIATE`、事务 setup、callback 启动与重试开始。已经启动的 callback 可以完成在 deadline 之后；COMMIT 一旦派发，deadline 不再中断它。
- 专用 write pool 只供协调写入使用，普通读请求继续共享多连接 reader pool；因 reader pool 耗尽不应阻塞 writer pool 获取。

### Edge cases / errors

- coordinator 自身 permit 不可用时，写入返回内部错误并带上下文。
- `last_active_at` best-effort 写入拿不到 writer permit 时只记录 debug 并跳过；若已取得 permit 但 SQLite busy/locked，则记录 warning，请求继续返回。
- 外部进程持有 SQLite writer lock 时，coordinator retry 后仍可失败，但失败必须可观测。
- foreground deadline 超时必须返回 retryable 状态与 `Retry-After`，而不是内部 500。
- background deadline 超时由持久化 worker/job 状态延后处理；一个失败尝试结束后不得在同一调用栈立即重试。
- best-effort 在 writer backlog 或 deadline 前无法启动 callback 时跳过持久化，主请求保持成功路径；已经启动的 callback 保持 writer permit 并返回实际结果。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `SqliteWriteCoordinator` | Rust runtime API | internal | Existing / extended | None | backend | backend runtime | 单 writer permit、priority lanes、busy retry、tracing |
| `rust-source-check` SQLite guard | source-quality contract | internal | New | `scripts/check-rust-source-quality.sh` | backend | protected production modules | AST 检查 direct pool write、未协调 pool transaction 与 raw `BEGIN IMMEDIATE` |

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

- Given production code adds a direct `SqlitePool` write, an uncoordinated pool transaction, or raw `BEGIN IMMEDIATE` in an audited module
  When `scripts/check-rust-source-quality.sh` runs
  Then the AST guard fails with the source location until the write is moved behind the coordinator or a verified coordinator facade boundary is declared.

- Given translation runtime reconciliation runs in production or in a test fixture
  When it updates running batch worker slots
  Then the production helper always receives a coordinator and the test-only wrapper uses a local coordinator; no optional writer fallback can execute a raw pool transaction.

- Given reader pool 的所有连接都被只读查询占用
  When foreground/background writer 获取连接
  Then writer 从独立单连接 write pool 获取连接，且不等待 reader pool 释放连接。

- Given writer queue、write pool 或事务 setup 在 lane deadline 前没有完成
  When coordinator 准备启动 callback 或 COMMIT
  Then 不得启动新的 callback、重试或 COMMIT；事务必须回滚，foreground 返回 retryable 503，background 持久化延后，best-effort 跳过写入。

- Given callback 或 COMMIT 在 lane deadline 前已经启动
  When SQLite 在 deadline 之后返回结果
  Then callback 持有 writer permit 直到结束，COMMIT 不被取消或 progress handler 中断，调用方获得 SQLite 的实际成功或错误结果；成功的晚完成必须记录 deadline overrun，连接可继续复用。

- Given 多个写请求排队并遇到 SQLite busy
  When 查看结构化 `sqlite.write` telemetry
  Then `writer_wait_ms`、`pool_wait_ms`、`begin_ms`、`transaction_ms` 与 `deadline_ms` 可用，且 writer 队列超时、write-pool 耗尽与 SQLite busy 使用不同原因字段。

- Given content processing recovery/claim、job claim 或 repo release claim 连续遇到 deadline、busy 或连接状态错误
  When worker 进入下一次 claim loop
  Then 同类 claim 使用 1/2/4/8/16/30 秒基础退避和不提前的抖动等待；成功 DB 尝试后退避复位，故障解除后任务继续领取且 lease/幂等状态保持正确。

## Verification

### VER-SQLITE-WRITER-001

- Method: SQLite WAL tests with multiple pool connections, competing foreground/background writes, busy retry fixtures, and coordinator telemetry assertions.
- covers: REQ-SQLITE-WRITER-001, REQ-SQLITE-WRITER-002, REQ-SQLITE-WRITER-003, REQ-SQLITE-WRITER-004, REQ-SQLITE-WRITER-005, REQ-SQLITE-WRITER-006
- Pass condition: writer sections are serialized, foreground work is admitted ahead of queued background work, best-effort work may skip without breaking the main request, retry is bounded, and logs expose lane, priority, wait, attempt and elapsed fields.

### VER-SQLITE-WRITER-002

- Method: social activity snapshot regression and production-shaped concurrency test with 397 owned-repo associations, at least 100000 search documents, dashboard updates, persisted session save and task enqueue.
- covers: REQ-SQLITE-WRITER-001, REQ-SQLITE-WRITER-002, REQ-SQLITE-WRITER-004, REQ-SQLITE-WRITER-006, REQ-SQLITE-WRITER-007
- Pass condition: candidate reads happen outside the writer permit, every write transaction is bounded to 64 candidates or a single baseline row, the first chunk releases the permit before the foreground operations run, no busy/500 result occurs, explicit follow state remains intact, and the snapshot can finish after resumption.

### VER-SQLITE-WRITER-003

- Method: source checker unit tests plus a full AST scan of the audited production modules, targeted runtime tests for admin settings (`admin_patch_llm_runtime_config_preserves_saved_model_limit_when_field_is_omitted`), LLM health/recovery (`llm_model_health_round_trips_and_rejects_unknown_failure_classes`, `stale_llm_recovery_requires_the_original_lease_snapshot`), translation worker slots (`runtime_resize_updates_running_batch_slot_metadata`, `runtime_resize_rolls_back_memory_when_slot_persistence_fails`), repo release recovery (`stale_repo_release_recovery_requires_the_original_lease_snapshot`), and high-frequency metadata (`reaction_pat_check_result_waits_for_foreground_writer`, `refresh_feed_reactions_skips_persist_failure_under_sqlite_write_pressure`).
- covers: REQ-SQLITE-WRITER-008, REQ-SQLITE-WRITER-009, REQ-SQLITE-WRITER-010
- Pass condition: production writes use the shared coordinator with an explicit lane, the translation runtime has no optional writer fallback, direct pool writes and uncoordinated transactions are rejected by the checker, and test-only/bootstrap/read-only exceptions remain documented and bounded. Recovery updates must compare the selected lease snapshot before failing a row, and runtime configuration must restore in-memory state when slot persistence fails.

### VER-SQLITE-WRITER-004

- Method: coordinator contention tests with independent reader and writer pools, held writer permits, an exhausted reader pool, external `BEGIN IMMEDIATE`, delayed transaction callbacks, a rollback-journal reader that releases after COMMIT dispatch, captured tracing events, and repeated deadline expiry followed by a successful write.
- covers: REQ-SQLITE-WRITER-011, REQ-SQLITE-WRITER-012
- Pass condition: foreground p99 stays within 900 ms under bounded contention; writer acquisition succeeds while the reader pool is exhausted; background deadlines persist one delayed retry; callbacks are not started after queue expiry and started callbacks retain the permit until returning; COMMIT dispatched before expiry may succeed after the deadline and reports its actual result; transactions expired before COMMIT roll back and return a reusable connection or hard-evict an unconfirmed connection; telemetry separates writer queue, pool acquisition, SQLite busy time, connection recovery, and late completion. Fake-clock worker tests prove the required bounded backoff floors without early retry.

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
- `bash scripts/check-rust-source-quality.sh`

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
