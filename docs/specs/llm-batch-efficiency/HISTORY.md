# 演进记录（LLM 批处理效率改造）

## 生命周期

- Lifecycle: active
- Created: 2026-02-25
- Last: 2026-06-25

## 变更记录

- 2026-02-25: 创建规格并冻结“固定信源 + 固定同步 + 固定兜底 + 单环境变量”约束。
- 2026-02-25: 完成后端批处理核心与前端自动翻译批调接入；前端构建受缺失依赖阻塞，待补依赖后复验。
- 2026-03-30: 完成 release feed visible-window 结果聚合接口、request/work item 双层去重、Storybook 场景与 Playwright 回归验证。
- 2026-06-24: 为 `POST /api/translate/results` 增加运行时背压快路径；在 writer / worker / LLM 已显式拥塞时优先复用当前 source hash 的 `queued/running` 快照，避免重复 resolve 放大 SQLite 写竞争。
- 2026-06-25: 补齐 release detail follow-up：request-based `wait` 调用在 `max_wait_ms` 预算耗尽后立即返回单-request pending 快照；批次内 retryable upstream `429` / rate-limit / plan exhaustion 改为回排到 `queued`，不再默认沉成终态错误。
