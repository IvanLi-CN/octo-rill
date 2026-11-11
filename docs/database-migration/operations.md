# PostgreSQL 18 迁移运维方案

本文是独立 PostgreSQL 18 单实例的通用操作模板，配套 [ADR 0023](../adr/0023-postgresql-18-migration.md) 与 [共享契约](./contracts.md)。它只描述可审计的迁移、容量、备份、监控和切换边界，不包含任何主机地址、凭据、Compose 文件或生产数据。

## 适用边界

- PostgreSQL 18 是 OctoRill 的独立单实例，应用使用专用数据库用户；实例不与其他项目共用数据库或账号。
- PostgreSQL 只读取 `migrations_pg/` 的 SQLx history。SQLite 的 `_sqlx_migrations`、原始快照和历史校验仍由 SQLite 侧拥有，不能复制到 PostgreSQL。
- 迁移阶段只写权威表；搜索索引、outbox 游标和 backfill 队列在权威数据校验后重建。
- 任何真实环境的停机、写入切换、备份保留期和删除操作都需要部署侧的独立授权。本文不授权部署或发布。

## 配置样例

`.env.example` 保留 SQLite 默认值，并提供 PG18 的无凭据示例。启用前必须确认应用 runtime 已实现这些配置项，且通过脱敏配置摘要核对有效值。

| 配置 | 目的 | 运维约束 |
| --- | --- | --- |
| `DATABASE_URL` | 应用主连接串 | 只指向本次迁移的 PG18 实例；密码来自 secret store，不放入 Git 或命令行 |
| `SESSION_STORE_DATABASE_URL` | Session store 连接串 | 默认与主连接串相同；分离时必须证明 schema、权限和备份覆盖一致 |
| `OCTORILL_DB_FOREGROUND_MAX_CONNECTIONS` | 前台连接上限 | 为健康探针和管理员操作保留余量 |
| `OCTORILL_DB_BACKGROUND_MAX_CONNECTIONS` | 后台连接上限 | 不得挤占前台保留容量 |
| `OCTORILL_DB_BACKGROUND_CONCURRENCY` | 后台并发信号量 | 与后台连接上限一起调小，容量不足时退避 |
| `OCTORILL_DB_ACQUIRE_TIMEOUT_SECONDS` | 获取连接超时 | 超时要记录为可聚合的容量信号 |
| `OCTORILL_DB_STATEMENT_TIMEOUT_SECONDS` | 语句超时 | 迁移、导入和运维查询使用单独的受控窗口 |

## 迁移前检查

1. 确认目标是空的 PostgreSQL 18 schema，实例、数据库、应用用户和备份目录均为本次迁移专属资源；不要把连接串或 secret 写入证据。
2. 确认应用和导入工具来自同一候选版本，记录 Git SHA、migration head、配置摘要、维护窗口和操作者。
3. 生成一致性只读 SQLite 快照和 manifest。manifest 至少包含源表、批次、行数、主键范围或哈希、目标 schema、开始/结束时间和异常分类。
4. 检查磁盘空间、备份目录可写性、PG18 `max_connections`、连接超时和监控 target；任何检查失败都停止在切换前。

## 容量门禁与连接监控

前台和后台使用同一 PostgreSQL 实例，但连接上限、获取超时和后台信号量必须独立配置。建议把应用保留容量设为 `max_connections` 的明确子集，并至少保留一组管理员/恢复连接；具体数值由压测和实例规格决定，不能从默认值推断。

切换前和切换后都记录以下只读查询的摘要（不得记录 query text 中的 secret）：

```sql
SHOW max_connections;

SELECT application_name, state, count(*) AS connections
FROM pg_stat_activity
GROUP BY application_name, state
ORDER BY application_name, state;

SELECT wait_event_type, wait_event, count(*) AS waiting
FROM pg_stat_activity
WHERE wait_event IS NOT NULL
GROUP BY wait_event_type, wait_event
ORDER BY waiting DESC;
```

至少满足以下门禁后才允许开放 PostgreSQL 写入：活动连接低于配置上限的 70%，没有持续增长的连接获取超时，没有长事务阻塞迁移或备份，后台连接未占满其独立配额，且 `deadlocks`、`40001` 和 `40P01` 重试率在验收窗口内为零或已解释。门禁阈值可以由部署侧收紧，但不能放宽到没有前台余量。

## 备份、恢复与导入

1. 在停止 SQLite 写入前保存原始 SQLite 快照并计算 SHA-256；快照只读保留，不能覆盖。
2. PG18 schema 初始化后执行独立 migrations，记录每个阶段的 migration head 和失败日志；migration 未完成时应用不得绑定业务监听。
3. 按 manifest 分批导入权威表，依次完成 sequence 修复、外键/唯一检查、Session decode/账户状态检查、权限抽样和派生索引重建。每阶段可暂停、可重跑、可审计。
4. 生成 PG 逻辑备份（例如 `pg_dump --format=custom`），保存备份 SHA-256、数据库版本、migration head 和目标目录；备份目录必须独立于运行数据卷。
5. 将备份恢复到隔离目标并执行 `pg_restore --list` 或等价可读性检查，再抽样校验权威行数、Session decode 和权限谓词。恢复失败时不得进入切换。

示例命令只使用部署侧注入的环境变量，不在命令行展开密码：

```console
pg_isready --dbname="$DATABASE_URL"
pg_dump --format=custom --file="$POSTGRES_BACKUP" "$DATABASE_URL"
pg_restore --list "$POSTGRES_BACKUP"
```

## 切换步骤与停止条件

1. 进入已批准的维护窗口，停止新写入并记录最后一次 SQLite 写入时间；保留 SQLite 快照和导入 manifest。
2. 在 PG18 上完成 migrations、导入、校验、派生索引重建和可恢复备份；任何一项失败都保持停写并标记 `blocked`。
3. 注入经过脱敏核对的 PG 配置，启动应用并确认 migration 完成后才绑定监听；检查 `/api/health`、登录、Session、API key、后台任务、核心读写和搜索。
4. 观察连接池、锁等待、事务重试、错误率、队列年龄和备份告警。只有核心 smoke checks 与容量门禁均通过，才解除维护窗口。

## 回退边界

- **开放 PostgreSQL 写入前**：可以停用候选应用，恢复已校验的 SQLite 快照和旧应用版本，再执行核心 smoke checks。
- **开放 PostgreSQL 写入后**：禁止直接把旧应用指回 SQLite；这会丢失 PostgreSQL 新写入。必须停写、保留 PG 数据和备份，执行受控反向导出或恢复到经过校验的目标，再重新完成行数、权限、Session、搜索和健康检查。
- 不执行未经验证的 PostgreSQL down migration，不删除 PG 数据目录来代替回退，不把失败导入标记为成功。

## 证据与收口

每次演练或切换至少保存脱敏的版本与 migration head、快照/备份 SHA-256、manifest 汇总、容量查询、锁/连接摘要、恢复检查、smoke checks、切换时间窗口和最终 disposition。证据缺失、备份不可恢复、连接门禁不满足或回退路径未演练时，任务保持未完成。
