#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

API_VERSION = "2022-11-28"
COMMENT_MARKER = "<!-- octo-rill:release-version -->"
DEFAULT_COMMENT_AUTHOR = "github-actions[bot]"
STABLE_TAG_RE = re.compile(r"^v?(?P<version>\d+\.\d+\.\d+)$")
RC_TAG_RE = re.compile(r"^v?(?P<version>\d+\.\d+\.\d+)-rc\.[0-9a-f]{7}$")


@dataclass(frozen=True)
class ReleaseIntent:
    should_release: bool
    bump_level: str | None
    channel: str | None
    prerelease: bool
    release_intent_label: str | None
    reason: str


@dataclass(frozen=True)
class ReleaseCandidate:
    sha: str
    pr_number: int
    pr_url: str
    intent: ReleaseIntent
    matching_tag: str | None
    release_exists: bool
    comment_exists: bool

    @property
    def pending_kind(self) -> str | None:
        if not self.intent.should_release:
            return None
        if not self.matching_tag:
            return "unpublished"
        if not self.release_exists or not self.comment_exists:
            return "repair"
        return None

    @property
    def pending_reason(self) -> str:
        if not self.matching_tag:
            return "missing_tag"
        if not self.release_exists:
            return "missing_github_release"
        if not self.comment_exists:
            return "missing_pr_comment"
        return ""


@dataclass(frozen=True)
class SelectionResult:
    selected_sha: str | None
    selected_candidate: ReleaseCandidate | None
    selection_reason: str
    next_candidate: ReleaseCandidate | None
    unpublished_count: int
    repair_count: int


class GitHubApiClient:
    def __init__(self, api_root: str, token: str) -> None:
        self.api_root = api_root.rstrip("/")
        self.token = token
        self._release_cache: dict[str, bool] = {}
        self._comment_cache: dict[int, bool] = {}

    def request_json(
        self,
        path: str,
        *,
        method: str = "GET",
        body: dict[str, Any] | None = None,
        query: dict[str, Any] | None = None,
        allow_404: bool = False,
    ) -> dict[str, Any] | None:
        url = self._build_url(path, query)
        headers = {
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": API_VERSION,
        }
        data = None
        if body is not None:
            headers["Content-Type"] = "application/json"
            data = json.dumps(body).encode("utf-8")

        request = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(request) as response:
                payload = response.read().decode("utf-8")
        except urllib.error.HTTPError as exc:
            if allow_404 and exc.code == 404:
                return None
            detail = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"GitHub API {method} {path} failed: {exc.code} {detail}") from exc

        if not payload:
            return {}
        decoded = json.loads(payload)
        if not isinstance(decoded, dict):
            raise RuntimeError(f"GitHub API {method} {path} returned non-object JSON")
        return decoded

    def request_json_list(self, path: str, *, query: dict[str, Any] | None = None) -> list[dict[str, Any]]:
        url = self._build_url(path, query)
        headers = {
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": API_VERSION,
        }
        request = urllib.request.Request(url, headers=headers, method="GET")
        try:
            with urllib.request.urlopen(request) as response:
                payload = response.read().decode("utf-8")
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"GitHub API GET {path} failed: {exc.code} {detail}") from exc

        decoded = json.loads(payload)
        if not isinstance(decoded, list):
            raise RuntimeError(f"GitHub API GET {path} returned non-array JSON")
        return [item for item in decoded if isinstance(item, dict)]

    def paginate(self, path: str, *, query: dict[str, Any] | None = None) -> list[dict[str, Any]]:
        page = 1
        items: list[dict[str, Any]] = []
        while True:
            page_query = {"per_page": 100, "page": page}
            if query:
                page_query.update(query)
            payload = self.request_json_list(path, query=page_query)
            if not payload:
                return items
            items.extend(payload)
            if len(payload) < 100:
                return items
            page += 1

    def pull_for_commit(self, repo: str, sha: str) -> tuple[int, str] | None:
        pulls = self.request_json_list(f"/repos/{repo}/commits/{sha}/pulls", query={"per_page": 100})
        if len(pulls) != 1:
            return None
        pr = pulls[0]
        number = pr.get("number")
        url = pr.get("html_url") or ""
        if not isinstance(number, int):
            return None
        return number, str(url)

    def labels_for_pull(self, repo: str, pr_number: int) -> list[str]:
        labels = self.request_json_list(f"/repos/{repo}/issues/{pr_number}/labels", query={"per_page": 100})
        names: list[str] = []
        for label in labels:
            name = label.get("name")
            if isinstance(name, str) and name:
                names.append(name)
        return names

    def release_exists(self, repo: str, tag: str) -> bool:
        cached = self._release_cache.get(tag)
        if cached is not None:
            return cached
        payload = self.request_json(
            f"/repos/{repo}/releases/tags/{urllib.parse.quote(tag, safe='')}",
            allow_404=True,
        )
        exists = payload is not None
        self._release_cache[tag] = exists
        return exists

    def has_managed_comment(
        self,
        repo: str,
        pr_number: int,
        *,
        author_login: str = DEFAULT_COMMENT_AUTHOR,
    ) -> bool:
        cached = self._comment_cache.get(pr_number)
        if cached is not None:
            return cached
        comments = self.paginate(f"/repos/{repo}/issues/{pr_number}/comments")
        exists = any(is_managed_comment(comment, author_login=author_login) for comment in comments)
        self._comment_cache[pr_number] = exists
        return exists

    def dispatch_workflow(self, repo: str, workflow_id: str, ref: str, head_sha: str) -> None:
        self.request_json(
            f"/repos/{repo}/actions/workflows/{workflow_id}/dispatches",
            method="POST",
            body={"ref": ref, "inputs": {"head_sha": head_sha}},
        )

    def latest_workflow_run(
        self,
        repo: str,
        workflow_id: str,
        *,
        head_sha: str,
        event: str,
    ) -> dict[str, Any] | None:
        payload = self.request_json(
            f"/repos/{repo}/actions/workflows/{workflow_id}/runs",
            query={"head_sha": head_sha, "event": event, "per_page": 20},
        )
        if not isinstance(payload, dict):
            return None
        runs = payload.get("workflow_runs")
        if not isinstance(runs, list):
            return None
        for run in runs:
            if isinstance(run, dict) and str(run.get("head_sha", "")) == head_sha:
                return run
        return None

    def _build_url(self, path: str, query: dict[str, Any] | None) -> str:
        params = urllib.parse.urlencode(query or {}, doseq=True)
        if not params:
            return f"{self.api_root}{path}"
        return f"{self.api_root}{path}?{params}"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Plan and dispatch release backfills for octo-rill.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    plan_parser = subparsers.add_parser("plan", help="Inspect main history and choose a release target.")
    plan_parser.add_argument("--repo-root", default=".", help="Repository root containing the git checkout.")
    plan_parser.add_argument("--head-ref", default="HEAD", help="Git ref to inspect (defaults to HEAD).")
    plan_parser.add_argument(
        "--default-sha",
        default=os.environ.get("REQUESTED_HEAD_SHA", "").strip(),
        help="Fallback SHA when no pending release candidate exists.",
    )
    plan_parser.add_argument(
        "--requested-sha",
        default=os.environ.get("RELEASE_REQUESTED_SHA", "").strip(),
        help="Explicit SHA to release (workflow_dispatch backfill).",
    )
    plan_parser.add_argument(
        "--exclude-sha",
        action="append",
        default=[],
        help="Commit SHA to exclude from selection. Repeatable.",
    )

    dispatch_parser = subparsers.add_parser("dispatch", help="Dispatch the release workflow for a target SHA.")
    dispatch_parser.add_argument("--head-sha", required=True, help="Commit SHA to dispatch.")
    dispatch_parser.add_argument("--ref", default="main", help="Branch ref used for workflow_dispatch.")
    dispatch_parser.add_argument(
        "--workflow-id",
        default="release.yml",
        help="Workflow file name or numeric workflow id used for workflow_dispatch.",
    )

    await_ci_parser = subparsers.add_parser("await-ci", help="Wait for a workflow run to reach a terminal state.")
    await_ci_parser.add_argument("--head-sha", required=True, help="Commit SHA whose CI workflow run should be awaited.")
    await_ci_parser.add_argument("--event", default="push", help="Workflow event name to filter runs by.")
    await_ci_parser.add_argument("--workflow-id", default="ci.yml", help="Workflow file name or numeric workflow id.")
    await_ci_parser.add_argument("--poll-interval", type=int, default=15, help="Polling interval in seconds.")
    await_ci_parser.add_argument("--timeout-seconds", type=int, default=1800, help="Maximum wait time in seconds.")

    return parser.parse_args()


def write_output(key: str, value: str) -> None:
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as handle:
            handle.write(f"{key}={value}\n")


def is_managed_comment(comment: dict[str, Any], *, author_login: str) -> bool:
    body = comment.get("body")
    user = comment.get("user")
    if not isinstance(body, str) or COMMENT_MARKER not in body:
        return False
    if not isinstance(user, dict):
        return False
    return user.get("login") == author_login


def parse_release_intent(labels: Iterable[str]) -> ReleaseIntent:
    names = [label for label in labels if label]
    allowed_type = {
        "type:docs",
        "type:skip",
        "type:patch",
        "type:minor",
        "type:major",
    }
    allowed_channel = {"channel:stable", "channel:rc"}

    type_labels = [label for label in names if label.startswith("type:")]
    channel_labels = [label for label in names if label.startswith("channel:")]

    unknown_type = sorted({label for label in type_labels if label not in allowed_type})
    unknown_channel = sorted({label for label in channel_labels if label not in allowed_channel})
    selected_type = sorted({label for label in type_labels if label in allowed_type})
    selected_channel = sorted({label for label in channel_labels if label in allowed_channel})

    if unknown_type:
        raise ValueError(f"unknown_type_labels({','.join(unknown_type)})")
    if unknown_channel:
        raise ValueError(f"unknown_channel_labels({','.join(unknown_channel)})")
    if len(selected_type) != 1:
        raise ValueError(f"invalid_type_label_count({len(selected_type)})")
    if len(selected_channel) != 1:
        raise ValueError(f"invalid_channel_label_count({len(selected_channel)})")

    type_label = selected_type[0]
    channel_label = selected_channel[0]
    channel = "rc" if channel_label == "channel:rc" else "stable"

    if type_label in {"type:docs", "type:skip"}:
        return ReleaseIntent(
            should_release=False,
            bump_level=None,
            channel=channel,
            prerelease=False,
            release_intent_label=type_label,
            reason="intent_skip",
        )

    return ReleaseIntent(
        should_release=True,
        bump_level=type_label.removeprefix("type:"),
        channel=channel,
        prerelease=channel == "rc",
        release_intent_label=type_label,
        reason="intent_release",
    )


def git_lines(repo_root: Path, *args: str) -> list[str]:
    result = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def git_output(repo_root: Path, *args: str) -> str:
    lines = git_lines(repo_root, *args)
    return lines[0] if lines else ""


def ensure_ref_contains_commit(repo_root: Path, commit_sha: str, head_ref: str) -> None:
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", commit_sha, head_ref],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"{commit_sha} is not reachable from {head_ref}")


def list_first_parent_merges(repo_root: Path, head_ref: str) -> list[str]:
    return git_lines(repo_root, "rev-list", "--reverse", "--merges", "--first-parent", head_ref)


def list_tags_pointing_at(repo_root: Path, sha: str) -> list[str]:
    return git_lines(repo_root, "tag", "--points-at", sha)


def select_matching_tag(tags: Iterable[str], channel: str | None) -> str | None:
    if channel == "rc":
        matching = sorted((tag for tag in tags if RC_TAG_RE.match(tag)), key=normalize_semver_sort_key)
    else:
        matching = sorted((tag for tag in tags if STABLE_TAG_RE.match(tag)), key=normalize_semver_sort_key)
    if not matching:
        return None
    return matching[-1]


def normalize_semver_sort_key(tag: str) -> tuple[int, int, int, str]:
    normalized = tag.removeprefix("v")
    base, _, suffix = normalized.partition("-")
    major, minor, patch = (int(part) for part in base.split("."))
    return (major, minor, patch, suffix)


def build_release_candidates(
    repo_root: Path,
    head_ref: str,
    *,
    repo: str,
    client: GitHubApiClient,
) -> list[ReleaseCandidate]:
    candidates: list[ReleaseCandidate] = []
    for sha in list_first_parent_merges(repo_root, head_ref):
        pull = client.pull_for_commit(repo, sha)
        if pull is None:
            continue
        pr_number, pr_url = pull
        try:
            intent = parse_release_intent(client.labels_for_pull(repo, pr_number))
        except ValueError as exc:
            print(
                f"release_backfill: skip sha={sha} pr=#{pr_number} due to invalid labels: {exc}",
                file=sys.stderr,
            )
            continue
        if not intent.should_release:
            continue

        tags = list_tags_pointing_at(repo_root, sha)
        matching_tag = select_matching_tag(tags, intent.channel)
        release_exists = False
        comment_exists = False
        if matching_tag:
            release_exists = client.release_exists(repo, matching_tag)
            comment_exists = client.has_managed_comment(repo, pr_number)

        candidates.append(
            ReleaseCandidate(
                sha=sha,
                pr_number=pr_number,
                pr_url=pr_url,
                intent=intent,
                matching_tag=matching_tag,
                release_exists=release_exists,
                comment_exists=comment_exists,
            )
        )
    return candidates


def pick_next_pending_candidate(
    candidates: Iterable[ReleaseCandidate],
    *,
    exclude_shas: set[str] | None = None,
) -> ReleaseCandidate | None:
    excluded = exclude_shas or set()
    unpublished = [
        candidate
        for candidate in candidates
        if candidate.sha not in excluded and candidate.pending_kind == "unpublished"
    ]
    if unpublished:
        return unpublished[0]
    repair = [
        candidate
        for candidate in candidates
        if candidate.sha not in excluded and candidate.pending_kind == "repair"
    ]
    if repair:
        return repair[0]
    return None


def select_release_candidate(
    candidates: list[ReleaseCandidate],
    *,
    default_sha: str,
    requested_sha: str | None,
    exclude_shas: set[str],
) -> SelectionResult:
    unpublished_count = sum(1 for candidate in candidates if candidate.pending_kind == "unpublished")
    repair_count = sum(1 for candidate in candidates if candidate.pending_kind == "repair")

    if requested_sha:
        selected_candidate = next((candidate for candidate in candidates if candidate.sha == requested_sha), None)
        next_candidate = pick_next_pending_candidate(candidates, exclude_shas=exclude_shas | {requested_sha})
        return SelectionResult(
            selected_sha=requested_sha,
            selected_candidate=selected_candidate,
            selection_reason="explicit_head_sha",
            next_candidate=next_candidate,
            unpublished_count=unpublished_count,
            repair_count=repair_count,
        )

    pending_candidate = pick_next_pending_candidate(candidates, exclude_shas=exclude_shas)
    if pending_candidate is not None:
        next_candidate = pick_next_pending_candidate(candidates, exclude_shas=exclude_shas | {pending_candidate.sha})
        return SelectionResult(
            selected_sha=pending_candidate.sha,
            selected_candidate=pending_candidate,
            selection_reason=f"oldest_{pending_candidate.pending_kind}",
            next_candidate=next_candidate,
            unpublished_count=unpublished_count,
            repair_count=repair_count,
        )

    selected_candidate = next((candidate for candidate in candidates if candidate.sha == default_sha), None)
    return SelectionResult(
        selected_sha=default_sha or None,
        selected_candidate=selected_candidate,
        selection_reason="requested_head_sha",
        next_candidate=None,
        unpublished_count=unpublished_count,
        repair_count=repair_count,
    )


def require_repo() -> str:
    repo = os.environ.get("GITHUB_REPOSITORY", "").strip()
    if not repo or "/" not in repo:
        raise SystemExit("release_backfill: missing or invalid GITHUB_REPOSITORY")
    return repo


def require_token() -> str:
    token = os.environ.get("GITHUB_TOKEN", "").strip()
    if not token:
        raise SystemExit("release_backfill: missing GITHUB_TOKEN")
    return token


def run_plan(args: argparse.Namespace) -> int:
    repo_root = Path(args.repo_root).resolve()
    repo = require_repo()
    token = require_token()
    requested_sha = args.requested_sha.strip() or None
    default_sha = args.default_sha.strip()
    head_ref = args.head_ref.strip() or "HEAD"

    if requested_sha:
        ensure_ref_contains_commit(repo_root, requested_sha, head_ref)
    if default_sha:
        ensure_ref_contains_commit(repo_root, default_sha, head_ref)

    client = GitHubApiClient(
        api_root=os.environ.get("GITHUB_API_URL", "https://api.github.com").strip() or "https://api.github.com",
        token=token,
    )
    candidates = build_release_candidates(repo_root, head_ref, repo=repo, client=client)
    result = select_release_candidate(
        candidates,
        default_sha=default_sha,
        requested_sha=requested_sha,
        exclude_shas={sha for sha in args.exclude_sha if sha},
    )

    selected_pr_number = str(result.selected_candidate.pr_number) if result.selected_candidate else ""
    selected_pr_url = result.selected_candidate.pr_url if result.selected_candidate else ""
    selected_pending_reason = result.selected_candidate.pending_reason if result.selected_candidate else ""
    next_sha = result.next_candidate.sha if result.next_candidate else ""
    next_pr_number = str(result.next_candidate.pr_number) if result.next_candidate else ""
    next_pending_reason = result.next_candidate.pending_reason if result.next_candidate else ""

    outputs = {
        "release_head_sha": result.selected_sha or "",
        "release_pr_number": selected_pr_number,
        "release_pr_url": selected_pr_url,
        "selection_reason": result.selection_reason,
        "selection_pending_reason": selected_pending_reason,
        "next_pending_head_sha": next_sha,
        "next_pending_pr_number": next_pr_number,
        "next_pending_reason": next_pending_reason,
        "pending_unpublished_count": str(result.unpublished_count),
        "pending_repair_count": str(result.repair_count),
    }
    for key, value in outputs.items():
        write_output(key, value)

    print("release_backfill: selected target")
    print(f"  release_head_sha={outputs['release_head_sha'] or '<none>'}")
    print(f"  selection_reason={outputs['selection_reason']}")
    print(f"  selection_pending_reason={outputs['selection_pending_reason'] or '<none>'}")
    print(f"  release_pr_number={selected_pr_number or '<none>'}")
    print(f"  next_pending_head_sha={next_sha or '<none>'}")
    print(f"  pending_unpublished_count={result.unpublished_count}")
    print(f"  pending_repair_count={result.repair_count}")
    return 0


def run_dispatch(args: argparse.Namespace) -> int:
    repo = require_repo()
    token = require_token()
    client = GitHubApiClient(
        api_root=os.environ.get("GITHUB_API_URL", "https://api.github.com").strip() or "https://api.github.com",
        token=token,
    )
    head_sha = args.head_sha.strip()
    if not head_sha:
        raise SystemExit("release_backfill: dispatch head_sha is required")
    client.dispatch_workflow(repo, args.workflow_id, args.ref, head_sha)
    write_output("dispatched_head_sha", head_sha)
    write_output("dispatched_workflow_id", args.workflow_id)
    print(f"release_backfill: dispatched workflow {args.workflow_id} for {head_sha}")
    return 0


def run_await_ci(args: argparse.Namespace) -> int:
    repo = require_repo()
    token = require_token()
    client = GitHubApiClient(
        api_root=os.environ.get("GITHUB_API_URL", "https://api.github.com").strip() or "https://api.github.com",
        token=token,
    )
    head_sha = args.head_sha.strip()
    if not head_sha:
        raise SystemExit("release_backfill: await-ci head_sha is required")

    deadline = time.monotonic() + args.timeout_seconds
    while True:
        run = client.latest_workflow_run(repo, args.workflow_id, head_sha=head_sha, event=args.event)
        if run is not None:
            status = str(run.get("status", "") or "")
            conclusion = str(run.get("conclusion", "") or "")
            run_id = str(run.get("id", "") or "")
            write_output("ci_run_id", run_id)
            write_output("ci_status", status)
            write_output("ci_conclusion", conclusion)
            if status == "completed":
                if conclusion == "success":
                    write_output("allow_release", "true")
                    write_output("backfill_head_sha", "")
                    write_output("blocked_ci_head_sha", "")
                    write_output("ci_reason", "ci_success")
                    print(f"release_backfill: ci run {run_id} completed successfully for {head_sha}")
                    return 0
                if conclusion == "cancelled":
                    write_output("allow_release", "true")
                    write_output("backfill_head_sha", "")
                    write_output("blocked_ci_head_sha", "")
                    write_output("ci_reason", "ci_cancelled")
                    print(f"release_backfill: ci run {run_id} was cancelled; continue release for {head_sha}")
                    return 0
                write_output("allow_release", "false")
                write_output("backfill_head_sha", "")
                write_output("blocked_ci_head_sha", head_sha)
                write_output("ci_reason", f"ci_{conclusion or 'unknown'}")
                print(
                    f"release_backfill: ci workflow {args.workflow_id} for {head_sha} "
                    f"completed with blocking conclusion={conclusion or '<unknown>'}"
                )
                return 0

            print(
                f"release_backfill: waiting for workflow {args.workflow_id} run {run_id or '<pending>'} "
                f"status={status or '<unknown>'} head_sha={head_sha}"
            )
        else:
            print(f"release_backfill: waiting for workflow {args.workflow_id} run for {head_sha} to appear")

        if time.monotonic() >= deadline:
            raise SystemExit(f"release_backfill: timed out waiting for {args.workflow_id} on {head_sha}")
        time.sleep(max(args.poll_interval, 1))


def main() -> int:
    args = parse_args()
    if args.command == "plan":
        return run_plan(args)
    if args.command == "dispatch":
        return run_dispatch(args)
    if args.command == "await-ci":
        return run_await_ci(args)
    raise SystemExit(f"Unsupported command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
