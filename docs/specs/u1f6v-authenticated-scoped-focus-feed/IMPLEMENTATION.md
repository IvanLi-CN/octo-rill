# Authenticated Scoped Focus Feed Implementation

## Status

- Lifecycle: active
- Delivery mode: fast-track
- Current state: 验证完成，等待 merge-ready 收口

## Scope coverage

- [x] 后端 `scope=repo|repos|org|mine` 契约扩展到 `/api/feed` 与 `/api/dashboard/updates`
- [x] `DashboardRouteState` / release detail round-trip / canonical scoped routes
- [x] feed 请求、live updates、warm snapshot scope-aware
- [x] repo-bearing feed 卡片站内跳转 focus route
- [x] 账号菜单 gated “我的仓库动态”入口
- [x] scoped shell：双 tab + summary sidebar/mobile summary + empty state
- [x] Storybook page/app-shell fallback 场景
- [x] E2E 覆盖与最终视觉证据

## Validation target

- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `cd web && bun run e2e -- dashboard-scoped-focus.spec.ts settings.spec.ts release-detail.spec.ts`

## Notes

- 本 spec 继承 `#2x7av` 的 route/deep-link contract，并复用 `#w5gaz` 的 `mine` / owner baseline 语义，不回写它们的主题边界。
- 最终视觉证据来自 Storybook page/app-shell fallback：
  - `pages-dashboard--scoped-focus-repo-all`
  - `pages-dashboard--scoped-focus-repo-releases`
  - `pages-dashboard--scoped-focus-mobile-summary`
  - `pages-dashboard--scoped-focus-empty-state`
  - `pages-dashboard--scoped-focus-mine-menu-entry-visible`
