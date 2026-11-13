# PostgreSQL 18 迁移实现契约

本仓库提供 `octo-rill-migrate` 作为 SQLite 快照到 PostgreSQL 18 的离线迁移边界。迁移器只读取 SQLite 源卷，并将业务权威表按批次写入 PostgreSQL `public` schema；迁移控制表和审计 manifest 位于独立的 `octo_rill` schema，源文件不会被覆盖。重复执行使用 `ON CONFLICT DO NOTHING`，随后由 manifest 校验行数和确定性 SHA-256 摘要，并把 manifest 同步写入 `octo_rill.migration_manifests`。长期运行时使用独立的 PostgreSQL SQLx migrator，不读取或复用 SQLite 的 `_sqlx_migrations` 历史。

迁移器分为 `preflight`、`import`、`verify` 和 `rebuild` 四个命令。`preflight` 以 SQLite 有效页大小扣除 freelist 后的估算值乘以 2，再加安全余量作为目标空间门槛，并读取 PostgreSQL `data_directory` 所在文件系统的可用空间；空间不足会在写入前失败。迁移容器与 PostgreSQL 分离时，通过 `MIGRATION_TARGET_PATH` 指向共享数据卷的可见路径，Compose 已将该路径设置为 `/var/lib/postgresql`。该门槛是动态的，不要求固定磁盘容量。

SQLite FTS5 内部表被记录为可重建投影并跳过导入。PostgreSQL baseline 创建 `public.search_documents` 和用户 lane 投影，使用 `octo_rill_simple`、生成的 `tsvector`、GIN 与 trigram 索引；`rebuild` 会分析并重建这些索引。FTS 排名不属于权威数据一致性条件。

长期运行时的连接池和 Session 初始化位于 `src/postgres_runtime.rs`：它强制 PostgreSQL 18 版本门禁，执行独立 baseline，校验最多 256 条持久 session 的 MessagePack `bytea` 和 expiry，并使用 PostgreSQL session store。业务模块仍需逐步把现有 SQLite 查询和写入协调器切换到该 runtime。

任务协调的 PostgreSQL 垂直切片位于 `src/postgres_jobs.rs`。它使用稳定的 `created_at, id` 排序、`FOR UPDATE SKIP LOCKED`、条件 `UPDATE ... RETURNING` 和 `40001`/`40P01` 全事务重试；`src/postgres_runtime_owner.rs` 同时提供 runtime owner 的注册和心跳持久化。`OCTORILL_PG_TASK_CLAIM_SMOKE=1` 可在 bootstrap smoke 中实际写入 owner lease、认领一个 queued 任务并写入 `task.running` 事件。

## 本地验证

```sh
cargo check --bin octo-rill-migrate
cargo test --bin octo-rill-migrate
SQLITE_SOURCE=sqlite:/path/to/snapshot.db \
POSTGRES_URL=postgres://user:password@127.0.0.1:5432/octo_rill \
cargo run --bin octo-rill-migrate -- preflight
```

PostgreSQL runtime 本身可用以下入口做无业务写入 smoke check：

```sh
OCTORILL_PG_BOOTSTRAP_ONLY=1 \
OCTORILL_DB_POOL_MAX_CONNECTIONS=8 \
DATABASE_URL=postgres://user:password@127.0.0.1:5432/octo_rill \
cargo run --bin octo-rill
```

该入口只执行 PostgreSQL 18 版本检查、baseline 和 Session schema 初始化，然后正常退出。

生产副本演练在 Agent VM 内执行。生产 secrets、上游凭据和真实服务网络不作为迁移输入；只复制经授权的 SQLite 快照及脱敏配置。Compose 使用 `compose.postgres.yml`：PostgreSQL 18 健康后先运行一次性 migration 容器，migration 成功后才允许 server 容器启动。

演练命令使用固定会话 ID，避免误用其他 VM：

```sh
export AGENT_VM_SESSION_ID=octo-rill-pg18
AGENT_VM_SCRIPT=/Users/ivan/.agents/skills/agent-vm/scripts/agent_vm.ts
bun "$AGENT_VM_SCRIPT" doctor
bun "$AGENT_VM_SCRIPT" status
bun "$AGENT_VM_SCRIPT" acquire
bun "$AGENT_VM_SCRIPT" sync --name octo-rill-pg18-migration
bun "$AGENT_VM_SCRIPT" exec -- sh -lc 'cd /workspace/worktrees/octo-rill-pg18-migration/src && cargo test --bin octo-rill-migrate'
```

完整演练脚本在 VM 内依次执行 `preflight`、`import`、`verify` 和 `rebuild`；设置 `AGENT_VM_MIGRATION_BINARY` 可使用宿主机构建的 Linux 迁移器，设置 `AGENT_VM_APP_BINARY` 并提供应用 smoke 所需 secrets 可继续执行 PostgreSQL runtime owner、Session 和任务认领探针。

如果 VM 镜像没有匹配的 Rust 工具链，可先在宿主机构建 Linux 迁移器，再设置 `AGENT_VM_MIGRATION_BINARY`；演练脚本会上传该二进制并在 VM 内执行同样的预检和迁移命令。

当前候选 HEAD 已在 session-owned Agent VM 使用 PostgreSQL 18.7 和脱敏 SQLite 夹具完成容量预检、导入、校验、搜索投影重建及重复导入；重复导入前后均通过校验，随后 bootstrap smoke 成功写入 runtime owner 并认领 queued task。该夹具验证不替代线上数据库副本演练。

当前应用业务模块仍以 SQLite 类型为编译时边界；PG service boundary 已能独立启动并提供数据库健康/版本探针，但业务路由尚未全部迁移。在完成逐模块 PostgreSQL runtime 改造前，不应把该 Compose 文件用于线上切换。线上停机、旧容器停止、数据卷切换和回滚均需要另行授权。

普通服务入口检测到 PostgreSQL `DATABASE_URL` 后进入独立的 PG service boundary；当前提供 `/health`、`/version` 和基于 PostgreSQL store 的 `/auth/logout`，未迁移的业务路由保持未注册。`OCTORILL_PG_BOOTSTRAP_ONLY=1` 仍仅用于数据库 bootstrap smoke check。

Agent VM 中的 PostgreSQL 18.7 bootstrap smoke 已验证独立的 `public._sqlx_migrations`、`octo_rill.schema_migrations`、`octo_rill.migration_manifests` 和 `octo_rill.tower_sessions` 均可幂等初始化。

后续 Agent VM 复测还验证了自引用 `job_tasks` 表、UTC 文本到 `timestamptz` 的导入转换、连续两次 import/verify 幂等，以及真实 PG18 任务认领：`t1` 变为 `running`，写入一条 `task.running` 事件，`t2` 保持 queued。
