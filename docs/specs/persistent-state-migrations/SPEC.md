# Persistent State Migrations

## Context And Scope

This topic governs release-to-release changes to OctoRill's durable SQLite state. It covers SQLx migration history, the online migration operator, the global content-processing control record, legacy observations, and runtime-owner leases.

## Requirements

- `REQ-PSM-ORDER`: DDL, current-state DML, and historical backfill are separate ordered operations with independent status, checksum, progress, and failure signals.
- `REQ-PSM-COMPATIBILITY`: Fresh databases finish SQLx initialization before HTTP listen. Existing databases validate applied version, checksum, dirty state, and internal gaps, then apply pending SQLx migrations through SQLx before HTTP listen; migration 81 remains accepted only with its exact legacy checksum.
- `REQ-PSM-ADMISSION`: Valid global content admission remains available during every online operation. A `legacy` or `rollback_freeze` control value is atomically repaired to `global` inside the foreground admission transaction before unique work identity lookup or insertion.
- `REQ-PSM-OBSERVATION`: Legacy rows remain read-only observations. The observation identity includes `(legacy_table, legacy_primary_key, legacy_source_hash)` so a deleted and reused legacy key creates a new incarnation.
- `REQ-PSM-PAUSE`: Historical backfill commits at most 100 rows per batch, persists its cursor before releasing the writer permit, and resumes from durable progress after pause or interruption.
- `REQ-PSM-LEASE`: Only the current named migration lease owner may advance an operation. A live runtime owner is never reclaimed; stale ownership is recoverable.
- `REQ-PSM-REPAIR`: Deployed durable state is immutable with respect to migration identity. Failures use forward repair; no down migration, blue-green switch, or migration-specific freeze window is part of the protocol.

## Interfaces

The administrator authentication boundary exposes `GET /admin/jobs/migrations`, `GET /admin/jobs/migrations/{migration_id}`, `POST /admin/jobs/migrations/{migration_id}/pause`, and `POST /admin/jobs/migrations/{migration_id}/resume`. These endpoints expose redacted state only and do not create user-facing migration APIs.

## Verification

- `VER-PSM-HISTORY`: Fresh initialization, a real v90-to-v91 existing-database upgrade, checksum/dirty/gap rejection, legacy version 81 compatibility, and transactional failure diagnostics cover the SQLx compatibility contract.
- `VER-PSM-OPERATIONS`: Operation checksums and ordering are immutable; the named lease, live-owner protection, and stale-owner recovery are tested.
- `VER-PSM-RESUME`: Bounded cursor batches, pause/resume, interruption re-entry, duplicate suppression, source-hash incarnations, and redacted failure state are tested.
- `VER-PSM-ADMISSION`: Valid submissions in `legacy`, `rollback_freeze`, and `global` preserve `202` and active-work `409` semantics. API adapters do not route `rollback_freeze` submissions into the compatibility writer's migration `503` path.
- `VER-PSM-COORDINATOR`: Migration writes use the SQLite background lane, yield to foreground writes, and retry transient background admission, busy, and deadline errors without recording a permanent migration failure.

## Acceptance

- Existing SQLx history is validated before pending SQLx migrations are applied transactionally by SQLx; the listener is not bound until the upgrade succeeds.
- DDL, current-state repair, and observation backfill remain independently observable, pauseable, and resumable.
- Legacy facts are never rewritten or promoted into global work, result, or attempt history.
- Foreground content admission retains its existing `202` or active-work `409` response while migration work yields.

## Related ADRs

- [ADR 0019: Validated SQLx Migrations Apply Before Startup](../../adr/0019-sqlx-startup-migration-compatibility.md)
