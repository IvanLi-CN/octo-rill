# SQLite 退役运行时删除：演进记录

## 2026-11-25 立项

- 线上管理页出现成片中性灰格与“旧事实冲突”。诊断确认：global 模式下翻译仍只写按用户隔离的 `translation_work_items`，无全局工作与投影，`ai_translations` 自 2026-09-14 停写；管理端把无 observation 的 legacy 工作行合成为冲突，公开详情返回 `translated=missing`。
- 同轮盘点确认结构债：63% 代码不可达、73% 测试跑死路径、活读模型 89% 复制、SQL 方言重写器、双 schema 且控制行取值相反。
- 决定拆分职责：功能等价进 `postgres-content-processing`（新增 REQ-PGCP-010/011）；删除与收敛独立为本主题。
- 处置过时 Spec：`sqlite-writer-coordinator` 由本主题接替（superseded）；`persistent-state-migrations` 收窄为存储引擎无关事实；`translation-scheduler` 去除 SQLite 假设；`postgresql-runtime` 用后继指针委托删除进度。
- 记录 [ADR 0025](../../adr/0025-runtime-schema-convergence-and-sqlite-retirement.md)：单一 `public` schema、原生 SQL、Git + `legacy-sqlite-final` 保留、分批删除、CI 先行。
