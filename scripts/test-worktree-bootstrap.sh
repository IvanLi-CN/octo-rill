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
mkdir -p "$fixture_repo" "$override_source"

cleanup() {
  set +e
  git -C "$fixture_repo" worktree remove -f "$worktree_default" >/dev/null 2>&1
  git -C "$fixture_repo" worktree remove -f "$worktree_override" >/dev/null 2>&1
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

echo "worktree bootstrap smoke test passed"
