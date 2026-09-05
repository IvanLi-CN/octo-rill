# Dashboard 页头品牌与账号菜单实现状态

> 当前有效规范以 `./SPEC.md` 为准；这里记录实现覆盖与 rollout 事实。

## Current Status

- Implementation: 已交付
- Lifecycle: active
- Catalog note: Dashboard 页头与账号菜单由同一前端组件提供。

## Implementation Coverage

- REQ-DASHBOARD-HEADER-001 / REQ-DASHBOARD-HEADER-004: `web/src/pages/DashboardHeader.tsx` 提供品牌位、主操作区与响应式布局；`web/src/stories/DashboardHeader.stories.tsx` 提供稳定审阅场景。
- REQ-DASHBOARD-HEADER-002: `DashboardUserMenu` 与 `DashboardUserInfoCard` 提供账号菜单及低频操作入口。
- REQ-DASHBOARD-HEADER-003: `DashboardUserMenu` 使用与账号菜单同宽的 hover bridge、短暂离开延迟及可中断的浮层入场/离场动画保护自然指针轨迹；离场动画的终态会保持到卸载，避免 CSS 动画结束后恢复初始样式而闪现。触摸输入不触发 hover 状态，减少动态效果时仅保留淡入淡出。
- Verification commands: `cd web && bun run build`、`cd web && bun run lint`、`cd web && bun run storybook:build`、`cd web && bun run e2e -- dashboard-access-sync.spec.ts --grep "dashboard keeps sync as a single header action for admins"`。
- Rollout facts: 不涉及 API、数据迁移或用户数据兼容性变更。

## Coverage / Rollout Summary

- 当前组件、Storybook 交互故事和 Playwright 路径共同覆盖账号菜单的可见性、布局与慢速指针进入行为。

## Remaining Gaps

- None.

## Related Changes

- 品牌优先页头初始交付与后续账号菜单 hover 可达性修复均保持在本主题合同内。

## References

- `./SPEC.md`
- `./HISTORY.md`
