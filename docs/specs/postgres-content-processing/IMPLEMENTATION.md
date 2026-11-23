# PostgreSQL 全局内容处理运行时 实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 进行中
- Lifecycle: active
- Catalog note: PostgreSQL 原生 producer/worker 分片交付；content schema 与 Release/公告读投影已就绪

## Implementation Coverage

- Requirement coverage:
  - `REQ-PGCP-001`: 待实现，计划模块 `src/postgres_content.rs`。
  - `REQ-PGCP-002`: 待实现，复用 `content_identity_upgrade` 身份与 `content_*` 表。
  - `REQ-PGCP-003`: 待实现，复用 `postgres_translate` 的 provider 客户端与审计写法。
  - `REQ-PGCP-004`: 待实现，复用 `llm_model_health`、`llm_recovery_flags` 与既有退避常量。
  - `REQ-PGCP-005`: 待实现；`postgres_content_overlay` 已覆盖 Release/公告的 translation/polishing 投影；通知当前走翻译请求/读取链路，其适配属于本主题。
  - `REQ-PGCP-006`: 待实现；当前生产仍有 `translation.generate` 独自运行。
  - `REQ-PGCP-007`: 读接口已存在；待新写入后验证。
  - `REQ-PGCP-008`: 待实现。
  - `REQ-PGCP-009`: 待实现。
- Verification commands: 见各分片测试；PostgreSQL 集成测试沿用 `TEST_PG_TRANSLATE_URL` 隔离库模式。
- Rollout facts: 生产 `content_processing_control.mode=global`；`content_current_result_projections` 12,023 行，最后发布 `2026-11-21T07:54:28Z`；切换后无新投影。

## Coverage / rollout summary

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

## Remaining Gaps

- 第 2–7 分片未开始。

## Related Changes

- PR #411：`fix(postgres): repair imported translation audit key`（`20dd61dd`, `f2157382`）。

## References

- `./SPEC.md`
- `./HISTORY.md`
