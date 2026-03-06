#!/bin/sh
set -eu

log() {
  printf 'hooks-install: %s\n' "$*"
}

canonical_dir() {
  CDPATH= cd -- "$1" && pwd -P
}

canonical_path() {
  target=$1
  parent=$(dirname -- "$target")
  base=$(basename -- "$target")
  printf '%s/%s\n' "$(canonical_dir "$parent")" "$base"
}

if ! git rev-parse --show-toplevel >/dev/null 2>&1; then
  log 'skip (not inside a Git worktree)'
  exit 0
fi

current_root=$(canonical_dir "$(git rev-parse --show-toplevel)")
common_dir=$(git rev-parse --git-common-dir)
case "$common_dir" in
  /*) common_dir=$(canonical_dir "$common_dir") ;;
  *) common_dir=$(canonical_path "$common_dir") ;;
esac
main_root=$(canonical_dir "$common_dir/..")
main_bin="$main_root/node_modules/.bin/lefthook"

if [ ! -x "$main_bin" ]; then
  if [ "$current_root" != "$main_root" ]; then
    log "skip (run bun install once from main worktree: $main_root)"
    exit 0
  fi

  if [ ! -x "$current_root/node_modules/.bin/lefthook" ]; then
    log 'skip (repo-local lefthook binary missing)'
    exit 0
  fi

  main_bin="$current_root/node_modules/.bin/lefthook"
fi

(
  cd "$main_root"
  "$main_bin" install
)
