# ADR 0024: PostgreSQL 原生全局内容处理运行时

## Status

Accepted

## Context

ADR 0023 将 PostgreSQL 定为运行时持久化权威。切换后，读路径改为从 `content_current_result_projections` 取翻译与润色结果，但全局内容处理（`content_processing`）的 admission、worker、provider admission 与投影发布仍只有 SQLite 实现。PostgreSQL 运行时因此没有新投影，站点内容自 `2026-11-21T07:54:28Z` 起停更；同时 `translation.generate` 只写 `translation_work_items`，不发布读投影，无法替代。

可选路径有三条：

1. 仅让 `translation.generate` 在完成后补发 translation 投影；
2. 把 SQLite 写入实现以双写或多进程方式保留；
3. 在 PostgreSQL 运行时原生实现全局内容处理 producer/worker。

## Decision

选择第 3 条：在 PostgreSQL 运行时实现原生的 admission、claim/lease、provider 调用、输出契约与投影发布，覆盖 Release 翻译、Release 润色/阅读、公告与通知；`content_*` schema 与身份模型继续沿用既有契约。Release 与公告读侧复用 `postgres_content_overlay` 投影，不新增读分支；通知当前不由该 overlay 读取，其请求/读取适配属于本主题。

## Consequences

- 运行时只有一套权威写入者；SQLite 不再接收新的内容处理写。
- 必须按分片交付：过渡修复、共享契约抽取、admission、worker、生产入口、pipeline 覆盖、退役 legacy 自动入队。
- 既有 `content_current_result_projections` 与身份表可直接复用；Release 与公告读侧无需改动，通知读取路径需随流水线适配。
- 本决策取代 `docs/specs/postgresql-runtime/HISTORY.md` 中“旧内容处理流水线不恢复”的结论，并修订 `REQ-PGR-009` 的自动入队退役边界。
- provider 调用与审计逻辑从 `postgres_translate` 复用，降低第二套实现漂移的风险。
- 迁移期间 `translation.generate` 仍需修复导入库唯一约束，避免任务中心继续产生伪失败。

## Alternatives Rejected

- **只补 translation 投影发布**：能恢复 Release 明细翻译，但润色/阅读、公告与通知仍停更，问题不彻底；且会在 `translation.generate` 上叠加一套与全局身份不一致的发布逻辑。
- **保留 SQLite 写入者**：违反 ADR 0023 的单一权威与无回退双写边界，切换后状态无法判定。
- **只改读路径回退到 legacy 表**：会让读身份与写身份再次分叉，掩盖新的不一致，且 legacy 表没有新数据。
