# CI 墙钟性能验收

## Context and Scope

- Context: CI Pipeline 的发布构建曾在无数据依赖的测试之后等待，并重复执行宿主 Rust release 编译。
- In scope: 发布构建的调度关系、Docker 发布物运行时 smoke、完整 Frontend E2E 的 CI worker/report 合同、现有翻译等待用例的确定性 fixture gate，以及可审计的受控 control/candidate 墙钟验收。
- Out of scope: 应用 Rust/前端逻辑、Dockerfile、Playwright 测试选择或删减、按文件分片、持久化 Docker/Rust cache、GitHub 分支规则和真实部署凭据。

## Terms and Interfaces

- `Build (Release)`: CI 中保持原 required-check 名称的 Docker 发布物构建与运行时 smoke job。
- `CI_SMOKE_VERSION`: 每次 CI run 唯一的发布物版本值，必须由 health response 回显。
- `Frontend E2E job`: 保持 required-check 名称不变的完整 Chromium Playwright job；CI 固定使用两个 worker，并保留测试级 retries。
- `Playwright result artifact`: 每次 Frontend E2E run 产生的 14 天 artifact，仅包含原始 JSON reporter 输出和确定性 JSON 摘要。
- `control target SHA`: 受控验收开始前验证并冻结的基线不可变提交。
- `candidate target SHA`: control 的受限严格后继；候选与 control 的差异只能落在本主题声明的 CI、报告/摘要、验收、确定性 E2E fixture、合同测试和主题文档路径。
- `acceptance dispatcher`: 当前 `main` 上承载稳定验收工具的 workflow SHA；每个受控 run 以唯一 nonce 标识，并 checkout 指定 target SHA 运行 required jobs。
- `Acceptance driver`: 通过 `gh api` 串行 dispatch、轮询和记录 control/candidate target runs 的 Python CLI。

## Requirements

### REQ-CI-WALLCLOCK-001

- The system MUST preserve the existing CI workflow triggers, required-check names, and same-SHA success requirement while allowing an explicitly enabled manual acceptance dispatch.
- Inputs: `workflow_dispatch.inputs.ci_performance_acceptance` is an optional boolean whose default is `false`; `ci_performance_target_sha` and `ci_performance_acceptance_nonce` are optional empty-default strings used only by enabled acceptance dispatches.
- Outputs: `Build (Release)` runs for pull requests, merge groups, `main` pushes, or an enabled acceptance dispatch, without waiting on unrelated CI jobs.

### REQ-CI-WALLCLOCK-001A

- The `Frontend E2E` job MUST keep the complete existing Chromium test selection, use `workers: 2` only when `CI` is set, and preserve `retries: 2` and the local default worker strategy.
- CI MUST emit both list output and `test-results/playwright-results.json`; an `always()` follow-up MUST write `test-results/playwright-summary.json` with the immutable `tested_sha`, total, final passed/failed/skipped, flaky, and retry counts.
- The job MUST append the summary to the GitHub Job Summary and upload only those two JSON files as `playwright-e2e-results` with `retention-days: 14`; report collection and upload steps MUST remain on test failure, and upload failure MUST NOT replace the original test failure.
- An enabled acceptance dispatch MUST checkout its target SHA for the test workload and use the dispatcher workflow SHA only for the deterministic summary tool. It MUST force list plus JSON reporting so historical targets that predate the JSON reporter still produce the same artifact contract.

### REQ-CI-WALLCLOCK-002

- The system MUST build one uniquely tagged, locally loaded Docker image and use that image for release runtime verification.
- Outputs: the smoke container starts with temporary SQLite and synthetic OAuth/encryption settings, `/api/health` returns `ok: true` and the exact `CI_SMOKE_VERSION`, and cleanup runs on success and failure.

### REQ-CI-WALLCLOCK-003

- The acceptance driver MUST make performance claims only from two target SHAs whose immutable existence, strict control-to-candidate ancestry, allowed file delta (including only the declared deterministic E2E fixture path), dispatcher SHA, nonce-correlated runs, run attempts, ordering, required jobs, Frontend E2E job timestamps, matching deterministic Playwright test identifier sets, and Playwright result artifacts are validated. The nonce is the unique run correlation authority; timestamp fields are retained as evidence, not used to reject a nonce-matched run because GitHub exposes them at second precision.
- Outputs: ten serial alternating control/candidate pairs are recorded as JSON; candidate passes only with ten successful Docker-smoke runs, Frontend E2E job nearest-rank P90 at most 420 seconds, zero final failed tests, candidate retry total no greater than control, and median at most 75% of control median.

### REQ-CI-WALLCLOCK-004

- The acceptance path MUST never use production secrets, persistent data, external application services, retries, concurrent dispatches, or mutable target or dispatcher resolution after preflight. Any `main` movement that changes a dispatched run's head SHA fails the acceptance.

## Verification

### VER-CI-WALLCLOCK-001

- Method: `.github/scripts/test-quality-gates-contract.sh` and the quality-gates contract checker.
- covers: `REQ-CI-WALLCLOCK-001`, `REQ-CI-WALLCLOCK-002`
- Pass condition: reverse fixtures fail for `needs`, host release compilation, missing dispatch default, missing `load: true`, missing version assertion, missing cleanup, or broken Frontend E2E report/artifact collection; the real workflow passes.

### VER-CI-WALLCLOCK-002

- Method: `.github/scripts/test-ci-performance-acceptance.sh` with an offline fake `gh` client.
- covers: `REQ-CI-WALLCLOCK-003`, `REQ-CI-WALLCLOCK-004`
- Pass condition: immutable target/dispatcher SHA checks, nonce correlation across second-resolution run timestamps, restricted delta, ten-pair alternating serial order, run-attempt/job validation, tested-SHA artifact parsing, deterministic test-identifier and test-count parity, retry/final-failure thresholds, statistics, and fail-closed cases are exercised without network access.

### VER-CI-WALLCLOCK-003

- Method: GitHub-hosted `Build (Release)` job followed by separately authorized `ci_performance_acceptance=true` dispatches.
- covers: `REQ-CI-WALLCLOCK-002`, `REQ-CI-WALLCLOCK-003`
- Pass condition: runtime smoke returns the exact version and the ten-pair thresholds are satisfied from each `Frontend E2E` job's `started_at` through `completed_at` duration, with artifact summaries proving the test-level outcome contract.

## Related ADRs

- None

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
- `../../repository-governance.md`
