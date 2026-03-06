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

resolve_main_root() {
  common_dir=$(git rev-parse --git-common-dir)
  case "$common_dir" in
    /*) canonical_dir "$common_dir/.." ;;
    *) canonical_dir "$common_dir/.." ;;
  esac
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
main_root=$(resolve_main_root)
main_bin=$(select_lefthook_bin "$main_root" || true)

if [ -z "$main_bin" ]; then
  if [ "$current_root" != "$main_root" ]; then
    log "skip (run bun install once from main worktree: $main_root)"
    exit 0
  fi

  current_bin=$(select_lefthook_bin "$current_root" || true)
  if [ -z "$current_bin" ]; then
    log 'skip (repo-local lefthook binary missing)'
    exit 0
  fi

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
