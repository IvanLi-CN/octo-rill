# 实现状态（Dashboard 按日报边界分组与历史日报折叠）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-04
- Last: 2026-07-15
- Summary: 已交付；Dashboard `发布 / 全部 / 日报` 与历史折叠统一对齐本地自然日分组，`/api/me.dashboard.daily_boundary_local` 固定为 `00:00`，设置页不再把它误读成自动出报时间。
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/product.md`
- `docs/specs/README.md`
- `docs/specs/dashboard-day-grouping/SPEC.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/dashboard-day-grouping/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 spec 并冻结 Dashboard 分组/折叠口径。
- [x] M2: 落地 Dashboard 日分组、历史日报折叠与启动期边界配置。
- [x] M3: 更新 Storybook、完成视觉证据并通过前端校验。
- [x] M4: 明确 scoped focus 页只读聚焦语义，完全隔离已有日报、覆盖关系与生成状态，只显示原始 scope 信息流。
