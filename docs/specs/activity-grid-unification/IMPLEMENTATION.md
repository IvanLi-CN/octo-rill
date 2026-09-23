# 统一活动图组件与探索交互实施覆盖

## Current Status

- Lifecycle: active
- Implementation: implemented in the working tree; final delivery validation is complete for the shared grid and Demo data-case controls.

## Coverage Map

| Requirement | Intended implementation surface | Status |
| --- | --- | --- |
| REQ-ACTIVITY-GRID-001 | shared `ActivityGrid` component and both activity adapters | implemented |
| REQ-ACTIVITY-GRID-002 | shared model, layout and scroll-policy contracts | implemented |
| REQ-ACTIVITY-GRID-003 | shared density tokens and responsive layout | implemented |
| REQ-ACTIVITY-GRID-004 | collection page flow and LLM panel flow | implemented |
| REQ-ACTIVITY-GRID-005 | layout-matched loading skeleton and stale-refresh state | implemented |
| REQ-ACTIVITY-GRID-006 | pointer, keyboard and Canvas accessibility interaction | implemented |
| REQ-ACTIVITY-GRID-007 | touch exploration, page-scroll handoff and safe preview | implemented |
| REQ-ACTIVITY-GRID-008 | shared preview positioning and adapter-provided content | implemented |
| REQ-ACTIVITY-GRID-009 | route-state stability and independent detail reads | implemented |
| REQ-ACTIVITY-GRID-010 | collection and LLM behavior-preservation coverage | implemented |
| REQ-ACTIVITY-GRID-011 | Demo Inspector Data case control, delayed loading and state-scoped caches | implemented |

## Verification Coverage

The implementation provides the verification evidence named in `SPEC.md`: unified rendering, density and scroll behavior, loading geometry, desktop interaction, touch interaction, request isolation, and Demo data-case switching. The dense same-hour Demo case is also captured from Ego Lite in `assets/admin-jobs-many-dense-ego.png` and copied to owner-inline assets.

## Remaining Gaps

- The repository does not provide `bin/spec_contract_check.py`; the spec drift check passes, but the optional contract checker cannot run.

## Related Changes

None.
