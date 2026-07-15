# 演进记录（Dashboard 按日报边界分组与历史日报折叠）

## 生命周期

- Lifecycle: active
- Created: 2026-04-04
- Last: 2026-07-15

## 历史摘要

- 2026-04-04: 建立该主题规格并冻结基础范围。
- 2026-04-16: 已交付；top-level tab copy aligned to 发布; spec wording refreshed for current dashboard labels
- 2026-06-29: 补充历史日组生成失败原位报错 / 重试的 current truth，并移除错误引入的“降级摘要”第四态表述。
- 2026-07-03: 明确历史日组“生成日报”只属于非 scoped 的全局 `全部` tab；scoped focus 页复用日组阅读结构但不暴露该生成动作。
- 2026-07-10: 收紧 scoped focus 日组合同：只复用日期边界分组，不再消费全局日报、日报覆盖关系或任何生成状态，所有 scope 原始动态保持可见。
- 2026-07-15: Dashboard 启动配置改为固定下发 `daily_boundary_local=00:00`，Feed 与历史折叠改按本地自然日对齐，不再把用户的自动出报时间当成分组边界。
