# ADR 0017: Rowid-Mapped Search FTS Recovery

## Status

Accepted

## Context

The command-palette search projection stores `doc_id` as an `UNINDEXED` FTS5 column. Source and repository-metadata triggers that delete or replace rows by `doc_id` therefore scan the complete global or user-lane FTS corpus. A repository rename or visibility change can fan out to tens of thousands of release documents while holding the SQLite writer, delaying foreground search, HTTP, and session writes. A migration-time corpus rebuild would also make listener startup depend on historical data volume.

## Decision

- Treat both FTS corpora as rebuildable caches rather than migration-owned source of truth. Migration `0090` creates v2 FTS tables and stable mapping tables from logical document identity to FTS rowid, then leaves the new cache empty for the background worker to populate.
- Preserve the old FTS table names as writable views with `INSTEAD OF` triggers. Existing source triggers continue to work, but every compatibility operation resolves the logical identity through the mapping table and mutates the v2 FTS row by explicit rowid.
- Gate FTS query optimization on a `ready` projection state with an empty metadata queue. During migration, historical rebuild, low-disk pause, or metadata lag, search uses the existing authorization predicates and `LIKE` fallback while the current metadata view supplies rename and canonical-target values.
- Represent repository metadata fanout as a deduplicated queue with a per-repository release rowid cursor. Metadata source deletion enqueues the affected repository, and every new metadata event resets that cursor to 0 so a changed prefix cannot remain stale. Each Background writer transaction updates at most 100 release rows, persists progress, yields between batches, and continues polling after the initial projection reaches `ready`.

## Considered Options

- Add a normal SQLite index to the FTS `doc_id` column: rejected because FTS5 virtual-table columns do not provide the required ordinary lookup path for an `UNINDEXED` identity field.
- Keep the old FTS tables and rebuild them synchronously for every repository metadata event: rejected because the cost scales with the entire repository corpus and blocks foreground writers.
- Rebuild the complete FTS corpus during migration: rejected because it makes startup time and WAL growth proportional to historical data volume and is not restart-safe.
- Replace FTS with only `LIKE`: rejected because it removes trigram search performance for the steady-state ready path; fallback remains a recovery mode, not the primary index.

## Consequences

The migration is schema-only with respect to historical rows, so startup remains bounded and recovery can pause or resume. Steady-state source and metadata maintenance is document-specific, while the metadata writer hold is bounded by 100 release rows. The compatibility views preserve existing trigger SQL but are an internal database contract; direct FTS table access outside the application must use the compatibility names or v2 mapping contract. During recovery, `LIKE` queries consume more CPU. Release repository fields and canonical targets come only from the current metadata view; when that view is temporarily empty or has no canonical target path, search omits those fields and suppresses the cached target URL instead of falling back to stale denormalized values. Cached content fields can still remain briefly behind the source while recovery progresses.
