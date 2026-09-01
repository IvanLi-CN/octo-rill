#!/usr/bin/env python3
"""Run the controlled CI wall-clock acceptance benchmark through ``gh api``."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Protocol
from urllib.parse import urlencode


BASELINE_SHA = "03bf0b9fa19bfb5b416a90a9db7a10a1ff32f789"
ACCEPTANCE_INPUT = "ci_performance_acceptance"
REQUIRED_JOBS = {
    "Lint & Checks",
    "Backend Tests",
    "Frontend E2E",
    "Worktree Bootstrap Smoke (ubuntu-latest)",
    "Worktree Bootstrap Smoke (macos-latest)",
    "Build (Release)",
}
ALLOWED_CHANGED_PATHS = {
    ".github/workflows/ci.yml",
    ".github/scripts/check_quality_gates_contract.py",
    ".github/scripts/test-quality-gates-contract.sh",
    ".github/scripts/fixtures/quality-gates-contract/ci.yml",
    ".github/scripts/ci_performance_acceptance.py",
    ".github/scripts/test-ci-performance-acceptance.sh",
    "docs/specs/README.md",
    "docs/specs/ci-wall-clock-acceptance/SPEC.md",
    "docs/specs/ci-wall-clock-acceptance/IMPLEMENTATION.md",
    "docs/specs/ci-wall-clock-acceptance/HISTORY.md",
    "docs/repository-governance.md",
    "web/e2e/admin-jobs.spec.ts",
}
SHA_RE = re.compile(r"^[0-9a-f]{40}$")


class AcceptanceError(RuntimeError):
    """Raised when the benchmark cannot make a valid performance claim."""


class ApiClient(Protocol):
    def api(self, endpoint: str, *, method: str = "GET", payload: dict[str, Any] | None = None) -> Any:
        ...


class GhApi:
    def __init__(self, gh_bin: str = "gh") -> None:
        self.gh_bin = gh_bin

    def api(self, endpoint: str, *, method: str = "GET", payload: dict[str, Any] | None = None) -> Any:
        command = [self.gh_bin, "api", endpoint]
        if method != "GET":
            command.extend(["--method", method, "--input", "-"])
        result = subprocess.run(
            command,
            input=json.dumps(payload) if payload is not None else None,
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            detail = result.stderr.strip() or result.stdout.strip()
            raise AcceptanceError(f"gh api failed ({method} {endpoint}): {detail}")
        try:
            return json.loads(result.stdout) if result.stdout.strip() else None
        except json.JSONDecodeError as exc:
            raise AcceptanceError(f"gh api returned invalid JSON ({method} {endpoint})") from exc


def parse_time(value: str) -> datetime:
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise AcceptanceError(f"invalid GitHub timestamp: {value!r}") from exc


def duration_seconds(run: dict[str, Any]) -> float:
    started = run.get("run_started_at")
    updated = run.get("updated_at")
    if not isinstance(started, str) or not isinstance(updated, str):
        raise AcceptanceError("workflow run is missing run_started_at or updated_at")
    duration = (parse_time(updated) - parse_time(started)).total_seconds()
    if duration < 0:
        raise AcceptanceError("workflow run updated_at precedes run_started_at")
    return duration


def median(values: list[float]) -> float:
    if not values:
        raise AcceptanceError("cannot calculate median of an empty sample")
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


def nearest_rank_p90(values: list[float]) -> float:
    if not values:
        raise AcceptanceError("cannot calculate P90 of an empty sample")
    ordered = sorted(values)
    rank = max(1, math.ceil(0.9 * len(ordered)))
    return ordered[rank - 1]


def ref_name(ref: str) -> str:
    return ref.removeprefix("refs/heads/")


def resolve_ref(client: ApiClient, repo: str, ref: str) -> str:
    response = client.api(f"repos/{repo}/git/ref/heads/{ref_name(ref)}")
    obj = response.get("object") if isinstance(response, dict) else None
    sha = obj.get("sha") if isinstance(obj, dict) else None
    obj_type = obj.get("type") if isinstance(obj, dict) else None
    if obj_type != "commit" or not isinstance(sha, str) or not SHA_RE.fullmatch(sha):
        raise AcceptanceError(f"ref {ref!r} did not resolve to an immutable commit SHA")
    return sha


def validate_ref_pair(client: ApiClient, repo: str, control_ref: str, candidate_ref: str) -> dict[str, Any]:
    if not control_ref or not candidate_ref or ref_name(control_ref) == ref_name(candidate_ref):
        raise AcceptanceError("control and candidate refs must be different")
    control_sha = resolve_ref(client, repo, control_ref)
    candidate_sha = resolve_ref(client, repo, candidate_ref)
    if control_sha == candidate_sha:
        raise AcceptanceError("control and candidate refs must resolve to different SHAs")
    baseline_compare = client.api(f"repos/{repo}/compare/{BASELINE_SHA}...{control_sha}")
    baseline_files = sorted(
        item.get("filename")
        for item in (baseline_compare.get("files", []) if isinstance(baseline_compare, dict) else [])
        if isinstance(item, dict) and isinstance(item.get("filename"), str)
    )
    if baseline_files != [".github/workflows/ci.yml"]:
        raise AcceptanceError(
            "control ref must be derived from the verified baseline with only the dispatch workflow change"
        )
    return {
        "control_ref": ref_name(control_ref),
        "control_sha": control_sha,
        "candidate_ref": ref_name(candidate_ref),
        "candidate_sha": candidate_sha,
    }


def validate_allowed_delta(client: ApiClient, repo: str, control_sha: str, candidate_sha: str) -> list[str]:
    response = client.api(f"repos/{repo}/compare/{control_sha}...{candidate_sha}")
    files = response.get("files", []) if isinstance(response, dict) else []
    changed = sorted(
        item.get("filename")
        for item in files
        if isinstance(item, dict) and isinstance(item.get("filename"), str)
    )
    unexpected = sorted(set(changed) - ALLOWED_CHANGED_PATHS)
    if unexpected:
        raise AcceptanceError(f"candidate ref contains unexpected files: {unexpected}")
    return changed


def run_is_active(run: dict[str, Any]) -> bool:
    return run.get("status") in {"queued", "in_progress", "pending", "waiting", "requested"}


def workflow_runs(client: ApiClient, repo: str, sha: str) -> list[dict[str, Any]]:
    query = urlencode({"event": "workflow_dispatch", "head_sha": sha, "per_page": "100"})
    response = client.api(f"repos/{repo}/actions/workflows/ci.yml/runs?{query}")
    runs = response.get("workflow_runs", []) if isinstance(response, dict) else []
    if not isinstance(runs, list) or any(not isinstance(item, dict) for item in runs):
        raise AcceptanceError("GitHub workflow-runs response is malformed")
    return runs


def validate_jobs(run: dict[str, Any], jobs_payload: dict[str, Any], *, require_runtime_smoke: bool) -> list[dict[str, Any]]:
    jobs = jobs_payload.get("jobs", []) if isinstance(jobs_payload, dict) else []
    if not isinstance(jobs, list) or any(not isinstance(item, dict) for item in jobs):
        raise AcceptanceError(f"run {run.get('id')} returned malformed jobs")
    by_name = {item.get("name"): item for item in jobs if isinstance(item.get("name"), str)}
    missing = sorted(REQUIRED_JOBS - set(by_name))
    if missing:
        raise AcceptanceError(f"run {run.get('id')} is missing required jobs: {missing}")
    failed = sorted(name for name in REQUIRED_JOBS if by_name[name].get("conclusion") != "success")
    if failed:
        raise AcceptanceError(f"run {run.get('id')} has unsuccessful jobs: {failed}")
    if require_runtime_smoke:
        build_steps = by_name["Build (Release)"].get("steps", [])
        smoke = next((step for step in build_steps if step.get("name") == "Run Docker release smoke"), None)
        if not isinstance(smoke, dict) or smoke.get("conclusion") != "success":
            raise AcceptanceError(f"run {run.get('id')} has no successful Docker runtime smoke step")
    return jobs


def validate_run(run: dict[str, Any], jobs_payload: dict[str, Any], expected_sha: str, *, require_runtime_smoke: bool) -> dict[str, Any]:
    if run.get("event") != "workflow_dispatch" or run.get("head_sha") != expected_sha:
        raise AcceptanceError(f"run {run.get('id')} has an unexpected event or head SHA")
    if run.get("run_attempt") != 1:
        raise AcceptanceError(f"run {run.get('id')} was retried (run_attempt must be 1)")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise AcceptanceError(f"run {run.get('id')} did not complete successfully")
    jobs = validate_jobs(run, jobs_payload, require_runtime_smoke=require_runtime_smoke)
    return {
        "pipeline": run,
        "id": run.get("id"),
        "name": run.get("name"),
        "ref": run.get("head_branch"),
        "sha": run.get("head_sha"),
        "run_attempt": run.get("run_attempt"),
        "created_at": run.get("created_at"),
        "run_started_at": run.get("run_started_at"),
        "updated_at": run.get("updated_at"),
        "duration_seconds": duration_seconds(run),
        "jobs": jobs,
    }


@dataclass
class AcceptanceRunner:
    client: ApiClient
    repo: str
    workflow: str = "ci.yml"
    poll_interval: float = 15
    timeout_seconds: float = 2100

    def _workflow_runs(self, sha: str) -> list[dict[str, Any]]:
        query = urlencode({"event": "workflow_dispatch", "head_sha": sha, "per_page": "100"})
        response = self.client.api(f"repos/{self.repo}/actions/workflows/{self.workflow}/runs?{query}")
        runs = response.get("workflow_runs", []) if isinstance(response, dict) else []
        if not isinstance(runs, list) or any(not isinstance(item, dict) for item in runs):
            raise AcceptanceError("GitHub workflow-runs response is malformed")
        return runs

    def _dispatch_one(self, ref: str, sha: str, *, require_runtime_smoke: bool) -> dict[str, Any]:
        active = [run for run in self._workflow_runs(sha) if run_is_active(run)]
        if active:
            raise AcceptanceError(f"ref {ref!r} already has an active workflow run")
        dispatched_at = datetime.now(timezone.utc)
        self.client.api(
            f"repos/{self.repo}/actions/workflows/{self.workflow}/dispatches",
            method="POST",
            payload={"ref": ref, "inputs": {ACCEPTANCE_INPUT: "true"}},
        )
        deadline = time.monotonic() + self.timeout_seconds
        selected: dict[str, Any] | None = None
        while time.monotonic() <= deadline:
            candidates = []
            for run in self._workflow_runs(sha):
                created = run.get("created_at")
                if (
                    run.get("head_sha") == sha
                    and run.get("event") == "workflow_dispatch"
                    and isinstance(created, str)
                    and parse_time(created) >= dispatched_at
                ):
                    candidates.append(run)
            if len(candidates) > 1:
                raise AcceptanceError(f"dispatch for {ref!r} produced concurrent workflow runs")
            if candidates:
                selected = candidates[0]
                if selected.get("run_attempt") != 1:
                    raise AcceptanceError(f"run {selected.get('id')} was retried")
                if selected.get("status") == "completed":
                    break
            time.sleep(self.poll_interval)
        if selected is None:
            raise AcceptanceError(f"no workflow run appeared for dispatch ref {ref!r}")
        if selected.get("status") != "completed":
            raise AcceptanceError(f"workflow run {selected.get('id')} timed out")
        run = self.client.api(f"repos/{self.repo}/actions/runs/{selected.get('id')}")
        jobs = self.client.api(f"repos/{self.repo}/actions/runs/{selected.get('id')}/jobs?per_page=100")
        return validate_run(run, jobs, sha, require_runtime_smoke=require_runtime_smoke)

    def run(self, refs: dict[str, str], *, pairs: int = 10) -> dict[str, Any]:
        if pairs != 10:
            raise AcceptanceError("controlled acceptance requires exactly 10 pairs")
        records: list[dict[str, Any]] = []
        for pair in range(1, pairs + 1):
            order = ("control", "candidate") if pair % 2 else ("candidate", "control")
            for role in order:
                ref = refs[f"{role}_ref"]
                sha = refs[f"{role}_sha"]
                records.append(
                    {
                        "pair": pair,
                        "role": role,
                        "result": self._dispatch_one(ref, sha, require_runtime_smoke=role == "candidate"),
                    }
                )
        control = [item["result"]["duration_seconds"] for item in records if item["role"] == "control"]
        candidate = [item["result"]["duration_seconds"] for item in records if item["role"] == "candidate"]
        stats = {
            "control_median_seconds": median(control),
            "control_p90_seconds": nearest_rank_p90(control),
            "candidate_median_seconds": median(candidate),
            "candidate_p90_seconds": nearest_rank_p90(candidate),
            "candidate_median_ratio": median(candidate) / median(control),
            "passed": False,
            "thresholds": {
                "candidate_median_max_seconds": 720,
                "candidate_p90_max_seconds": 840,
                "candidate_median_ratio_max": 0.75,
            },
        }
        stats["passed"] = not (
            stats["candidate_median_seconds"] > 720
            or stats["candidate_p90_seconds"] > 840
            or stats["candidate_median_ratio"] > 0.75
        )
        return {"refs": refs, "pairs": pairs, "runs": records, "statistics": stats}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--control-ref", required=True)
    parser.add_argument("--candidate-ref", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--poll-interval", type=float, default=15)
    parser.add_argument("--timeout-seconds", type=float, default=2100)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.repo:
        print("--repo or GITHUB_REPOSITORY is required", file=sys.stderr)
        return 2
    client = GhApi(os.environ.get("GH_BIN", "gh"))
    try:
        refs = validate_ref_pair(client, args.repo, args.control_ref, args.candidate_ref)
        refs["changed_paths"] = validate_allowed_delta(client, args.repo, refs["control_sha"], refs["candidate_sha"])
        result = AcceptanceRunner(
            client,
            args.repo,
            poll_interval=args.poll_interval,
            timeout_seconds=args.timeout_seconds,
        ).run(refs)
    except AcceptanceError as exc:
        print(f"ci-performance-acceptance: {exc}", file=sys.stderr)
        return 1
    encoded = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(encoded, encoding="utf-8")
    print(encoded, end="")
    if not result["statistics"]["passed"]:
        print("ci-performance-acceptance: performance thresholds failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
