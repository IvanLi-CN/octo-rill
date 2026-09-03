# 演进记录（统一翻译调度器与独立管理界面改造）

## 生命周期

- Lifecycle: active
- Created: 2026-03-07
- Last: 2026-09-04

## 历史摘要

- 2026-03-07: 建立该主题规格并冻结基础范围。
- 2026-03-27: 已交付；PR #38; unified request scheduler + stale runtime recovery completed
- 2026-06-25: 同步 scheduler spec：移除已被 `#apras` 取代的旧表口径，并明确 `wait` 预算耗尽后的 pending 快照语义。
- 翻译与润色可恢复失败改为持久化错误状态和固定恢复梯度；批次存在失败或缺失项时保持 `partial`，历史未分类失败仅人工重试。
- 2026-09-04: 为结构化翻译与润色 work item 增加追加式、元数据专用的尝试审计；“内容处理”按 Release、公告、日报提供分页记录和尝试详情，旧记录不回填发现时间或尝试历史。
