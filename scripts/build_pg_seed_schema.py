#!/usr/bin/env python3
"""Materialize the SQLite schema snapshot the PostgreSQL import suite needs.

`pg_seed_import_test.py` replays the legacy SQLite schema into a throwaway
PostgreSQL database, so it needs a SQLite source that carries every table the
importer expects.

The source is now the frozen dump in `ops/fixtures/legacy-sqlite-schema.sql`,
not `migrations/`. The retirement plan deletes `migrations/` (REQ-RTR-006), and
the import suite must keep working after that: it describes a historical shape
that never changes again, so a frozen fixture is the honest representation.

Regenerating the fixture requires the retired `migrations/` directory and is
only meaningful if the exported shape itself must change:

    python3 scripts/build_pg_seed_schema.py --output .ci/pg-seed-schema.db \
        --migrations migrations --write-fixture

The `tower_sessions` table is created by `tower-sessions-sqlx-store` at runtime
rather than by a migration, so it is added explicitly; without it the importer
rejects the snapshot as incomplete.

Usage: `python3 scripts/build_pg_seed_schema.py --output .ci/pg-seed-schema.db`
"""
import argparse
import pathlib
import sqlite3

ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURE = ROOT / 'ops' / 'fixtures' / 'legacy-sqlite-schema.sql'
MIGRATIONS = ROOT / 'migrations'

REQUIRED_TABLES = (
    'users',
    'github_connections',
    'job_tasks',
    'content_work_items',
    'translation_work_items',
    'tower_sessions',
)


def replay_migrations(migrations: pathlib.Path) -> list[str]:
    """Build the export statement list from the retired migration history."""
    scratch = sqlite3.connect(':memory:')
    try:
        for migration in sorted(migrations.glob('*.sql')):
            scratch.executescript(migration.read_text())
        scratch.commit()
        return export_statements(scratch)
    finally:
        scratch.close()


def export_statements(connection: sqlite3.Connection) -> list[str]:
    rows = connection.execute(
        """
        SELECT sql FROM sqlite_master
        WHERE sql IS NOT NULL
        ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'index' THEN 1 ELSE 2 END, name
        """
    )
    return [sql.rstrip().rstrip(';') + ';' for (sql,) in rows]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=pathlib.Path)
    parser.add_argument('--fixture', type=pathlib.Path, default=FIXTURE)
    parser.add_argument('--migrations', type=pathlib.Path, default=None)
    parser.add_argument(
        '--write-fixture',
        action='store_true',
        help='rewrite the frozen fixture from --migrations',
    )
    args = parser.parse_args()

    if args.write_fixture:
        if args.migrations is None:
            raise SystemExit('--write-fixture requires --migrations')
        statements = replay_migrations(args.migrations)
        args.fixture.parent.mkdir(parents=True, exist_ok=True)
        args.fixture.write_text('\n'.join(statements) + '\n')
        print(f'wrote {args.fixture} with {len(statements)} statements')

    if not args.fixture.exists():
        raise SystemExit(
            f'missing schema fixture: {args.fixture}. Regenerate it with '
            '--migrations <dir> --write-fixture before deleting migrations/.'
        )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.unlink(missing_ok=True)
    connection = sqlite3.connect(args.output)
    try:
        connection.executescript(args.fixture.read_text())
        connection.commit()
        tables = connection.execute(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'"
        ).fetchone()[0]
        missing = [
            name
            for name in REQUIRED_TABLES
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
