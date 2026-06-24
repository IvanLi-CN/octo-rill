# SQLite 单写入调度层演进历史（#5prh7）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-05-09：线上 `octo-rill` 容器在高 worker 并发下出现 `database is locked`、慢 SQL 与 500；决定保留业务并发，不通过降低 worker 数量或 SQLite pool=1 解决。
- 2026-05-09：实现选择为应用内 `SqliteWriteCoordinator` 单 writer permit；显式写事务返回原生 SQLx transaction，避免破坏 SQLx executor 行为，同时让 permit 覆盖 `BEGIN IMMEDIATE` 到 `commit` 的完整事务段。
- 2026-05-10：线上仍保留高 `repo_release_worker_concurrency` 等运行时值，说明降低默认值不能修复已有生产配置；writer coordinator 扩展为 foreground/background/best-effort priority，并覆盖 session、job enqueue、LLM lifecycle 与 repo release sync-state 等绕过路径。
- 2026-06-24：线上日志继续暴露 `translation_batches ... database is locked` 与 reaction refresh 持久化放大错误；决定把 translation batch 启动写段补齐到 coordinator，并把非关键 reaction counts persist 降级为可跳过 best-effort 写入。
- 2026-06-24：线上剩余 `sync.subscriptions` 日志继续出现 `insert social activity event`、`upsert follower current member` 与 `delete stale repo star current members` 的 `database is locked`；决定把 social activity snapshot 与 feed activity event 两个仍用 deferred transaction 的路径一并迁到 coordinator + `BEGIN IMMEDIATE`。
- 2026-06-24：复核 host 101 线上日志后，进一步确认高频后台直写仍会挤占协调后的 claim / heartbeat 路径；决定把 `sync_subscription_events` 写入、订阅历史裁剪与 `llm_calls` 保留清理一并收回 coordinator，并在 writer permit 不可得或 SQLite busy 时统一走可观测的 non-fatal downgrade。
- 2026-06-24：继续复核当前代码后，确认 `starred_repos` 增量 upsert、通知 inbox upsert / open-url repair 与 `public_repo_release_usage` 元数据刷新仍直接写 `state.pool`；决定把这些高频后台增量写也收回 coordinator，避免遗漏路径继续抢占 SQLite writer。

## Key Reasons / Replacements

- SQLite WAL 的单 writer 约束需要在应用内显式建模，否则高并发后台任务会把写锁竞争暴露到用户请求。
- 既有 `BEGIN IMMEDIATE` 修复解决 read-then-write stale snapshot 问题，但不足以提供全局写入背压与可观测排队。
- worker 数主要缓解网络 IO 吞吐，不能作为 SQLite 写入背压的长期旋钮；数据库写段必须集中排队，网络/AI 阶段继续并发。
- writer permit 不覆盖 GitHub / AI / 网络阶段，避免把数据库单 writer 约束扩大成业务并发降级。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `docs/solutions/backend/sqlite-wal-write-transactions.md`
