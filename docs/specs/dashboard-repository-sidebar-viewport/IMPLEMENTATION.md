# Dashboard 仓库侧栏视口高度实现状态

## Current Status

- Implementation: 已实现
- Lifecycle: active
- Catalog note: Focus repository panel viewport contract with the root Dashboard Inbox boundary.

## Implementation Coverage

- Requirement coverage: `REQ-DASHBOARD-REPOSITORY-SIDEBAR-001` -> `web/src/pages/Dashboard.tsx`; `REQ-DASHBOARD-REPOSITORY-SIDEBAR-002` and `REQ-DASHBOARD-REPOSITORY-SIDEBAR-003` -> `web/src/dashboard/useRepositoryPanelViewportHeight.ts`; `REQ-DASHBOARD-REPOSITORY-SIDEBAR-004` -> Dashboard route, demo transport, Storybook, and E2E coverage. The root Dashboard `全部` tab keeps its Inbox quick list and does not consume the repository sidebar query.
- Verification commands: Dashboard lint/build, Storybook tests, Spec contract check, and scoped Dashboard Playwright coverage.
- Rollout facts: The behavior is client-only and reuses the existing following and personal repository responses.

## Coverage / rollout summary

- Focus following and mine repository sidebars are rendered at the existing `lg` breakpoint.
- The root Dashboard `全部` tab renders the Inbox quick list and leaves following repository data to scoped routes.
- Every scoped desktop repository list reserves two rendered project-card heights. Lists shorter than two cards fill to the footer boundary when that space is available; all other lists retain natural height until the preferred footer cap. When a compact viewport cannot fit the two-card minimum alongside normal panel chrome, the non-list panel content compacts and the panel expands to contain the complete two-card minimum; longer lists scroll inside the repository list without changing height during normal document scrolling.

## Remaining Gaps

- None known.

## Related Changes

- None.

## References

- `./SPEC.md`
- `./HISTORY.md`
