# ADR 0001: LLM Recovery Boundary

## Status

Accepted

## Context

LLM failures were previously treated as one retryable path. That made empty responses, transient transport failures, rate limits, and configuration errors indistinguishable. Translation and polishing work could also disappear from the business result while the executor itself reported success.

## Decision

- Parse upstream failures once at the integration boundary into exactly four safe classes: `empty_content`, `transient`, `rate_limited`, and `configuration`.
- A logical call may use each configured candidate at most once for route recovery, with a hard total attempt cap. Configuration failures end the call without automatic recovery.
- Persist route health in a rolling ten-minute window. Two relevant failures cool a route for ten minutes; success does not erase the window history. The persisted health table is the restart source of truth.
- Persist final route, fallback count, failure class, recovery timestamps, and per-attempt events. External responses expose only the safe class and truthful business state.
- Translation and polishing failures with a structured recoverable class are scheduled at one, five, fifteen, sixty, and two-hundred-forty minute intervals, then every two-hundred-forty minutes until twenty-four hours. A source hash change cancels the old schedule. Unclassified historical failures remain manual-only.
- A completed executor batch with any failed or missing item is a `partial` business result.
- Recovery is behind a default-off canary flag. Static connectivity checks and any live enablement require separate owner approval.

## Consequences

Operators can distinguish a real failure from a scheduled recovery and inspect the complete attempt history without exposing upstream payloads. Recovery state survives restarts and remains auditable. Some historical failures require manual retry because their original class cannot be reconstructed safely.

All examples and fixtures must use synthetic placeholders. Documentation must not enumerate production routes, route order, production identifiers, user content, or live records.
