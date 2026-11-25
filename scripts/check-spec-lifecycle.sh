#!/usr/bin/env bash
# Validate docs/specs lifecycle: catalog rows, successors, and retirement gates.
# Portable to bash 3.2 (macOS default): no associative arrays.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
catalog="$root/docs/specs/README.md"
specs_dir="$root/docs/specs"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
rows="$tmp/rows"
tab=$'\t'
: >"$rows"
fail=0

fail_msg() {
  printf 'spec-lifecycle: %s\n' "$1" >&2
  fail=1
}

trim() {
  local v="$1"
  v="${v#"${v%%[![:space:]]*}"}"
  v="${v%"${v##*[![:space:]]}"}"
  printf '%s' "$v"
}

[[ -f "$catalog" ]] || {
  fail_msg "missing catalog: docs/specs/README.md"
  exit 1
}

while IFS= read -r line; do
  case "$line" in "| "*) ;; *) continue ;; esac
  IFS='|' read -r _ topic lifecycle _implementation spec successor _notes <<<"$line"
  topic="$(trim "$topic")"
  lifecycle="$(trim "$lifecycle")"
  spec="$(trim "$spec")"
  successor="$(trim "$successor")"

  case "$topic" in
    Topic | "---"* | "") continue ;;
  esac

  case "$lifecycle" in
    active | superseded | archived) ;;
    *) fail_msg "$topic: invalid lifecycle '$lifecycle'"; continue ;;
  esac

  spec_path="${spec#*](./}"
  spec_path="${spec_path%%)*}"
  [[ -f "$specs_dir/$spec_path" ]] || fail_msg "$topic: missing SPEC file '$spec_path'"
  slug="${spec_path%/SPEC.md}"
  printf '%s\t%s\t%s\n' "$slug" "$lifecycle" "$successor" >>"$rows"

  if [[ "$lifecycle" == "superseded" ]]; then
    if [[ -z "$successor" || "$successor" == "-" ]]; then
      fail_msg "$topic: superseded without a Successor"
      continue
    fi
    target="${successor#*](./}"
    target="${target%%)*}"
    [[ -f "$specs_dir/$target" ]] || fail_msg "$topic: Successor target '$target' does not exist"
  fi
done <"$catalog"


lifecycle_of() {
  awk -F "$tab" -v topic="$1" '$1 == topic { print $2; exit }' "$rows"
}

# Every spec directory must be registered exactly once.
while IFS= read -r dir; do
  topic="$(basename "$dir")"
  count="$(awk -F "$tab" -v topic="$topic" '$1 == topic { n++ } END { print n + 0 }' "$rows")"
  [[ "$count" -eq 1 ]] || fail_msg "$topic: registered $count times (expected 1)"
done < <(find "$specs_dir" -mindepth 1 -maxdepth 1 -type d | sort)

# Retired SQLite artifacts must not remain a live contract once they are gone.
if [[ ! -d "$root/migrations" ]]; then
  [[ "$(lifecycle_of sqlite-writer-coordinator)" != "active" ]] ||
    fail_msg "sqlite-writer-coordinator: migrations/ is gone but lifecycle is still active"
  [[ -f "$specs_dir/sqlite-runtime-retirement/SPEC.md" ]] ||
    fail_msg "sqlite-runtime-retirement: missing retirement spec"
fi

if [[ -f "$specs_dir/runtime-path-consolidation/SPEC.md" ]]; then
  fail_msg "runtime-path-consolidation: stale directory (renamed to sqlite-runtime-retirement)"
fi

if ((fail)); then
  exit 1
fi
printf 'spec-lifecycle: ok\n'
