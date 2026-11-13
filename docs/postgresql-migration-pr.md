# PostgreSQL 18 迁移

## 问题

OctoRill 的运行时仍以 SQLite 为主，无法在停机迁移后直接使用 PostgreSQL 18，也缺少可重试、可校验和可审计的离线导入边界。

## 实现

- 增加独立的 PostgreSQL SQLx migration history、PG 18 版本门禁、Session store、runtime owner，以及带 owner lease 校验和终结事件的任务协调事务。
- 增加项目内 Rust `octo-rill-migrate`，支持动态容量预检、SQLite 快照导入、幂等重跑、manifest/checksum、verify 和搜索投影 rebuild。
- 增加 PG service boundary，迁移核心账户、搜索、feed、brief、任务、指标、webhook reconcile 入队、管理员任务详情/取消/重试、日报调度槽读写，以及 LinuxDo/passkey 解绑和部分管理员读写路径。
- 增加 Compose 中 PostgreSQL、一次性 migration 和 server 的启动依赖及必需配置门禁。
- 保留 SQLite 原卷只读迁移输入，并在导入前检查核心权威表存在。
- 增加统一的 PostgreSQL task lifecycle runner、按注册类型过滤和显式 task-type registry；`sync.notifications` 与 `sync.releases` 已接入 GitHub REST 与 PostgreSQL upsert，服务 worker 可受配置启停；日报、webhook 等业务 executor 仍需接入。

## 验证

- `cargo check --all-targets --locked`
- `cargo test --locked postgres_ -- --nocapture`
- `cargo test --locked --bin octo-rill-migrate -- --nocapture`
- `RUST_TEST_THREADS=4 cargo test --locked --quiet`：主二进制 1254 passed、3 ignored，迁移器 8 passed
- Compose YAML 依赖和必需环境变量契约检查
- Linux x86_64 目标构建
- Agent VM PostgreSQL 18.7 演练：容量门禁、import、verify、rebuild、重复导入、task claim→finalize，以及 `sync.releases` 零变更任务由 worker 成功终结

## 当前发布边界

本分支尚未声明线上切换完成。PostgreSQL 原生 worker 执行链和部分管理员/webhook 业务路由仍需完成后，才满足完整 PR 验收和线上切换前置条件。
