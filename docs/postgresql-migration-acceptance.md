# PostgreSQL 18 迁移验收证据

本文记录当前分支的可复核证据，不把未执行的线上切换或未完成的业务迁移标记为通过。

| 门槛 | 当前证据 | 状态 |
| --- | --- | --- |
| PostgreSQL 18 版本门禁 | `src/postgres_runtime.rs` 检查 `server_version_num >= 180000`；对应单元测试通过 | 已证明 |
| Rust 全量回归 | `cargo test --locked --quiet`：主二进制 1249 passed、3 ignored；迁移器 8 passed | 已证明 |
| 独立 SQLx migration history | `migrations_pg/` 与 `postgres_database_migrations` 测试 | 已证明 |
| Session store | PostgreSQL `tower_sessions` store、MessagePack `bytea` 校验和 expiry 校验 | 已证明 |
| 任务认领与终结事务 | `FOR UPDATE SKIP LOCKED`、条件 `UPDATE ... RETURNING`、owner lease 校验的终结更新、`task.running`/`task.completed` 事件和 `40001`/`40P01` 重试测试 | 已证明 |
| SQLite 离线导入器 | `octo-rill-migrate` 的 snapshot/import/verify/rebuild 实现；8 项迁移器测试通过，并在导入前检查 `users`、`github_connections`、`job_tasks` 核心权威表；追加型 `job_task_events` 允许快照后保留运行时事件 | 已证明 |
| Compose 启动门禁 | migration `service_completed_successfully` 后才启动 server；必需配置变量缺失时拒绝展开 | 已证明 |
| PostgreSQL service boundary | health、session、账户、搜索、feed、brief、任务、webhook 设置与 reconcile 入队、LinuxDo 解绑、passkey 删除和部分管理员路由（包括 dashboard 汇总、任务事件、用户列表、用户详情、profile 读取与受保护的用户状态更新）已接入 | 部分证明 |
| 完整业务 worker | 同步和日报任务目前只入 PostgreSQL 队列，尚未由 PG-native worker 执行 | 未完成 |
| 全量业务路由 | 管理员写操作、webhook、LinuxDo/passkey 写流程及其他 SQLite 路由仍未全部迁移 | 未完成 |
| Agent VM 生产副本演练 | 当前 HEAD 的 Linux x86_64 二进制在 Agent VM 的 PostgreSQL 18.7 上完成动态容量门禁、import、verify、rebuild、重复 import/verify、bootstrap task claim→finalize 及运行时事件追加校验；演练后容器与 VM 已清理 | 已证明 |
| PR 收口 | 当前分支尚未推送或创建远端 PR | 未完成 |

## 可重复检查

```sh
cargo check --all-targets --locked
cargo test --locked postgres_ -- --nocapture
cargo test --locked --bin octo-rill-migrate -- --nocapture
```

提交前还会执行仓库现有 lefthook；Biome 当前仅报告仓库既有 schema 版本提示和两个 warning，不修改无关前端文件。
