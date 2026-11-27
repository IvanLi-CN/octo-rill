# PostgreSQL 全局内容处理运行时 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 进行中
- Lifecycle: active
- Catalog note: PostgreSQL 原生 producer/worker 分片交付；content schema 与 Release/公告读投影已就绪

## Implementation Coverage

- Requirement coverage:
  - `REQ-PGCP-001`: 已实现 `src/postgres_content.rs`：claim/lease、attempt、provider admission、审计与投影发布均已接线；worker 随 `OCTORILL_PG_WORKER_ENABLED` 启动。
  - `REQ-PGCP-002`: 待实现，复用 `content_identity_upgrade` 身份与 `content_*` 表。
  - `REQ-PGCP-003`: 已实现，复用 `postgres_translate::complete_with_audit` 与 `content_processing::validate_output`。
  - `REQ-PGCP-004`: 待实现，复用 `llm_model_health`、`llm_recovery_flags` 与既有退避常量。
  - `REQ-PGCP-005`: 部分实现：Release `smart` 润色已可由 PG worker 执行并发布投影；reading 变体（`smart_public`/`smart_authorized`）、公告与通知仍待 evidence producer 与对应适配。
  - `REQ-PGCP-006`: 待实现；当前生产仍有 `translation.generate` 独自运行。第 1.5 分片已确保终态工作不会被自动入队重新排队，读投影的 PostgreSQL 生产者仍缺位。
  - `REQ-PGCP-007`: 读接口已存在；待新写入后验证。
  - `REQ-PGCP-008`: 已实现：单行 `FOR UPDATE SKIP LOCKED` 认领、发布与审计写入幂等、单条失败不影响其他工作项。
  - `REQ-PGCP-009`: 已实现结构化日志；指标接入待补。
- Verification commands: 见各分片测试；PostgreSQL 集成测试沿用 `TEST_PG_TRANSLATE_URL` 隔离库模式。
- Rollout facts: 生产 `content_processing_control.mode=global`；`content_current_result_projections` 12,023 行，最后发布 `2026-11-21T07:54:28Z`；切换后无新投影。

## Coverage / rollout summary

- 2026-11-24 第 1.5 分片：终端请求对账。新增 `postgres_translate::reconcile_terminal_translation_requests`，在启动时与 60 秒调度 lane 中把 `translation_requests` 的 `queued`/`running` 视图按工作项真实终态收敛（`completed` 复制结果，否则 `failed` 保留错误分类），不调用 provider、不重写尝试/LLM 审计、跳过仍有排队或运行任务的 work item；同时自动入队拒绝把终态 work item 重新排回队列，避免产生新的陈旧请求。
- 2026-11-23 完成只读诊断：PostgreSQL 运行时无内容处理 producer/worker；`translation.generate` 因导入库缺少 `translation_attempt_events` 唯一约束而全部失败。
- 2026-11-23 第 1 分片交付：`migrations_pg/0020_translation_attempt_unique.sql`（导入快照去重 + 唯一索引）与 `insert_translation_request` 幂等重排队；PR #411。
- 分片计划：
  1. 过渡修复：补齐导入库唯一约束，请求重排队幂等，恢复任务审计可见性。
  2. 契约抽取：把输出契约、prompt、失败分类等数据库无关逻辑抽成共享模块。
  3. PG admission：身份、请求关联、缓存命中、supersede、admission 事件。
  4. PG worker：claim/lease、attempt 事件、provider admission、LLM 审计、投影发布。
  5. 生产入口：Release 刷新自动入队、翻译请求 API 切换到全局流水线。
  6. 润色/阅读、公告、通知、批量与恢复对齐。
  7. 退役 legacy `translation.generate` 自动入队并完成切换验证。

- 2026-11-24 第 2 分片：PostgreSQL 原生全局内容 worker。新增 `src/postgres_content.rs`：受限期认领（`FOR UPDATE SKIP LOCKED`）、attempt 事件与路由快照、每次 provider 请求前的 lease 续约与 provider admission、复用 `postgres_translate` 的模型路由/健康与 `llm_calls` 审计、复用 `content_processing::{build_prompt,validate_output}` 输出契约、发布结果投影与当前投影、过期 lease 恢复；Release 刷新在既有 `smart` 合约上限量准入润色工作（不伪造 reading evidence）。

## 修复顺序

1. 管理读真实状态：去掉无 observation 时对当前 legacy 工作行的 `legacy_conflict` 合成，按真实状态与尝试数展示（REQ-PGCP-010）。
2. 翻译投影发布 + 受众润色变体：翻译完成写投影，producer 补 `smart_public`/`smart_authorized`（REQ-PGCP-011）。
3. 既有 366 条输出回填为投影：零新增尝试、按 identity + `source_hash` 幂等写入。
4. 翻译 admission 迁入全局生产者；按用户端点变适配器。
5. 退役旧 `translation.generate` 自动入队（SQLite 退役 PR 4 的功能门禁）。

## Remaining Gaps

- 契约抽取：`build_prompt`/`validate_output` 已共用，但失败分类仍分属于两条链路。
- PG admission：请求关联、缓存命中与 supersede 仍走 `translation.generate` 路径；只有 Release 刷新的 `smart` 润色有了 PG 侧准入。
- 生产入口：`/api/translate/requests` 仍写 `translation_work_items`；全局通路尚未切换。
- 润色/阅读：`smart_public`/`smart_authorized` 需要 PostgreSQL evidence producer；公告与通知未接入。
- 指标：worker 目前只有结构化日志，未接 Prometheus 计数。
- 退役 legacy `translation.generate` 自动入队尚未开始。
- 管理读模型：切换后翻译只落按用户工作行且无 observation，管理端与活动图把它合成为 `legacy_conflict`；`REQ-PGCP-010` 未实现（2026-11-25 实测 48/48 活动 cell 为中性）。
- 公开读：global 模式详情只读全局投影，翻译输出未投影，公开详情返回 `translated.status=missing`；切换后 366/366 条已完成翻译无缓存行。`REQ-PGCP-011` 未实现。
- 受众变体：全局 producer 只 admit `smart`，公开受众只读 `smart_public`；公开 `smart.status=missing`。
- 数据修复：需要把上述 366 条既有输出以零新增尝试转为投影，尚未开始。

## Related Changes

- PR #411：`fix(postgres): repair imported translation audit key`（`20dd61dd`, `f2157382`）。

## References

- `./SPEC.md`
- `./HISTORY.md`
