"""Shared helpers for the PostgreSQL seed suites.

`psql` failures must surface the server error text instead of a bare non-zero
exit status, otherwise a schema mismatch in a fixture looks like an assertion
failure in the feature under test.
"""
import subprocess


def psql(db: str, query: str) -> str:
    result = subprocess.run(
        ['psql', db, '-XAt', '-v', 'ON_ERROR_STOP=1', '-c', query],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f'psql failed for {db}:\n{result.stderr.strip()}\nquery: {query[:2000]}'
        )
    return result.stdout.strip()
