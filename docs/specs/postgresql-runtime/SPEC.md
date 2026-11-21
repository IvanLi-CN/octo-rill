# PostgreSQL 运行时持久化

> 本文件是该主题的持久需求契约。当前实现事实在 `IMPLEMENTATION.md`；生命周期与变更引用在 `HISTORY.md`。

## Context and Scope

- Context: OctoRill 的运行时持久化从 SQLite 单写入调度层切换为 PostgreSQL 18，并保留 SQLite 快照作为一次性迁移输入与回滚产物。
- In scope: 运行时权威存储、schema 所有权、任务与调度语义、单仓 Release 刷新语义、全局内容处理读投影、切换与回滚契约。
- Out of scope: 旧 SQLite 模块的删除进度、生产凭据与 OAuth 端到端验收、发布流水线本身。

## Terms and Interfaces

- `runtime authority`: 唯一接受运行时写入的持久化存储。
- `import`: `octo-rill-migrate` 把 SQLite 快照写入新建 PostgreSQL 库的操作，含 `preflight`/`import`/`verify`/`rebuild`。
- `work item` / `task`: 分别为 legacy 刷新队列行与 PostgreSQL `job_tasks` 行；运行时只认后者。
- Interface: `octo-rill-migrate` 子命令、`DATABASE_URL`、`OCTORILL_PG_WORKER_ENABLED`、`OCTORILL_PG_SCHEDULER_ENABLED`、`admin_runtime_settings.repo_release_worker_concurrency`。

## Requirements

### REQ-PGR-001

- 系统 MUST 只把 PostgreSQL 当作运行时权威存储；`DATABASE_URL` 指向 SQLite 时启动失败关闭。
- Inputs: 部署配置。
- Outputs: 进程启动结果与明确错误。

### REQ-PGR-002

- 导入 MUST 只写新建库，按表校验行数与确定性摘要；源侧删除或同表内容变化 MUST 失败关闭并提示导入新库；中断重入使用同一目标库补齐。
- Inputs: SQLite 快照路径、目标库、manifest。
- Outputs: 导入/校验退出码与 manifest。

### REQ-PGR-003

- PostgreSQL schema MUST 由 `migrations_pg/` 独立维护；已发布迁移不可修改；导入库与空库 MUST 启动到同一可运行 schema，SQLite 类型差异通过 forward migration 或读路径归一。
- Inputs: 已发布迁移集合。
- Outputs: 两库一致的列类型与可启动服务。

### REQ-PGR-004

- 任务执行 MUST 使用 durable 认领、租约心跳、事务重试与按 payload 作用域单飞；七个调度通道 MUST 幂等入队。
- Inputs: `job_tasks` 队列与通道触发时刻。
- Outputs: `queued → running → succeeded/failed` 流转与调度游标。

### REQ-PGR-005

- 单仓 Release 刷新 MUST 共用一套判定：候选用户轮换仅在凭据/权限类失败时发生，公共仓库可在凭据耗尽后匿名读取，并发上限取自 `repo_release_worker_concurrency`；账号级任务 MUST NOT 因单仓不可读而整体失败。
- Inputs: 仓库、请求用户、连接凭据、usage 登记。
- Outputs: Release 快照、`via` 记录（轮换到的账号身份只进服务端日志）、usage 状态与失败分类。

### REQ-PGR-006

- 全局内容处理读投影 MUST 使用与写入一致的身份重建；MUST NOT 为 legacy 不存在的宽松形状增加读取分支。
- Inputs: `content_result_projections`、`content_work_identities`、`html_url`。
- Outputs: 与写入一致的内容结果读取。

### REQ-PGR-007

- 切换 MUST 冻结写入、取快照、导入新库、定向校验、冒烟后判定；回滚只对冻结前数据成立，切换后写入在回滚时丢弃；回滚产物 MUST 在冻结时刻复验索引与校验和。
- Inputs: 冻结时刻、快照、目标库。
- Outputs: 成功判定或回滚动作及其公告内容。

## Verification

### VER-PGR-001

- Method: 以 SQLite URL 启动二进制。
- covers: `REQ-PGR-001`
- Pass condition: 进程以明确错误退出，不进入服务监听。

### VER-PGR-002

- Method: `scripts/pg_seed_import_test.py` 与切换演练（源侧删除、同表内容变化、中断重入）。
- covers: `REQ-PGR-002`
- Pass condition: 正常导入通过；删除与内容变化失败关闭；重入补齐成功。

### VER-PGR-003

- Method: `scripts/pg_seed_fresh_schema_test.py`、导入库与空库启动、列类型审计。
- covers: `REQ-PGR-003`
- Pass condition: 两库均可启动，抽查列类型一致，`_sqlx_migrations` 不出现在导入库。

### VER-PGR-004

- Method: `scripts/pg_seed_worker_test.py`、`scripts/pg_seed_scheduler_test.py`。
- covers: `REQ-PGR-004`
- Pass condition: 认领/终结/租约恢复通过；重复触发不产生重复 queued 行。

### VER-PGR-005

- Method: `scripts/pg_seed_repo_release_fallback_test.py`。
- covers: `REQ-PGR-005`
- Pass condition: 候选轮换、公共 fallback、失败分类、usage 状态、账号级韧性与在途并发上界全部通过。

### VER-PGR-006

- Method: `scripts/pg_content_parity.py` 对生产副本抽样。
- covers: `REQ-PGR-006`
- Pass condition: 抽样投影全部 `ready`，无因身份不一致导致的缺失。

### VER-PGR-007

- Method: `docs/migrations/postgresql/cutover-plan.md` 演练记录与生产副本读写探测。
- covers: `REQ-PGR-007`
- Pass condition: 可逆写全部还原，读探测除环境限制项外通过，回滚契约与公告文本已确认。

## Related ADRs

- [ADR 0019: Validated SQLx Migrations Apply Before Startup](../../adr/0019-sqlx-startup-migration-compatibility.md)
- [ADR 0023: PostgreSQL 作为运行时持久化权威](../../adr/0023-postgresql-runtime-authority.md)

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
