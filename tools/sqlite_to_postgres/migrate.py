#!/usr/bin/env python3
"""Safely snapshot SQLite and import it into PostgreSQL.

The tool deliberately keeps snapshot, import, verification, and index rebuilding as
separate commands.  Import never chooses a database from the environment and refuses
SQLite targets, so a rerun always names the exact destination explicitly.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import sqlite3
import sys
import tempfile
import uuid
from pathlib import Path
from typing import Any, Iterable, Iterator, Sequence

TOOL_VERSION = "1"
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_$]*$")


class MigrationError(RuntimeError):
    def __init__(self, category: str, message: str, *, table: str | None = None):
        super().__init__(message)
        self.category, self.table = category, table


def quote_identifier(value: str) -> str:
    """Quote a SQL identifier without allowing an expression through."""
    if not value or "\x00" in value:
        raise ValueError("invalid SQL identifier")
    return '"' + value.replace('"', '""') + '"'


def canonical_value(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, (bytes, bytearray, memoryview)):
        return {"__bytes__": bytes(value).hex()}
    if isinstance(value, (dt.datetime, dt.date, dt.time)):
        return {"__datetime__": value.isoformat()}
    if isinstance(value, (list, tuple)):
        return [canonical_value(item) for item in value]
    if isinstance(value, dict):
        return {str(k): canonical_value(v) for k, v in sorted(value.items())}
    return {"__repr__": repr(value)}


def hash_rows(columns: Sequence[str], rows: Iterable[Sequence[Any]]) -> str:
    digest = hashlib.sha256()
    for row in rows:
        payload = json.dumps(
            {"columns": list(columns), "row": canonical_value(list(row))},
            ensure_ascii=False,
            separators=(",", ":"),
        ).encode("utf-8")
        digest.update(payload)
        digest.update(b"\n")
    return digest.hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sqlite_path(value: str) -> str:
    if value.startswith("sqlite:"):
        value = value.removeprefix("sqlite:")
    if value in {":memory:", ""}:
        raise MigrationError("snapshot", "an on-disk SQLite database is required")
    return value


def connect_snapshot(path: str) -> sqlite3.Connection:
    source = sqlite_path(path)
    if not Path(source).exists():
        raise MigrationError("snapshot", f"SQLite source does not exist: {source}")
    connection = sqlite3.connect(f"file:{Path(source).resolve()}?mode=ro", uri=True)
    connection.execute("PRAGMA query_only=ON")
    connection.execute("PRAGMA busy_timeout=5000")
    return connection


def table_names(connection: sqlite3.Connection) -> list[str]:
    return [
        row[0]
        for row in connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        )
    ]


def sqlite_table_manifest(connection: sqlite3.Connection, table: str) -> dict[str, Any]:
    columns = [row[1] for row in connection.execute(f"PRAGMA table_info({quote_identifier(table)})")]
    primary_key = [row[1] for row in connection.execute(f"PRAGMA table_info({quote_identifier(table)})") if row[5]]
    quoted = ", ".join(quote_identifier(column) for column in columns)
    order = ", ".join(quote_identifier(column) for column in primary_key) or quoted
    rows = connection.execute(f"SELECT {quoted} FROM {quote_identifier(table)} ORDER BY {order}")
    row_list = list(rows)
    return {
        "name": table,
        "columns": columns,
        "primary_key": primary_key,
        "row_count": len(row_list),
        "row_hash": hash_rows(columns, row_list),
        "foreign_keys": [
            {"column": row[3], "parent_table": row[2], "parent_column": row[4]}
            for row in connection.execute(f"PRAGMA foreign_key_list({quote_identifier(table)})")
        ],
    }


def snapshot(args: argparse.Namespace) -> dict[str, Any]:
    destination = Path(args.output).expanduser().resolve()
    destination.mkdir(parents=True, exist_ok=True)
    source = sqlite_path(args.source)
    connection = connect_snapshot(source)
    snapshot_file = destination / "snapshot.sqlite"
    if snapshot_file.exists():
        raise MigrationError("snapshot", f"refusing to overwrite existing snapshot: {snapshot_file}")
    try:
        target = sqlite3.connect(snapshot_file)
        with target:
            connection.backup(target)
        target.close()
        tables = [sqlite_table_manifest(connection, table) for table in table_names(connection)]
        manifest = {
            "format": "octo-rill-sqlite-snapshot-v1",
            "tool_version": TOOL_VERSION,
            "source": str(Path(source).resolve()),
            "snapshot": snapshot_file.name,
            "snapshot_sha256": sha256_file(snapshot_file),
            "created_at": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z"),
            "tables": tables,
        }
    finally:
        connection.close()
    manifest_file = destination / "manifest.json"
    manifest_file.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    return {"snapshot": str(snapshot_file), "manifest": str(manifest_file), "tables": len(tables)}


def load_manifest(path: str) -> tuple[Path, dict[str, Any]]:
    root = Path(path).expanduser().resolve()
    manifest_file = root / "manifest.json" if root.is_dir() else root
    manifest = json.loads(manifest_file.read_text())
    snapshot_file = manifest_file.parent / manifest["snapshot"]
    if sha256_file(snapshot_file) != manifest["snapshot_sha256"]:
        raise MigrationError("snapshot", "snapshot checksum does not match manifest")
    return snapshot_file, manifest


def load_psycopg():
    try:
        import psycopg  # type: ignore
    except ImportError as exc:  # pragma: no cover - depends on deployment image
        raise MigrationError("connection", "psycopg is required for PostgreSQL commands") from exc
    return psycopg


def pg_rows(connection: Any, table: str, columns: Sequence[str], primary_key: Sequence[str]) -> Iterator[tuple[Any, ...]]:
    order = ", ".join(quote_identifier(column) for column in primary_key) or ", ".join(quote_identifier(column) for column in columns)
    quoted = ", ".join(quote_identifier(column) for column in columns)
    with connection.cursor() as cursor:
        cursor.execute(f"SELECT {quoted} FROM {quote_identifier(table)} ORDER BY {order}")
        yield from cursor.fetchall()


def pg_tables(connection: Any) -> set[str]:
    with connection.cursor() as cursor:
        cursor.execute("SELECT tablename FROM pg_catalog.pg_tables WHERE schemaname = current_schema()")
        return {row[0] for row in cursor.fetchall()}


def dependency_order(items: Sequence[dict[str, Any]]) -> list[dict[str, Any]]:
    """Load referenced tables first while retaining deterministic ordering."""
    by_name = {item["name"]: item for item in items}
    visiting: set[str] = set()
    visited: set[str] = set()
    ordered: list[dict[str, Any]] = []

    def visit(name: str) -> None:
        if name in visited:
            return
        if name in visiting:
            # Cyclic foreign keys are valid; PostgreSQL constraints are checked at
            # statement/transaction boundaries, so keep the cycle deterministic.
            return
        visiting.add(name)
        item = by_name[name]
        for foreign_key in sorted(item.get("foreign_keys", []), key=lambda value: value.get("parent_table", "")):
            parent = foreign_key.get("parent_table")
            if parent in by_name:
                visit(parent)
        visiting.remove(name)
        visited.add(name)
        ordered.append(item)

    for name in sorted(by_name):
        visit(name)
    return ordered


def copy_rows(connection: Any, table: str, columns: Sequence[str], rows: Iterable[Sequence[Any]], batch_size: int) -> int:
    quoted_columns = ", ".join(quote_identifier(column) for column in columns)
    count = 0
    with connection.cursor() as cursor:
        with cursor.copy(f"COPY {quote_identifier(table)} ({quoted_columns}) FROM STDIN WITH (FORMAT CSV, NULL '\\N')") as copy:
            buffer = tempfile.SpooledTemporaryFile(max_size=4 * 1024 * 1024, mode="w+", newline="")
            writer = csv.writer(buffer, lineterminator="\n")
            for row in rows:
                encoded = []
                for value in row:
                    if value is None:
                        encoded.append("\\N")
                    elif isinstance(value, (bytes, bytearray, memoryview)):
                        encoded.append("\\x" + bytes(value).hex())
                    else:
                        encoded.append(str(value))
                writer.writerow(encoded)
                count += 1
                if count % batch_size == 0:
                    buffer.seek(0)
                    copy.write(buffer.read())
                    buffer.seek(0)
                    buffer.truncate(0)
            buffer.seek(0)
            copy.write(buffer.read())
            buffer.close()
    return count


def import_snapshot(args: argparse.Namespace) -> dict[str, Any]:
    if args.target.startswith("sqlite:") or args.target.endswith(".db"):
        raise MigrationError("target", "import target must be an explicit PostgreSQL URL")
    snapshot_file, manifest = load_manifest(args.snapshot)
    psycopg = load_psycopg()
    tables = dependency_order([item for item in manifest["tables"] if not args.tables or item["name"] in args.tables])
    if not tables:
        raise MigrationError("schema", "no requested tables exist in the snapshot")
    connection = psycopg.connect(args.target)
    imported: list[dict[str, Any]] = []
    run_id = args.run_id
    try:
        with connection.transaction():
            with connection.cursor() as cursor:
                cursor.execute("CREATE TABLE IF NOT EXISTS sqlite_import_runs (run_id TEXT PRIMARY KEY, snapshot_sha256 TEXT NOT NULL, completed_at TIMESTAMPTZ NOT NULL DEFAULT now())")
                cursor.execute("SELECT 1 FROM sqlite_import_runs WHERE run_id=%s AND snapshot_sha256=%s", (run_id, manifest["snapshot_sha256"]))
                if cursor.fetchone():
                    return {"run_id": run_id, "snapshot_sha256": manifest["snapshot_sha256"], "tables": [], "mode": "already-completed"}
            source = sqlite3.connect(f"file:{snapshot_file}?mode=ro", uri=True)
            source.execute("PRAGMA query_only=ON")
            for item in tables:
                table, columns = item["name"], item["columns"]
                quoted = ", ".join(quote_identifier(column) for column in columns)
                rows = source.execute(f"SELECT {quoted} FROM {quote_identifier(table)}")
                staging = "_sqlite_import_" + uuid.uuid4().hex
                try:
                    with connection.cursor() as cursor:
                        cursor.execute(f"CREATE TEMP TABLE {quote_identifier(staging)} (LIKE {quote_identifier(table)} INCLUDING DEFAULTS) ON COMMIT DROP")
                    count = copy_rows(connection, staging, columns, rows, args.batch_size)
                    primary_key = item["primary_key"]
                    with connection.cursor() as cursor:
                        if primary_key:
                            conflict = ", ".join(quote_identifier(column) for column in primary_key)
                            updates = [column for column in columns if column not in primary_key]
                            assignment = ", ".join(f"{quote_identifier(column)} = EXCLUDED.{quote_identifier(column)}" for column in updates)
                            action = f"ON CONFLICT ({conflict}) DO UPDATE SET {assignment}" if assignment else f"ON CONFLICT ({conflict}) DO NOTHING"
                        else:
                            action = ""
                        cursor.execute(f"INSERT INTO {quote_identifier(table)} ({quoted}) SELECT {quoted} FROM {quote_identifier(staging)} {action}")
                except Exception as exc:
                    raise MigrationError("data", f"failed importing table {table}: {exc}", table=table) from exc
                imported.append({"table": table, "rows": count})
                with connection.cursor() as cursor:
                    for primary_key in item["primary_key"]:
                        try:
                            cursor.execute("SAVEPOINT sqlite_import_sequence_repair")
                            cursor.execute("SELECT pg_get_serial_sequence(%s, %s)", (table, primary_key))
                            sequence = cursor.fetchone()[0]
                            if sequence:
                                cursor.execute(f"SELECT setval(%s, GREATEST(COALESCE((SELECT MAX({quote_identifier(primary_key)}) FROM {quote_identifier(table)}), 1), 1), true)", (sequence,))
                            cursor.execute("RELEASE SAVEPOINT sqlite_import_sequence_repair")
                        except Exception:
                            # Non-serial/text primary keys do not need sequence repair.
                            cursor.execute("ROLLBACK TO SAVEPOINT sqlite_import_sequence_repair")
                            cursor.execute("RELEASE SAVEPOINT sqlite_import_sequence_repair")
            source.close()
            with connection.cursor() as cursor:
                cursor.execute("INSERT INTO sqlite_import_runs(run_id, snapshot_sha256) VALUES (%s, %s)", (run_id, manifest["snapshot_sha256"]))
    finally:
        connection.close()
    return {"run_id": run_id, "snapshot_sha256": manifest["snapshot_sha256"], "tables": imported, "mode": "import"}


def verify(args: argparse.Namespace) -> dict[str, Any]:
    _, manifest = load_manifest(args.snapshot)
    psycopg = load_psycopg()
    connection = psycopg.connect(args.target)
    failures: list[dict[str, Any]] = []
    results: list[dict[str, Any]] = []
    try:
        available = pg_tables(connection)
        for item in manifest["tables"]:
            table = item["name"]
            if table not in available:
                failures.append({"category": "schema", "table": table, "message": "table is missing"})
                continue
            with connection.cursor() as cursor:
                cursor.execute(f"SELECT count(*) FROM {quote_identifier(table)}")
                count = cursor.fetchone()[0]
            actual_hash = hash_rows(item["columns"], pg_rows(connection, table, item["columns"], item["primary_key"]))
            result = {"table": table, "expected_rows": item["row_count"], "actual_rows": count, "expected_hash": item["row_hash"], "actual_hash": actual_hash}
            results.append(result)
            if count != item["row_count"] or actual_hash != item["row_hash"]:
                failures.append({"category": "data", "table": table, "message": "row count or hash mismatch", "result": result})
    finally:
        connection.close()
    return {"mode": "verify", "ok": not failures, "tables": results, "failures": failures}


def rebuild(args: argparse.Namespace) -> dict[str, Any]:
    if args.target.startswith("sqlite:"):
        raise MigrationError("target", "rebuild target must be PostgreSQL")
    psycopg = load_psycopg()
    connection = psycopg.connect(args.target)
    tables = args.tables
    try:
        with connection.transaction():
            with connection.cursor() as cursor:
                if not tables:
                    cursor.execute("SELECT tablename FROM pg_catalog.pg_tables WHERE schemaname=current_schema() ORDER BY tablename")
                    tables = [row[0] for row in cursor.fetchall()]
                for table in tables:
                    cursor.execute(f"ANALYZE {quote_identifier(table)}")
                    cursor.execute(f"REINDEX TABLE {quote_identifier(table)}")
    finally:
        connection.close()
    return {"mode": "rebuild", "tables": tables}


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    sub = root.add_subparsers(dest="command", required=True)
    snap = sub.add_parser("snapshot", help="create a consistent read-only SQLite snapshot")
    snap.add_argument("--source", required=True)
    snap.add_argument("--output", required=True)
    imp = sub.add_parser("import", help="stream a snapshot into an explicitly named PostgreSQL target")
    imp.add_argument("--snapshot", required=True)
    imp.add_argument("--target", required=True)
    imp.add_argument("--run-id", required=True)
    imp.add_argument("--tables", nargs="*")
    imp.add_argument("--batch-size", type=int, default=1000)
    imp.add_argument("--force", action="store_true")
    ver = sub.add_parser("verify", help="compare PostgreSQL counts and hashes with a snapshot manifest")
    ver.add_argument("--snapshot", required=True)
    ver.add_argument("--target", required=True)
    reb = sub.add_parser("rebuild", help="rebuild/analyze indexes as a separate post-import phase")
    reb.add_argument("--target", required=True)
    reb.add_argument("--tables", nargs="*")
    return root


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        function_name = {"import": "import_snapshot"}.get(args.command, args.command)
        result = {"ok": True, **globals()[function_name](args)}
        print(json.dumps(result, ensure_ascii=False, indent=2, default=str))
        return 0
    except (MigrationError, sqlite3.Error, OSError) as exc:
        error = {"ok": False, "error": {"category": getattr(exc, "category", "unknown"), "message": str(exc)}}
        if getattr(exc, "table", None):
            error["error"]["table"] = exc.table
        print(json.dumps(error, ensure_ascii=False, indent=2), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
