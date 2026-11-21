# PostgreSQL 迁移需求清单

## 验收边界

彻底迁移运行时持久化至 PostgreSQL；SQLite 仅保留为离线导入源。保持现有接口、鉴权、响应字段和任务语义，禁止用 501、空结果或简化业务流程充当迁移完成。生产切换不在本次授权内；当前任务只读取生产源，创建、转移并清理临时备份，在同一 Agent VM 验证。

## 批次

| 编号 | 文件与函数 | 需求 | 批内自动化验证 |
| --- | --- | --- | --- |
| B01 | `src/bin/octo-rill-migrate.rs::{preflight,import,verify,copy_table_bulk,translate_ddl,translate_index_ddl,skipped_source_table_reason}` | 一致性快照、COPY 导入、类型与约束保真、完整性校验、失败重入、迁移耗时 | Seed SQLite → PG，异常中断与重入，逐表数据对比 |
| B02 | `src/postgres_runtime.rs::{connect,verify_application_schema,verify_session_store}`、`migrations_pg/0015_application_tables.sql`、`src/main.rs::main`、`src/config.rs::AppConfig::from_env` | 新库完整建表与升级（含 59 张业务表与 3 个视图）、会话兼容、PG 唯一生产入口（SQLite URL 必须失败关闭） | `pg_seed_fresh_schema_test.py`：空库启动、表/视图齐全、4 个读接口；SQLite URL 拒绝 |
| B03 | `src/postgres_server.rs`、`src/api.rs`、`src/auth.rs`、`src/passkeys.rs`、`src/api_keys.rs` | 全部 API 原有语义与鉴权迁移，消除能力占位 | Seed PG，真实 HTTP 与响应断言 |
| B04 | `src/postgres_tasks.rs::*Executor::execute`、`src/postgres_jobs.rs`、`src/postgres_worker.rs`、`src/postgres_scheduler.rs`、`src/sync.rs`、`src/webhook_push.rs` | 同步、周期调度（七个通道）、锁租约、幂等、取消重试及外部调用语义保真 | `pg_seed_scheduler_test.py`（到期/单飞/幂等）+ Seed PG + 本地上游 mock，不访问生产外部服务 |
| B05 | `src/content_processing.rs`、`src/translations.rs`、`src/briefs.rs`、`src/ai.rs`、`src/admin_runtime.rs`、`src/admin_ai_records.rs` | 翻译、内容处理、日报、审计及运行配置完整迁移 | Seed PG，任务生命周期及持久化结果断言 |
| B06 | `src/search.rs`、`src/search_index.rs`、`src/release_links.rs`、`src/public_metrics.rs`、`src/metrics.rs`、`src/online_migrations.rs`、`src/content_identity_upgrade.rs` | 搜索投影与更新、指标、在线迁移及历史状态保真 | Seed PG，搜索隔离、更新删除、指标与重入 |
| B07 | `src/main.rs`、`Cargo.toml`、`src/state.rs`、`src/session_store.rs`、`src/sqlite_write.rs`、部署示例与文档 | 清除运行时 SQLite 依赖与配置，离线 importer 保留独立读取能力 | 全量测试、依赖审计、PG 启动、SQLite 启动被拒 |
| B08 | `scripts/pg_read_acceptance.py`、`scripts/pg_read_fixtures.py`、`scripts/pg_write_acceptance.py` | VM 导入生产快照，冻结后台副作用，覆盖读契约与**可逆、无外部依赖**的写路径 | 普通用户/管理员/匿名；写探测逐项还原并断言无任务进入 `running` |
| B09 | `src/postgres_repo_releases.rs`、`src/postgres_reactions.rs`、`src/postgres_dashboard.rs` | 仓库/公开发布读与写、reaction token 与 reaction 写、feed 与 dashboard 全部读接口 | Seed PG：`pg_seed_repo_releases_test.py`、`pg_seed_reactions_test.py`、`pg_seed_dashboard_test.py` |
| B10 | `src/postgres_admin_actions.rs`、`src/postgres_auth_ext.rs` | 管理端行为接口（迁移控制、内容处理、搜索索引、运行配置、审计访问）与 passkey/OAuth 入口 | Seed PG：`pg_seed_admin_actions_test.py`、`pg_seed_auth_ext_test.py` |
| B11 | `src/postgres_translate.rs`、`src/postgres_webhook.rs` | 翻译批量/流式/通知接口与 GitHub webhook 接收（签名、幂等、入队） | Seed PG：`pg_seed_translate_test.py`、`pg_seed_webhook_test.py` |
| B12 | `src/github_api.rs`、`scripts/pg_seed_common.py`、`scripts/pg_seed_suite.sh` | 上游 GitHub 端点可替换（测试不访问真实网络）、批内 Seed 套件统一入口 | 套件脚本可独立运行并逐套件报告 |
| B13 | `migrations_pg/0014_public_text_search_configuration.sql`、`src/search.rs::query_postgres` | 新库与导入库都必须解析全文检索配置 | Seed PG 搜索查询 + 生产副本读探测 |
| B14 | `src/postgres_dashboard.rs::{load_dashboard_readable_brief_dates,load_dashboard_readable_briefs_for_dates,load_dashboard_brief_signatures}` | 日报窗口时间在新库（timestamptz）与导入库（text）上都可读 | Seed PG + 生产副本 `/api/dashboard/feed`、`/api/dashboard/updates` |

## 已消除的 PG 能力占位

历史清单中的 46 条 `postgres_capability_unavailable` 路由已全部替换为真实实现；`src/postgres_server.rs` 不再引用该占位处理器。新增批次 B09–B11 覆盖以下此前占位的接口族：

- 仓库与公开发布：`/api/repos/{owner}/{repo}/releases/tag/{tag}/detail`、`/discussions/{number}/detail`、`/public-release`（GET/POST/DELETE）、`/api/public/repos/{owner}/{repo}/releases`（含 `tag`、`content`）。
- Dashboard 与 feed：`/api/dashboard/updates`、`/api/dashboard/feed`、`/api/dashboard/feed/sections/{section_id}/items`、`/api/feed`（GET/HEAD）。
- Reaction：`/api/reaction-token/check`、`/api/reaction-token`（PUT）、`/api/release/reactions/toggle`、`/api/feed/reactions/refresh`。
- 管理端：`/api/admin/public-release-repos/{usage_id}`、`/api/admin/jobs/release-reading/backfill`、`/api/admin/jobs/llm/runtime-config`、`/api/admin/jobs/llm/calls/{call_id}/diagnostic-access`、`/api/admin/jobs/migrations/{migration_id}/pause|resume`、`/api/admin/jobs/content-processing/cutover|freeze`、`/api/admin/jobs/search-index/resume`、`/api/admin/users/{user_id}/profile`（PATCH）、`/api/admin/jobs/translations/runtime-config`。
- 认证：4 个 passkey 端点、`/auth/github/connect`、`/auth/linuxdo/login|callback`；`/auth/github/callback` 的 connect 模式改走 `finalize_github_auth`。
- 翻译：`/translate/releases/batch`（含 stream）、`/translate/release`（含 stream）、`/api/translate/release/detail`（含 batch）、`/api/translate/notifications/batch`、`/api/translate/notification`。
- Webhook：`/api/webhooks/github/releases`。

| B15 | `src/postgres_content_overlay.rs`、`src/postgres_dashboard.rs`、`src/postgres_repo_releases.rs` | `mode='global'` 时按全局投影（`content_current_result_projections`）提供译文，legacy 模式不变 | `pg_seed_global_overlay_test.py` + `scripts/pg_content_parity.py` 抽样生产副本 |
| B16 | `src/postgres_star_epochs.rs`、`src/postgres_tasks.rs::StarredExecutor` | Star 同步按 epoch 切片推进（分页预算、到期续跑、失败重试、成员差异） | `pg_seed_star_epochs_test.py`（stub 分页） |
| B17 | `src/postgres_repo_release_worker.rs`、`src/postgres_webhook.rs` | 单仓库 release 刷新执行器 + webhook 入队对齐（不再整账号扫描） | 扩展 `pg_seed_webhook_test.py` + worker 持久化断言 |
| B18 | `src/postgres_translate.rs`、`src/postgres_tasks.rs::TranslationExecutor` | 异步翻译写入 `llm_calls`/`llm_call_events`/`translation_attempt_llm_calls` | 扩展 `pg_seed_translate_test.py` 的 task_id 场景 |

| B19 | `src/postgres_repo_release_worker.rs`、`src/postgres_tasks.rs::ReleasesExecutor` | 单仓刷新候选用户轮换 + 公共仓库匿名 fallback（生产有 11 个公共仓库无任何用户 token 覆盖） | stub GitHub：候选轮换、匿名成功、私有失败分类、usage 状态流转 |
| B20 | `src/postgres_scheduler.rs`（star 通道）、`src/postgres_star_epochs.rs` | epoch 到期即入队，而不是只按 delta 间隔 | 调度器套件：到期入队、未到期不入队、已有排队任务不重复 |
| B21 | `src/postgres_content_overlay.rs`、`src/postgres_repo_releases.rs`、`src/postgres_server.rs` | 译文载荷补齐 `request_id` / `auto_translate`（legacy `TranslatedItem` 字段） | global 与 legacy 两套套件断言 |
| B22 | `src/postgres_translate.rs::translate_work_item_for_task` | 异步翻译 prompt 使用持久化 `source_snapshot_json` 块，而非实时内容 | 翻译套件：快照文本进入 prompt + 缺失快照回退 |

## 仍待完成（本轮未通过验收的部分）

- **基线漂移检测**：`0015` 由导入副本的 `pg_dump` 生成，目前靠 `pg_seed_fresh_schema_test.py` 校验表/视图齐全；尚未与 SQLite 迁移历史做自动漂移比对。
- **全局内容处理语义**：dashboard/repo-release 读路径在 `content_processing_control.mode='global'` 时仍读本地投影；PG 运行时目前无法进入 global 模式。
- **翻译异步执行审计**：`return_mode=task_id` 由 `postgres_tasks::TranslationExecutor` 执行时未写 `llm_calls` 审计（同步/SSE/流式路径已审计）。
- **Webhook 入队形态**：PG 复用 `sync.releases` 全量扫描任务，而非旧实现的单仓库 release 刷新任务。
- **旧 SQLite 运行时代码清理**：`src/api.rs`、`src/server.rs` 等仍编译（供离线 importer 与历史测试参考），但运行时已强制 PostgreSQL；彻底删除属于后续重构。
- **写路径覆盖边界**：生产副本写探测只覆盖 13 个 DB-only、可逆变更；reaction/translate/public-release/webhook 等 24 个外部依赖写接口按 owner 要求跳过，由 Seed PG + 本地 mock 覆盖。
- **切换演练**：见 `cutover-plan.md`；增量重放已失败关闭，切换必须导入新库。
- **Interactive OAuth 全链路**：`/auth/github/*`、`/auth/linuxdo/*`、`/api/auth/bind-context` 需真实第三方凭据，自动探测中保持 `not_run`。

## 证据规则

需求清单与完成清单分离；只有批内 Seed PG 自动化验证通过才在完成清单标记完成。生产数据与凭据只放受控临时目录或租赁 VM，不进入 Git。

## 本轮具体函数修复记录范围

- `src/bin/octo-rill-migrate.rs::{import,verify,copy_table,copy_table_bulk,canonical_source_checksum,skipped_source_table_reason}`：导入去重扫描与逐表耗时、跳过 SQLite `_sqlx_migrations`、跨库规范化校验复用同一次源扫描。
- `src/postgres_server.rs::{postgres_worker_enabled,reaction_token_status,me_profile,admin_user_profile,personal_repos,admin_translation_requests,admin_translation_batches,admin_translation_request_detail,admin_translation_status,admin_translation_attempt_events,admin_migrations,admin_webhook_runtime_config,admin_public_release_repos,admin_search_index_status,admin_identity_upgrade_status}`：禁用副作用、配置位置、读接口与查询成本、`protocol_version` 关联。
- `src/main.rs::main`：生产入口固定为 `postgres_server::serve_postgres`，SQLite `DATABASE_URL` 失败关闭。
- `src/search.rs::query_postgres`：关键词括号与 `public.octo_rill_simple` 配置引用。
- `migrations_pg/0010_application_schema_placement.sql`、`0011_translation_retry_compatibility.sql`、`0012_translation_attempt_read_compatibility.sql`、`0014_public_text_search_configuration.sql`：新库向前修复。
