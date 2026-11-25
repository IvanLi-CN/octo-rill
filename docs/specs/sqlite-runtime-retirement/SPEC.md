# SQLite 退役运行时删除与运行时路径收敛

> 本文件是该主题的持久需求契约。当前实现事实在 `IMPLEMENTATION.md`；生命周期与变更引用在 `HISTORY.md`。
> 功能侧（翻译进入全局流水线、管理端显示真实状态）由 [`../postgres-content-processing/SPEC.md`](../postgres-content-processing/SPEC.md) 拥有；本主题只拥有结构收敛与删除。

## Context and Scope

- Context: 生产运行时只接受 PostgreSQL（ADR 0023），但 SQLite 运行时仍全量编译，持有 73% 的测试与一批被 PostgreSQL 路径引用的纯函数；读模型整段复制，SQL 以 SQLite 方言书写再由运行期重写器翻译；同一库中存在两套同名 schema。
- 当前基线（2026-11-25 实测）：
  - `src/` 220,088 行，28 个 `#[allow(dead_code)]` 模块、139,214 行从 `main` 不可达；1,331 个测试中 970 个只跑不可达路径。
  - `postgres_admin_ai_records.rs` 3,629 行、0 个直接测试，与死文件 89% 行级相同，69 个同名函数。
  - 活 SQL 仍有 25 处 `julianday()`，库中无该函数，靠 `pg_sql()` 扫描器改写方言、占位符与表前缀。
  - `public` 与 `octo_rill` 各有同名表；`octo_rill` 20 张表中仅 `tower_sessions`、迁移记录与 3 行控制/标志有数据，`content_processing_control` 与 `public` 取值相反。
- In scope: 不可达模块删除、共用纯核心抽取、原生 PostgreSQL SQL、schema 收敛、测试迁移与 CI PostgreSQL 测试层、SQLite 迁移历史退役、退役保留与生命周期处置。
- Out of scope: 翻译/润色语义本身（PGCP）、`api.rs`/`sync.rs` 的其余大文件拆分、部署拓扑、把 SQLite 恢复为可运行后端。

## Terms and Interfaces

- **活路径（live path）**: `main` 可达、生产实际执行的模块。
- **退役运行时（retired runtime）**: 只保留历史价值、不再编译进二进制的 SQLite 运行时代码与 `migrations/`。
- **旧事实（legacy evidence）**: 切换前的只读历史行，与退役运行时不是同一概念。
- **SQLite 快照 / 导入器**: 只读迁移输入与独立的 `octo-rill-migrate` 二进制，不属于退役范围。
- Interface: 模块树、`Cargo.toml` targets、`migrations_pg/`、CI 工作流、管理端与公开读 API 的响应形状。

## Requirements

### REQ-RTR-001 单一运行时入口

- `main` 的唯一运行时路径 MUST 是 PostgreSQL；SQLite 运行时模块 MUST 从编译单元移除。
- `octo-rill-migrate` 与 `octo-rill-validation-session` MUST 保持独立、不依赖退役模块。
- 删除完成后 MUST NOT 再用 `#[allow(dead_code)]` 掩盖不可达运行时代码。

### REQ-RTR-002 共用核心只实现一次

- 被活路径引用的符号 MUST 先抽到按领域命名的活模块，再删除原文件；实测抽取面为 43 个符号（`api` 10、`translations` 22、`content_processing` 4、`jobs` 4、`ai` 3）。目标模块为 `content_source_identity`、`release_content`、`markdown`、`translation_admin_types`、`translation_types`、`translation_error`、`translation_runtime`、`llm_contract`、`postgres_admin_dashboard`、`postgres_tasks`，落位清单见 `IMPLEMENTATION.md`。
- 源哈希与 revision、状态映射、尝试分类、摘要/活动聚合、`legacy_*` 判定 MUST 各只有一份实现。
- 管理读模型 MUST NOT 与退役副本并存；同名函数差异 MUST 只剩数据源方言差异。

### REQ-RTR-003 原生 PostgreSQL SQL

- 活路径 SQL MUST 使用 PostgreSQL 原生类型、函数与占位符；`julianday`、`datetime`、`json_extract`、`INSERT OR IGNORE`、`sqlite_master` MUST NOT 出现在活查询中。
- `pg_sql()` 重写器 MUST 删除；表限定方式 MUST 统一，MUST NOT 依赖运行期文本前缀注入。

### REQ-RTR-004 单一 schema 与单一控制行

- 运行时表 MUST 只存在于 `public`；`octo_rill` 的空副本 MUST 删除，有数据者 MUST 迁移。
- MUST 只存在一份 `content_processing_control`；`tower_sessions` 与迁移记录 MUST 落在 `public`。
- 代码 MUST NOT 依赖 `search_path` 顺序的巧合。

### REQ-RTR-005 活路径测试义务

- 每个抽取出的核心与每个 PostgreSQL 读模型 MUST 有直接测试；测试 MUST NOT 只存在于退役副本。
- 一个独立的 PostgreSQL 18 service job MUST 先于测试迁移落地：一次性数据库、设置 `TEST_PG_TRANSLATE_URL` 与 `TEST_PG_*`、取消常驻 `#[ignore]`，并运行 `postgres_*` 测试与 `pg_seed_*` / `pg_read_acceptance.py`。该 job MUST 与主测试 job 分开，便于单独重跑与限时。
- 删除后测试总数 MUST NOT 下降。

### REQ-RTR-006 迁移历史收敛

- `migrations_pg/` MUST 成为唯一运行时迁移历史；删除后二进制 MUST NOT 在编译期包含 `migrations/`。
- 已发布 PostgreSQL 迁移 MUST 不可修改；schema 收敛 MUST 走 forward migration。

### REQ-RTR-007 分批删除与保留点

- 顺序 MUST 为五个可独立回退的 PR：CI PostgreSQL job → 抽共用件与测试 → 原生 SQL → 删模块 → 删 `migrations/` 与 schema 收敛。
- schema 收敛 MUST 是 forward-only 的最后一步：先改会话落 `public`，再以一条迁移迁 `tower_sessions` 并 `DROP TABLE` 空副本，最后 `DROP SCHEMA octo_rill`；MUST NOT 逐表拆迁移。
- 在删除模块 PR 的 base 提交上 MUST 打注释标签 `legacy-sqlite-final`，CI 绿后、合并前推送；该提交 MUST 同时包含完整 `migrations/` 与 SQLite 模块。
- 每一步 MUST 可独立回退；MUST NOT 以保留 `docs/legacy/` 副本作为保留方式。

### REQ-RTR-008 功能门禁

- 删除 `translation.generate` 旧自动入队之前，PGCP 的翻译全局通路 MUST 已发布等价结果，并满足 `REQ-PGCP-006`。
- 按用户翻译表 `translation_work_items` / `translation_requests` 与 `ai_translations` 的只读兼容 MUST 保留；删除的是写入方与运行时代码，不是历史行。

### REQ-RTR-009 生命周期与词表处置

- 下列处置 MUST 执行：`sqlite-writer-coordinator` 标 `superseded` 并由本文件接替；`persistent-state-migrations` 修订为 PostgreSQL 持久事实与旧事实观察；`translation-scheduler` 修订去除 SQLite 快照与 coordinator 假设；`postgresql-runtime` 用后继指针指向本文件。
- `superseded` 标记 MUST 随替代需求合并立即生效，不等删除完成；删除完成后相关篇目 MUST 为 `superseded` 或 `archived`。
- 词表 MUST 区分“旧事实”与“退役运行时”。仅顺带提及 SQLite 的篇目 MUST 只在契约语义变化时加注。

## Non-goals

- 不做大爆炸重写；不重写本主题外的业务逻辑。
- 不拆 `api.rs`（47k）/`sync.rs`（28k）的其余部分。
- 不重做部署、镜像、Traefik 与监控。
- 不删除 `ai_translations` 与按用户翻译表的历史行。

## Verification

- VER-RTR-REACH: covers REQ-RTR-001、REQ-RTR-007。模块树与两个独立二进制构建通过；`allow(dead_code)` 清单归零；标签包含完整退役源码。
- VER-RTR-CORE: covers REQ-RTR-002。共用函数单实现；抽取面清单逐项有活模块归属；管理读模型只剩一份。
- VER-RTR-SQL: covers REQ-RTR-003。方言 `grep` 零命中；同一查询在空库与导入库计划一致。
- VER-RTR-SCHEMA: covers REQ-RTR-004、REQ-RTR-006。仅解析出 `public`；控制行唯一；会话与迁移同 schema；升级后服务可启动。
- VER-RTR-TESTS: covers REQ-RTR-005。CI PostgreSQL job 绿；删除前后测试数不降；无新增常驻 `#[ignore]`。
- VER-RTR-GATES: covers REQ-RTR-003、REQ-RTR-004。`grep -RE 'julianday|json_extract|sqlite_master|INSERT OR IGNORE' src/postgres_*` 与 `grep -c 'allow(dead_code)' src/main.rs` 归零；`bash scripts/check-spec-lifecycle.sh` 绿。
- VER-RTR-GATE: covers REQ-RTR-008。翻译全局通路等价结果已发布；按用户端点 wire shape 快照不变；旧事实行未被改写。
- VER-RTR-LIFECYCLE: covers REQ-RTR-009。`scripts/check-spec-lifecycle.sh` 绿；superseded 行均有后继；词表无 `legacy` 裸用。

## Interfaces & Contracts

| Interface | Kind | Scope | Change | Contract | Owner | Consumers |
| --- | --- | --- | --- | --- | --- | --- |
| 模块树与二进制 targets | Rust module tree | internal | Modify | 本文件 | backend | maintainers |
| 运行时 schema 与迁移 | DB schema | internal | Modify | 本文件 + ADR 0025 | backend | scheduler, worker, admin read |
| 管理端与公开读 API | HTTP API | external | Preserve | `../global-translation-and-polish/contracts/http-apis.md` | backend | web |
| Spec 目录生命周期 | Docs | internal | Modify | `../README.md` | maintainers | maintainers |

## Related ADRs

- [ADR 0023: PostgreSQL 作为运行时持久化权威](../../adr/0023-postgresql-runtime-authority.md)
- [ADR 0024: PostgreSQL 原生全局内容处理运行时](../../adr/0024-postgres-content-processing-runtime.md)
- [ADR 0025: 运行时 schema 收敛与 SQLite 退役边界](../../adr/0025-runtime-schema-convergence-and-sqlite-retirement.md)
