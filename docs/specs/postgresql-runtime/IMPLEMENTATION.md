# PostgreSQL 运行时持久化 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: 运行时已固定 PostgreSQL；Seed 套件 15 套件全绿，主二进制单元测试 1274 通过。
- Lifecycle: active
- Catalog note: 代码与验证已完成；生产切换待运维窗口与授权。

## Implementation Coverage

- `REQ-PGR-001`: `src/main.rs` 固定 `postgres_server::serve_postgres`，SQLite URL 失败关闭。
- `REQ-PGR-002`: `src/bin/octo-rill-migrate.rs`（`preflight`/`import`/`verify`/`rebuild`），逐表行数与摘要校验，删除/内容变化失败关闭。
- Release 源修订修复：`src/bin/octo-rill-migrate.rs`（`repair-release-revisions --repo-id N` 或 `--release-id N`，默认 dry-run），从 GitHub 强制覆盖写回内容与 `updated_at`，绕过 revision guard；仅用于修复被表情计数刷新等非内容写入推进的修订号。
- `REQ-PGR-003`: `migrations_pg/0010`–`0017`；`0015_application_tables.sql` 提供 59 表 + 3 视图，`0016_normalize_flag_columns.sql` 归一导入库的布尔列。
- `REQ-PGR-004`: `src/postgres_jobs.rs`、`src/postgres_worker.rs`、`src/postgres_scheduler.rs`、`src/postgres_runtime_owner.rs`。
- `REQ-PGR-005`: `src/postgres_repo_release_fetch.rs`（共享判定、`RefreshFailure::retries_request` 与 `request_page` 有界重试、单页 50 条与 20 页预算恰好覆盖 1000 条结果上限、422 结果上限 `pagination_limit` 正常终止）、`src/postgres_repo_release_worker.rs`（单仓任务）、`src/postgres_tasks.rs::ReleasesExecutor`（账号级有界并发、`summarize_release_run` 聚合摘要与单条告警）。
- `REQ-PGR-006`: `src/postgres_content_overlay.rs`。
- `REQ-PGR-007`: `仓库外运维记录`。
- `REQ-PGR-008`: `src/postgres_repo_release_fetch.rs::AnonymousBudget`（进程级共享预算，两个执行器共用 `shared_anonymous_budget()`）、`BudgetDeferral`/`ReleaseFetchError::Deferred`、`src/metrics.rs` 的 `anonymous_github_budget_remaining` 与 `anonymous_github_deferrals`。
- `REQ-PGR-009`: `src/postgres_translate.rs::enqueue_release_translations`（自动排队、去重与单飞判定）、`enqueue_pending_translation_tasks`（每轮最多 25 个：先到期自动重试，再补积压）、`automatic_retry_delay`（10 分钟/1 小时退避，最多 3 次尝试）与 `finish_request` 的 `next_retry_at` 调度；调用方为 `src/postgres_tasks.rs::ReleasesExecutor` 与 `src/postgres_repo_release_worker.rs::RepoReleaseRefreshExecutor`，两者仅在配置 AI provider 时排队并在每次刷新后排空一节积压。数据库回归为 `postgres_backlog_drains_every_release_across_bounded_cycles` 与 `postgres_backlog_retries_only_due_recoverable_failures`（`TEST_PG_TRANSLATE_URL`）。

## Coverage / rollout summary

- Seed 入口：`PG_ADMIN_URL=… PG_SEED_SCHEMA_SOURCE=… BINARY_DIR=target/debug bash scripts/pg_seed_suite.sh`，18 套件（含 `repo_release_fallback`、`repo_release_anon_budget`；`import` 需要 `PG_SEED_SCHEMA_SOURCE`，缺失时报告为 skipped）。
- 生产副本验收：读探测 75 通过 / 2 环境限制 / 7 未运行（OAuth）；写探测 14 项全部还原；内容投影 30/30 `ready`。
- 生产副本限制：缺少生产加密密钥与任务日志文件，OAuth 端点无法在副本内执行。

## Remaining Gaps

- 生产凭据冒烟（GitHub 登录、`github_connections` 解密、真实 LLM 调用）只能在切换后执行。
- 基线漂移自动比对（`0015` 与 SQLite 迁移历史）尚未实现。
- 旧 SQLite 模块尚未删除。
- 回滚产物的 `REINDEX`（issue #381）待授权。

## Related Changes

- 见 `./HISTORY.md`。

## References

- `./SPEC.md`
- `./HISTORY.md`
