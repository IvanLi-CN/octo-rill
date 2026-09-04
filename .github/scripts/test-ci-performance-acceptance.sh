#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

python3 - "$repo_root/.github/scripts/ci_performance_acceptance.py" <<'PY'
from __future__ import annotations

import importlib.util
import json
import sys
import zipfile
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
    def __init__(self, candidate_retry_count=1, candidate_failed_tests=0):
        self.control_sha = "1" * 40
        self.candidate_sha = "2" * 40
        self.candidate_retry_count = candidate_retry_count
        self.candidate_failed_tests = candidate_failed_tests
        self.next_id = 1000
        self.runs = {}
        self.dispatches = []

    def api(self, endpoint, *, method="GET", payload=None):
        if endpoint.endswith("git/ref/heads/main"):
            return {"object": {"type": "commit", "sha": self.control_sha}}
        if endpoint.endswith("git/ref/heads/candidate"):
            return {"object": {"type": "commit", "sha": self.candidate_sha}}
        if "/compare/" in endpoint:
            return {
                "status": "ahead",
                "ahead_by": 1,
                "behind_by": 0,
                "files": [{"filename": path} for path in sorted(module.ALLOWED_CHANGED_PATHS)],
            }
        if endpoint.endswith("/dispatches") and method == "POST":
            assert payload["inputs"] == {module.ACCEPTANCE_INPUT: "true"}
            self.dispatches.append(payload["ref"])
            self.next_id += 1
            sha = self.control_sha if payload["ref"] in {"control", "main"} else self.candidate_sha
            started = datetime.now(timezone.utc)
            duration = 500 if sha == self.control_sha else 300 + (self.next_id % 5) * 10
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
            retry_count = 1 if sha == self.control_sha else self.candidate_retry_count
            failed_tests = 0 if sha == self.control_sha else self.candidate_failed_tests
            summary = {
                "schema_version": 1,
                "total_tests": 250,
                "passed_tests": 250 - failed_tests,
                "failed_tests": failed_tests,
                "skipped_tests": 0,
                "flaky_tests": min(1, retry_count) if failed_tests == 0 else 0,
                "retry_count": retry_count,
            }
            self.runs[self.next_id] = (run, sha, summary, duration)
            return None
        if "/actions/workflows/ci.yml/runs?" in endpoint:
            query = parse_qs(urlparse(endpoint).query)
            sha = query["head_sha"][0]
            return {"workflow_runs": [run for run, run_sha, _summary, _duration in self.runs.values() if run_sha == sha]}
        if "/actions/runs/" in endpoint and endpoint.endswith("/jobs?per_page=100"):
            run_id = int(endpoint.split("/actions/runs/")[1].split("/", 1)[0])
            run, sha, _summary, duration = self.runs[run_id]
            jobs = [{"name": name, "conclusion": "success", "steps": []} for name in sorted(module.REQUIRED_JOBS)]
            e2e = next(job for job in jobs if job["name"] == "Frontend E2E")
            e2e["started_at"] = run["run_started_at"]
            e2e["completed_at"] = (module.parse_time(run["run_started_at"]) + timedelta(seconds=duration)).isoformat().replace("+00:00", "Z")
            if sha == self.candidate_sha:
                next(job for job in jobs if job["name"] == "Build (Release)")["steps"] = [
                    {"name": "Run Docker release smoke", "conclusion": "success"}
                ]
            return {"jobs": jobs}
        if "/actions/runs/" in endpoint and endpoint.endswith("/artifacts?per_page=100"):
            run_id = int(endpoint.split("/actions/runs/")[1].split("/", 1)[0])
            return {"artifacts": [{"id": run_id, "name": module.SUMMARY_ARTIFACT_NAME, "expired": False}]}
        if "/actions/runs/" in endpoint:
            run_id = int(endpoint.rsplit("/", 1)[1])
            return self.runs[run_id][0]
        raise AssertionError(f"unexpected fake gh endpoint: {method} {endpoint}")

    def download(self, endpoint, output):
        artifact_id = int(endpoint.split("/artifacts/")[1].split("/", 1)[0])
        summary = self.runs[artifact_id][2]
        with zipfile.ZipFile(output, "w") as archive:
            archive.writestr("web/test-results/playwright-results.json", "{}")
            archive.writestr("web/test-results/playwright-summary.json", json.dumps(summary))


def run_fake(fake):
    refs = module.validate_ref_pair(fake, "IvanLi-CN/octo-rill", "main", "candidate")
    refs["changed_paths"] = module.validate_allowed_delta(fake, "IvanLi-CN/octo-rill", refs["control_sha"], refs["candidate_sha"])
    return refs, module.AcceptanceRunner(fake, "IvanLi-CN/octo-rill", poll_interval=0, timeout_seconds=1).run(refs)


fake = FakeGh()
refs, result = run_fake(fake)
expected_order = ["main", "candidate", "candidate", "main"] * 5
assert fake.dispatches == expected_order, fake.dispatches
assert len(result["runs"]) == 20
assert result["statistics"]["candidate_e2e_p90_seconds"] <= 420
assert result["statistics"]["candidate_final_failures"] == 0
assert result["statistics"]["candidate_retry_total"] <= result["statistics"]["control_retry_total"]
assert result["statistics"]["candidate_median_ratio"] <= 0.75
assert module.median([1, 3, 5, 7]) == 4
assert module.nearest_rank_p90(list(range(1, 11))) == 9

retry_heavy = FakeGh(candidate_retry_count=2)
_retry_refs, retry_result = run_fake(retry_heavy)
assert retry_result["statistics"]["candidate_retry_total"] > retry_result["statistics"]["control_retry_total"]
assert not retry_result["statistics"]["passed"]

failed_candidate = FakeGh(candidate_failed_tests=1)
_failed_refs, failed_result = run_fake(failed_candidate)
assert failed_result["statistics"]["candidate_final_failures"] == 10
assert not failed_result["statistics"]["passed"]

try:
    module.validate_playwright_summary({
        "schema_version": 1,
        "total_tests": 2,
        "passed_tests": 1,
        "failed_tests": 0,
        "skipped_tests": 0,
        "flaky_tests": 0,
        "retry_count": 2,
    })
except module.AcceptanceError:
    pass
else:
    raise AssertionError("inconsistent summary must be rejected")

retry = next(iter(fake.runs.values()))[0].copy()
retry["run_attempt"] = 2
try:
    module.validate_run(retry, {"jobs": []}, fake.control_sha, require_runtime_smoke=False)
except module.AcceptanceError as error:
    assert "retried" in str(error)
else:
    raise AssertionError("retry must be rejected")

try:
    module.validate_ref_pair(fake, "IvanLi-CN/octo-rill", "main", "main")
except module.AcceptanceError as error:
    assert "different" in str(error)
else:
    raise AssertionError("identical refs must be rejected")

get_results = iter([
    SimpleNamespace(returncode=1, stderr="TLS handshake timeout", stdout=""),
    SimpleNamespace(returncode=1, stderr="unexpected EOF", stdout=""),
    SimpleNamespace(returncode=0, stderr="", stdout='{"ok": true}'),
])
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
