# ADR 0023: PostgreSQL 作为运行时持久化权威

## Status

Accepted

## Context

OctoRill 的运行时持久化长期只有 SQLite 单写入调度层。`迁移边界说明` 建立了离线导入边界：`octo-rill-migrate` 读取 SQLite 快照并写入 PostgreSQL 18，通过 manifest 校验行数与摘要；`src/postgres_runtime.rs` 与 `src/postgres_jobs.rs` 建立了 PostgreSQL 连接、Session store、runtime owner 与任务协调事务。业务垂直切片逐步迁入，但服务仍可在两个存储后端之间摇摆，切换语义与回滚代价没有成文决策。

## Decision

- 生产运行时的持久化权威是 PostgreSQL。`DATABASE_URL` 指向 SQLite 时进程启动即失败关闭，不存在“可回退的双写运行态”。
- 迁移输入是 SQLite 快照，不是运行中的数据库；导入只允许写入**新建**的 PostgreSQL 库。导入器按表校验行数与确定性摘要，源侧删除与内容变更一律失败关闭并提示导入新库；中断后的重入仍使用同一目标库补齐。
- SQLite 的 `_sqlx_migrations` 历史不参与 PostgreSQL。运行时迁移由 `migrations_pg/` 独立维护，已发布迁移不可修改；导入库与空库通过 `0010`–`0017` 收敛到同一可启动 schema，SQLite 翻译出的类型差异在读路径或 forward migration 中归一，不改写已发布迁移。
- 任务与调度语义由 PostgreSQL 承担：`FOR UPDATE SKIP LOCKED` 认领、租约心跳、`40P01/40001` 重试、按 `(task_type, payload scope)` 的单飞去重，以及日报、订阅、Star、失败重试、webhook 推送、账号暂停、管理端汇总七个通道。
- 单仓 Release 刷新有一个共享判定模块：候选用户轮换、公共仓库匿名 fallback、站点级并发上限来自 `admin_runtime_settings.repo_release_worker_concurrency`。账号级与单仓任务不得各自实现一套抓取与限流语义。
- 全局内容处理的读投影与写入身份必须一致：投影以 `html_url` 解析出的 `owner/repo` 与 repo 标识重建，解析失败才回退 `unknown/{repo_id}`；不为 legacy 不存在的“宽松投影”增加分支。
- 切换是可判定的运维动作：冻结写入、快照、导入新库、定向校验、冒烟，然后判定成功或回滚。回滚只在冻结前的数据上成立；切换后写入 PostgreSQL 的状态在回滚时丢弃，必须在公告中说明。回滚产物（只读 SQLite 副本）在冻结时刻接受 `REINDEX` 与校验和复验后才可信。

## Consequences

- 部署必须提供可用的 PostgreSQL 18 与精确的 `DATABASE_URL`；Compose 在 migration 容器成功后才启动 server。
- 生产 SQLite 文件成为只读迁移输入与回滚产物，不再接受运行时写入；旧索引缺失等既有缺陷不会被切换修复，需单独维护窗口处理。
- 导入耗时与校验耗时进入切换预算：全量 `verify` 放在预演，窗口内只做导入与定向校验。
- 运行时迁移与 SQLite 时代的迁移历史彼此独立，迁移器不得读取或复用 `_sqlx_migrations`。
- 任务语义（租约、单飞、重试分类）成为可被 Seed 套件覆盖的契约，不再是运行中数据库的偶然行为。

## Alternatives Rejected

- 双写 SQLite 与 PostgreSQL 被拒绝：两套写入无法给出单一权威顺序，冲突与回滚语义不可判定。
- 允许对已导入库做增量重放被拒绝：导入器不传播源侧删除与更新，增量路径会静默保留被删数据；已改为失败关闭。
- 保留“SQLite 可回退运行态”作为常态选项被拒绝：运行时会在两个后端之间漂移，切换点无法审计。
- 在 PostgreSQL 侧放宽投影身份以兼容 legacy 读取被拒绝：读身份与写身份不一致才是缺陷根因，宽松分支会掩盖新的不一致。
