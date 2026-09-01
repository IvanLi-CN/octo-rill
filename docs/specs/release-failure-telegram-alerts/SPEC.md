# Release 失败 Telegram 告警接入

## Context and Scope

### Context

`octo-rill` needs a repository-local notification path for failed Release runs. The notification path covers both automatic `workflow_run` failures and the existing manual smoke entry point, while the Release workflow retains its manual backfill failure behavior.

### Goals

- Route the three existing release-failure calls through the pinned Oidrune OIDC reusable workflow.
- Keep the existing trigger filters, failure predicates, Release context, and project-specific behavior.
- Make every caller provide the complete notification metadata required by Oidrune.

### In Scope

- `.github/workflows/notify-release-failure.yml` and its `notify_failure` and `smoke_test` jobs.
- `.github/workflows/release.yml` and its inline `notify-on-failure` job.
- The release-failure workflow contract tests and the associated implementation documentation.

### Out of Scope

- Release version calculation, tags, GitHub Releases, Docker images, or other Release jobs.
- Adding another notification channel or changing the existing workflow event filters.
- Oidrune gateway control-plane configuration or a real Telegram smoke notification.

## Requirements

- REQ-RELEASE-FAILURE-ROUTE: When a `Release` workflow fails on `main`, the notifier MUST preserve the existing `workflow_run` route and failure-only behavior.
- REQ-TRUSTED-OIDRUNE-REF: All three caller jobs MUST use `IvanLi-CN/oidrune/.github/workflows/notify.yml@e48822f99c6402a753ed86557ea029754cbab20b`; a moving ref such as `@main` MUST NOT be used.
- REQ-OIDC-PERMISSION: Each caller job MUST grant `id-token: write` to the reusable workflow.
- REQ-CALLER-SUMMARY: Each caller MUST pass `outcome` and a complete `summary` containing the project name, status/result, target SHA, run URL, and the applicable failure, smoke, or Release title.
- REQ-DEFAULT-GATEWAY: Callers MUST NOT pass `gateway_url` or `oidc_audience`, so Oidrune resolves its default gateway and audience.
- REQ-NO-LEGACY-SECRET: Callers MUST NOT pass the legacy `SHOUTRRR_URL` secret or any other old Telegram secret wiring.
- REQ-SMOKE-PATH: The existing `workflow_dispatch` smoke path MUST remain available and MUST call Oidrune with explicit smoke title/context.
- REQ-NON-MAIN-FILTER: The notifier MUST continue excluding non-`main` Release failures from production-style notification behavior.
- REQ-MANUAL-RELEASE-PREDICATE: The inline Release notifier MUST preserve its existing `needs` list and notify only when a relevant manual Release dependency fails.

## Verification

- VER-WORKFLOW-ROUTE: The workflow contract test covers: REQ-RELEASE-FAILURE-ROUTE, REQ-TRUSTED-OIDRUNE-REF, REQ-OIDC-PERMISSION, REQ-DEFAULT-GATEWAY, and REQ-NO-LEGACY-SECRET.
- VER-SUMMARY-METADATA: The workflow contract test covers: REQ-CALLER-SUMMARY by asserting project, status/result, target SHA, run URL, and title fragments for all three calls.
- VER-SMOKE-FILTER: The workflow contract test covers: REQ-SMOKE-PATH and REQ-NON-MAIN-FILTER by asserting the manual trigger and existing workflow-run predicates.
- VER-MANUAL-FAILURE: The workflow contract test covers: REQ-MANUAL-RELEASE-PREDICATE by asserting the unchanged `needs` list and failure conditions.

## Related ADRs

- None
