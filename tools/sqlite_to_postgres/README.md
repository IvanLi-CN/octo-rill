# SQLite to PostgreSQL migration

This tool keeps migration phases explicit and repeatable:

1. `snapshot` opens SQLite in read-only mode, enables `query_only`, uses SQLite's online backup API to create a consistent copy, and writes a manifest with table columns, primary keys, row counts, foreign keys, and SHA-256 row hashes.
2. `import` requires an explicit PostgreSQL URL and run ID. It streams rows through PostgreSQL `COPY`, records the source fingerprint in `sqlite_import_runs`, and refuses to silently repeat an already completed run.
3. `verify` compares row counts and deterministic hashes against the manifest.
4. `rebuild` runs `ANALYZE` and `REINDEX` as a separate phase.

The source snapshot and manifest are the recovery boundary. Keep them outside the repository and retain the original SQLite file. The command never selects a production target implicitly and refuses SQLite targets for import, verify, and rebuild.

Example:

```sh
python tools/sqlite_to_postgres/migrate.py snapshot \
  --source /srv/octo-rill.sqlite --output /srv/migration/run-20261111
python tools/sqlite_to_postgres/migrate.py import \
  --snapshot /srv/migration/run-20261111 \
  --target "$DATABASE_URL" --run-id cutover-20261111
python tools/sqlite_to_postgres/migrate.py verify \
  --snapshot /srv/migration/run-20261111 --target "$DATABASE_URL"
python tools/sqlite_to_postgres/migrate.py rebuild --target "$DATABASE_URL"
```

`--force` is an explicit operator choice for a completed run; use a new run ID for a normal retry after correcting a failed target.
