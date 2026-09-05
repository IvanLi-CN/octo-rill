#!/usr/bin/env python3
"""Run the controlled CI wall-clock acceptance benchmark through ``gh api``."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import secrets
import subprocess
import sys
import tempfile
import time
import zipfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Protocol
from urllib.parse import urlencode


ACCEPTANCE_INPUT = "ci_performance_acceptance"
ACCEPTANCE_TARGET_SHA_INPUT = "ci_performance_target_sha"
ACCEPTANCE_NONCE_INPUT = "ci_performance_acceptance_nonce"
ACCEPTANCE_RUN_TITLE_PREFIX = "CI performance acceptance"
SUMMARY_ARTIFACT_NAME = "playwright-e2e-results"
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
    "web/playwright.config.ts",
    "web/e2e/pwa-installability.spec.ts",
    "web/e2e/release-detail.spec.ts",
    "web/scripts/summarize-playwright-results.ts",
    "web/scripts/test-summarize-playwright-results.ts",
    "docs/specs/ci-wall-clock-acceptance/SPEC.md",
    "docs/specs/ci-wall-clock-acceptance/IMPLEMENTATION.md",
    "docs/specs/ci-wall-clock-acceptance/HISTORY.md",
}
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
TRANSIENT_GET_ERROR_MARKERS = (
    "tls handshake timeout",
    "eof",
    "unexpected eof",
    "connection reset by peer",
    "connection refused",
    "i/o timeout",
    "context deadline exceeded",
    "temporarily unavailable",
    "network is unreachable",
)
GET_MAX_ATTEMPTS = 6
MAX_RETRY_DELAY_SECONDS = 10


class AcceptanceError(RuntimeError):
    """Raised when the benchmark cannot make a valid performance claim."""


class ApiClient(Protocol):
    def api(self, endpoint: str, *, method: str = "GET", payload: dict[str, Any] | None = None) -> Any:
        ...

    def download(self, endpoint: str, output: Path) -> None:
        ...


class GhApi:
    def __init__(self, gh_bin: str = "gh") -> None:
        self.gh_bin = gh_bin

    def api(self, endpoint: str, *, method: str = "GET", payload: dict[str, Any] | None = None) -> Any:
        command = [self.gh_bin, "api", endpoint]
        if method != "GET":
            command.extend(["--method", method, "--input", "-"])
        for attempt in range(1, GET_MAX_ATTEMPTS + 1):
            result = subprocess.run(
                command,
                input=json.dumps(payload) if payload is not None else None,
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode == 0:
                break
            detail = result.stderr.strip() or result.stdout.strip()
            can_retry = method == "GET" and any(
                marker in detail.lower() for marker in TRANSIENT_GET_ERROR_MARKERS
            )
            if not can_retry or attempt == GET_MAX_ATTEMPTS:
                raise AcceptanceError(f"gh api failed ({method} {endpoint}): {detail}")
            time.sleep(min(2 ** (attempt - 1), MAX_RETRY_DELAY_SECONDS))
        try:
            return json.loads(result.stdout) if result.stdout.strip() else None
        except json.JSONDecodeError as exc:
            raise AcceptanceError(f"gh api returned invalid JSON ({method} {endpoint})") from exc

    def download(self, endpoint: str, output: Path) -> None:
        command = [self.gh_bin, "api", endpoint]
        for attempt in range(1, GET_MAX_ATTEMPTS + 1):
            result = subprocess.run(command, capture_output=True, text=False, check=False)
            if result.returncode == 0:
                output.write_bytes(result.stdout)
                return
            detail_bytes = result.stderr.strip() or result.stdout.strip()
            detail = detail_bytes.decode("utf-8", errors="replace")
            can_retry = any(marker in detail.lower() for marker in TRANSIENT_GET_ERROR_MARKERS)
            if not can_retry or attempt == GET_MAX_ATTEMPTS:
                raise AcceptanceError(f"gh api download failed (GET {endpoint}): {detail}")
            time.sleep(min(2 ** (attempt - 1), MAX_RETRY_DELAY_SECONDS))


def parse_time(value: str) -> datetime:
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise AcceptanceError(f"invalid GitHub timestamp: {value!r}") from exc


def interval_seconds(item: dict[str, Any], started_key: str, completed_key: str, label: str) -> float:
    started = item.get(started_key)
    completed = item.get(completed_key)
    if not isinstance(started, str) or not isinstance(completed, str):
        raise AcceptanceError(f"{label} is missing {started_key} or {completed_key}")
    duration = (parse_time(completed) - parse_time(started)).total_seconds()
    if duration < 0:
        raise AcceptanceError(f"{label} completed before it started")
    return duration


def duration_seconds(run: dict[str, Any]) -> float:
    return interval_seconds(run, "run_started_at", "updated_at", "workflow run")


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


def validate_commit_sha(client: ApiClient, repo: str, sha: str, label: str) -> str:
    if not SHA_RE.fullmatch(sha):
        raise AcceptanceError(f"{label} must be a 40-character immutable commit SHA")
    response = client.api(f"repos/{repo}/git/commits/{sha}")
    resolved = response.get("sha") if isinstance(response, dict) else None
    if resolved != sha:
        raise AcceptanceError(f"{label} did not resolve to the requested immutable commit SHA")
    return sha


def validate_target_pair(
    client: ApiClient,
    repo: str,
    control_sha: str,
    candidate_sha: str,
    dispatch_ref: str,
) -> dict[str, Any]:
    if ref_name(dispatch_ref) != "main":
        raise AcceptanceError("dispatch ref must be main")
    if control_sha == candidate_sha:
        raise AcceptanceError("control and candidate SHAs must be different")
    control_sha = validate_commit_sha(client, repo, control_sha, "control SHA")
    candidate_sha = validate_commit_sha(client, repo, candidate_sha, "candidate SHA")
    dispatch_sha = resolve_ref(client, repo, dispatch_ref)
    comparison = client.api(f"repos/{repo}/compare/{control_sha}...{candidate_sha}")
    if not isinstance(comparison, dict) or comparison.get("status") != "ahead":
        raise AcceptanceError("candidate SHA must be a strict descendant of control")
    ahead_by = comparison.get("ahead_by")
    behind_by = comparison.get("behind_by")
    if not isinstance(ahead_by, int) or ahead_by < 1 or (isinstance(behind_by, int) and behind_by != 0):
        raise AcceptanceError("candidate SHA must be a strict descendant of control")
    return {
        "control_sha": control_sha,
        "candidate_sha": candidate_sha,
        "dispatch_ref": ref_name(dispatch_ref),
        "dispatch_sha": dispatch_sha,
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


def validate_playwright_summary(summary: Any, expected_tested_sha: str) -> dict[str, int]:
    if not isinstance(summary, dict) or summary.get("schema_version") != 1:
        raise AcceptanceError("Playwright summary schema_version must be 1")
    if summary.get("tested_sha") != expected_tested_sha:
        raise AcceptanceError("Playwright summary tested_sha does not match the requested target SHA")
    keys = ("total_tests", "passed_tests", "failed_tests", "skipped_tests", "flaky_tests", "retry_count")
    values: dict[str, int] = {}
    for key in keys:
        value = summary.get(key)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise AcceptanceError(f"Playwright summary field {key} must be a non-negative integer")
        values[key] = value
    if values["passed_tests"] + values["failed_tests"] + values["skipped_tests"] != values["total_tests"]:
        raise AcceptanceError("Playwright summary test counts do not add up")
    if values["flaky_tests"] > values["passed_tests"] or values["flaky_tests"] > values["retry_count"]:
        raise AcceptanceError("Playwright summary flaky/retry counts are inconsistent")
    if summary.get("collection_error"):
        raise AcceptanceError(f"Playwright summary contains a collection error: {summary['collection_error']}")
    return values


def playwright_test_ids(report: Any) -> list[str]:
    if not isinstance(report, dict) or not isinstance(report.get("suites"), list):
        raise AcceptanceError("Playwright JSON report has no suites array")

    identifiers: list[str] = []

    def visit_suite(value: Any, suite_titles: tuple[str, ...], inherited_file: str | None) -> None:
        if not isinstance(value, dict):
            raise AcceptanceError("Playwright JSON report contains a malformed suite")
        title = value.get("title")
        if not isinstance(title, str) or not title:
            raise AcceptanceError("Playwright JSON report suite has no title")
        source_file = value.get("file", inherited_file)
        if not isinstance(source_file, str) or not source_file:
            raise AcceptanceError("Playwright JSON report suite has no source file")
        next_titles = (*suite_titles, title)
        specs = value.get("specs", [])
        if not isinstance(specs, list):
            raise AcceptanceError("Playwright JSON report suite has malformed specs")
        for spec in specs:
            if not isinstance(spec, dict):
                raise AcceptanceError("Playwright JSON report contains a malformed spec")
            spec_title = spec.get("title")
            tests = spec.get("tests", [])
            if not isinstance(spec_title, str) or not spec_title or not isinstance(tests, list):
                raise AcceptanceError("Playwright JSON report spec has no title or malformed tests")
            for test in tests:
                if not isinstance(test, dict):
                    raise AcceptanceError("Playwright JSON report contains a malformed test")
                project_name = test.get("projectName")
                if not isinstance(project_name, str) or not project_name:
                    raise AcceptanceError("Playwright JSON report test has no project name")
                identifiers.append(
                    json.dumps(
                        [source_file, next_titles, spec_title, project_name],
                        ensure_ascii=False,
                        separators=(",", ":"),
                    )
                )
        nested_suites = value.get("suites", [])
        if not isinstance(nested_suites, list):
            raise AcceptanceError("Playwright JSON report suite has malformed nested suites")
        for nested in nested_suites:
            visit_suite(nested, next_titles, source_file)

    for suite in report["suites"]:
        visit_suite(suite, (), None)
    if len(set(identifiers)) != len(identifiers):
        raise AcceptanceError("Playwright JSON report contains duplicate deterministic test identifiers")
    return sorted(identifiers)


def download_playwright_summary(
    client: ApiClient, repo: str, run_id: Any, expected_tested_sha: str
) -> dict[str, Any]:
    artifacts_payload = client.api(f"repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100")
    artifacts = artifacts_payload.get("artifacts", []) if isinstance(artifacts_payload, dict) else []
    matches = [item for item in artifacts if isinstance(item, dict) and item.get("name") == SUMMARY_ARTIFACT_NAME]
    if len(matches) != 1:
        raise AcceptanceError(f"run {run_id} must contain exactly one {SUMMARY_ARTIFACT_NAME} artifact")
    artifact = matches[0]
    if artifact.get("expired") is True:
        raise AcceptanceError(f"run {run_id} has an expired {SUMMARY_ARTIFACT_NAME} artifact")
    artifact_id = artifact.get("id")
    if not isinstance(artifact_id, int):
        raise AcceptanceError(f"run {run_id} artifact has no numeric id")
    with tempfile.TemporaryDirectory(prefix="ci-e2e-artifact-") as directory:
        archive = Path(directory) / "results.zip"
        client.download(f"repos/{repo}/actions/artifacts/{artifact_id}/zip", archive)
        try:
            with zipfile.ZipFile(archive) as zipped:
                file_names = [Path(name).name for name in zipped.namelist() if not name.endswith("/")]
                if sorted(file_names) != ["playwright-results.json", "playwright-summary.json"]:
                    raise AcceptanceError(
                        f"run {run_id} artifact must contain only playwright-results.json and playwright-summary.json"
                    )
                raw_entries = [name for name in zipped.namelist() if Path(name).name == "playwright-results.json"]
                entries = [name for name in zipped.namelist() if Path(name).name == "playwright-summary.json"]
                try:
                    report = json.loads(zipped.read(raw_entries[0]))
                    summary = json.loads(zipped.read(entries[0]))
                except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
                    raise AcceptanceError(f"run {run_id} has invalid Playwright JSON report or summary") from exc
        except (FileNotFoundError, OSError, zipfile.BadZipFile) as exc:
            raise AcceptanceError(f"run {run_id} artifact is not a valid ZIP archive") from exc
    values = validate_playwright_summary(summary, expected_tested_sha)
    test_ids = playwright_test_ids(report)
    if len(test_ids) != values["total_tests"]:
        raise AcceptanceError("Playwright JSON report test identifiers do not match summary total")
    return {**values, "test_ids": test_ids}


def validate_run(
    run: dict[str, Any],
    jobs_payload: dict[str, Any],
    expected_dispatch_sha: str,
    expected_target_sha: str,
    *,
    require_runtime_smoke: bool,
) -> dict[str, Any]:
    if run.get("event") != "workflow_dispatch" or run.get("head_sha") != expected_dispatch_sha:
        raise AcceptanceError(f"run {run.get('id')} has an unexpected event or head SHA")
    if run.get("run_attempt") != 1:
        raise AcceptanceError(f"run {run.get('id')} was retried (run_attempt must be 1)")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise AcceptanceError(f"run {run.get('id')} did not complete successfully")
    jobs = validate_jobs(run, jobs_payload, require_runtime_smoke=require_runtime_smoke)
    e2e_job = next(job for job in jobs if job.get("name") == "Frontend E2E")
    return {
        "pipeline": run,
        "id": run.get("id"),
        "name": run.get("name"),
        "dispatch_ref": run.get("head_branch"),
        "dispatch_sha": run.get("head_sha"),
        "tested_sha": expected_target_sha,
        "run_attempt": run.get("run_attempt"),
        "created_at": run.get("created_at"),
        "run_started_at": run.get("run_started_at"),
        "updated_at": run.get("updated_at"),
        "duration_seconds": duration_seconds(run),
        "e2e_job_duration_seconds": interval_seconds(
            e2e_job, "started_at", "completed_at", "Frontend E2E job"
        ),
        "jobs": jobs,
    }


def acceptance_run_title(nonce: str) -> str:
    return f"{ACCEPTANCE_RUN_TITLE_PREFIX} {nonce}"


@dataclass
class AcceptanceRunner:
    client: ApiClient
    repo: str
    workflow: str = "ci.yml"
    poll_interval: float = 15
    timeout_seconds: float = 2100

    def _workflow_runs(self, dispatch_sha: str) -> list[dict[str, Any]]:
        query = urlencode({"event": "workflow_dispatch", "head_sha": dispatch_sha, "per_page": "100"})
        response = self.client.api(f"repos/{self.repo}/actions/workflows/{self.workflow}/runs?{query}")
        runs = response.get("workflow_runs", []) if isinstance(response, dict) else []
        if not isinstance(runs, list) or any(not isinstance(item, dict) for item in runs):
            raise AcceptanceError("GitHub workflow-runs response is malformed")
        return runs

    def _dispatch_one(
        self,
        refs: dict[str, Any],
        target_sha: str,
        *,
        role: str,
        require_runtime_smoke: bool,
    ) -> dict[str, Any]:
        dispatch_sha = refs["dispatch_sha"]
        active = [
            run
            for run in self._workflow_runs(dispatch_sha)
            if run_is_active(run)
            and isinstance(run.get("display_title"), str)
            and run["display_title"].startswith(f"{ACCEPTANCE_RUN_TITLE_PREFIX} ")
        ]
        if active:
            raise AcceptanceError("another controlled acceptance workflow run is already active")

        nonce = f"{role}-{secrets.token_hex(12)}"
        expected_title = acceptance_run_title(nonce)
        self.client.api(
            f"repos/{self.repo}/actions/workflows/{self.workflow}/dispatches",
            method="POST",
            payload={
                "ref": refs["dispatch_ref"],
                "inputs": {
                    ACCEPTANCE_INPUT: "true",
                    ACCEPTANCE_TARGET_SHA_INPUT: target_sha,
                    ACCEPTANCE_NONCE_INPUT: nonce,
                },
            },
        )
        deadline = time.monotonic() + self.timeout_seconds
        selected: dict[str, Any] | None = None
        while time.monotonic() <= deadline:
            candidates = []
            for run in self._workflow_runs(dispatch_sha):
                if (
                    run.get("head_sha") == dispatch_sha
                    and run.get("event") == "workflow_dispatch"
                    and run.get("display_title") == expected_title
                ):
                    candidates.append(run)
            if len(candidates) > 1:
                raise AcceptanceError(f"dispatch for target SHA {target_sha} produced concurrent workflow runs")
            if candidates:
                selected = candidates[0]
                if selected.get("run_attempt") != 1:
                    raise AcceptanceError(f"run {selected.get('id')} was retried")
                if selected.get("status") == "completed":
                    break
            time.sleep(self.poll_interval)
        if selected is None:
            raise AcceptanceError(f"no workflow run appeared for target SHA {target_sha}")
        if selected.get("status") != "completed":
            raise AcceptanceError(f"workflow run {selected.get('id')} timed out")
        run = self.client.api(f"repos/{self.repo}/actions/runs/{selected.get('id')}")
        jobs = self.client.api(f"repos/{self.repo}/actions/runs/{selected.get('id')}/jobs?per_page=100")
        result = validate_run(
            run,
            jobs,
            dispatch_sha,
            target_sha,
            require_runtime_smoke=require_runtime_smoke,
        )
        result["acceptance_nonce"] = nonce
        result["playwright_summary"] = download_playwright_summary(
            self.client,
            self.repo,
            selected.get("id"),
            target_sha,
        )
        return result

    def run(self, refs: dict[str, Any], *, pairs: int = 10) -> dict[str, Any]:
        if pairs != 10:
            raise AcceptanceError("controlled acceptance requires exactly 10 pairs")
        records: list[dict[str, Any]] = []
        for pair in range(1, pairs + 1):
            order = ("control", "candidate") if pair % 2 else ("candidate", "control")
            for role in order:
                target_sha = refs[f"{role}_sha"]
                records.append(
                    {
                        "pair": pair,
                        "role": role,
                        "result": self._dispatch_one(
                            refs,
                            target_sha,
                            role=role,
                            require_runtime_smoke=role == "candidate",
                        ),
                    }
                )
        for pair in range(1, pairs + 1):
            pair_records = [item for item in records if item["pair"] == pair]
            control_summary = next(item["result"]["playwright_summary"] for item in pair_records if item["role"] == "control")
            candidate_summary = next(item["result"]["playwright_summary"] for item in pair_records if item["role"] == "candidate")
            if control_summary["total_tests"] != candidate_summary["total_tests"]:
                raise AcceptanceError(f"pair {pair} control/candidate test totals differ")
            if control_summary["test_ids"] != candidate_summary["test_ids"]:
                raise AcceptanceError(f"pair {pair} control/candidate test identifiers differ")

        control = [item["result"]["e2e_job_duration_seconds"] for item in records if item["role"] == "control"]
        candidate = [item["result"]["e2e_job_duration_seconds"] for item in records if item["role"] == "candidate"]
        control_retry_total = sum(item["result"]["playwright_summary"]["retry_count"] for item in records if item["role"] == "control")
        candidate_retry_total = sum(item["result"]["playwright_summary"]["retry_count"] for item in records if item["role"] == "candidate")
        candidate_final_failures = sum(item["result"]["playwright_summary"]["failed_tests"] for item in records if item["role"] == "candidate")
        stats = {
            "control_e2e_median_seconds": median(control),
            "control_e2e_p90_seconds": nearest_rank_p90(control),
            "candidate_e2e_median_seconds": median(candidate),
            "candidate_e2e_p90_seconds": nearest_rank_p90(candidate),
            "candidate_median_ratio": median(candidate) / median(control),
            "control_retry_total": control_retry_total,
            "candidate_retry_total": candidate_retry_total,
            "candidate_final_failures": candidate_final_failures,
            "passed": False,
            "thresholds": {
                "candidate_e2e_p90_max_seconds": 420,
                "candidate_final_failures_max": 0,
                "candidate_retry_total_max": control_retry_total,
                "candidate_median_ratio_max": 0.75,
            },
        }
        stats["passed"] = not (
            stats["candidate_e2e_p90_seconds"] > 420
            or stats["candidate_final_failures"] != 0
            or stats["candidate_retry_total"] > control_retry_total
            or stats["candidate_median_ratio"] > 0.75
        )
        return {"refs": refs, "pairs": pairs, "runs": records, "statistics": stats}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--control-sha", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--dispatch-ref", default="main")
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
        refs = validate_target_pair(
            client,
            args.repo,
            args.control_sha,
            args.candidate_sha,
            args.dispatch_ref,
        )
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
