#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" \
  --repo-root "$repo_root" \
  --declaration "$repo_root/.github/quality-gates.json" \
  --metadata-script "$repo_root/.github/scripts/metadata_gate.py" \
  --profile bootstrap

if python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" \
  --repo-root "$repo_root" \
  --declaration "$repo_root/.github/quality-gates.json" \
  --metadata-script "$repo_root/.github/scripts/metadata_gate.py" \
  --profile final >/dev/null 2>"$tmp_dir/profile-mismatch.log"; then
  echo "expected bootstrap declaration to reject final profile validation" >&2
  exit 1
fi

grep -q "implementation_profile='bootstrap' does not match workflow profile 'final'" "$tmp_dir/profile-mismatch.log"

coverage_repo="$tmp_dir/coverage-repo"
cp -R "$repo_root/." "$coverage_repo"
python3 - <<'PY' "$coverage_repo"
from pathlib import Path
import json
import sys

repo = Path(sys.argv[1])
path = repo / ".github/quality-gates.json"
payload = json.loads(path.read_text())
payload["required_checks"] = [item for item in payload["required_checks"] if item != "Build (Release)"]
payload["policy"]["branch_protection"]["required_status_checks"]["integrations"].pop("Build (Release)", None)
for workflow in payload["expected_pr_workflows"]:
    if workflow.get("workflow") == "CI Pipeline":
        workflow["jobs"] = [item for item in workflow["jobs"] if item != "Build (Release)"]
path.write_text(json.dumps(payload, indent=2) + "\n")
PY

if python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" --repo-root "$coverage_repo" --profile bootstrap >/dev/null 2>"$tmp_dir/coverage.log"; then
  echo "expected CI job coverage fixture to fail" >&2
  exit 1
fi

grep -q "unexpected=\['Build (Release)'\]" "$tmp_dir/coverage.log"

label_repo="$tmp_dir/label-repo"
cp -R "$repo_root/." "$label_repo"
python3 - <<'PY' "$label_repo"
from pathlib import Path
import sys

repo = Path(sys.argv[1])
path = repo / ".github/workflows/label-gate.yml"
text = path.read_text()
needle = "    types: [opened, synchronize, reopened, labeled, unlabeled, ready_for_review, edited]\n"
replacement = "    types: [opened, synchronize, reopened, labeled, unlabeled, ready_for_review]\n"
if needle not in text:
    raise SystemExit("failed to rewrite label-gate pull_request types")
path.write_text(text.replace(needle, replacement, 1))
PY

if python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" --repo-root "$label_repo" --profile bootstrap >/dev/null 2>"$tmp_dir/label.log"; then
  echo "expected label-gate edited trigger drift to fail" >&2
  exit 1
fi

grep -q "label-gate.yml.on.pull_request.types drifted" "$tmp_dir/label.log"

review_repo="$tmp_dir/review-repo"
cp -R "$repo_root/." "$review_repo"
python3 - <<'PY' "$review_repo"
from pathlib import Path
import sys

repo = Path(sys.argv[1])
path = repo / ".github/workflows/review-policy.yml"
text = path.read_text()
needle = "                if (!decisionStates.has(review.state)) {\n                  continue\n                }\n"
if needle not in text:
    raise SystemExit("failed to locate review decision-state guard")
path.write_text(text.replace(needle, "", 1))
PY

if python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" --repo-root "$review_repo" --profile bootstrap >/dev/null 2>"$tmp_dir/review.log"; then
  echo "expected review-policy decision-state drift to fail" >&2
  exit 1
fi

grep -q "bootstrap review gate must retain the latest decision review only" "$tmp_dir/review.log"

echo "test-quality-gates-contract: all checks passed"
