# 演进记录（统一翻译调度器与独立管理界面改造）

## 生命周期

- Lifecycle: active
- Created: 2026-03-07
- Last: 2026-06-25

## 历史摘要

- 2026-03-07: 建立该主题规格并冻结基础范围。
- 2026-03-27: 已交付；PR #38; unified request scheduler + stale runtime recovery completed
- 2026-06-25: 同步当前真相到 scheduler spec：移除已被 `#apras` 取代的 `translation_request_items` / `translation_work_watchers` 口径，明确 `wait` 预算耗尽后返回单-request pending 快照，以及 release detail 在 retryable upstream `429` 下回排到 `queued` 而非沉成终态错误。
