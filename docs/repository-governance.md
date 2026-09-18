# Repository governance

This document defines the repository-level workflow and GitHub configuration for OctoRill maintainers.

## Protected branch

`main` is the production branch. It represents the source used for releases, GitHub Pages assembly, and deployment-oriented automation.

The `Main Branch Quality Gate` ruleset protects `main` with these guarantees:

- every production change reaches `main` through a pull request;
- every merged commit is signed and verifiable;
- each pull request is tested against the latest `main`;
- every release-bearing pull request declares exactly one `type:*` label and one `channel:*` label;
- the review policy gate records whether the author or an eligible reviewer satisfies review requirements;
- GitHub can update `main` through the pull request merge path after the required gates pass.

## Required checks

`.github/quality-gates.json` is the machine-readable source for required checks. The GitHub ruleset mirrors that file.

Required pull request checks:

- `Release intent label gate`
- `Lint & Checks`
- `Backend Tests`
- `Frontend E2E`
- `Worktree Bootstrap Smoke (ubuntu-latest)`
- `Worktree Bootstrap Smoke (macos-latest)`
- `Build (Release)`
- `Review Policy Gate`

The expected workflow owners are:

- `PR Label Gate` owns `Release intent label gate`;
- `CI Pipeline` owns lint, backend, frontend, worktree bootstrap, and release build checks;
- `Review Policy` owns `Review Policy Gate`.

## Test responsibility by delivery stage

The repository separates low-latency commit feedback from complete delivery evidence. The policy below is adopted and implemented in the checked-in hook configuration.

| Stage | Responsibility | Boundary |
| --- | --- | --- |
| Ordinary `git commit` | `pre-commit` formats Rust and stages fixes, runs web lint when applicable, and `commit-msg` runs the existing commitlint. | No `cargo test`, no Clippy, and no full-test `pre-push` hook. |
| Explicit local validation | A developer may run targeted tests or the host-equivalent complete checks: format check, Clippy, locked all-targets check, locked all-features tests, and applicable web lint/build/Storybook/E2E. | These checks run only when explicitly requested; a successful commit does not claim they ran. |
| Topic branch push without a PR | No new complete-test workflow is required. | Do not add a topic-branch push workflow solely to compensate for removing commit-time full tests. |
| Push updating a PR | The existing PR synchronization trigger starts the complete CI gate for the new PR head. | The gate is bound to the latest head SHA. |
| PR or merge group | Required checks run through the existing PR/merge-group workflows. | Hook, workflow, and documentation changes use the same gate as product changes. |
| `main` push after merge | CI runs again for the merged target SHA. | This is the release precondition; it is not a replacement for the PR gate. |
| Release | `release.yml` waits for successful CI for the same target SHA before release metadata, GitHub Release, or Docker image publication. | A release must stop when that CI or a release-specific step fails. |

The complete CI gate currently consists of `Release intent label gate`, `Lint & Checks`, `Backend Tests`, `Frontend E2E`, both `Worktree Bootstrap Smoke` checks, `Build (Release)`, and `Review Policy Gate`. Docker smoke, browser E2E, cross-platform bootstrap, and controlled performance acceptance are heavy validations owned by CI or `$shared-testbox`; none is an implicit ordinary-commit or local pre-push responsibility.

There is no path-based exemption for workflow, hook, governance, or documentation changes. They affect repository delivery behavior and therefore use the same PR gate. The existing CI and release workflow definitions, `.github/quality-gates.json`, GitHub ruleset, and hook installation script are not changed by this policy decision.

## Failure handling

- A fast hook failure blocks the commit. Fix the reported issue and retry; `--no-verify` is not a normal recovery path.
- A failed explicit local check is reported as incomplete local validation. It does not require an automatic rollback and must not be represented as passing evidence.
- A failed PR or merge-group required check blocks merge. Code failures require a new fix and head SHA; a confirmed infrastructure failure may be retried on the same SHA.
- A failed `main` push CI blocks release automation. No release image or publication should proceed until the target SHA has successful CI, either through the normal flow or an explicitly authorized backfill.
- A release-specific Docker build, publication, or metadata failure stops delivery. Preserve the failure evidence and diagnose the release step before retrying through the existing release workflow.
- An emergency hook bypass requires explicit owner authorization and never bypasses PR or `main` CI.

## Implementation verification

The hook migration is implemented in `lefthook.yml`, and the resulting delivery contract is verified as follows:

1. The checked-in hook removes only the `pre-commit` Clippy and Rust all-features test commands. Rust format auto-staging, web lint, commitlint, parallel execution, and hook installation behavior remain unchanged, and there is no `pre-push` hook.
2. Static checks confirm that the hook has no `cargo test` or `cargo clippy` command and no `pre-push` section. Local verification is limited to the permitted fast checks and documentation checks; the Rust full suite, full Playwright, Docker, and Compose are not implicit workstation checks.
3. Open a PR and require every current required check to pass for the latest head SHA. Confirm that hook, workflow, and documentation changes are covered without a path exemption.
4. After merge, confirm that `main` push CI succeeds for the merged SHA and that `release.yml` waits for that same SHA before any release publication step.
5. If a quick check fails, repair the quick-check change. If PR CI fails, distinguish code failures from transient infrastructure failures before retrying. If `main` CI or release fails, stop publication and continue only through the existing repair or authorized backfill path.

The migration may be reverted or repaired if it causes a quick-hook regression. Restoring full tests to ordinary `pre-commit` is not the default rollback because it recreates the policy violation; any exception requires a new owner decision.

## Controlled CI performance acceptance

`CI Pipeline` exposes a manual `workflow_dispatch` boolean input named
`ci_performance_acceptance`. It defaults to `false`; only an explicitly enabled
dispatch runs the acceptance path. Normal pull request, merge-group, and
`main` push triggers remain unchanged, including the `Build (Release)` required
check name.

The acceptance driver uses two owner-prepared refs whose commit SHAs are
resolved immediately before dispatch. The control ref contains the dispatch
input change and the verified E2E stabilization file shared by both arms; the
candidate ref is derived from control and contains the approved CI workflow,
contract-checker, offline-fixture, driver, test, and documentation paths. The
driver verifies the shared file blob is identical, then rejects mutable or
unexpected refs, retries, concurrent runs, unexpected SHAs, failed required
jobs, and candidate runs without a successful Docker runtime smoke step.

Each pair is dispatched serially in alternating control/candidate order for ten
pairs. It records the workflow run, jobs, and terminal timestamps as JSON. The
candidate passes only when all ten runs succeed, candidate median wall-clock is
at most 720 seconds, nearest-rank P90 is at most 840 seconds, and candidate
median is at most 75% of control median. The measurement interval is
`run_started_at` through `updated_at`; median uses the average of sorted values
five and six, and P90 uses sorted value nine.

Preparing/pushing the temporary control ref, dispatching the acceptance runs,
and deleting temporary refs are separately authorized operational actions. They
must use synthetic credentials and temporary SQLite only; no production data,
secrets, or external services are permitted.

## Pull request workflow

Feature work starts on a topic branch and lands through a pull request targeting `main`.

Before merge, maintainers confirm:

- the pull request is ready for review;
- the branch is current with `main`;
- required checks are successful for the latest head commit;
- review policy has passed;
- release intent labels match the planned release channel and version impact;
- documentation reflects stable project truth when the change affects product behavior, architecture, operations, or repository workflow.

## Release automation

Release automation runs from `push` events on `main` and can also be backfilled with `workflow_dispatch(head_sha)`.

The release planner scans first-parent commits on `main`, resolves each commit to its pull request, and uses the PR `type:*` and `channel:*` labels as release intent. This includes merge commits, squash/direct PR commits, and rebase-merged PR commits. When multiple mainline commits resolve to the same PR, the planner keeps the PR's last mainline commit so each release-bearing PR contributes at most one candidate to the backfill and repair queue.

## Review policy

Review policy is enforced by `Review Policy Gate`.

The policy values live in `.github/quality-gates.json`:

- repository owners and maintainers satisfy review policy through author permission;
- external contributors satisfy review policy through an approval from an eligible reviewer;
- eligible reviewers have `write`, `maintain`, or `admin` permission.

## Maintaining the ruleset

When GitHub repository settings change, maintainers update the project truth in this order:

1. Update `.github/quality-gates.json`.
2. Update workflow files under `.github/workflows/` when check ownership changes.
3. Update this document.
4. Run the quality-gate contract checks through the normal CI path.

The GitHub ruleset and `.github/quality-gates.json` describe the same policy from two angles: GitHub enforces the live repository rules, and the JSON file keeps the intended policy reviewable in code.
