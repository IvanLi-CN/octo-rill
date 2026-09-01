# CI 墙钟性能验收

## Context and Scope

- Context: CI Pipeline 的发布构建曾在无数据依赖的测试之后等待，并重复执行宿主 Rust release 编译。
- In scope: 发布构建的调度关系、Docker 发布物运行时 smoke、以及可审计的受控 control/candidate 墙钟验收。
- Out of scope: 应用 Rust/前端逻辑、Dockerfile、Playwright worker 数、持久化 Docker/Rust cache、GitHub 分支规则和真实部署凭据。

## Terms and Interfaces

- `Build (Release)`: CI 中保持原 required-check 名称的 Docker 发布物构建与运行时 smoke job。
- `CI_SMOKE_VERSION`: 每次 CI run 唯一的发布物版本值，必须由 health response 回显。
- `control ref`: 仅包含受控 dispatch 输入合同的不可变提交引用。
- `candidate ref`: 从 control ref 派生并包含本主题批准实现的不可变提交引用。
- `Acceptance driver`: 通过 `gh api` 串行 dispatch、轮询和记录 control/candidate runs 的 Python CLI。

## Requirements

### REQ-CI-WALLCLOCK-001

- The system MUST preserve the existing CI workflow triggers, required-check names, and same-SHA success requirement while allowing an explicitly enabled manual acceptance dispatch.
- Inputs: `workflow_dispatch.inputs.ci_performance_acceptance` is an optional boolean whose default is `false`.
- Outputs: `Build (Release)` runs for pull requests, merge groups, `main` pushes, or an enabled acceptance dispatch, without waiting on unrelated CI jobs.

### REQ-CI-WALLCLOCK-002

- The system MUST build one uniquely tagged, locally loaded Docker image and use that image for release runtime verification.
- Outputs: the smoke container starts with temporary SQLite and synthetic OAuth/encryption settings, `/api/health` returns `ok: true` and the exact `CI_SMOKE_VERSION`, and cleanup runs on success and failure.

### REQ-CI-WALLCLOCK-003

- The acceptance driver MUST make performance claims only from two owner-prepared refs whose immutable SHAs, allowed file delta, run attempts, ordering, jobs, and terminal timestamps are validated.
- Outputs: ten serial alternating control/candidate pairs are recorded as JSON; candidate passes only with ten successful Docker-smoke runs, median at most 720 seconds, nearest-rank P90 at most 840 seconds, and median at most 75% of control median.

### REQ-CI-WALLCLOCK-004

- The acceptance path MUST never use production secrets, persistent data, external application services, retries, concurrent dispatches, or mutable ref resolution after the preflight.

## Verification

### VER-CI-WALLCLOCK-001

- Method: `.github/scripts/test-quality-gates-contract.sh` and the quality-gates contract checker.
- covers: `REQ-CI-WALLCLOCK-001`, `REQ-CI-WALLCLOCK-002`
- Pass condition: reverse fixtures fail for `needs`, host release compilation, missing dispatch default, missing `load: true`, missing version assertion, or missing cleanup; the real workflow passes.

### VER-CI-WALLCLOCK-002

- Method: `.github/scripts/test-ci-performance-acceptance.sh` with an offline fake `gh` client.
- covers: `REQ-CI-WALLCLOCK-003`, `REQ-CI-WALLCLOCK-004`
- Pass condition: immutable SHA checks, ten-pair alternating serial order, run-attempt and job validation, statistics, and fail-closed cases are exercised without network access.

### VER-CI-WALLCLOCK-003

- Method: GitHub-hosted `Build (Release)` job followed by separately authorized `ci_performance_acceptance=true` dispatches.
- covers: `REQ-CI-WALLCLOCK-002`, `REQ-CI-WALLCLOCK-003`
- Pass condition: runtime smoke returns the exact version and the ten-pair thresholds are satisfied from recorded `run_started_at` through `updated_at` durations.

## Related ADRs

- None

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
- `../../repository-governance.md`
