# CI 墙钟性能验收实现状态

## Current Status

- Implementation: 已完成；本地合同验证、GitHub-hosted Docker runtime smoke 与受控性能验收均通过
- Lifecycle: active
- Catalog note: fast-track / controlled A/B acceptance

## Implementation Coverage

- Requirement coverage: `REQ-CI-WALLCLOCK-001` and `REQ-CI-WALLCLOCK-002` are implemented in `.github/workflows/ci.yml` and checked by `.github/scripts/check_quality_gates_contract.py`; `REQ-CI-WALLCLOCK-003` and `REQ-CI-WALLCLOCK-004` are implemented by `.github/scripts/ci_performance_acceptance.py` and its offline test.
- Verification commands: `bash .github/scripts/test-quality-gates-contract.sh`, `bash .github/scripts/test-ci-performance-acceptance.sh`, `ADR_REFS=none bash /Users/ivan/.codex/skills/spec-sync/scripts/spec_drift_check.sh --base-ref 03bf0b9fa19bfb5b416a90a9db7a10a1ff32f789 --spec-path docs/specs/ci-wall-clock-acceptance/SPEC.md`.
- Rollout facts: normal CI triggers remain active; `ci_performance_acceptance` defaults to `false`; controlled refs and dispatch are owner-authorized operational steps and are not run automatically.

## Coverage / rollout summary

- The release job builds once through Docker Buildx with `load: true`, then verifies the loaded image through a temporary runtime container.
- The Python driver resolves and freezes both ref SHAs before serial dispatch, verifies the shared E2E stabilization blob, and writes auditable run/job records and statistics.

## Controlled Acceptance

- Ten serial control/candidate pairs completed with 20/20 runs successful, `run_attempt=1`, all required jobs successful, and 10/10 candidate Docker smoke steps successful.
- Control median/P90 were 1057/1167 seconds; candidate median/P90 were 564.5/598 seconds; the candidate median ratio was 0.534. All configured thresholds passed.

## Remaining Gaps

- None for the approved implementation and controlled acceptance scope.

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
