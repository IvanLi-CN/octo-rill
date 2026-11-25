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
| 4 | 删模块 + 标签 | 28 个模块删除；`allow(dead_code)` 归零；`legacy-sqlite-final` 已推送到 base 提交 | revert PR（标签保留） |
| 5 | 迁移与 schema | `tower_sessions` 已落 `public`；一条 forward migration 迁表并 `DROP TABLE` 空副本；`DROP SCHEMA octo_rill`；`migrations/` 删除 | 前向迁移，不回退 |

## CI PostgreSQL job

- 独立 job，一次性 `postgres:18` service；每次运行创建隔离数据库，结束即销毁。
- 环境：`TEST_PG_TRANSLATE_URL` 指向服务实例；seed 脚本以 `--admin-url` 指向可建库的超级用户，`psql` 与 `createdb` 可用。
- 取消 `postgres_*` 用例的常驻 `#[ignore]`；保留需要人工环境的显式 ignore 必须在用例注释说明。
- seed 脚本各自创建一次性库并携带 `--binary-dir` 启动被测服务；`pg_read_acceptance.py` 以 `--base-url`、`--cookie-file` 与 `--output` 运行。限时与主测试 job 分开，便于单独重跑。

## Schema 迁移内容（PR 5）

1. 先改 `session_store` 的 schema 为 `public` 并部署。
2. 一条迁移：`ALTER TABLE octo_rill.tower_sessions SET SCHEMA public`；对 20 张空副本逐表 `DROP TABLE`（含 `content_processing_control`、`translation_work_items` 等）；保留 `_sqlx_migrations` 中仍需的历史行迁移说明。
3. `DROP SCHEMA octo_rill`；删除 `migrations/`。
4. 启动冒烟：会话可读写、控制行唯一、管理读与公开读正常。

## 固定门禁

```bash
grep -RE 'julianday|json_extract|sqlite_master|INSERT OR IGNORE' src/postgres_*.rs   # 应为空
grep -c 'allow(dead_code)' src/main.rs                                               # 应为 0
bash scripts/check-spec-lifecycle.sh                                                 # 应为 ok
```

门禁在 PR 3、4、5 合并前必须全绿；PR 1、2 只需新增测试与编译通过。

## 当前事实

- 目录生命周期检查已落地并接入 `check-rust-source-quality.sh` 与 lefthook。
- `migrations/` 仍存在，删除后门禁尚未生效。
- 功能门禁（PGCP 翻译等价）未完成，PR 4 不可先合并。
