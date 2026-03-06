#!/bin/sh
set -eu

MAIN_ROOT_KEY=codex.worktree-sync.main-root

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

record_main_root() {
  main_root=$1
  git -C "$main_root" config --local "$MAIN_ROOT_KEY" "$main_root"
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

native_lefthook_bin() {
  base_root=$1
  os_arch=$(uname | tr '[:upper:]' '[:lower:]')
  cpu_arch=$(uname -m | sed 's/aarch64/arm64/;s/x86_64/x64/')
  candidate="$base_root/node_modules/lefthook-${os_arch}-${cpu_arch}/bin/lefthook"
  if [ -x "$candidate" ]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  return 1
}

select_lefthook_bin() {
  base_root=$1
  native_candidate=$(native_lefthook_bin "$base_root" || true)
  if [ -n "$native_candidate" ]; then
    printf '%s\n' "$native_candidate"
    return 0
  fi

  wrapper_candidate="$base_root/node_modules/.bin/lefthook"
  if [ -x "$wrapper_candidate" ]; then
    printf '%s\n' "$wrapper_candidate"
    return 0
  fi

  return 1
}

pin_hook_binary() {
  hook_path=$1
  hook_bin=$2
  tmp_path="$hook_path.tmp"
  escaped_bin=$(printf '%s' "$hook_bin" | sed 's/["\\]/\\&/g')

  {
    IFS= read -r first_line || first_line='#!/bin/sh'
    printf '%s\n' "$first_line"
    printf 'LEFTHOOK_BIN="%s"\n' "$escaped_bin"
    printf 'export LEFTHOOK_BIN\n\n'
    cat
  } < "$hook_path" > "$tmp_path"

  mv "$tmp_path" "$hook_path"
  chmod +x "$hook_path"
}

if ! git rev-parse --show-toplevel >/dev/null 2>&1; then
  log 'skip (not inside a Git worktree)'
  exit 0
fi

current_root=$(canonical_dir "$(git rev-parse --show-toplevel)")
main_root=$(discover_main_root "$current_root" || true)
main_bin=''

if [ -n "$main_root" ]; then
  record_main_root "$main_root"
  main_bin=$(select_lefthook_bin "$main_root" || true)
fi

if [ -z "$main_bin" ]; then
  if [ -n "$main_root" ] && [ "$current_root" != "$main_root" ]; then
    log "skip (run bun install once from main worktree: $main_root)"
    exit 0
  fi

  current_bin=$(select_lefthook_bin "$current_root" || true)
  if [ -z "$current_bin" ]; then
    if [ -n "$main_root" ]; then
      log "skip (repo-local lefthook binary missing in: $main_root)"
    else
      log 'skip (repo-local lefthook binary missing)'
    fi
    exit 0
  fi

  main_root=$current_root
  record_main_root "$main_root"
  main_bin=$current_bin
fi

(
  cd "$main_root"
  "$main_bin" install
)

for hook_name in pre-commit commit-msg post-checkout; do
  hook_path=$(git -C "$main_root" rev-parse --git-path "hooks/$hook_name")
  case "$hook_path" in
    /*) : ;;
    *) hook_path="$main_root/$hook_path" ;;
  esac
  if [ -f "$hook_path" ]; then
    pin_hook_binary "$hook_path" "$main_bin"
  fi
done
