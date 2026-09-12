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
- Short repository lists fill the measured space above the fixed footer; longer lists scroll inside the repository list.

## Remaining Gaps

- None known.

## Related Changes

- None.

## References

- `./SPEC.md`
- `./HISTORY.md`
