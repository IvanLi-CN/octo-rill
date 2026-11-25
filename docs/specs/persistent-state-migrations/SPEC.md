# Persistent State Migrations

> SQLite 运行时的 SQLx 迁移历史与 writer coordinator 已进入退役队列，见 [SQLite 退役运行时删除](../sqlite-runtime-retirement/SPEC.md)。本主题保留与存储引擎无关的持久事实契约：控制记录修复、旧事实观察、有界回填、迁移操作租约与前向修复。

## Context And Scope

- Context: 运行时权威存储是 PostgreSQL（ADR 0023）。内容处理有一个可在线修复的控制记录、只读旧事实观察，以及需要可暂停、可重入的历史回填。SQLite 时代的 SQLx 迁移历史不再演进。
- In scope: `content_processing_control` 修复、`content_legacy_observations` 身份与只读性、历史回填的有界游标、迁移操作命名租约、前向修复与管理员只读面。
- Out of scope: SQLite SQLx 历史与 `_sqlx_migrations` 兼容、`SqliteWriteCoordinator` 后台 lane、内容处理业务语义（由 `postgres-content-processing` 拥有）、旧事实迁移为全局工作或尝试。

## Requirements

- `REQ-PSM-ORDER`: DDL、当前状态修复与历史回填是独立排序的操作，拥有各自的状态、校验和、进度与失败信号。
- `REQ-PSM-ADMISSION`: 每个在线操作期间有效的内容 admission 必须保持可用。`legacy` 或 `rollback_freeze` 控制值必须在准入事务内原子修复为 `global`，且发生在唯一工作身份查询或插入之前。
- `REQ-PSM-OBSERVATION`: 旧行保持只读观察。观察身份包含 `(legacy_table, legacy_primary_key, legacy_source_hash)`，使被删除后复用的旧主键产生新化身。
- `REQ-PSM-PAUSE`: 历史回填每批最多提交 100 行，在释放写许可前持久化游标，并在暂停或中断后从持久进度恢复。
- `REQ-PSM-LEASE`: 只有当前的命名迁移租约持有者可以推进操作。存活的 runtime owner 永不被回收；陈旧所有权必须可恢复。
- `REQ-PSM-REPAIR`: 已部署的持久状态对迁移身份不可变。失败使用前向修复；协议不包含 down migration、蓝绿切换或迁移专属冻结窗口。

## Interfaces

管理员认证边界暴露 `GET /admin/jobs/migrations`、`POST /admin/jobs/migrations/{migration_id}/pause` 与 `POST /admin/jobs/migrations/{migration_id}/resume`。读接口按 `operation_order` 返回运行与全部操作；它是唯一迁移读面，因为运行时恰好定义一次迁移。接口只暴露脱敏状态，不创建面向用户的迁移 API。

## Verification

- `VER-PSM-OPERATIONS`: 操作校验和与顺序不可变；命名租约、存活所有者保护与陈旧所有者恢复有测试。
- `VER-PSM-RESUME`: 有界游标批次、暂停/恢复、中断重入、重复抑制、source-hash 化身与脱敏失败状态有测试。
- `VER-PSM-ADMISSION`: `legacy`、`rollback_freeze` 与 `global` 下的有效提交保持 `202` 与活动工作 `409` 语义；API 适配器不得把 `rollback_freeze` 提交路由进兼容写入者的迁移 `503` 路径。
- `VER-PSM-OBSERVATION`: 旧事实不被改写或提升为全局工作、结果或尝试历史；删除并复用旧主键产生新观察化身。

## Acceptance

- DDL、当前状态修复与观察回填保持独立可观察、可暂停、可恢复。
- 旧事实永不改写、永不提升为全局工作/结果/尝试历史。
- 内容 admission 在迁移工作让出写许可期间保持既有 `202` 或活动工作 `409` 响应。
- 删除 SQLite 运行时代码不改变本文件的任何契约。

## Related ADRs

- [ADR 0018: Online Persistent State Migrations](../../adr/0018-online-persistent-state-migrations.md)
- [ADR 0023: PostgreSQL 作为运行时持久化权威](../../adr/0023-postgresql-runtime-authority.md)
