import json
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from migrate import dependency_order, hash_rows, quote_identifier


class MigrationHelpersTest(unittest.TestCase):
    def test_quote_identifier_escapes_names(self):
        self.assertEqual(quote_identifier('a"b'), '"a""b"')

    def test_hash_is_deterministic_and_binary_safe(self):
        rows = [(1, b"\x00\xff"), (2, None)]
        self.assertEqual(hash_rows(["id", "payload"], rows), hash_rows(["id", "payload"], rows))

    def test_dependency_order_loads_parent_first(self):
        items = [
            {"name": "child", "foreign_keys": [{"parent_table": "parent"}]},
            {"name": "parent", "foreign_keys": []},
        ]
        self.assertEqual([item["name"] for item in dependency_order(items)], ["parent", "child"])

    def test_snapshot_creates_manifest_without_mutating_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.sqlite"
            connection = sqlite3.connect(source)
            connection.executescript(
                "CREATE TABLE parent(id INTEGER PRIMARY KEY, secret BLOB NOT NULL);"
                "CREATE TABLE child(id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id));"
                "INSERT INTO parent VALUES (1, x'00ff'); INSERT INTO child VALUES (2, 1);"
            )
            connection.commit()
            before = source.stat().st_mtime_ns
            connection.close()
            result = subprocess.run(
                [sys.executable, "migrate.py", "snapshot", "--source", str(source), "--output", str(root / "run")],
                check=True,
                capture_output=True,
                text=True,
                cwd=Path(__file__).parent,
            )
            self.assertTrue(json.loads(result.stdout)["ok"])
            self.assertEqual(source.stat().st_mtime_ns, before)
            manifest = json.loads((root / "run" / "manifest.json").read_text())
            self.assertEqual([table["name"] for table in manifest["tables"]], ["child", "parent"])
            self.assertEqual(manifest["tables"][0]["foreign_keys"][0]["parent_table"], "parent")


if __name__ == "__main__":
    unittest.main()
