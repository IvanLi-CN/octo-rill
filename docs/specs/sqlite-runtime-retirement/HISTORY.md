# SQLite 退役运行时删除：演进记录

## 2026-11-25 立项

- 线上管理页出现成片中性灰格与“旧事实冲突”。诊断确认：global 模式下翻译仍只写按用户隔离的 `translation_work_items`，无全局工作与投影，`ai_translations` 自 2026-09-14 停写；管理端把无 observation 的 legacy 工作行合成为冲突，公开详情返回 `translated=missing`。
- 同轮盘点确认结构债：63% 代码不可达、73% 测试跑死路径、活读模型 89% 复制、SQL 方言重写器、双 schema 且控制行取值相反。
- 决定拆分职责：功能等价进 `postgres-content-processing`（新增 REQ-PGCP-010/011）；删除与收敛独立为本主题。
- 处置过时 Spec：`sqlite-writer-coordinator` 由本主题接替（superseded）；`persistent-state-migrations` 收窄为存储引擎无关事实；`translation-scheduler` 去除 SQLite 假设；`postgresql-runtime` 用后继指针委托删除进度。
- 记录 [ADR 0025](../../adr/0025-runtime-schema-convergence-and-sqlite-retirement.md)：单一 `public` schema、原生 SQL、Git + `legacy-sqlite-final` 保留、分批删除、CI 先行。

## 执行切分

- 决定符号按领域落位（`content_source_identity`、`release_content`、`markdown`、`translation_*`、`llm_contract`，已有模块 `postgres_tasks`/`postgres_admin_dashboard`），不沿用原文件名。
- `api.rs`/`sync.rs` 搬完引用符号后整文件删除，测试随符号迁移。
- 五个可回退 PR；CI PostgreSQL job 独立且先行；schema 收敛为最后一条 forward migration；`legacy-sqlite-final` 打在删模块 PR 的 base 提交。
- 门禁固定为方言零命中、`allow(dead_code)` 归零与生命周期检查。

## 审查修正

- 补全 43 个符号落位中的两个 smart feed source hash。
- 明确 `_sqlx_migrations` 必须在删除 schema 前迁入 `public`，并逐表列出迁移/删除清单与验证方式。
- 修正“五个 PR 全部可回退”为 PR 1–4 可回退、PR 5 forward-only。
- 把 PGCP-010 从“继承 GTP 管理读”改为“修订”，并同步 GTP 原文；补充投影唯一发布者。
- 将在先迁移不变量迁入 `postgresql-runtime`；PSM 去除写许可措辞并标注 operator 退役边界。
- 门禁改为扫描整个 `src/`；翻译表适配条款转为带验证的 REQ。
