# 演进记录（翻译请求单条记录制重建）

## 生命周期

- Lifecycle: active
- Created: 2026-03-09
- Last: 2026-03-10

## 变更记录

- 2026-03-09: 新增 follow-up spec，明确 request 层单条化、破坏式迁移与公开 API breaking changes；替代旧 spec 中 request-stage aggregation 的描述。
- 2026-03-09: 实现与验证同步完成：后端迁移/调度逻辑切到单 request 直连 work item，前端与 fixtures 改为 singular result，并补跑 Rust、web build、Playwright 与 Storybook build。
- 2026-03-09: 创建 PR #35，补上发布标签门禁与类型窄化修复；当前 checks 全绿、review-loop 无阻塞项，状态更新为 `已完成`。
- 2026-03-10: 补上 work item 去重竞争保护，并将 `wait` 语义收敛为“最多等待 `max_wait_ms` 后返回当前单结果快照”，前端调用方同步增加超时保护。
