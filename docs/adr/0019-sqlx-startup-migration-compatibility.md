# ADR 0019: Validated SQLx Migrations Apply Before Startup

## Status

Accepted. This ADR supersedes ADR 0018's SQLx startup-history rule.

## Context

`database_migrations::run` executes before the HTTP listener is bound. The application historically validated non-empty SQLx history but rejected every pending migration, which made a valid release-to-release schema upgrade fail before startup. Release v2.76.1 exposed this boundary when an existing database at migration 90 encountered the real schema migration 91.

## Decision

- A fresh database or an empty `_sqlx_migrations` table uses the checked-in SQLx migrator before the listener is bound.
- A non-empty history is first validated for successful rows, known versions, exact checksums, the legacy version 81 checksum, and internal history gaps. Only after validation does the application derive pending versions and call the SQLx migrator to apply them.
- SQLx owns migration transactions and `_sqlx_migrations` history writes. Application code does not insert a success marker for an unapplied migration or skip its SQL.
- Pending migration failures return an error that includes the pending version list and SQLx execution context. The listener remains unbound and a failed migration does not leave its history row or partial schema changes committed.
- SQLx schema migrations remain separate from the bounded, resumable online operator. A migration must not rebuild historical search or content data synchronously during startup.

## Consequences

Existing databases may spend the schema-migration execution time unavailable during startup, but valid releases can complete their ordered SQLx upgrade without operator intervention. Migration 91 remains schema-only and leaves search corpus recovery to the existing background worker. Invalid history still fails closed, and deployed migration identities remain immutable; deterministic failures require a forward repair or corrected release.

## Alternatives Rejected

- Rejecting every pending migration was rejected because it prevents valid schema releases from starting on databases at the immediately previous migration.
- Inserting a manual `_sqlx_migrations` row was rejected because it can report success while leaving indexes, triggers, or other schema changes unapplied.
- Running migration SQL outside SQLx history management was rejected because it would split transaction and checksum ownership between application code and SQLx.
- Binding the listener before schema completion was rejected because requests could observe a partially upgraded schema.
