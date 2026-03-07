#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
repo_root="$(cd "$repo_root" && pwd -P)"

if ! command -v bun >/dev/null 2>&1; then
  echo "bun is required for worktree bootstrap smoke tests" >&2
  exit 1
fi

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/octo-rill-worktree-test.XXXXXX")"
tmp_root="$(cd "$tmp_root" && pwd -P)"
fixture_repo="$tmp_root/repo"
worktree_default="$tmp_root/default-worktree"
worktree_override="$tmp_root/override-worktree"
override_source="$tmp_root/override-source"
separate_git_dir="$tmp_root/separate.git"
separate_repo="$tmp_root/separate-repo"
separate_worktree="$tmp_root/separate-worktree"
legacy_repo="$tmp_root/legacy-repo"
missing_absolute_source="$tmp_root/does-not-exist/deeper"
mkdir -p "$fixture_repo" "$override_source"

cleanup() {
  set +e
  git -C "$fixture_repo" worktree remove -f "$worktree_default" >/dev/null 2>&1
  git -C "$fixture_repo" worktree remove -f "$worktree_override" >/dev/null 2>&1
  git -C "$separate_repo" worktree remove -f "$separate_worktree" >/dev/null 2>&1
  rm -rf "$tmp_root"
}
trap cleanup EXIT

assert_file_content() {
  local path="$1"
  local expected="$2"
  local actual
  if [[ ! -f "$path" ]]; then
    echo "expected file missing: $path" >&2
    exit 1
  fi
  actual="$(cat "$path")"
  if [[ "$actual" != "$expected" ]]; then
    echo "unexpected content for $path" >&2
    printf 'expected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
    exit 1
  fi
}

assert_output_contains() {
  local output="$1"
  local needle="$2"
  if [[ "$output" != *"$needle"* ]]; then
    echo "expected output to contain '$needle'" >&2
    printf 'actual output:\n%s\n' "$output" >&2
    exit 1
  fi
}

assert_output_not_contains() {
  local output="$1"
  local needle="$2"
  if [[ "$output" == *"$needle"* ]]; then
    echo "expected output to omit '$needle'" >&2
    printf 'actual output:\n%s\n' "$output" >&2
    exit 1
  fi
}

assert_checkout_succeeds_without_hook_exec_error() {
  local repo="$1"
  local target_ref="$2"
  local checkout_output

  checkout_output="$(git -C "$repo" checkout "$target_ref" 2>&1)"
  assert_output_not_contains "$checkout_output" "No such file or directory"
  assert_output_not_contains "$checkout_output" "exit status 127"
}

cp "$repo_root/package.json" "$fixture_repo/package.json"
cp "$repo_root/bun.lock" "$fixture_repo/bun.lock"
cp "$repo_root/lefthook.yml" "$fixture_repo/lefthook.yml"
cp -R "$repo_root/scripts" "$fixture_repo/scripts"

chmod +x "$fixture_repo/scripts/sync-worktree-resources.sh" "$fixture_repo/scripts/test-worktree-bootstrap.sh"

git -C "$fixture_repo" init -b main >/dev/null
git -C "$fixture_repo" config user.name 'Codex Test'
git -C "$fixture_repo" config user.email 'codex-test@example.com'
git -C "$fixture_repo" add package.json bun.lock lefthook.yml scripts
git -C "$fixture_repo" commit -m 'test: bootstrap fixture' >/dev/null

bun install --cwd "$fixture_repo" --frozen-lockfile >/dev/null
fixture_main_root="$(git -C "$fixture_repo" config --local --get codex.worktree-sync.main-root || true)"
if [[ "$fixture_main_root" != "$fixture_repo" ]]; then
  echo "expected shared main root config for fixture repo" >&2
  exit 1
fi

cat > "$fixture_repo/.env.local" <<'ENVLOCAL'
OCTORILL_ENCRYPTION_KEY_BASE64=source-local
ENVLOCAL
cat > "$fixture_repo/.env" <<'ENVFILE'
GITHUB_CLIENT_ID=source-env
ENVFILE

git -C "$fixture_repo" worktree add --detach "$worktree_default" HEAD >/dev/null
assert_file_content "$worktree_default/.env.local" "OCTORILL_ENCRYPTION_KEY_BASE64=source-local"
assert_file_content "$worktree_default/.env" "GITHUB_CLIENT_ID=source-env"

bun install --cwd "$worktree_default" --frozen-lockfile >/dev/null
hook_path="$(git -C "$fixture_repo" rev-parse --git-path hooks/post-checkout)"
if [[ "$hook_path" != /* ]]; then
  hook_path="$fixture_repo/$hook_path"
fi
hook_body="$(cat "$hook_path")"
assert_output_contains "$hook_body" "LEFTHOOK_BIN=\""
assert_output_contains "$hook_body" "$fixture_repo"
if [[ "$hook_body" == *"$worktree_default"* ]]; then
  echo "hook installation must stay pinned to main worktree" >&2
  exit 1
fi

printf 'custom-target\n' > "$worktree_default/.env.local"
preserve_output="$(cd "$worktree_default" && WORKTREE_SYNC_FORCE=1 "$fixture_repo/scripts/sync-worktree-resources.sh" ignored ignored 1)"
assert_output_contains "$preserve_output" "keep target exists: .env.local"
assert_file_content "$worktree_default/.env.local" "custom-target"

rm -f "$worktree_default/.env"
rm -f "$fixture_repo/.env"
missing_output="$(cd "$worktree_default" && WORKTREE_SYNC_FORCE=1 "$fixture_repo/scripts/sync-worktree-resources.sh" ignored ignored 1)"
assert_output_contains "$missing_output" "skip source missing: .env"
if [[ -e "$worktree_default/.env" ]]; then
  echo "expected missing source to keep target absent" >&2
  exit 1
fi

rerun_output="$(cd "$worktree_default" && WORKTREE_SYNC_FORCE=1 "$fixture_repo/scripts/sync-worktree-resources.sh" ignored ignored 1)"
assert_output_contains "$rerun_output" "keep target exists: .env.local"

cat > "$override_source/.env.local" <<'OVERRIDELOCAL'
OCTORILL_ENCRYPTION_KEY_BASE64=override-local
OVERRIDELOCAL
cat > "$override_source/.env" <<'OVERRIDEENV'
GITHUB_CLIENT_ID=override-env
OVERRIDEENV

override_relative="$(python3 - <<'PY' "$fixture_repo" "$override_source"
import os
import sys
print(os.path.relpath(sys.argv[2], sys.argv[1]))
PY
)"
git -C "$fixture_repo" config --local codex.worktree-sync.source-root "$override_relative"
git -C "$fixture_repo" worktree add --detach "$worktree_override" HEAD >/dev/null
assert_file_content "$worktree_override/.env.local" "OCTORILL_ENCRYPTION_KEY_BASE64=override-local"
assert_file_content "$worktree_override/.env" "GITHUB_CLIENT_ID=override-env"

rm -f "$fixture_repo/.env.local"
main_output="$(cd "$fixture_repo" && WORKTREE_SYNC_FORCE=1 "$fixture_repo/scripts/sync-worktree-resources.sh" ignored ignored 1)"
assert_output_contains "$main_output" "skip main worktree"
if [[ -e "$fixture_repo/.env.local" ]]; then
  echo "main worktree should remain unchanged" >&2
  exit 1
fi

rm -f "$worktree_override/.env.local" "$worktree_override/.env"
dry_run_output="$(cd "$worktree_override" && WORKTREE_SYNC_FORCE=1 WORKTREE_SYNC_DRY_RUN=1 "$fixture_repo/scripts/sync-worktree-resources.sh" ignored ignored 1)"
assert_output_contains "$dry_run_output" "would copy: .env.local"
assert_output_contains "$dry_run_output" "dry-run complete"
if [[ -e "$worktree_override/.env.local" || -e "$worktree_override/.env" ]]; then
  echo "dry-run should not materialize target files" >&2
  exit 1
fi

git -C "$fixture_repo" config --local codex.worktree-sync.source-root missing/dir
invalid_override_output="$(cd "$worktree_override" && WORKTREE_SYNC_FORCE=1 "$fixture_repo/scripts/sync-worktree-resources.sh" ignored ignored 1 2>&1)"
assert_output_contains "$invalid_override_output" "skip source missing: .env.local"
assert_output_not_contains "$invalid_override_output" "No such file or directory"

git -C "$fixture_repo" config --local codex.worktree-sync.source-root "$missing_absolute_source"
absolute_invalid_override_output="$(cd "$worktree_override" && WORKTREE_SYNC_FORCE=1 "$fixture_repo/scripts/sync-worktree-resources.sh" ignored ignored 1 2>&1)"
assert_output_contains "$absolute_invalid_override_output" "skip source missing: .env.local"
assert_output_not_contains "$absolute_invalid_override_output" "No such file or directory"
git -C "$fixture_repo" config --local --unset codex.worktree-sync.source-root

git clone --separate-git-dir="$separate_git_dir" "$fixture_repo" "$separate_repo" >/dev/null
bun install --cwd "$separate_repo" --frozen-lockfile >/dev/null
separate_main_root="$(git -C "$separate_repo" config --local --get codex.worktree-sync.main-root || true)"
if [[ "$separate_main_root" != "$separate_repo" ]]; then
  echo "expected shared main root config for separate git dir repo" >&2
  exit 1
fi
cat > "$separate_repo/.env.local" <<'SEPARATELOCAL'
OCTORILL_ENCRYPTION_KEY_BASE64=separate-local
SEPARATELOCAL
cat > "$separate_repo/.env" <<'SEPARATEENV'
GITHUB_CLIENT_ID=separate-env
SEPARATEENV

git -C "$separate_repo" worktree add --detach "$separate_worktree" HEAD >/dev/null
assert_file_content "$separate_worktree/.env.local" "OCTORILL_ENCRYPTION_KEY_BASE64=separate-local"
assert_file_content "$separate_worktree/.env" "GITHUB_CLIENT_ID=separate-env"

separate_hook_path="$(git -C "$separate_repo" rev-parse --git-path hooks/post-checkout)"
if [[ "$separate_hook_path" != /* ]]; then
  separate_hook_path="$separate_repo/$separate_hook_path"
fi
separate_hook_body="$(cat "$separate_hook_path")"
assert_output_contains "$separate_hook_body" "LEFTHOOK_BIN=\""
assert_output_contains "$separate_hook_body" "$separate_repo"
if [[ "$separate_hook_body" == *"$separate_git_dir"* ]]; then
  echo "hook installation must not pin the shared git dir path" >&2
  exit 1
fi

mkdir -p "$legacy_repo"
git -C "$legacy_repo" init -b main >/dev/null
git -C "$legacy_repo" config user.name 'Codex Test'
git -C "$legacy_repo" config user.email 'codex-test@example.com'
cp "$repo_root/lefthook.yml" "$legacy_repo/lefthook.yml"
git -C "$legacy_repo" add lefthook.yml
git -C "$legacy_repo" commit -m 'test: legacy hook config' >/dev/null
legacy_base_sha="$(git -C "$legacy_repo" rev-parse HEAD)"
cp "$repo_root/package.json" "$legacy_repo/package.json"
cp "$repo_root/bun.lock" "$legacy_repo/bun.lock"
cp -R "$repo_root/scripts" "$legacy_repo/scripts"
git -C "$legacy_repo" add package.json bun.lock scripts
git -C "$legacy_repo" commit -m 'test: add worktree bootstrap scripts' >/dev/null
legacy_head_sha="$(git -C "$legacy_repo" rev-parse HEAD)"
bun install --cwd "$legacy_repo" --frozen-lockfile >/dev/null
assert_checkout_succeeds_without_hook_exec_error "$legacy_repo" "$legacy_base_sha"
rm -rf "$legacy_repo/node_modules"
assert_checkout_succeeds_without_hook_exec_error "$legacy_repo" "$legacy_head_sha"
assert_checkout_succeeds_without_hook_exec_error "$legacy_repo" "$legacy_base_sha"

echo "worktree bootstrap smoke test passed"
