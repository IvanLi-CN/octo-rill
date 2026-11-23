# PostgreSQL 全局内容处理运行时 主题历史

> 这里记录主题局部生命周期、替换、兼容性与必要背景；完整 ADR 取舍保留在 `docs/adr/`。单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- PostgreSQL schema 已包含 `content_*` 表、身份表、投影表与尝试审计表；导入库与空库的 content schema 已由 `migrations_pg/0015` 收敛，读路径由 `postgres_content_overlay` 消费。
- 旧 SQLite `content_processing` 写入实现保留为历史事实，不再作为运行时生产者。

## Replacements / Background

- 2026-11-21 PostgreSQL 18 切换后，读路径进入 global 模式，但 `content_processing` 的 producer/worker 没有随切换迁入 PostgreSQL；`content_current_result_projections` 自 `2026-11-21T07:54:28Z` 后没有新行。
- PostgreSQL 运行时只注册了 `translation.generate` 等任务执行器；该链路仅写 `translation_work_items` 与审计表，不发布读投影，因此不能替代全局内容处理。
- ADR 0024 记录“原生移植而非仅补投影发布”的取舍，并取代 `../postgresql-runtime/HISTORY.md` 中“旧 content_processing 流水线不随本变更恢复”的结论。
- 本主题对 [`REQ-PGR-009`](../postgresql-runtime/SPEC.md) 做显式修订：覆盖管线证明等价后，legacy `translation.generate` 自动入队退役，不再与全局运行时并行。

## Related Changes

- None

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
