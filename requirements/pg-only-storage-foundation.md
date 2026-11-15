# PostgreSQL-only 存储基础

应用运行时使用单一 PostgreSQL 连接池、事务边界和 Session 存储。SQLite 只属于离线导入器与测试夹具，不能由运行时连接配置启用。

PostgreSQL schema migration 使用 `migrations_pg` 的独立 SQLx 历史，并在运行时连接建立后执行。离线导入器负责从 SQLite 快照预检、导入、校验和重建投影；在线 migration 模块属于旧 SQLite 服务边界，不得被 PostgreSQL runtime 调用。

验收证据包括 PostgreSQL URL 运行时门禁、独立迁移目录与 advisory transaction lock、PostgreSQL Session store 配置、离线导入器的 SQLite source/PostgreSQL target 校验，以及针对性 Rust 测试。
