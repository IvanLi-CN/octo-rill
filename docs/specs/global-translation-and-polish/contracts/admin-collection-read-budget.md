# 管理采集记录读取预算证据

## Scope

该证据覆盖 `0079_admin_collection_read_budget_indexes.sql` 应用后的线上 SQLite 副本，只记录查询计划和耗时统计，不包含记录内容、凭据或数据库副本。

## Reproduction

- 共享测试机运行目录：`/srv/codex/workspaces/ivan/octo-rill__2f31_20260911/runs/20260911_admin_collection_19735_140842/`
- 副本操作：只读 `.backup`，在副本上应用 `0079`；验证后清理运行目录。
- 查询计划检查：通知规范来源使用 `idx_notifications_admin_canonical_source`；工作项候选使用 `idx_translation_work_items_admin_entity_kind_attempt`；代表性查询未出现按来源行执行的相关工作项全表扫描。
- 负载：四类列表各先执行一次预热读取，再对 31 天窗口连续测量 30 次；只测 SQL 候选集读取，不打印任何记录内容。

## Results

| 记录类型 | p95 | p99 | 最大值 |
| --- | ---: | ---: | ---: |
| Release | 0.13s | 0.15s | 0.15s |
| 公告 | 0.01s | 0.01s | 0.01s |
| 通知 | 0.05s | 0.05s | 0.05s |
| 日报 | 0.01s | 0.01s | 0.01s |

所有测量均低于 1s p95、2s p99 和 5s 单次读取预算。该文件不替代线上发布检查；生产数据库仍只允许通过受控只读副本执行验证。
