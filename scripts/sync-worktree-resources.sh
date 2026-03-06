#!/bin/sh
set -eu

ZERO_OID=0000000000000000000000000000000000000000
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

current_root=$(git rev-parse --show-toplevel)
current_root=$(canonical_dir "$current_root")
git_dir=$(resolve_git_path --git-dir)
common_dir=$(resolve_git_path --git-common-dir)
default_source_root=$(canonical_dir "$common_dir/..")

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

source_override=$(git config --path --get codex.worktree-sync.source-root 2>/dev/null || true)
if [ -n "$source_override" ]; then
  case "$source_override" in
    /*) source_root=$(canonical_dir "$source_override") ;;
    *) source_root=$(canonical_path "$default_source_root/$source_override") ;;
  esac
else
  source_root=$default_source_root
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
