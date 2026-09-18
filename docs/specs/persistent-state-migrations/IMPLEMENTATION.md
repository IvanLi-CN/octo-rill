# Persistent State Migration Implementation

## Runtime Lifecycle

`database_migrations::run` executes before the listener is bound. A fresh database applies the checked-in SQLx set before listen. A database that already has `_sqlx_migrations` is validation-only: every applied row is checked for version, checksum and dirty state, and no pending SQLx migration is applied in place. The one retained legacy search migration is accepted only with its recorded checksum.

After `runtime::register_runtime_owner`, the server starts the online operator. A named `online-migration-operator` lease creates the control tables and idempotently registers the forward-repair run `content-processing-online-v2` with three operations: `ddl-001`, `dml-001`, and `backfill-001`. The DDL operation also upgrades the observation identity to include the legacy source hash, preserving existing rows while allowing a deleted-and-reused primary key to be observed as a new incarnation. Each operation has an immutable definition checksum. The prior `content-processing-online-v1` run and its checksums remain historical and are validated read-only; v2 repairs forward from whatever v1 state was durably recorded.

The operator claims a migration-priority SQLite permit, renews the persistent lease, checks pause state, and commits one operation step. The backfill processes no more than 100 rows and selects only legacy rows without a durable observation. SQLite `rowid` is retained as an informational progress cursor, not as the completion predicate, so deletion and rowid reuse cannot hide a new legacy row. Errors are truncated at a UTF-8 boundary before being stored on both the run and the current operation; a later process version repairs forward from the durable operation state.

Legacy work rows and non-displayable or incomplete cache rows are recorded as `legacy_conflict`. Only a ready cache with visible title or summary content is recorded as `legacy_cached`; paired work/cache evidence is later suppressed by the read model without changing either legacy row.

## Content Admission

`content_processing::submit_item` opens the serialized write transaction and repairs `content_processing_control.mode` from `legacy` or `rollback_freeze` to `global` before looking up or inserting the unique `content_work_items` identity and its request link. The old tables are never rewritten. The global work lease and heartbeats use `runtime_owner_id`; recovery only reclaims an expired lease when its owner heartbeat is stale.

## Safety Boundaries

The topology remains one Compose service with SQLite. There is no blue-green deployment, freeze/cutover management route, migration-specific `503`, second database, queue or down-migration. Published projections remain readable while a newer work item is queued or running.
