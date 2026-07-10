# Authenticated Scoped Focus Feed Implementation

## Status

- Lifecycle: active
- Delivery mode: fast-track
- Current state: scoped feed 已隔离全局日报数据，等待 merge-ready 收口

## Scope coverage

- [x] 后端 `scope=repo|repos|org|mine` 契约扩展到 `/api/feed` 与 `/api/dashboard/updates`
- [x] `DashboardRouteState` / release detail round-trip / canonical scoped routes
- [x] feed 请求、live updates、warm snapshot scope-aware
- [x] repo-bearing feed 卡片站内跳转 focus route
- [x] 账号菜单常驻“个人仓库”入口
- [x] `/api/me/personal-repos` 返回当前 session GitHub viewer 的完整个人仓库清单
- [x] `/focus/mine` summary 使用后端个人仓库总数与右侧完整仓库列表，不再从当前 feed 首屏去重倒推
- [x] `/focus/mine` feed 使用同一 viewer owner repo baseline 作为聚焦范围，仍只展示真实动态
- [x] scoped shell：双 tab + summary sidebar/mobile summary + empty state
- [x] 单仓 focus 摘要卡公开 Release 页入口与 private owner repo 发布/取消发布控件
- [x] scoped `全部` 页完全忽略全局日报及其覆盖关系，屏蔽已有日报、生成动作、pending/error 状态并保留完整历史原始列表
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
  - `pages-dashboard--scoped-focus-mine-personal-repos`
  - `dashboard-repopublicreleasecontrols--public-repo`
  - `dashboard-repopublicreleasecontrols--private-unpublished`
  - `dashboard-repopublicreleasecontrols--private-published`
