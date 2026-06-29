# 实现状态（仓库刷新治理页与预算调度收敛）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-06-29
- Last: 2026-06-29
- Summary: 已交付；effective repo pool、10 分钟 budgeted governance snapshots、`/admin/repos` 独立治理页、预算编辑收口到订阅同步设置弹窗、Storybook 视觉证据与 build validation 已完成
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑

- [x] M1: schema / runtime config / governance snapshot tables
- [x] M2: system budgeted repo selection and cycle tracking
- [x] M3: `/api/admin/repos/overview` and `/api/admin/repos`
- [x] M4: `/api/admin/users` `repo_total` effective-pool semantics
- [x] M5: `/admin/repos` route, nav entry, budget entry hint, summary, activity grid, detail list
- [x] M6: Storybook fallback and owner-facing visual evidence
- [x] M7: final frontend build / storybook / e2e validation sweep

## 本轮收口

- `/admin/repos` 补齐概览/明细分离的 loading 与 error 状态，避免单一请求失败污染整页反馈。
- 活动图新增屏幕阅读器等价摘要与颜色图例，同时把页面与格子颜色迁回 token / dark-mode-safe 语义色。
- 预算入口从静态提示改为真实 CTA：跳转到任务中心订阅同步页签并自动展开“订阅同步设置”弹窗。
- “订阅同步设置”弹窗统一系统预算术语，放大帮助 icon 触达面积，并为 Release worker 刻度按钮补齐可访问标签。
- 治理页文案收口到中文优先表达，补充“颜色 / 目标窗口 / 迫切值”解码说明，并把仓库明细改成以排序、目标窗口与迫切值为先的比较型信息结构。
