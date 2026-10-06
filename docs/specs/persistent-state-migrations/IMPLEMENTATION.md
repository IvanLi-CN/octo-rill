# Persistent State Migration Implementation

## Runtime Lifecycle

`database_migrations::run` executes before the listener is bound. A fresh database applies the checked-in SQLx set before listen. A database that already has `_sqlx_migrations` is validation-only: applied rows are checked for version, checksum, and dirty state, and pending SQLx migrations are not applied in place.

After `runtime::register_runtime_owner`, the server starts the online operator. The named `online-migration-operator` lease creates the control tables and registers `content-processing-online-v2` with three ordered operations: `ddl-001`, `dml-001`, and `backfill-001`. The previous `content-processing-online-v1` identity and checksums remain historical and are validated read-only.

The DDL operation upgrades `content_legacy_observations` to include source-hash identity while preserving prior rows. The DML operation repairs historical `content_processing_control` values to `global`. The backfill observes legacy work and cache rows in batches of at most 100. Observation absence, rather than a rowid cursor alone, is the completion predicate; rowid is retained only as an informational progress cursor.

Each step renews the lease and runtime-owner heartbeat in a serialized SQLite transaction. The operator uses the background writer lane. Foreground admission has priority, and transient background admission, busy, and deadline errors defer the step without changing the durable run to `failed`. Deterministic checksum or schema errors are persisted as redacted failure state. Pause takes effect between committed operation batches.

A failed operation remains idle until an administrator resumes the migration; resume clears the failed operation's transient owner/error fields and returns it to `pending`. A malformed persisted backfill phase or rowid cursor is rejected instead of being interpreted as another legacy table.

## Content Admission

`content_processing::submit_item` opens the foreground writer transaction and repairs `legacy` or `rollback_freeze` to `global` before the unique content identity and request link are read or created. The response keeps the existing queued `202` and active-work `409` semantics. Public adapters that support global work route `rollback_freeze` through this boundary instead of the legacy writer's transition response; the compatibility `legacy` path remains available until the normal operator repair or an explicit global adapter is selected.

`record_legacy_observations` uses the same source-hash identity schema and deterministic incarnation keys. Legacy work, incomplete cache, and inconsistent evidence are recorded as `legacy_conflict`; only displayable ready cache evidence is `legacy_cached`. No old row is modified and no observation creates global work or a provider attempt.

## Safety Boundaries

The topology remains one Compose service with SQLite. There is no second database, queue, blue-green switch, down migration, or migration-specific public API. Published projections remain readable while newer work is queued or running. Administrator state is read-only and redacted except for the explicit pause/resume control flag.

## Current Validation

The focused migration suite covers bounded backfill, cursor re-entry, source-hash incarnations, checksum identity, stale-owner protection, redaction, and admin pause/read behavior. Content admission tests cover repair from both historical modes and preserve `202`/active `409` results. Current-head coordinator pressure tests cover foreground session writes while background work contends for SQLite.
