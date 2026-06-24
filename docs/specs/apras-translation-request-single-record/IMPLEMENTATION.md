# 实现状态（翻译请求单条记录制重建）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-03-09
- Last: 2026-06-25
- Summary: 已交付；single-request contract live; release-detail wait pending snapshot and retryable-upstream requeue follow-up synced
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: follow-up spec 与 breaking contracts 冻结。
- [x] M2: 迁移与后端 request/work-item/batch 逻辑重建，移除 request-item/watcher 依赖。
- [x] M3: 前端 API、feed 自动翻译与 admin 请求视图切换到单条 request 语义。
- [x] M4: 测试、快车道 PR、checks 与 review-loop 收敛。
- Follow-up contract sync：Release Detail `wait` 路径现以 `max_wait_ms` 为严格同步等待上限；命中 pending 时前端立即转入后台轮询。批次内若出现 retryable upstream `429` / rate-limit，则 request/work item 会回排到 `queued`，`GET /api/translate/requests/{request_id}` 继续暴露同一 request 的 pending 快照。
