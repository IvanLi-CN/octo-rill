# 演进记录（管理员任务中心：LLM 调度观测与调用排障）

## 生命周期

- Lifecycle: active
- Created: 2026-02-27
- Last: 2026-02-27

## 变更记录

- 2026-02-27: 创建规格并冻结实现边界。
- 2026-02-27: 完成后端/前端实现，新增迁移与管理员 LLM 观测页面，并通过 `cargo test` + `web build` + `admin-jobs e2e` 验证。
- 2026-02-27: 增补多轮消息 JSON 展示与 token 指标（含 cached tokens），扩展 llm_calls schema 与 admin API/UI。
- 2026-02-27: 增补首字等待时间（first token wait）落库与详情/列表展示，用于排查模型首包延迟。
- 2026-02-27: 新增 `llm_call_events` 与 `llm.call` SSE 事件，支持后台页面对 LLM 调用列表/状态/详情的实时刷新。
