# Repository governance

This document defines the repository-level workflow and GitHub configuration for OctoRill maintainers.

## Protected branch

`main` is the production branch. It represents the source used for releases, GitHub Pages assembly, and container publication automation.

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

Release automation is responsible for:

- computing the effective version;
- creating or repairing the GitHub Release;
- publishing the GHCR image tags for that version.

Release automation is not the production rollout mechanism. OctoRill production reconciliation on machine 101 is host-side ops automation maintained under `/home/ivan/srv/octo-rill/`; if a GitHub Release exists but the live version is stale, maintainers should inspect the matched deployment card and rollout timer on the host instead of expecting an in-repo GitHub Actions deploy job.

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
