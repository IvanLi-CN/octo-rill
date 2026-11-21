# PostgreSQL 迁移完成清单

数据与验证均在租约 Agent VM（PostgreSQL 18.7）上完成；生产环境未切换，未读取或复制生产凭据。

## 批次状态

| 批次 | 状态 | 证据 |
| --- | --- | --- |
| B01 | 通过（批内 Seed + 生产副本） | 合成 Seed SQLite→PG：`pg_seed_import_test.py` 5 项通过（import/verify/可见性视图/重入/同数量篡改拒绝）；生产快照导入 90 张业务表 461 秒（上一版 1500 秒，去掉重复扫描后约 3.2×）；`_sqlx_migrations` 不再导入，避免 PG migrator 读到 SQLite 版本历史而中止 |
| B02 | 通过（全新空库可独立启动） | `migrations_pg/0015_application_tables.sql` 补齐 59 张业务表与 3 个视图；`pg_seed_fresh_schema_test.py` 证明空库 bootstrap 后 `/health`、`/api/me`、`/api/feed`、`/api/dashboard/feed`、`/api/releases/{id}/detail`、`/api/admin/jobs/overview` 全部 200 且表/视图齐全；`DATABASE_URL=sqlite:...` 启动返回 `application runtime requires a PostgreSQL DATABASE_URL` |
| B03 | 通过（Seed PG18 HTTP 断言） | 11 个 Seed 套件共 84 项断言全绿，覆盖读接口语义、鉴权与错误分类（见下表） |
| B04 | 通过（调度器 + 执行器） | 七个周期通道迁入 `src/postgres_scheduler.rs`（日报小时槽、订阅同步、Star 同步、失败重试、webhook push、账号暂停、管理端汇总），均为入队/状态写入且带 durable 单飞（bucket claim + 部分唯一索引）；`pg_seed_scheduler_test.py` 覆盖到期/未到期/重复 tick/启用-禁用小时槽/暂停维护/汇总幂等；live smoke 启动 12 秒无 dispatch 失败、无 ERROR。遗留：webhook 入队复用全量 `sync.releases`，翻译异步执行器仍未写 `llm_calls` |
| B05 | 通过（Seed PG18） | 翻译批量/详情/通知/流式、审计关联、管理端翻译读取全部通过；内容处理 cutover/freeze 与身份升级控制通过 |
| B06 | 通过（Seed + 生产副本） | 搜索关键词、可见性隔离、索引状态恢复通过；新库全文检索配置改为 `public.octo_rill_simple`（0014） |
| B07 | 部分通过 | 运行时入口固定为 PostgreSQL，SQLite URL 失败关闭；旧 SQLite 模块仍编译（2035 条 dead-code 警告），彻底删除属后续重构 |
| B08 | 读+写探测完成 | 读：75 通过 / 2 失败 / 7 未运行（失败=缺生产加密密钥、任务日志文件不在快照）；写：13 项可逆变更 14/14 通过、24 条外部依赖写接口显式跳过、22 条越界写接口跳过、全部还原（`restored: true`），并断言期间无任务进入 `running` |
| B09–B14 | 通过（Seed PG18） | 见下表逐套件结果；46 条能力占位路由全部替换 |
| B15 | 通过（Seed + 生产副本抽样） | `src/postgres_content_overlay.rs`：`mode='global'` 时按 `content_current_result_projections`（含 `active_work_item_id` 保留投影与 upgrade 前回退）提供译文，覆盖 feed/sections/release detail/announcement detail/public content；`pg_seed_global_overlay_test.py` 7/7；生产副本抽样 `pg_content_parity.py` **30/30 全部 ready**，含 8 个此前因 `unknown/<repo_id>` 写入身份缺失的 Astro 预发布——修复点是从 `html_url` 重建写入身份（斜杠 tag 导致 URL 六段，解析失败回退 `unknown/{repo_id}`），与 legacy 读侧一致 |
| B16 | 通过（Seed PG18） | `src/postgres_star_epochs.rs` + `StarredExecutor` 切片：`pg_seed_star_epochs_test.py` 5/5（单页推进、到期门禁、完成、失败重试、成员差异） |
| B17 | 通过（Seed PG18） | `src/postgres_repo_release_worker.rs`（`sync.repo_release`）与 webhook 入队对齐：`pg_seed_webhook_test.py` 10/10（契约、单飞、worker 持久化、条件请求 304、失败分类） |
| B18 | 通过（Seed PG18） | 异步翻译写入 `llm_calls`/`llm_call_events`/`translation_attempt_llm_calls`：`pg_seed_translate_test.py` 11/11，含 `task_id_llm_audit`、`task_id_persisted_prompt` |
| B19 | 通过（Seed PG18） | 新增 `src/postgres_repo_release_fetch.rs`：候选用户轮换 + 公共仓库匿名 fallback，`ReleasesExecutor` 与单仓 worker 共用；`pg_seed_repo_release_fallback_test.py` 6/6。生产影响已量化：84 个 `github_public` 仓库中 11 个没有任何用户 token 可读 |
| B23 | 通过（Seed PG18） | 账户级刷新按 `admin_runtime_settings.repo_release_worker_concurrency` 并发（凭据缓存改为可共享），替代串行遍历；生产最大单账户 9,040 仓，串行会把一轮刷新拖到 45 分钟以上，超过 10 分钟周期。套件断言在途请求上界 4、观测值 2–4 |
| B24 | 通过（Seed PG18） | 评审修复：webhook reconcile demand 完成后确认 generation 并推进 `webhook_push_last_completed_check_at`；refresh 遵守 `backoff_until`；候选查询排除已取关用户；非凭据类失败后仍尝试匿名 fallback；空库补翻译重试列与日报唯一键；导入 `verify` 先比行数再比摘要以保留 append-only 语义 |
| B20 | 通过（Seed PG18） | star 通道新增 due-epoch 探测（`load_due_epoch_user_ids` + 单飞去重），`pg_seed_scheduler_test.py` 覆盖到期/未到期/已有排队任务/暂停用户 |
| B21 | 通过（Seed + 生产副本） | 译文载荷补齐 `request_id` / `auto_translate`（legacy `TranslatedItem` 语义），global 与 legacy 两套套件断言 |
| B22 | 通过（Seed PG18） | 异步翻译 prompt 改用持久化 `source_snapshot_json` 块（缺失时回退实时内容并告警一次） |

## Seed PG18 套件（全部在 VM 上运行）

| 套件 | 断言组 | 结果 |
| --- | --- | --- |
| `pg_seed_import_test.py` | synthetic_import / verify / live_visibility_views / idempotent_reentry / same_count_corruption_rejected | 5/5 |
| `pg_seed_worker_test.py` | seeded_pg / health / authenticated_me / anonymous_me / disabled_worker_queue_unchanged / reaction_token_status / profile_runtime_schedule / search_text_query / personal_repo_release_count / admin_translation_request_list | 10/10 |
| `pg_seed_llm_admin_test.py` | calls_filters_and_auth / detail_and_global_fallback / activity_50_buckets / status_aggregates | 4/4 |
| `pg_seed_admin_actions_test.py` | route_authentication / public_repo_delete_and_cleanup / release_reading_backfill / llm_runtime_config / diagnostic_access_audit / online_migration_control / content_processing_transitions / identity_upgrade_control / search_index_resume / admin_user_profile / translation_runtime_config | 11/11 |
| `pg_seed_auth_ext_test.py` | passkey_options_and_rejections / authenticated_passkey_options / github_connect_redirect / linuxdo_login_and_state_rejection / linuxdo_unconfigured_behavior / persistence | 6/6 |
| `pg_seed_dashboard_test.py` | feed_projection / feed_cursor_pagination / head_feed_warmup / dashboard_sections / section_items_cursor / brief_visibility_and_attachment / time_zone_day_boundary / updates_token_stability / updates_change_detection / anonymous_401 | 10/10 |
| `pg_seed_reactions_test.py` | auth_boundaries / reaction_token_check / reaction_token_upsert / release_reaction_toggle / feed_reaction_refresh / api_key_auth / pat_invalid_persistence | 7/7 |
| `pg_seed_repo_releases_test.py` | release_detail / announcement_detail / public_list_pagination_and_highlight / public_detail_and_content / public_release_lifecycle / pending_sync | 6/6 |
| `pg_seed_translate_test.py` | disabled_payloads / request_validation / sync_release_and_cache / failure_classification / notification_paths / task_id_persistence / task_id_llm_audit / sse_event_order / batch_stream_ndjson / llm_audit_visibility | 10/10 |
| `pg_seed_webhook_test.py` | 签名/未知 key/payload/ignored/幂等 + `repo_release_task_contract`、`repo_release_single_flight`、`repo_release_worker_persistence`、`repo_release_conditional_refresh`、`repo_release_failure_classification` | 10/10 |
| `pg_seed_scheduler_test.py` | 七个通道的到期入队、单飞、禁用条件、暂停维护与汇总幂等 | 全绿 |
| `pg_seed_global_overlay_test.py` | global/legacy 切换、feed/sections/detail/public content 投影来源、source_hash 失配回退、无投影保持 missing | 7/7 |
| `pg_seed_star_epochs_test.py` | 单页推进 / 到期门禁 / 完成 / 失败重试 / 成员差异 | 5/5 |
| `pg_seed_fresh_schema_test.py` | 空库建表/视图齐全 + 6 个读接口 | 6/6 |

另有 `pg_seed_fresh_schema_test.py`：全新空库（无 SQLite 导入）bootstrap 后表/视图齐全，`/health`、`/api/me`、`/api/feed`、`/api/dashboard/feed`、`/api/releases/{id}/detail`、`/api/admin/jobs/overview` 全部 200。

统一入口：`scripts/pg_seed_suite.sh`（`PG_ADMIN_URL`、`PG_SEED_SCHEMA_SOURCE`、`BINARY_DIR`、`PORT_BASE`）。最后一次完整运行：**14 套件通过、0 跳过、0 失败**。

## 生产副本数据等价

- 快照：`VACUUM INTO` 于生产库只读连接，119 秒生成 7.39 GB 紧凑副本（源库 10.48 GB），`PRAGMA quick_check=ok`；`sha256=d37bc6e0c2a69a1b8ef2c8a610ee56c16076a819d5cda63b5e0ca346478bd9a8`，两端一致后删除服务器临时文件。
- 导入：`preflight` 报告 103 张权威表、源 7.39 GB，导入 461 秒（COPY + 会话级 `synchronous_commit=off`、`maintenance_work_mem=1GB`、`jit=off`）。
- 逐表规范化校验：在 `octo_rill_check` 纯净副本上重跑 `import && verify`，**90 张表全部通过**（`verification passed for 90 tables`），pipeline 退出码 0，导入+校验合计 1783 秒。
- 导入库上的 `public_repo_release_usage` 校验差异已定位为**读探测导致的计数漂移**：全列比对仅 `api_*`/`page_*` 计数、`last_*` 时间与 2 行新增 usage 行不同，其余列一致。

## 生产副本读探测（B08）

对导入的生产库启动服务（`OCTORILL_PG_WORKER_ENABLED=0`，真实管理员会话），覆盖 `src/postgres_server.rs` 全部 GET 路由：

- **75 通过**、**2 失败**、**7 未运行**。
- 失败 1：`/api/repos/{owner}/{repo}/discussions/{number}/detail` → 500，原因是 VM 使用占位加密密钥，无法解密生产 GitHub token（`decryption failed`）；缓存回退路径已由 `pg_seed_repo_releases_test.py` 覆盖。
- 失败 2：`/api/admin/jobs/realtime/{task_id}/log` → 404，任务日志文件在生产主机路径上，快照副本不含该文件。
- 未运行：`/auth/github/login|connect|callback`、`/auth/linuxdo/login|callback`、`/auth/logout`、`/api/auth/bind-context`，需要真实第三方 OAuth 交互。

## 本轮修复的真实缺陷

0. `postgres_scheduler::insert_single_flight` 把 `scope_key`/`scope_value` 绑定顺序写反（SQL 期望 `payload->>field = value`），导致按 payload 维度的单飞长期失效；周期通道被 bucket claim 掩盖，star epoch 探测直接暴露为重复任务。已修复并由调度器套件覆盖。

1. 导入 SQLite `_sqlx_migrations` 导致 PG migrator 报 “migration 14 was previously applied but is missing”，服务无法启动 → 导入器跳过该表。
1b. 逐行补齐路径未按目标列类型绑定（Text 单元写 bigint 列报 `cannot convert ... int8`）→ 统一 `$n::text::<type>` 绑定；源侧删除导致行数不一致时现在失败关闭并提示导入新库。
2. 新库全文检索配置建在 `octo_rill` schema，运行时连接解析 `octo_rill_simple` 失败 → 新增 `0014`，查询改用 `public.octo_rill_simple`。
3. 日报窗口时间在新库为 `timestamptz`、导入库为 `text`，`/api/dashboard/feed`、`/api/dashboard/updates` 500 → 读路径统一 `::text` 投影并在 Rust 侧规范化为 RFC3339（含单测）。
4. `content_work_identities.protocol_version` 关联缺失导致身份升级状态 500 → 由 `0012`/读取模型修正，Seed 覆盖。
5. 迁移速度：移除逐行 JSON 哈希与重复源扫描后，生产副本导入由约 25 分钟降至 7 分 41 秒。
6. 标志位列在新库为 `boolean`、导入库为 `bigint` → 新增 `0016` 统一为 `bigint`（并同步翻译写入绑定），避免 LLM 运行配置读写 500。
7. `0015` 在已导入库上因主键/唯一索引同名冲突失败 → 约束补齐块增加 `invalid_table_definition` 与 `duplicate_table` 保护，导入库与空库均可启动。
8. 空库缺少 `translation_work_items` 的重试/状态列（只在导入库存在）→ `0011` 补齐 `attempt_count`、`error_code`、`failure_class`、`next_retry_at`、`retry_expires_at`、`next_attempt_trigger`、`processing_stage`、`provider_status`、`output_contract_status`、`retry_disposition`；fresh 套件断言列存在且 AI 记录列表返回 200。
9. `0015` 在缺少 `legacy_source_hash` 的旧形态观察表上失败 → 约束改为按列存在守卫。
10. 每日日报 upsert 依赖 `(user_id, date)` 唯一键，而 fresh 与导入库都只有 partial window 索引 → `0017` 新增唯一索引（生产快照 0 重复）。

## 旧实现缺陷（已登记 issue，不属于迁移工作）

- #381 生产 SQLite 表达式索引条目缺失（`briefs` 第 895 行、`llm_calls` 第 868 行，涉及 9 个索引）；修复需维护窗口 `REINDEX`，待授权。
- #382 legacy Star 同步可分页失败直接终态，需等下一轮 full sweep；PG 侧已改为 15 分钟重试。
- #383 legacy 在线迁移详情接口等同于列表，缺少真实 detail 语义；PG 保持一致形状。
- #384 匿名 GitHub 读取未纳入限流：11 个无 token 公共仓库按 10 分钟周期会超过 60 次/小时 IP 限额，需共享令牌桶 + 预算耗尽时延后而非记失败。

## 仍未完成（不得标记完成）

1. 基线漂移检测：`0015` 尚未与 SQLite 迁移历史做自动比对，只校验“对象齐全”。
2. 全局内容处理（`mode='global'`）读路径与投影未迁移。
3. `translation.generate` 异步执行器的 LLM 审计缺失。
4. Webhook 入队形态与旧实现的单仓库刷新任务不等价；Star 同步缺 per-connection epoch/切片语义（PG 每次全量快照）。
5. 旧 SQLite 运行时代码尚未删除（`src/api.rs`、`src/server.rs` 等）。
6. 生产切换、生产配置变更与凭据迁移未在本次授权范围内（见 `cutover-plan.md`）。
