#!/bin/sh
set -eu

ZERO_OID=0000000000000000000000000000000000000000
MAIN_ROOT_KEY=codex.worktree-sync.main-root
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
MANIFEST_PATH="$SCRIPT_DIR/worktree-sync.paths"
FORCE_SYNC=${WORKTREE_SYNC_FORCE:-0}
DRY_RUN=${WORKTREE_SYNC_DRY_RUN:-0}

log() {
  printf 'worktree-sync: %s\n' "$*"
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

resolve_git_path() {
  git_path=$(git rev-parse "$1")
  case "$git_path" in
    /*) printf '%s\n' "$git_path" ;;
    *) canonical_path "$git_path" ;;
  esac
}

read_recorded_main_root() {
  recorded_root=$(git config --path --get "$MAIN_ROOT_KEY" 2>/dev/null || true)
  if [ -z "$recorded_root" ] || [ ! -d "$recorded_root" ]; then
    return 1
  fi
  canonical_dir "$recorded_root"
}

discover_main_root() {
  current_root=$1
  git_dir=$(resolve_git_path --git-dir)
  common_dir=$(resolve_git_path --git-common-dir)

  recorded_root=$(read_recorded_main_root || true)
  if [ -n "$recorded_root" ]; then
    printf '%s\n' "$recorded_root"
    return 0
  fi

  if [ "$git_dir" = "$common_dir" ]; then
    printf '%s\n' "$current_root"
    return 0
  fi

  listed_root=$(git worktree list --porcelain 2>/dev/null | awk '
    index($0, "worktree ") == 1 {
      print substr($0, 10)
      exit
    }
  ')
  if [ -n "$listed_root" ]; then
    listed_root=$(canonical_dir "$listed_root")
    if [ "$listed_root" != "$common_dir" ]; then
      printf '%s\n' "$listed_root"
      return 0
    fi
  fi

  if [ "$(basename -- "$common_dir")" = ".git" ]; then
    canonical_dir "$common_dir/.."
    return 0
  fi

  return 1
}

resolve_optional_path() {
  target=$1
  parent=$(dirname -- "$target")
  base=$(basename -- "$target")
  if [ -d "$parent" ]; then
    printf '%s/%s\n' "$(canonical_dir "$parent")" "$base"
    return 0
  fi
  printf '%s\n' "$target"
}

current_root=$(canonical_dir "$(git rev-parse --show-toplevel)")
git_dir=$(resolve_git_path --git-dir)
common_dir=$(resolve_git_path --git-common-dir)

if [ "$git_dir" = "$common_dir" ]; then
  log "skip main worktree"
  exit 0
fi

if [ ! -f "$MANIFEST_PATH" ]; then
  log "skip manifest missing: $MANIFEST_PATH"
  exit 0
fi

old_head=${1:-}
new_head=${2:-}
is_branch_checkout=${3:-0}

if [ "$FORCE_SYNC" != "1" ]; then
  if [ "$old_head" != "$ZERO_OID" ] || [ "$is_branch_checkout" != "1" ]; then
    log "skip non-initial checkout"
    exit 0
  fi
fi

main_root=$(discover_main_root "$current_root" || true)
source_override=$(git config --get codex.worktree-sync.source-root 2>/dev/null || true)
if [ -n "$source_override" ]; then
  if [ -z "$main_root" ]; then
    log "skip source root unresolved"
    exit 0
  fi

  case "$source_override" in
    /*) source_root=$(resolve_optional_path "$source_override") ;;
    *) source_root=$(resolve_optional_path "$main_root/$source_override") ;;
  esac
else
  if [ -z "$main_root" ]; then
    log "skip source root unresolved"
    exit 0
  fi
  source_root=$main_root
fi

copy_resource() {
  rel_path=$1
  src_path="$source_root/$rel_path"
  dst_path="$current_root/$rel_path"

  if [ ! -e "$src_path" ] && [ ! -L "$src_path" ]; then
    log "skip source missing: $rel_path"
    return 0
  fi

  if [ -e "$dst_path" ] || [ -L "$dst_path" ]; then
    log "keep target exists: $rel_path"
    return 0
  fi

  if [ "$DRY_RUN" = "1" ]; then
    log "would copy: $rel_path"
    return 0
  fi

  mkdir -p "$(dirname -- "$dst_path")"
  cp -R "$src_path" "$dst_path"
  log "copied: $rel_path"
}

while IFS= read -r entry || [ -n "$entry" ]; do
  case "$entry" in
    ''|'#'*)
      continue
      ;;
  esac
  copy_resource "$entry"
done < "$MANIFEST_PATH"

if [ "$DRY_RUN" = "1" ]; then
  log "dry-run complete"
else
  log "sync complete"
fi
