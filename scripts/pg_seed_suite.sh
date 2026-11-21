#!/usr/bin/env bash
# Run every PostgreSQL seed suite against a disposable-database admin URL.
#
# Each suite creates its own throwaway database, starts the service on its own
# port with workers disabled, asserts HTTP behaviour, then drops the database.
# Results stay visible per suite instead of being collapsed into one status.
set -euo pipefail

: "${PG_ADMIN_URL:=postgres://octo:octo@127.0.0.1:5432/postgres}"
: "${BINARY_DIR:=target/debug}"
: "${PORT_BASE:=3100}"
# SQLite schema source for the offline importer suite; without it that suite is
# reported as skipped instead of failing on missing arguments.
: "${PG_SEED_SCHEMA_SOURCE:=}"

cd "$(dirname "$0")/.."

port="$PORT_BASE"
failed=()
passed=()
skipped=()
for test in scripts/pg_seed_*_test.py; do
  name="$(basename "$test" .py)"
  args=(--admin-url "$PG_ADMIN_URL")
  if [ "$name" = pg_seed_import_test ]; then
    if [ -z "$PG_SEED_SCHEMA_SOURCE" ]; then
      skipped+=("$name")
      port=$((port + 1))
      continue
    fi
    args=(--admin-url "$PG_ADMIN_URL" --schema-source "$PG_SEED_SCHEMA_SOURCE"
          --binary "$BINARY_DIR/octo-rill-migrate")
  else
    args+=(--binary-dir "$BINARY_DIR" --port "$port")
  fi
  if python3 "$test" "${args[@]}"; then
    passed+=("$name")
  else
    failed+=("$name")
  fi
  port=$((port + 1))
done

printf 'suites passed: %d\n' "${#passed[@]}"
printf 'suites skipped: %d\n' "${#skipped[@]}"
for name in "${skipped[@]:-}"; do
  [ -n "$name" ] && printf 'skipped suite: %s (PG_SEED_SCHEMA_SOURCE unset)\n' "$name"
done
printf 'suites failed: %d\n' "${#failed[@]}"
for name in "${failed[@]:-}"; do
  [ -n "$name" ] && printf 'failed suite: %s\n' "$name"
done
[ "${#failed[@]}" -eq 0 ]
