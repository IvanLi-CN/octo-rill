# PostgreSQL 18 迁移实现契约

本仓库提供 `octo-rill-migrate` 作为 SQLite 快照到 PostgreSQL 18 的离线迁移边界。迁移器只读取 SQLite 源卷，并将权威表按批次写入独立的 `octo_rill` schema；源文件不会被覆盖。重复执行使用 `ON CONFLICT DO NOTHING`，随后由 manifest 校验行数和确定性 SHA-256 摘要。

迁移器分为 `preflight`、`import`、`verify` 和 `rebuild` 四个命令。`preflight` 以 SQLite 有效页大小扣除 freelist 后的估算值乘以 2，再加安全余量作为目标空间门槛，并读取 PostgreSQL `data_directory` 的可用空间；空间不足会在写入前失败。该门槛是动态的，不要求固定磁盘容量。

SQLite FTS5 内部表被记录为可重建投影并跳过导入。`rebuild` 负责启用 `pg_trgm`，应用专用的全文检索配置和 GIN/trigram 索引应由 PostgreSQL 迁移定义创建。FTS 排名不属于权威数据一致性条件。

长期运行时的连接池和 Session 初始化位于 `src/postgres_runtime.rs`：它强制 PostgreSQL 18 版本门禁，执行独立 baseline，并使用 MessagePack `bytea` 的 PostgreSQL session store。业务模块仍需逐步把现有 SQLite 查询和写入协调器切换到该 runtime。

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
OCTORILL_PG_POOL_MAX_CONNECTIONS=8 \
DATABASE_URL=postgres://user:password@127.0.0.1:5432/octo_rill \
cargo run --bin octo-rill
```

该入口只执行 PostgreSQL 18 版本检查、baseline 和 Session schema 初始化，然后正常退出。

生产副本演练在 Agent VM 内执行。生产 secrets、上游凭据和真实服务网络不作为迁移输入；只复制经授权的 SQLite 快照及脱敏配置。Compose 使用 `compose.postgres.yml`：PostgreSQL 18 健康后先运行一次性 migration 容器，migration 成功后才允许 server 容器启动。

演练命令使用固定会话 ID，避免误用其他 VM：

```sh
export AGENT_VM_SESSION_ID=octo-rill-pg18
AGENT_VM_SCRIPT=/Users/ivan/.agents/resources/skills/vendor/shared/IvanLi-CN/agent-vm/skills/agent-vm/scripts/agent_vm.ts
bun "$AGENT_VM_SCRIPT" doctor
bun "$AGENT_VM_SCRIPT" status
bun "$AGENT_VM_SCRIPT" acquire
bun "$AGENT_VM_SCRIPT" sync --name octo-rill-pg18-migration
bun "$AGENT_VM_SCRIPT" exec -- sh -lc 'cd /workspace/worktrees/octo-rill-pg18-migration && cargo test --bin octo-rill-migrate'
```

本次执行记录：`doctor` 返回 `maintenance_unhealthy`，`status` 返回 `effective_lease=false`，因此 VM 演练状态为 `not_run`，没有伪造通过证据。

当前应用业务模块仍以 SQLite 类型为编译时边界；在完成逐模块 PostgreSQL runtime 改造前，不应把该 Compose 文件用于线上切换。线上停机、旧容器停止、数据卷切换和回滚均需要另行授权。
