#!/usr/bin/env bash
set -euo pipefail

: "${AGENT_VM_SESSION_ID:=octo-rill-pg18}"
: "${AGENT_VM_SCRIPT:=/Users/ivan/.agents/resources/skills/vendor/shared/IvanLi-CN/agent-vm/skills/agent-vm/scripts/agent_vm.ts}"
: "${AGENT_VM_WORKTREE_NAME:=octo-rill-pg18-migration}"

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
run_vm exec -- sh -lc "
  set -eu
  cd /workspace/worktrees/$AGENT_VM_WORKTREE_NAME
  cargo test --bin octo-rill-migrate --locked
  test -n \"\${POSTGRES_URL:-}\" || { echo 'POSTGRES_URL is required inside the VM' >&2; exit 3; }
  SQLITE_SOURCE=\"\${SQLITE_SOURCE:?SQLITE_SOURCE is required inside the VM}\" \\
    POSTGRES_URL=\"\$POSTGRES_URL\" \\
    cargo run --locked --bin octo-rill-migrate -- preflight
"
