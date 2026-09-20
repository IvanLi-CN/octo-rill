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

## Activity Read Validation

迁移 `0084_admin_collection_activity_indexes.sql` 只新增索引。查询计划和延迟验证在 SQLite 内存合成副本上进行，每类创建 100,000 条源行，不含真实记录内容；每类先预热一次，再测 30 次。EXPLAIN 必须命中来源时间索引，并命中公告 canonical、通知 canonical、日报最新 LLM call 索引。

| 记录类型 | 12h cells | p95 | p99 | 最大值 |
| --- | ---: | ---: | ---: | ---: |
| Release | 5,000 | 0.193s | 0.231s | 0.231s |
| 公告 | 2,500 | 0.154s | 0.156s | 0.156s |
| 通知 | 2,500 | 0.066s | 0.067s | 0.067s |
| 日报 | 5,000 | 0.132s | 0.145s | 0.145s |

验证命令：`cargo test --locked admin_collection_activity_production_shape_budget -- --ignored --nocapture`。输出只包含 kind、行数、索引计划与耗时，不包含记录内容；四类均满足 p95 1s、p99 2s 和单次 5s 预算。
