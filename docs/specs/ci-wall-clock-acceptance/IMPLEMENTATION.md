# CI 墙钟性能验收实现状态

## Current Status

- Implementation: 已完成；本地合同验证与 GitHub-hosted Docker runtime smoke 已通过，受控性能验收在 current `main` 工具层发布后执行
- Lifecycle: active
- Catalog note: fast-track / controlled A/B acceptance

## Implementation Coverage

- Requirement coverage: `REQ-CI-WALLCLOCK-001`, `REQ-CI-WALLCLOCK-001A`, and `REQ-CI-WALLCLOCK-002` are implemented in `.github/workflows/ci.yml`, `web/playwright.config.ts`, and the Playwright summary script, and checked by `.github/scripts/check_quality_gates_contract.py`; `REQ-CI-WALLCLOCK-003` and `REQ-CI-WALLCLOCK-004` are implemented by `.github/scripts/ci_performance_acceptance.py` and its offline test. Enabled acceptance runs use the dispatcher workflow SHA for the summary tool and checkout an explicit target SHA for every required job.
- Verification commands: `bun ./scripts/test-summarize-playwright-results.ts` (from `web`), `bash .github/scripts/test-quality-gates-contract.sh`, `bash .github/scripts/test-ci-performance-acceptance.sh`, `CI=1 bunx playwright test --list` (from `web`), and `git diff --check`.
- Rollout facts: normal CI triggers remain active; `ci_performance_acceptance` defaults to `false`; controlled target SHA, nonce, and dispatch are owner-authorized operational steps and are not run automatically.

## Coverage / rollout summary

- The release job builds once through Docker Buildx with `load: true`, then verifies the loaded image through a temporary runtime container.
- The Frontend E2E job uses two CI workers and emits list plus JSON reporter output. An always-run summary step appends Job Summary and uploads only the two JSON files for 14 days.
- The Python driver validates and freezes control/candidate target SHAs plus the `main` dispatcher SHA before serial dispatch, verifies strict ancestry and the restricted file delta, requires nonce-correlated runs and matching artifact `tested_sha`, derives deterministic test identifiers from each raw report, downloads each run's result artifact, and writes auditable run/job records and E2E statistics.

## Controlled Acceptance

- Ten serial control/candidate pairs are required. Every run must be successful with `run_attempt=1`, all required jobs successful, a valid `playwright-e2e-results` artifact, and 10/10 candidate Docker smoke steps successful.
- Acceptance statistics use the `Frontend E2E` job interval and compare deterministic test identifier sets, test totals, final failures, and retry totals between each control/candidate pair. Candidate E2E P90 is capped at 420 seconds.

## Remaining Gaps

- Controlled GitHub-hosted acceptance must run only after the dispatcher workflow is available on `main`; its result is the release evidence for the 10-pair contract.

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
