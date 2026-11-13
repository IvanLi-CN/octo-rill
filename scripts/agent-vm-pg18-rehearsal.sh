#!/usr/bin/env bash
set -euo pipefail

: "${AGENT_VM_SESSION_ID:=octo-rill-pg18}"
: "${AGENT_VM_SCRIPT:=/Users/ivan/.agents/skills/agent-vm/scripts/agent_vm.ts}"
: "${AGENT_VM_WORKTREE_NAME:=octo-rill-pg18-migration}"
: "${AGENT_VM_MIGRATION_BINARY:=}"

run_vm() {
  AGENT_VM_SESSION_ID="$AGENT_VM_SESSION_ID" bun "$AGENT_VM_SCRIPT" "$@"
}

echo "checking Agent VM control-plane health"
doctor_json="$(run_vm doctor)"
if [[ "$doctor_json" != *'"ready":true'* ]]; then
  echo "Agent VM is not ready; refusing to acquire or run migration rehearsal" >&2
  echo "$doctor_json" >&2
  exit 2
fi

run_vm acquire
run_vm sync --name "$AGENT_VM_WORKTREE_NAME"
if [[ -n "$AGENT_VM_MIGRATION_BINARY" ]]; then
  run_vm upload "$AGENT_VM_MIGRATION_BINARY" /tmp/octo-rill-migrate
  run_vm exec -- chmod +x /tmp/octo-rill-migrate
  migration_command='/tmp/octo-rill-migrate'
else
  migration_command='cargo run --locked --bin octo-rill-migrate --'
fi
run_vm exec -- sh -lc "
  set -eu
  cd /workspace/worktrees/$AGENT_VM_WORKTREE_NAME/src
  if [[ '$AGENT_VM_MIGRATION_BINARY' == '' ]]; then
    cargo test --bin octo-rill-migrate --locked
  fi
  test -n \"\${POSTGRES_URL:-}\" || { echo 'POSTGRES_URL is required inside the VM' >&2; exit 3; }
  SQLITE_SOURCE=\"\${SQLITE_SOURCE:?SQLITE_SOURCE is required inside the VM}\" \\
    POSTGRES_URL=\"\$POSTGRES_URL\" \\
    $migration_command preflight
"
