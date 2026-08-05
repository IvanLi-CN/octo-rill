# 演进记录（日报快照与时区去敏改造）

## 生命周期

- Lifecycle: active
- Created: 2026-04-13
- Last: 2026-07-15

## 历史摘要

- 2026-04-13: 建立该主题规格并冻结基础范围。
- 2026-04-13: 已交付；把 brief 升级为不可变快照：每条 brief 必须记录窗口、时区、本地边界与显式 release memberships。
- 2026-05-08: 修正 Dashboard raw fallback 日组标题，窗口仍按 08:00 边界归组，但用户可见日期显示为窗口结束日。
- 2026-07-05: `/api/briefs` 默认响应瘦身为摘要列表，移除完整 `content_markdown`；新增 `GET /api/briefs/{brief_id}` 供 Dashboard 选中 brief 后懒加载完整正文。
- 2026-07-15: 日报内容窗口统一改成“昨天自然日”口径；自动出报时间改为管理员全局运行时设置，`brief.date` 与 Dashboard 历史分组改为稳定表达被回顾的本地自然日。
- 2026-07-15: 复用 `brief.history_recompute` 重算所有非自然日口径的历史 brief，使快照窗口、`effective_local_boundary=00:00` 与前端折叠语义收敛为同一套 current truth。
