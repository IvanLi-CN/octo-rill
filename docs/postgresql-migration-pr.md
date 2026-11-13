# PostgreSQL 18 迁移

## 问题

OctoRill 的运行时仍以 SQLite 为主，无法在停机迁移后直接使用 PostgreSQL 18，也缺少可重试、可校验和可审计的离线导入边界。

## 实现

- 增加独立的 PostgreSQL SQLx migration history、PG 18 版本门禁、Session store、runtime owner 和任务认领事务。
- 增加项目内 Rust `octo-rill-migrate`，支持动态容量预检、SQLite 快照导入、幂等重跑、manifest/checksum、verify 和搜索投影 rebuild。
- 增加 PG service boundary，迁移核心账户、搜索、feed、brief、任务、指标和部分管理员读写路径。
- 增加 Compose 中 PostgreSQL、一次性 migration 和 server 的启动依赖及必需配置门禁。
- 保留 SQLite 原卷只读迁移输入，并在导入前检查核心权威表存在。

## 验证

- `cargo check --all-targets --locked`
- `cargo test --locked postgres_ -- --nocapture`
- `cargo test --locked --bin octo-rill-migrate -- --nocapture`
- `cargo test --locked --quiet`：主二进制 1249 passed、3 ignored，迁移器 7 passed
- Compose YAML 依赖和必需环境变量契约检查
- Linux x86_64 目标构建
- Agent VM PostgreSQL 18.7 演练：容量门禁、import、verify、rebuild、重复导入、task claim、health、metrics 和未认证保护

## 当前发布边界

本分支尚未声明线上切换完成。PostgreSQL 原生 worker 执行链和部分管理员/webhook 业务路由仍需完成后，才满足完整 PR 验收和线上切换前置条件。
