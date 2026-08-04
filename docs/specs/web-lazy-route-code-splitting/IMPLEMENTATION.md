# 实现状态（Web 前端懒路由与按需拆包）

## 当前状态

- Lifecycle: active
- Implementation: 待实现
- Created: 2026-04-20
- Last: 2026-04-20
- Summary: 待实现；fast-track / lazy routes + branch-level split
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 启用 Router 自动拆包，并抽离会导致主包回流的 route-state helpers。
- [x] M2: `/`、`/settings`、`/admin*` 完成 lazy route / branch split 与 pending fallback。
- [x] M3: Storybook 与 Playwright 补齐 lazy loading 验证入口。
- [ ] M4: 构建、视觉证据、review-loop、PR 合并与 cleanup 收口。
