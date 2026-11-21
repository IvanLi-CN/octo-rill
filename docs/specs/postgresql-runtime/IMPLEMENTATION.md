# PostgreSQL 运行时持久化 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: 运行时已固定 PostgreSQL；Seed 套件 15 套件全绿，主二进制单元测试 1274 通过。
- Lifecycle: active
- Catalog note: 代码与验证已完成；生产切换待运维窗口与授权。

## Implementation Coverage

- `REQ-PGR-001`: `src/main.rs` 固定 `postgres_server::serve_postgres`，SQLite URL 失败关闭。
- `REQ-PGR-002`: `src/bin/octo-rill-migrate.rs`（`preflight`/`import`/`verify`/`rebuild`），逐表行数与摘要校验，删除/内容变化失败关闭。
- `REQ-PGR-003`: `migrations_pg/0010`–`0017`；`0015_application_tables.sql` 提供 59 表 + 3 视图，`0016_normalize_flag_columns.sql` 归一导入库的布尔列。
- `REQ-PGR-004`: `src/postgres_jobs.rs`、`src/postgres_worker.rs`、`src/postgres_scheduler.rs`、`src/postgres_runtime_owner.rs`。
- `REQ-PGR-005`: `src/postgres_repo_release_fetch.rs`（共享判定）、`src/postgres_repo_release_worker.rs`（单仓任务）、`src/postgres_tasks.rs::ReleasesExecutor`（账号级有界并发）。
- `REQ-PGR-006`: `src/postgres_content_overlay.rs`。
- `REQ-PGR-007`: `仓库外运维记录`。

## Coverage / rollout summary

- Seed 入口：`PG_ADMIN_URL=… PG_SEED_SCHEMA_SOURCE=… BINARY_DIR=target/debug bash scripts/pg_seed_suite.sh`，15 套件（import、worker、llm_admin、admin_actions、auth_ext、dashboard、reactions、repo_releases、translate、webhook、scheduler、star_epochs、global_overlay、fresh_schema、repo_release_fallback）。
- 生产副本验收：读探测 75 通过 / 2 环境限制 / 7 未运行（OAuth）；写探测 14 项全部还原；内容投影 30/30 `ready`。
- 生产副本限制：缺少生产加密密钥与任务日志文件，OAuth 端点无法在副本内执行。

## Remaining Gaps

- 生产凭据冒烟（GitHub 登录、`github_connections` 解密、真实 LLM 调用）只能在切换后执行。
- 基线漂移自动比对（`0015` 与 SQLite 迁移历史）尚未实现。
- 旧 SQLite 模块尚未删除。
- 回滚产物的 `REINDEX`（issue #381）待授权；匿名读取限流（issue #384）待实现。

## Related Changes

- 见 `./HISTORY.md`。

## References

- `./SPEC.md`
- `./HISTORY.md`
