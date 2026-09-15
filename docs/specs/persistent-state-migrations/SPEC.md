# Persistent State Migrations

## Context and Scope

This topic governs release-to-release changes to OctoRill's SQLite durable state. It covers SQLx migration history, the online migration operator, the global content-processing control record, legacy observation rows and the runtime-owner lease used to make migration progress resumable.

## Requirements

- `REQ-PSM-ORDER`: DDL, current-state DML and historical backfill are separate ordered operations with independent status, checksum, progress and failure signals.
- `REQ-PSM-COMPATIBILITY`: Fresh databases finish SQLx initialization before HTTP listen. Existing databases validate applied version, checksum and dirty state without applying pending SQLx history in place.
- `REQ-PSM-ADMISSION`: Valid content-processing submission remains admitted during every migration operation. A `legacy` or `rollback_freeze` control value is atomically repaired to `global` before unique global work admission.
- `REQ-PSM-PAUSE`: Backfill commits at most 100 rows per batch, stores a cursor before releasing the SQLite permit, and resumes from that cursor after pause or interruption.
- `REQ-PSM-LEASE`: Only the current named migration lease owner may advance an operation. A live runtime owner is never reclaimed; stale ownership is recoverable.
- `REQ-PSM-REPAIR`: Deployed durable state is immutable with respect to migration identity. Failures use forward repair; no down-migration, blue-green switch or deployment window is part of the protocol.

## Interfaces

The existing administrator authentication boundary exposes `GET /admin/jobs/migrations`, `GET /admin/jobs/migrations/{migration_id}`, `POST /admin/jobs/migrations/{migration_id}/pause` and `POST /admin/jobs/migrations/{migration_id}/resume`. These endpoints expose redacted state only and do not create user-facing migration APIs.

## Verification

- `VER-PSM-HISTORY`: covers: `REQ-PSM-COMPATIBILITY`. Verify fresh initialization completes before listen, while an existing SQLx history is checksum/dirty/version validated without applying pending SQLx migrations.
- `VER-PSM-OPERATIONS`: covers: `REQ-PSM-ORDER`, `REQ-PSM-LEASE`. Verify immutable run and operation checksums, DDL/DML/backfill ordering, named lease ownership and live-owner protection across two runtimes.
- `VER-PSM-RESUME`: covers: `REQ-PSM-PAUSE`, `REQ-PSM-REPAIR`. Verify 100-row cursor commits, pause/resume, interruption re-entry without duplicate observations, redacted failure state and forward-only recovery.
- `VER-PSM-ADMISSION`: covers: `REQ-PSM-ADMISSION`. Verify valid submissions in `legacy`, `rollback_freeze` and `global` preserve `202`/active-work `409` semantics and never return a migration-specific `503`.

## Acceptance

- Fresh initialization and existing-history validation are covered by database migration tests.
- Operation checksums and ordering are immutable and idempotent.
- Cursor batches, pause/resume and interruption re-entry preserve progress without duplicate legacy observations.
- Foreground writes keep their existing admission status (`202` or active-work `409`) while migration work yields.

## Related ADRs

- [ADR 0011: 无感持久化状态迁移](../../adr/0011-online-persistent-state-migrations.md)
