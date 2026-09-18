# ADR 0011: Test Responsibilities by Delivery Stage

## Status

Accepted

## Context

The existing `pre-commit` hook runs the Rust all-features test suite together with formatting, Clippy, and web lint. That makes an ordinary commit unexpectedly expensive and duplicates the complete checks already owned by pull-request and `main` CI. The repository needs a durable boundary between fast commit feedback, deliberate local validation, and delivery gates.

## Decision

- Ordinary commits use only the fast hook contract: Rust formatting with staged fixes, web lint when `web/package.json` exists, and the existing commit-message lint. The `pre-commit` hook must not run `cargo test` or `cargo clippy`, and the repository does not add a full-test `pre-push` hook.
- Explicit local validation remains available to developers and is never inferred from an ordinary commit. The host-equivalent complete check consists of `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo check --locked --all-targets --all-features`, and `cargo test --locked --all-features`, with applicable web lint, build, Storybook, and E2E checks added for the changed surface.
- A push to a topic branch without a pull request does not start a new complete-test workflow. A push that updates an existing pull request is validated by the PR workflow through the existing `pull_request` synchronization trigger.
- Pull requests and merge groups use the existing required CI checks as the complete CI gate: lint and checks, backend tests, frontend E2E, both worktree-bootstrap smoke jobs, release build smoke, release-intent labeling, and review policy. Workflow, hook, and documentation changes use the same PR gate; they are not exempt merely because they do not change product code.
- A `main` push re-runs CI for the merged target SHA. Release automation waits for that same-SHA push CI to succeed before preparing a release or pushing a release image. It does not duplicate the complete test suite.
- Docker release smoke, browser E2E, cross-platform bootstrap, and controlled performance acceptance remain CI or `$shared-testbox` responsibilities. They are not hidden inside commit hooks.

## Considered Options

- Keeping `cargo test --all-features` and Clippy in `pre-commit`: rejected because every ordinary commit pays the complete-suite cost, while the same checks already exist as explicit delivery gates.
- Moving the complete suite to a local `pre-push`: rejected because it still makes a routine local action a duplicate gate and cannot replace CI's same-SHA, cross-platform, browser, and release-environment evidence.
- Adding a new CI workflow for every topic-branch push: rejected because it would duplicate the PR workflow without creating a stronger merge or release guarantee.
- Selectively skipping CI for documentation, hook, or workflow changes: rejected because those paths change the repository's delivery behavior and must be covered by the same policy contract.

## Consequences

Commits become predictable and low-latency, while complete validation remains mandatory at PR and delivery boundaries. Developers must explicitly run broader local checks when they need earlier feedback, and CI remains the authoritative evidence for merge and release decisions. The hook migration is implemented in the checked-in configuration, so ordinary commits now enforce the fast boundary described by this ADR.
