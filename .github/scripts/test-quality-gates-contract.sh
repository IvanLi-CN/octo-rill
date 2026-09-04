#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

python3 - "$repo_root/web/playwright.config.ts" <<'PY'
from pathlib import Path
import sys

text = Path(sys.argv[1]).read_text()
assert "workers: process.env.CI ? 2 : undefined" in text
assert '["list"]' in text and '["json", { outputFile: "test-results/playwright-results.json" }]' in text
assert "retries: process.env.CI ? 2 : 0" in text
print("test-web-playwright-contract: all checks passed")
PY

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" \
  --repo-root "$repo_root" \
  --declaration "$repo_root/.github/quality-gates.json" \
  --metadata-script "$repo_root/.github/scripts/metadata_gate.py" \
  --profile final

if python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" \
  --repo-root "$repo_root" \
  --declaration "$repo_root/.github/quality-gates.json" \
  --metadata-script "$repo_root/.github/scripts/metadata_gate.py" \
  --profile bootstrap >/dev/null 2>"$tmp_dir/profile-mismatch.log"; then
  echo "expected final declaration to reject bootstrap profile validation" >&2
  exit 1
fi

grep -q "implementation_profile='final' does not match workflow profile 'bootstrap'" "$tmp_dir/profile-mismatch.log"

coverage_repo="$tmp_dir/coverage-repo"
mkdir -p "$coverage_repo"
cp -R "$repo_root/.github" "$coverage_repo/.github"
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

if python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" --repo-root "$coverage_repo" --profile final >/dev/null 2>"$tmp_dir/coverage.log"; then
  echo "expected CI job coverage fixture to fail" >&2
  exit 1
fi

grep -q "unexpected=\['Build (Release)'\]" "$tmp_dir/coverage.log"

label_repo="$tmp_dir/label-repo"
mkdir -p "$label_repo"
cp -R "$repo_root/.github" "$label_repo/.github"
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

if python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" --repo-root "$label_repo" --profile final >/dev/null 2>"$tmp_dir/label.log"; then
  echo "expected label-gate edited trigger drift to fail" >&2
  exit 1
fi

grep -q "label-gate.yml.on.pull_request_target.types drifted" "$tmp_dir/label.log"

review_repo="$tmp_dir/review-repo"
mkdir -p "$review_repo"
cp -R "$repo_root/.github" "$review_repo/.github"
python3 - <<'PY' "$review_repo"
from pathlib import Path
import sys

repo = Path(sys.argv[1])
path = repo / ".github/workflows/review-policy.yml"
text = path.read_text()
needle = 'git fetch --no-tags --depth=1 origin "${{ github.event.pull_request.base.ref }}"\n'
if needle not in text:
    raise SystemExit("failed to locate review-policy trusted-source fetch")
path.write_text(text.replace(needle, "", 1))
PY

if python3 "$repo_root/.github/scripts/check_quality_gates_contract.py" --repo-root "$review_repo" --profile final >/dev/null 2>"$tmp_dir/review.log"; then
  echo "expected review-policy trusted-source drift to fail" >&2
  exit 1
fi

grep -q "review-policy.yml: trusted-source fetch drifted" "$tmp_dir/review.log"

python3 - <<'PY' "$repo_root" "$tmp_dir"
from pathlib import Path
import shutil
import subprocess
import sys

repo_root = Path(sys.argv[1])
tmp_dir = Path(sys.argv[2])
cases = (
    (
        "ci.yml",
        "group: ci-${{ github.event_name == 'pull_request' && github.event.action == 'edited' && format('metadata-{0}', github.run_id) || github.ref }}",
        "group: ci-${{ github.ref }}",
        "ci.yml.concurrency.group drifted",
    ),
    (
        "label-gate.yml",
        "group: label-gate-${{ github.event.action == 'edited' && format('metadata-{0}', github.run_id) || github.event.pull_request.number || github.run_id }}",
        "group: label-gate-${{ github.event.pull_request.number || github.run_id }}",
        "label-gate.yml.concurrency.group drifted",
    ),
    (
        "review-policy.yml",
        "group: review-policy-${{ github.event_name == 'pull_request' && github.event.action == 'edited' && format('metadata-{0}', github.run_id) || github.event.pull_request.number || github.run_id }}",
        "group: review-policy-${{ github.event.pull_request.number || github.run_id }}",
        "review-policy.yml.concurrency.group drifted",
    ),
)

for workflow, expected, replacement, failure in cases:
    candidate = tmp_dir / f"{workflow}-concurrency-repo"
    shutil.copytree(repo_root / ".github", candidate / ".github")
    path = candidate / ".github" / "workflows" / workflow
    text = path.read_text()
    if expected not in text:
        raise SystemExit(f"failed to locate metadata concurrency group in {workflow}")
    path.write_text(text.replace(expected, replacement, 1))
    result = subprocess.run(
        [
            sys.executable,
            str(repo_root / ".github" / "scripts" / "check_quality_gates_contract.py"),
            "--repo-root",
            str(candidate),
            "--profile",
            "final",
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        raise SystemExit(f"expected {workflow} concurrency drift to fail")
    if failure not in result.stderr:
        raise SystemExit(f"missing {workflow} concurrency drift assertion: {result.stderr}")

workflow_cases = (
    (
        "build-needs",
        lambda text: text.replace(
            "    runs-on: ubuntu-latest\n    if: github.event_name ==",
            "    runs-on: ubuntu-latest\n    needs: []\n    if: github.event_name ==",
            1,
        ),
        "ci.yml.jobs.build must not wait on unrelated jobs",
    ),
    (
        "host-release-build",
        lambda text: text.replace(
            "      - name: Set up Docker Buildx\n",
            "      - name: Host release compilation\n        run: cargo build --release --locked\n\n      - name: Set up Docker Buildx\n",
            1,
        ),
        "must not repeat a host release compilation",
    ),
    (
        "dispatch-default",
        lambda text: text.replace("        default: false\n", "        default: true\n", 1),
        "ci.yml: ci_performance_acceptance must default to false",
    ),
    (
        "dispatch-target-input",
        lambda text: text.replace("      ci_performance_target_sha:\n", "      ci_performance_target:\n", 1),
        "ci.yml.on.workflow_dispatch.inputs.ci_performance_target_sha must be an object",
    ),
    (
        "acceptance-run-name",
        lambda text: text.replace(
            "run-name: ${{ github.event_name == 'workflow_dispatch' && inputs.ci_performance_acceptance && format('CI performance acceptance {0}', inputs.ci_performance_acceptance_nonce) || 'CI Pipeline' }}\n",
            "run-name: CI Pipeline\n",
            1,
        ),
        "ci.yml: controlled acceptance run-name drifted",
    ),
    (
        "acceptance-checkout-ref",
        lambda text: text.replace(
            "          ref: ${{ github.event_name == 'workflow_dispatch' && inputs.ci_performance_acceptance && inputs.ci_performance_target_sha || github.sha }}\n",
            "          ref: ${{ github.sha }}\n",
            1,
        ),
        "ci.yml.jobs.lint: controlled checkout ref drifted",
    ),
    (
        "acceptance-tooling-ref",
        lambda text: text.replace(
            "          ref: ${{ github.workflow_sha }}\n",
            "          ref: main\n",
            1,
        ),
        "ci.yml: acceptance tooling must bind github.workflow_sha",
    ),
    (
        "docker-load",
        lambda text: text.replace("          load: true\n", "", 1),
        "ci.yml: Docker smoke build must load the image",
    ),
    (
        "runtime-version-assertion",
        lambda text: text.replace(".ok == true and .version == $version", ".ok == true", 1),
        "ci.yml: Docker runtime smoke must contain '.ok == true and .version == $version'",
    ),
    (
        "runtime-cleanup",
        lambda text: text.replace('            docker rm -f "$container"', '            docker rm "$container"', 1),
        "ci.yml: Docker runtime smoke must contain 'docker rm -f'",
    ),
    (
        "e2e-summary-always",
        lambda text: text.replace(
            "      - name: Summarize Playwright results\n        id: summary\n        if: ${{ always() }}",
            "      - name: Summarize Playwright results\n        id: summary",
            1,
        ),
        "ci.yml: Playwright summary must run with always()",
    ),
    (
        "e2e-historical-json",
        lambda text: text.replace(" --reporter=list,json", "", 1),
        "ci.yml: controlled E2E must force a JSON reporter for immutable historical targets",
    ),
    (
        "e2e-artifact-retention",
        lambda text: text.replace("          retention-days: 14\n", "", 1),
        "ci.yml: Playwright artifact retention must be 14 days",
    ),
    (
        "e2e-upload-failure",
        lambda text: text.replace(
            "        continue-on-error: ${{ steps.playwright.outcome == 'failure' || steps.summary.outcome == 'failure' }}\n",
            "",
            1,
        ),
        "ci.yml: Playwright artifact upload must not mask the original test failure",
    ),
)

for name, rewrite, failure in workflow_cases:
    candidate = tmp_dir / f"ci-{name}-repo"
    shutil.copytree(repo_root / ".github", candidate / ".github")
    path = candidate / ".github/workflows/ci.yml"
    rewritten = rewrite(path.read_text())
    if rewritten == path.read_text():
        raise SystemExit(f"failed to rewrite workflow case {name}")
    path.write_text(rewritten)
    result = subprocess.run(
        [
            sys.executable,
            str(repo_root / ".github/scripts/check_quality_gates_contract.py"),
            "--repo-root",
            str(candidate),
            "--profile",
            "final",
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        raise SystemExit(f"expected workflow case {name} to fail")
    if failure not in result.stderr:
        raise SystemExit(f"missing workflow case {name} assertion: {result.stderr}")
PY

echo "test-quality-gates-contract: all checks passed"
