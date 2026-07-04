# 实现状态（Dashboard 按日报边界分组与历史日报折叠）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-04
- Last: 2026-07-03
- Summary: 已交付；top-level tab copy aligned to 发布; scoped focus 页不继承全局历史日组的“生成日报”动作
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/product.md`
- `docs/specs/README.md`
- `docs/specs/xaycu-dashboard-day-grouping/SPEC.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/xaycu-dashboard-day-grouping/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 spec 并冻结 Dashboard 分组/折叠口径。
- [x] M2: 落地 Dashboard 日分组、历史日报折叠与启动期边界配置。
- [x] M3: 更新 Storybook、完成视觉证据并通过前端校验。
- [x] M4: 明确 scoped focus 页只读聚焦语义，不显示历史日组“生成日报”动作。
