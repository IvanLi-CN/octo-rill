# CI 墙钟性能验收实现状态

## Current Status

- Implementation: 本地实现与离线合同验证通过；远端验收待单独授权
- Lifecycle: active
- Catalog note: fast-track / controlled A/B acceptance

## Implementation Coverage

- Requirement coverage: `REQ-CI-WALLCLOCK-001` and `REQ-CI-WALLCLOCK-002` are implemented in `.github/workflows/ci.yml` and checked by `.github/scripts/check_quality_gates_contract.py`; `REQ-CI-WALLCLOCK-003` and `REQ-CI-WALLCLOCK-004` are implemented by `.github/scripts/ci_performance_acceptance.py` and its offline test.
- Verification commands: `bash .github/scripts/test-quality-gates-contract.sh`, `bash .github/scripts/test-ci-performance-acceptance.sh`, `ADR_REFS=none bash /Users/ivan/.codex/skills/spec-sync/scripts/spec_drift_check.sh --base-ref 03bf0b9fa19bfb5b416a90a9db7a10a1ff32f789 --spec-path docs/specs/ci-wall-clock-acceptance/SPEC.md`.
- Rollout facts: normal CI triggers remain active; controlled refs and dispatch are owner-authorized operational steps and are not run automatically.

## Coverage / rollout summary

- The release job builds once through Docker Buildx with `load: true`, then verifies the loaded image through a temporary runtime container.
- The Python driver resolves and freezes both ref SHAs before serial dispatch, verifies the shared E2E stabilization blob, and writes auditable run/job records and statistics.

## Remaining Gaps

- GitHub-hosted Buildx/runtime smoke and the ten-pair control/candidate benchmark require a separately authorized remote run.

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
