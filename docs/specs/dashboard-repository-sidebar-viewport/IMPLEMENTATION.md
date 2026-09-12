# Dashboard 仓库侧栏视口高度实现状态

## Current Status

- Implementation: 已实现
- Lifecycle: active
- Catalog note: Dashboard root following sidebar and shared repository panel viewport contract.

## Implementation Coverage

- Requirement coverage: `REQ-DASHBOARD-REPOSITORY-SIDEBAR-001` -> `web/src/pages/Dashboard.tsx`; `REQ-DASHBOARD-REPOSITORY-SIDEBAR-002` and `REQ-DASHBOARD-REPOSITORY-SIDEBAR-003` -> `web/src/dashboard/useRepositoryPanelViewportHeight.ts`; `REQ-DASHBOARD-REPOSITORY-SIDEBAR-004` -> Dashboard route, demo transport, Storybook, and E2E coverage.
- Verification commands: Dashboard lint/build, Storybook tests, Spec contract check, and scoped Dashboard Playwright coverage.
- Rollout facts: The behavior is client-only and reuses the existing following and personal repository responses.

## Coverage / rollout summary

- Desktop root following sidebar is rendered at the existing `lg` breakpoint.
- Every desktop repository list reserves two rendered project-card heights above the fixed footer. Lists shorter than two cards fill to the footer boundary; all other lists retain natural height until that boundary. When a compact viewport cannot fit the two-card minimum alongside normal panel chrome, the non-list panel content compacts and longer lists scroll inside the repository list without changing height during normal document scrolling.

## Remaining Gaps

- None known.

## Related Changes

- None.

## References

- `./SPEC.md`
- `./HISTORY.md`
