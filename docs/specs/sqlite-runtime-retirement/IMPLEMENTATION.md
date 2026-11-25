# SQLite 退役运行时删除：实现状态

> 需求契约见 `./SPEC.md`。功能等价由 `../postgres-content-processing/` 拥有。

## Baseline（2026-11-25 实测）

- `src/` 220,088 行；28 个 `#[allow(dead_code)]` 模块、139,214 行从 `main` 不可达。
- 1,331 个测试中 970 个只跑不可达路径；`postgres_admin_ai_records.rs` 3,629 行、0 个直接测试，与死文件 89% 行级相同，69 个同名函数。
- 活 SQL 25 处 `julianday()`；`pg_sql()` 承担方言、占位符与表前缀改写。
- `public` 与 `octo_rill` 双 schema；`octo_rill` 仅 `tower_sessions`、迁移记录与 3 行控制/标志有数据。
- PG 测试全部 `#[ignore]`，依赖 `TEST_PG_TRANSLATE_URL`；CI 无 PG 测试 job，仅有 Docker 冒烟。

## 有序计划

1. **CI 隔离 PostgreSQL 18 job**：跑 `postgres_*` 测试与 `scripts/pg_seed_*` / `pg_read_acceptance.py`，去掉常驻 `#[ignore]`。状态：pending。
2. **抽共用件与测试**：按 43 个被引用符号（`api` 10、`translations` 22、`content_processing` 4、`jobs` 4、`ai` 3）抽到聚焦活模块，测试随符号迁移。状态：pending。
3. **原生 SQL**：改写活查询，删除 `pg_sql()`。状态：pending。
4. **删模块**：删除 28 个不可达模块；`api.rs`/`sync.rs` 只搬引用部分，其余保留原地待后续拆分。状态：pending。
5. **删 `migrations/` 与 schema 收敛**：`tower_sessions` 与迁移记录迁入 `public`，删除 `octo_rill` 空副本与 schema。状态：pending。
6. **标签**：在步骤 4 的 base 提交打 `legacy-sqlite-final` 并推送。状态：pending。

## 当前事实

- 目录生命周期检查已落地：`scripts/check-spec-lifecycle.sh`，接入 `check-rust-source-quality.sh` 与 lefthook。
- `migrations/` 仍存在，因此删除后门禁尚未生效。
- 功能门禁（PGCP 翻译等价）未完成，步骤 4 不可先合并。
