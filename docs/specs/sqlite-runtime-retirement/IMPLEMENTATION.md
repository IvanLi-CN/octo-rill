# SQLite 退役运行时删除：实现状态

> 需求契约见 `./SPEC.md`。功能等价由 `../postgres-content-processing/` 拥有。实现尚未开始，本文件固定执行切分，供设计审查。

## Baseline（2026-11-25 实测）

- `src/` 220,088 行；28 个 `#[allow(dead_code)]` 模块、139,214 行从 `main` 不可达。
- 1,331 个测试中 970 个只跑不可达路径；`postgres_admin_ai_records.rs` 3,629 行、0 个直接测试，与死文件 89% 行级相同，69 个同名函数。
- 活 SQL 25 处 `julianday()`；`pg_sql()` 承担方言、占位符与表前缀改写。
- `public` 与 `octo_rill` 双 schema；`octo_rill` 仅 `tower_sessions`、迁移记录与 3 行控制/标志有数据。
- PG 测试全部 `#[ignore]`，依赖 `TEST_PG_TRANSLATE_URL`；CI 无 PG 测试 job，仅有 Docker 冒烟。

## 符号落位

抽取面共 43 个被活路径引用的符号，按领域落位；同域测试随符号迁移。

| 目标模块 | 来源 | 符号 |
| --- | --- | --- |
| `content_source_identity`（新增） | `api` | `announcement_detail_source_hash`、`announcement_discussion_key`、`announcement_feed_translation_source_hash`、`parse_discussion_number_from_github_url`、`release_detail_source_hash`、`release_feed_translation_source_hash` |
| `content_source_identity` | `content_processing` | `authoritative_release_revision`、`notification_source_revision_tiebreak`、`release_source_revision_content_tiebreak`、`source_hash_for_item` |
| `content_source_identity` | `ai` | `sha256_hex` |
| `release_content`（新增） | `api` | `release_feed_body` |
| `markdown`（新增） | `api` | `markdown_structure_preserved` |
| `content_source_identity` | `translations` | `announcement_smart_feed_source_hash`、`release_smart_feed_source_hash` |
| `translation_admin_types`（新增） | `translations` | `AdminTranslationAttemptEvent`、`AdminTranslationAttemptEventsResponse`、`AdminTranslationAttemptListQuery`、`AdminTranslationBatchResultSummary`、`AdminTranslationBatchRow`、`AdminTranslationBatchesResponse`、`AdminTranslationLinkedLlmCall`、`AdminTranslationListQuery`、`AdminTranslationRequestDetailResponse`、`AdminTranslationRequestsResponse`、`AdminTranslationStatusResponse`、`AdminTranslationWorkerStatus`、`RequestRow`、`admin_translation_result_item`、`request_row_select_sql` |
| `translation_types`（新增） | `translations` | `TranslationResultItem`、`translation_batch_business_outcome` |
| `translation_error`（新增） | `translations` | `classify_translation_error`；`api` 的 `SMART_NO_VALUABLE_VERSION_INFO` |
| `translation_runtime`（新增） | `translations` | `DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY`、`DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY` |
| `llm_contract`（新增） | `ai` | `LlmFailureClass`、`resolve_model_input_limit_for_model` |
| `postgres_admin_dashboard`（现有） | `api` | `ADMIN_DASHBOARD_PREAGGREGATE_DAYS` |
| `postgres_tasks`（现有） | `jobs` | `TASK_SYNC_NOTIFICATIONS`、`TASK_SYNC_RELEASES`、`TASK_SYNC_STARRED`、`TASK_SYNC_SUBSCRIPTIONS` |

落位后 `api.rs`（346 测试）与 `sync.rs`（155 测试）中的同域测试随符号迁移，其余整体删除。

## PR 序列

每个 PR 独立可回退，前一个的 CI 全绿才开下一个。

| # | 范围 | 退出条件 | 回退 |
| --- | --- | --- | --- |
| 1 | CI PostgreSQL job | 新 job 跑 `postgres_*`、`pg_seed_*`、`pg_read_acceptance.py`；无新增常驻 `#[ignore]` | 删除 job 定义 |
| 2 | 抽共用件与测试 | 43 个符号全部有活模块归属；同域测试在新位置通过；`api.rs`/`sync.rs` 只剩未搬代码 | revert PR |
| 3 | 原生 SQL | `pg_sql()` 删除；方言 `grep` 零命中；空库与导入库计划一致 | revert PR |
| 4 | 删模块 + 标签 | 29 个 `allow(dead_code)` 模块处置完毕；`allow(dead_code)` 归零；`legacy-sqlite-final` 已推送到 base 提交 | revert PR（标签保留） |
| 5 | 迁移与 schema | 迁移历史、会话、导入 manifest 已落 `public`；陈旧副本、退役 ts 配置与 `pg_trgm` 已迁走；`DROP SCHEMA octo_rill` 已生效（`0023_public_schema_convergence.sql`）；`migrations/` 与无写入者的迁移管理接口待删 | forward-only，不回退 |

## CI PostgreSQL job

- 独立 job，一次性 `postgres:18` service；每次运行创建隔离数据库，结束即销毁。
- 环境只要求 `TEST_PG_TRANSLATE_URL`（指向服务实例）；seed 脚本以 `--admin-url` 指向可建库的超级用户，`psql` 与 `createdb` 可用。
- 取消 `postgres_*` 用例的常驻 `#[ignore]`；保留需要人工环境的显式 ignore 必须在用例注释说明。
- seed 脚本各自创建一次性库并携带 `--binary-dir` 启动被测服务；`pg_read_acceptance.py` 以 `--base-url`、`--cookie-file` 与 `--output` 运行。限时与主测试 job 分开，便于单独重跑。

## Schema 迁移内容（PR 5）

代码先改为显式使用 `public`，但同一次启动内迁移先于会话 store 初始化，因此不需要中间版本。

1. 一条 forward migration 内按顺序执行：
   - `ALTER TABLE octo_rill.tower_sessions SET SCHEMA public`
   - `ALTER TABLE octo_rill._sqlx_migrations SET SCHEMA public`（该迁移因此成为历史表里的最后一条）
   - `ALTER TABLE octo_rill.migration_manifests SET SCHEMA public`
   - 删除陈旧副本（见下表）
2. `DROP SCHEMA octo_rill`；删除 `migrations/`。
3. 验证：空库启动、导入库启动、重启各一次；`public._sqlx_migrations` 只含一套历史且无 pending；会话可读写；无 `octo_rill` schema 残留。
4. 若 SQLx 在迁移内移动自身历史表后无法记录版本，退化为两条迁移：先迁表，再单独 `DROP SCHEMA`；不得手工补写版本行。

| 表 | 行数 | 处置 |
| --- | --- | --- |
| `_sqlx_migrations` | 22 | 迁入 `public`，作为唯一运行时迁移历史 |
| `tower_sessions` | 7 | 迁入 `public` |
| `migration_manifests` | 1 | 迁入 `public`，保留导入证据 |
| `content_processing_control` | 1 | 陈旧副本；`public` 为权威，删除 |
| `llm_recovery_flags` | 1 | 与 `public` 行比对；代码只读 `public`，确认后删除 |
| 其余 15 张 | 0 | `briefs`、`brief_release_memberships`、`content_batch_items`、`content_batches`、`content_result_projections`、`content_work_items`、`llm_diagnostic_access_audit`、`llm_model_health`、`schema_migrations`、`translation_attempt_events`、`translation_attempt_llm_calls`、`translation_batch_items`、`translation_batches`、`translation_requests`、`translation_work_items`，全部删除 |

## R4 实测面（2026-11-27 两轮实测）

第一轮（base `c8b4be01`，纯文本引用统计，排除 `#[cfg(test)]` 与注释）：
- 29 个 `#[allow(dead_code)]` 模块中 20 个仍被生产代码引用（44 个符号），9 个零生产引用。
- 结论文档当时写成「抽符号 → 删空文件」，但该结论忽略了退役模块之间的互相引用。

第二轮（base `34ff145e`，以 `cargo check --all-targets` 为唯一判据）：
- 移除 `server.rs` 后新增 4 个模块彻底不可达并已删除：`server`、`admin_ai_records`、`auth`、`public_metrics`；同时删除只被 `server` 用到的 `public_metrics_cors_origins_from_env` 与其测试。
- 逐模块「试删 + 编译」循环（`/tmp/modrm.py`）在同一轮内再删 `database_migrations`、`runtime` 等，但只要被测或未删模块仍引用，编译器即点名要求恢复。
- 批量法（一次删全部 25 个）确认：**退役模块构成强连通死代码团**。删任意一个都会断链，因此「删文件」必须同时抽走被活代码引用的 44 个符号。
- 独立计算（跨模块闭包，含 `impl` 块并集）：222 个符号跨 21 个模块；其中 8 个模块零活引用（`jobs`、`linuxdo`、`online_migrations`、`passkeys`、`runtime`、`sync`、`webhook_push`、`worker_backoff`），但这 8 个仍被其他退役模块引用，不能单独删。
- 活模块内另有 1,820–2,012 个死项（随删除进度浮动）。它们互相引用，任何单项删除都会断链；已实测四种自动策略（逐项贪心、按文件、全量一次性、闭包裁剪）均在同一处失败。

因此 R4 的剩余工作是一个**必须手工分片的重构**：按领域把 44 个活符号搬进活模块并补测试，再整团删除退役模块；不能用脚本一次完成。本轮已完成其中可直接验证的部分（−12,170 行，0 错误 0 警告）。

自动删除不可用的具体原因（供后续实现参考）：
- `rustc` 的 `dead_code` 对一部分项报告的是**使用点**而非声明点（`ItemRange` 从该行回溯会吞掉前一个函数体），必须先过滤「首行必须匹配声明关键字」。
- 即使过滤后，删除一个死函数仍会打断其被其他死函数引用的事实；必须按引用拓扑自内向外的顺序删，或一次性删掉整个闭包。
- 一次性删闭包时，`impl` 块同时以类型名为键，闭包需显式并集；否则会切出半个 `impl`。

## 已落地

- **R4 已完成部分**：`server.rs` 的 SPA 回退拆成 `static_site` 后，4 个模块彻底不可达并删除（−12,170 行）：`server`、`admin_ai_records`、`auth`、`public_metrics`。剩余 25 个 `allow(dead_code)` 模块需要按上面的实测结论手工分片。
- **R5 schema 收敛**：`migrations_pg/0023_public_schema_convergence.sql` 已落地并通过迁移器验证。它按序迁移 `_sqlx_migrations`、`tower_sessions`、`migration_manifests`，重建两个绑定 `octo_rill` 的 `search_vector` 生成列，将 `pg_trgm` 与文本搜索配置迁到 `public`，再 `DROP SCHEMA octo_rill`。`session_store` 与 `postgres_runtime` 已改读 `public.tower_sessions`。

## 已完成（PR #420，合并于 `6a9f8735`）

PR 序列 1、2、3 与 5 的 schema 部分已合并：CI PostgreSQL job、43 个共用符号抽取、原生 SQL、`public` schema 收敛。验证基线：22 个 PG 门禁用例、1275 个单元测试、18 个 seed 套件、77 条路由探针全部通过，CI 10/10 绿。

导入套件的 schema 源已改为 `ops/fixtures/legacy-sqlite-schema.sql`（339 个对象与迁移构建结果逐一相同），因此删除 `migrations/` 不再会破坏 CI。

## 待办：剩余工作清单

实测依据见上文「R4 实测面」。以下六项全部未完成，按依赖顺序排列：

| # | 任务 | 退出条件 | 阻塞原因 |
| --- | --- | --- | --- |
| 1 | 从 25 个退役模块搬出 44 个活符号 | 每个符号有活模块归属，同域测试随迁 | 模块间强连通，无法先删文件 |
| 2 | 删除 25 个退役模块 | `grep -R 'allow(dead_code)' src/main.rs` 为空 | 依赖第 1 项 |
| 3 | `allow(dead_code)` 归零 | `grep -R 'allow(dead_code)' src/` 空，`rust-source-quality.toml` 对应条目删除 | 依赖第 2 项 |
| 4 | 打 `legacy-sqlite-final` 标签 | 标签指向删除提交的 base，且含完整 `migrations/` 与 SQLite 模块 | 需在第 2 项提交前创建 |
| 5 | 删除 `migrations/` | Dockerfile 与 `include_str!`/`sqlx::migrate!` 引用全部消失 | 依赖第 2 项（退役模块的测试用 `include_str!` 绑定该目录） |
| 6 | 删除 `/api/admin/jobs/migrations` 三条路由 | 路由、`admin_migrations`、pause/resume handler 与 `EXPECTED_NOT_RUN` 条目一并删除 | 无阻塞：PG 侧只有读与 pause/resume 写，没有任何 worker 创建或推进 migration；`web/` 零引用 |

第 1 项的建议顺序按符号数从少到多：`release_links`(1) → `error`(1) → `session_store`(1) → `translations`(1) → `content_processing`(1) → `api_keys`(6) → `jobs`(2) → `admin_runtime`(3) → `github`(3) → `state`(4) → `metrics`(8) → `briefs`(11)。每步后跑 `bash scripts/pg_seed_suite.sh`，基线为 18/18。

## 固定门禁

```bash
grep -RE 'julianday|json_extract|sqlite_master|INSERT OR IGNORE' src/postgres_*.rs   # 应为空
grep -R 'allow(dead_code)' src/                                                      # 应为空
bash scripts/check-spec-lifecycle.sh                                                 # 应为 ok
```

门禁在 PR 3、4、5 合并前必须全绿；PR 1、2 只需新增测试与编译通过。

## 当前事实

- 目录生命周期检查已落地并接入 `check-rust-source-quality.sh` 与 lefthook。
- `migrations/` 仍存在，删除后门禁尚未生效。
- 功能门禁（PGCP 翻译等价）未完成，PR 4 不可先合并。
- 在线迁移 operator 与 `/api/admin/jobs/migrations` 在 PG 运行时无写入者；PR 5 删除接口与控件，不保留只读假象。
