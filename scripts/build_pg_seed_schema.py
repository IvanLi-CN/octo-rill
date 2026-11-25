#!/usr/bin/env python3
"""Materialize the SQLite schema snapshot the PostgreSQL import suite needs.

`pg_seed_import_test.py` replays the legacy SQLite schema into a throwaway
PostgreSQL database, so it needs a SQLite source that carries every table the
importer expects. Building that source from `migrations/` keeps the fixture in
lockstep with the published schema instead of committing a binary snapshot.

The `tower_sessions` table is created by `tower-sessions-sqlx-store` at runtime
rather than by a migration, so it is added here explicitly; without it the
importer rejects the snapshot as incomplete.

Usage: `python3 scripts/build_pg_seed_schema.py --output .ci/pg-seed-schema.db`
"""
import argparse
import pathlib
import sqlite3

MIGRATIONS = pathlib.Path(__file__).resolve().parent.parent / 'migrations'

# Mirrors `SqliteStore::migrate` in tower-sessions-sqlx-store 0.15.
SESSION_TABLE = """
CREATE TABLE IF NOT EXISTS tower_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    data BLOB NOT NULL,
    expiry_date INTEGER NOT NULL
)
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=pathlib.Path)
    parser.add_argument('--migrations', type=pathlib.Path, default=MIGRATIONS)
    args = parser.parse_args()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.unlink(missing_ok=True)
    connection = sqlite3.connect(args.output)
    try:
        for migration in sorted(args.migrations.glob('*.sql')):
            connection.executescript(migration.read_text())
            connection.commit()
        connection.executescript(SESSION_TABLE)
        connection.commit()
        tables = connection.execute(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'"
        ).fetchone()[0]
        missing = [
            name
            for name in (
                'users',
                'github_connections',
                'job_tasks',
                'content_work_items',
                'translation_work_items',
                'tower_sessions',
            )
            if connection.execute(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
                (name,),
            ).fetchone()[0]
            == 0
        ]
    finally:
        connection.close()

    if missing:
        raise SystemExit(f'fixture is missing authoritative tables: {", ".join(missing)}')
    print(f'wrote {args.output} with {tables} tables')


if __name__ == '__main__':
    main()
