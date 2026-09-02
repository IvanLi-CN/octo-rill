#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

python3 - "$repo_root/.github/scripts/ci_performance_acceptance.py" <<'PY'
from __future__ import annotations

import importlib.util
import sys
from datetime import datetime, timedelta, timezone
from types import SimpleNamespace
from urllib.parse import parse_qs, urlparse

module_path = sys.argv[1]
spec = importlib.util.spec_from_file_location("ci_performance_acceptance", module_path)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class FakeGh:
    def __init__(self):
        self.control_sha = "1" * 40
        self.candidate_sha = "2" * 40
        self.shared_blob_sha = "3" * 40
        self.candidate_shared_blob_sha = self.shared_blob_sha
        self.next_id = 1000
        self.runs = {}
        self.dispatches = []

    def api(self, endpoint, *, method="GET", payload=None):
        if endpoint.endswith("git/ref/heads/control"):
            return {"object": {"type": "commit", "sha": self.control_sha}}
        if endpoint.endswith("git/ref/heads/candidate"):
            return {"object": {"type": "commit", "sha": self.candidate_sha}}
        if "/contents/web/e2e/admin-jobs.spec.ts?ref=" in endpoint:
            blob_sha = self.shared_blob_sha if "ref=" + self.control_sha in endpoint else self.candidate_shared_blob_sha
            return {"type": "file", "sha": blob_sha}
        if "/compare/" in endpoint:
            if f"{module.BASELINE_SHA}...{self.control_sha}" in endpoint:
                return {"files": [{"filename": path} for path in sorted(module.CONTROL_BASELINE_PATHS)]}
            return {"files": [{"filename": path} for path in sorted(module.ALLOWED_CHANGED_PATHS)]}
        if endpoint.endswith("/dispatches") and method == "POST":
            assert payload["inputs"] == {module.ACCEPTANCE_INPUT: "true"}
            self.dispatches.append(payload["ref"])
            self.next_id += 1
            sha = self.control_sha if payload["ref"] == "control" else self.candidate_sha
            started = datetime.now(timezone.utc)
            duration = 900 if sha == self.control_sha else 600 + (self.next_id % 5) * 10
            created = started.isoformat().replace("+00:00", "Z")
            run = {
                "id": self.next_id,
                "name": "CI Pipeline",
                "event": "workflow_dispatch",
                "head_branch": payload["ref"],
                "head_sha": sha,
                "run_attempt": 1,
                "status": "completed",
                "conclusion": "success",
                "created_at": created,
                "run_started_at": created,
                "updated_at": (started + timedelta(seconds=duration)).isoformat().replace("+00:00", "Z"),
            }
            self.runs[self.next_id] = (run, sha)
            return None
        if "/actions/workflows/ci.yml/runs?" in endpoint:
            query = parse_qs(urlparse(endpoint).query)
            sha = query["head_sha"][0]
            return {"workflow_runs": [run for run, run_sha in self.runs.values() if run_sha == sha]}
        if "/actions/runs/" in endpoint and endpoint.endswith("/jobs?per_page=100"):
            run_id = int(endpoint.split("/actions/runs/")[1].split("/", 1)[0])
            _run, sha = self.runs[run_id]
            jobs = [{"name": name, "conclusion": "success", "steps": []} for name in sorted(module.REQUIRED_JOBS)]
            if sha == self.candidate_sha:
                next(job for job in jobs if job["name"] == "Build (Release)")["steps"] = [
                    {"name": "Run Docker release smoke", "conclusion": "success"}
                ]
            return {"jobs": jobs}
        if "/actions/runs/" in endpoint:
            run_id = int(endpoint.rsplit("/", 1)[1])
            return self.runs[run_id][0]
        raise AssertionError(f"unexpected fake gh endpoint: {method} {endpoint}")


fake = FakeGh()
refs = module.validate_ref_pair(fake, "IvanLi-CN/octo-rill", "control", "candidate")
changed = module.validate_allowed_delta(fake, "IvanLi-CN/octo-rill", refs["control_sha"], refs["candidate_sha"])
refs["changed_paths"] = changed
result = module.AcceptanceRunner(fake, "IvanLi-CN/octo-rill", poll_interval=0, timeout_seconds=1).run(refs)
expected_order = ["control", "candidate", "candidate", "control"] * 5
assert fake.dispatches == expected_order, fake.dispatches
assert len(result["runs"]) == 20
assert "web/e2e/admin-jobs.spec.ts" not in changed
assert result["statistics"]["candidate_median_seconds"] <= 720
assert result["statistics"]["candidate_p90_seconds"] <= 840
assert result["statistics"]["candidate_median_ratio"] <= 0.75
assert module.median([1, 3, 5, 7]) == 4
assert module.nearest_rank_p90(list(range(1, 11))) == 9

retry = next(iter(fake.runs.values()))[0].copy()
retry["run_attempt"] = 2
try:
    module.validate_run(retry, {"jobs": []}, fake.control_sha, require_runtime_smoke=False)
except module.AcceptanceError as error:
    assert "retried" in str(error)
else:
    raise AssertionError("retry must be rejected")

try:
    module.validate_ref_pair(fake, "IvanLi-CN/octo-rill", "control", "control")
except module.AcceptanceError as error:
    assert "different" in str(error)
else:
    raise AssertionError("identical refs must be rejected")

fake.candidate_shared_blob_sha = "4" * 40
try:
    module.validate_ref_pair(fake, "IvanLi-CN/octo-rill", "control", "candidate")
except module.AcceptanceError as error:
    assert "share the exact" in str(error)
else:
    raise AssertionError("shared control files must match exactly")

get_results = iter(
    [
        SimpleNamespace(returncode=1, stderr="TLS handshake timeout", stdout=""),
        SimpleNamespace(returncode=1, stderr="unexpected EOF", stdout=""),
        SimpleNamespace(returncode=0, stderr="", stdout='{"ok": true}'),
    ]
)
get_calls = []
sleep_calls = []
original_run = module.subprocess.run
original_sleep = module.time.sleep
try:
    def fake_get_run(command, **kwargs):
        get_calls.append((command, kwargs))
        return next(get_results)

    module.subprocess.run = fake_get_run
    module.time.sleep = lambda seconds: sleep_calls.append(seconds)
    assert module.GhApi().api("repos/IvanLi-CN/octo-rill/actions/runs") == {"ok": True}
    assert len(get_calls) == 3
    assert sleep_calls == [1, 2]

    post_calls = []

    def fake_post_run(command, **kwargs):
        post_calls.append((command, kwargs))
        return SimpleNamespace(returncode=1, stderr="temporarily unavailable", stdout="")

    module.subprocess.run = fake_post_run
    try:
        module.GhApi().api(
            "repos/IvanLi-CN/octo-rill/actions/workflows/ci.yml/dispatches",
            method="POST",
            payload={"ref": "candidate"},
        )
    except module.AcceptanceError as error:
        assert "POST" in str(error)
    else:
        raise AssertionError("POST failures must not be retried")
    assert len(post_calls) == 1
finally:
    module.subprocess.run = original_run
    module.time.sleep = original_sleep

print("test-ci-performance-acceptance: all checks passed")
PY
