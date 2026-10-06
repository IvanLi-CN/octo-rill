# ADR 0018: Online Persistent State Migrations

## Status

Accepted

## Decision

Durable SQLite migrations must keep valid content processing available while state evolves. Structural DDL, current-state repair, and historical observation backfill are separate ordered operations with independent checksums, progress, pause points, and failure signals.

Global admission repairs a historical `legacy` or `rollback_freeze` control value to `global` in the same foreground SQLite transaction that performs unique work admission. The response remains the established queued `202` or active-work `409`; callers do not receive a migration-specific `503`.

Historical tables are read-only evidence. Legacy observation identity includes the source hash, and backfill completion is based on observation absence rather than a cursor alone. A named lease protects the operator, and stale ownership is recoverable only when the old runtime owner heartbeat is stale.

## Consequences

- Existing SQLx history is validated in place; pending SQLx history is not silently applied to a deployed database.
- Backfill is bounded and resumable, and it yields to foreground SQLite writes through the coordinator.
- Deployed migration identities are immutable. A faulty release is repaired forward by a new program version; rollback and backup restore remain separate concerns.
- Old content facts are never rewritten or promoted into global work, projections, or attempts.

## Alternatives Rejected

- A migration-specific freeze or `503` window was rejected because it changes valid admission semantics and makes operator progress depend on request retries.
- Down migration and blue-green dual-state operation were rejected because they make durable identity and lease ownership ambiguous.
