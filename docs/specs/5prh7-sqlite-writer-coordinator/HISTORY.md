# SQLite 单写入调度层演进历史（#5prh7）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-05-09：线上 `octo-rill` 容器在高 worker 并发下出现 `database is locked`、慢 SQL 与 500；决定保留业务并发，不通过降低 worker 数量或 SQLite pool=1 解决。
- 2026-05-09：实现选择为应用内 `SqliteWriteCoordinator` 单 writer permit；显式写事务返回原生 SQLx transaction，避免破坏 SQLx executor 行为，同时让 permit 覆盖 `BEGIN IMMEDIATE` 到 `commit` 的完整事务段。

## Key Reasons / Replacements

- SQLite WAL 的单 writer 约束需要在应用内显式建模，否则高并发后台任务会把写锁竞争暴露到用户请求。
- 既有 `BEGIN IMMEDIATE` 修复解决 read-then-write stale snapshot 问题，但不足以提供全局写入背压与可观测排队。
- writer permit 不覆盖 GitHub / AI / 网络阶段，避免把数据库单 writer 约束扩大成业务并发降级。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `docs/solutions/backend/sqlite-wal-write-transactions.md`
