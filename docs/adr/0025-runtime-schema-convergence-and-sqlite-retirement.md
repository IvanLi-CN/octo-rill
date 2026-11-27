# ADR 0025: 运行时 schema 收敛与 SQLite 退役边界

## Status

Accepted

## Context

ADR 0023 已把 PostgreSQL 定为唯一运行时权威，ADR 0024 已决定在 PostgreSQL 原生实现全局内容处理。但退役没有边界与保留方式：

- 运行时代码仍全量编译：28 个 `#[allow(dead_code)]` 模块、139,214 行从 `main` 不可达；73% 的测试只跑这些路径（2026-11-25 基线）。
- 活 SQL 仍以 SQLite 方言书写，由 `pg_sql()` 在运行期改写 `julianday`/`json_extract`/占位符/表前缀。
- 数据库同时存在 `public` 与 `octo_rill` 两套同名表。`public` 持有全部活数据；`octo_rill` 20 张表中只有 `tower_sessions`、迁移记录与 3 行控制/标志有数据，且 `content_processing_control` 与 `public` 取值相反。
- 现成的 PG 测试靠 `#[ignore]` + `TEST_PG_TRANSLATE_URL`，CI 未接线；删除旧测试后没有自动回归。

可选路径：保留专用 `octo_rill` schema 并固定 `search_path`；或收敛到 `public`；或继续允许双 schema 靠限定前缀维持。退役保留可选 Git 历史、`docs/legacy/` 副本或继续编译。

## Decision

- **单一 schema = `public`**。`octo_rill._sqlx_migrations` 是当前唯一运行时迁移历史，必须在同一条 forward migration 内先迁入 `public`，再迁会话与 `migration_manifests`，逐表删除陈旧副本，最后删除 schema。活数据已在 `public`，迁移面最小；限定方式随之统一，代码不再依赖 `search_path` 顺序。
- **原生 SQL**。活查询改为 PostgreSQL 原生写法，删除 `pg_sql()` 重写器。已发布 `migrations_pg/` 不改写，schema 变更走 forward migration。
- **退役保留 = Git 历史 + 注释标签 `legacy-sqlite-final`**，打在删除 PR 的 base 提交、CI 绿后、合并前推送。该提交同时包含完整 `migrations/` 与 SQLite 模块；不保留 `docs/legacy/` 副本。`octo-rill-migrate` 与 `octo-rill-validation-session` 独立保留。
- **分批删除**：CI 隔离 PostgreSQL 18 job → 抽共用件与测试 → 原生 SQL → 删模块 → 删 `migrations/` 与 schema 收敛。PR 1–4 可独立回退，PR 5 forward-only；CI 先于测试迁移，否则搬走的测试无人执行。
- **功能门禁**：PGCP 翻译全局通路发布等价结果前，不删旧 `translation.generate` 自动入队；按用户翻译表与 `ai_translations` 的历史行保留只读兼容。
- **无写入者不保留**：在线迁移 operator 及其管理接口在 PostgreSQL 运行时没有写入者，随退役删除，不保留只读假象。
- **命名**：“旧事实（legacy evidence）”指只读历史行；“退役运行时（retired runtime）”指被删除的代码与迁移目录。二者不互相指代。

## Consequences

- 删除后二进制不再包含 SQLite 运行时与 `migrations/`，编译面与测试面同时缩小。
- 回滚到 SQLite 可运行版本需要 `legacy-sqlite-final` 源码与旧镜像；数据库内存态不随代码回滚。
- 共用纯函数必须先抽到活模块，`api.rs`/`sync.rs` 只搬被引用部分；大文件其余拆分延后。
- PG 测试将纳入合并门禁，`scripts/check-spec-lifecycle.sh` 校验目录生命周期与 successor。
- 五个 PR 的退出条件与门禁命令见 `docs/specs/sqlite-runtime-retirement/IMPLEMENTATION.md`；PR 5 不回退。
- `octo_rill` schema 删除后，`tower_sessions` 的显式 schema 配置需同步修改；任何仍写 `octo_rill.` 的代码会显式失败而不是静默读空表。

## Alternatives Rejected

- **保留专用 `octo_rill` schema 并固定 `search_path`**：命名更干净，但要搬 60+ 张活表、重写全部限定，且历史 `public` 仍在，收益不抵迁移风险。
- **继续双 schema 靠前缀共存**：当前 `content_processing_control` 已有两份相反取值，任何遗漏限定都会静默读空；删除比长期约束更可靠。
- **继续编译死代码作为“文档”**：970 个测试与 139k 行持续参与编译，但覆盖率错觉掩盖活路径无测试。
- **`docs/legacy/` 源码副本**：与 Git 重复，且无法编译验证，容易腐化。
- **一次性大爆炸删除**：共用符号仍被 PG 路径引用，先删会同时丢逻辑与 970 个测试。
