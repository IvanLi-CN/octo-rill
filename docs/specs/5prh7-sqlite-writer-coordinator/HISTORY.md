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
- 2026-06-24：继续沿 claim / heartbeat 邻近路径复核后，确认 jobs scheduler 的 `daily_brief_hour_slots` / `scheduled_task_dispatch_state` 周期写入与 brief failure 标记仍直接写 `state.pool`；决定把这些 20s/45s 周期后台写与失败补偿写也收回 coordinator，补齐 task orchestration 周边的 single-writer 合同。
- 2026-06-24：PR review 继续指出 notification open-url repair 虽然已接入 coordinator，但把 GitHub thread lookup 也包进了 writer permit；决定补并发回归并把 lookup 前移到 permit 外，只保留最终 repair update 的短事务。
- 2026-06-24：继续 review 时又确认 subscription history prune 的 watcher 裁剪一旦 busy 会提前短路整个函数，导致 event retention 同轮完全不跑；决定把两个 prune 相位拆成独立 best-effort 控制流，并补回归锁住“watcher skip 不得短路 event prune”。
- 2026-06-24：继续 review 又确认 notification open-url repair 在 lookup 前移后仍会用旧快照覆盖并发更新的新字段；决定在 writer 事务内按 `thread_id` 重读当前 notification，只在仍需 repair 时更新 URL 相关字段，并补并发回归锁住“repair 不得回滚更新更晚的通知数据”。
- 2026-06-24：继续 review 再确认 notification open-url repair 重新读取当前行后，仍会把“当前非空但更旧”的标题/类型/reason 保留到最终写回，导致 fresher thread refresh 无法真正修复 stale metadata；决定按 `updated_at` 比较 freshness，仅在线程刷新不早于当前行时让其 metadata 覆盖旧值，并补回归锁住“旧行 metadata 能被 fresher thread 修正，但不会覆盖更新更晚的并发 upsert”。

## Key Reasons / Replacements

- SQLite WAL 的单 writer 约束需要在应用内显式建模，否则高并发后台任务会把写锁竞争暴露到用户请求。
- 既有 `BEGIN IMMEDIATE` 修复解决 read-then-write stale snapshot 问题，但不足以提供全局写入背压与可观测排队。
- worker 数主要缓解网络 IO 吞吐，不能作为 SQLite 写入背压的长期旋钮；数据库写段必须集中排队，网络/AI 阶段继续并发。
- writer permit 不覆盖 GitHub / AI / 网络阶段，避免把数据库单 writer 约束扩大成业务并发降级。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `docs/solutions/backend/sqlite-wal-write-transactions.md`
