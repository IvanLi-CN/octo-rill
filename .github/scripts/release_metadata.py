#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import re
import subprocess
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

API_VERSION = "2022-11-28"
STABLE_TAG_RE = re.compile(r"^v?(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)$")
RC_TAG_RE = re.compile(
    r"^v?(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)-rc\.(?P<suffix>[0-9A-Za-z._-]+)$"
)


@dataclass(frozen=True)
class ReleaseTag:
    tag: str
    version: str
    prerelease: bool
    major: int
    minor: int
    patch: int
    suffix: str = ""

    @property
    def channel(self) -> str:
        return "rc" if self.prerelease else "stable"

    @property
    def sort_key(self) -> tuple[int, int, int, str]:
        return (self.major, self.minor, self.patch, self.suffix)


@dataclass(frozen=True)
class ReleaseMetadata:
    should_release: bool
    reason: str
    release_mode: str
    release_head_sha: str
    app_effective_version: str
    app_release_tag: str
    app_is_prerelease: bool
    reuse_existing_tag: bool
    publish_docker: bool
    generate_release_notes: bool
    release_tag_source: str


class GitHubApiClient:
    def __init__(self, api_root: str, token: str) -> None:
        self.api_root = api_root.rstrip("/")
        self.token = token

    def request_json(self, path: str) -> dict[str, Any]:
        url = f"{self.api_root}{path}"
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
        if not isinstance(decoded, dict):
            raise RuntimeError(f"GitHub API GET {path} returned non-object JSON")
        return decoded

    def request_json_list(self, path: str) -> list[dict[str, Any]]:
        url = f"{self.api_root}{path}"
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

    def get_latest_release(self, repository: str) -> dict[str, Any]:
        return self.request_json(f"/repos/{repository}/releases/latest")

    def get_release_by_tag(self, repository: str, tag: str) -> dict[str, Any]:
        encoded_tag = urllib.parse.quote(tag, safe="")
        return self.request_json(f"/repos/{repository}/releases/tags/{encoded_tag}")

    def get_most_recent_published_release(self, repository: str) -> dict[str, Any]:
        releases = self.request_json_list(f"/repos/{repository}/releases?per_page=20&page=1")
        for release in releases:
            if release.get("draft") is True:
                continue
            return release
        raise RuntimeError(f"GitHub API GET /repos/{repository}/releases returned no published releases")


def parse_release_tag(raw: str) -> ReleaseTag | None:
    value = raw.strip()
    if not value:
        return None

    stable_match = STABLE_TAG_RE.match(value)
    if stable_match:
        major = int(stable_match.group("major"))
        minor = int(stable_match.group("minor"))
        patch = int(stable_match.group("patch"))
        version = f"{major}.{minor}.{patch}"
        return ReleaseTag(
            tag=f"v{version}",
            version=version,
            prerelease=False,
            major=major,
            minor=minor,
            patch=patch,
        )

    rc_match = RC_TAG_RE.match(value)
    if rc_match:
        major = int(rc_match.group("major"))
        minor = int(rc_match.group("minor"))
        patch = int(rc_match.group("patch"))
        suffix = rc_match.group("suffix")
        version = f"{major}.{minor}.{patch}"
        return ReleaseTag(
            tag=f"v{version}-rc.{suffix}",
            version=version,
            prerelease=True,
            major=major,
            minor=minor,
            patch=patch,
            suffix=suffix,
        )

    return None


def normalize_channel(raw: str) -> str:
    normalized = raw.strip().lower()
    if normalized in {"stable", "channel:stable"}:
        return "stable"
    if normalized in {"rc", "channel:rc"}:
        return "rc"
    raise ValueError(f"invalid release channel: {raw}")


def parse_bool(raw: str | None, *, default: bool = False) -> bool:
    if raw is None:
        return default
    return raw.strip().lower() == "true"


def run_git(repo_root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"git {' '.join(args)} failed with rc={result.returncode}: {result.stderr.strip()}"
        )
    return result.stdout.strip()


def read_cargo_version(repo_root: Path) -> str:
    cargo_toml = repo_root / "Cargo.toml"
    content = cargo_toml.read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"(\d+\.\d+\.\d+)"', content, re.MULTILINE)
    if match is None:
        raise RuntimeError("failed to detect version from Cargo.toml")
    return match.group(1)


def list_all_release_tags(repo_root: Path) -> list[ReleaseTag]:
    raw_tags = run_git(repo_root, "tag", "-l")
    parsed: list[ReleaseTag] = []
    for line in raw_tags.splitlines():
        tag = parse_release_tag(line)
        if tag is not None:
            parsed.append(tag)
    return parsed


def tags_pointing_at(repo_root: Path, commit_sha: str) -> list[ReleaseTag]:
    raw_tags = run_git(repo_root, "tag", "--points-at", commit_sha)
    parsed: list[ReleaseTag] = []
    for line in raw_tags.splitlines():
        tag = parse_release_tag(line)
        if tag is not None:
            parsed.append(tag)
    return parsed


def choose_existing_tag(tags: list[ReleaseTag], *, channel: str | None) -> ReleaseTag | None:
    if channel is not None:
        scoped = [tag for tag in tags if tag.channel == channel]
        if scoped:
            return max(scoped, key=lambda item: item.sort_key)

    stable = [tag for tag in tags if not tag.prerelease]
    if stable:
        return max(stable, key=lambda item: item.sort_key)

    prerelease = [tag for tag in tags if tag.prerelease]
    if prerelease:
        return max(prerelease, key=lambda item: item.sort_key)

    return None


def resolve_existing_tag(repo_root: Path, commit_sha: str, *, channel: str | None) -> ReleaseTag | None:
    return choose_existing_tag(tags_pointing_at(repo_root, commit_sha), channel=channel)


def compute_next_release_tag(
    repo_root: Path,
    *,
    bump_level: str,
    channel: str,
    commit_sha: str,
) -> ReleaseTag:
    all_tags = list_all_release_tags(repo_root)
    stable_tags = [tag for tag in all_tags if not tag.prerelease]
    cargo_version = read_cargo_version(repo_root)
    if stable_tags:
        base_tag = max(stable_tags, key=lambda item: item.sort_key)
        base_major, base_minor, base_patch = base_tag.major, base_tag.minor, base_tag.patch
    else:
        base_major, base_minor, base_patch = (int(part) for part in cargo_version.split("."))

    if bump_level == "major":
        next_major, next_minor, next_patch = base_major + 1, 0, 0
    elif bump_level == "minor":
        next_major, next_minor, next_patch = base_major, base_minor + 1, 0
    elif bump_level == "patch":
        next_major, next_minor, next_patch = base_major, base_minor, base_patch + 1
    else:
        raise RuntimeError(f"invalid bump level: {bump_level}")

    existing_canonical_tags = {tag.tag for tag in all_tags}
    candidate_patch = next_patch
    while f"v{next_major}.{next_minor}.{candidate_patch}" in existing_canonical_tags:
        candidate_patch += 1

    version = f"{next_major}.{next_minor}.{candidate_patch}"
    if channel == "stable":
        return ReleaseTag(
            tag=f"v{version}",
            version=version,
            prerelease=False,
            major=next_major,
            minor=next_minor,
            patch=candidate_patch,
        )

    short_sha = commit_sha[:7]
    return ReleaseTag(
        tag=f"v{version}-rc.{short_sha}",
        version=version,
        prerelease=True,
        major=next_major,
        minor=next_minor,
        patch=candidate_patch,
        suffix=short_sha,
    )


def resolve_tag_commit(repo_root: Path, tag: str) -> str:
    return run_git(repo_root, "rev-list", "-n", "1", tag)


def resolve_release_metadata(
    *,
    repo_root: Path,
    repository: str,
    event_name: str,
    workflow_run_sha: str,
    input_head_sha: str,
    input_release_tag: str,
    intent_should_release: bool,
    intent_bump_level: str,
    intent_channel: str,
    intent_reason: str,
    client: GitHubApiClient | Any | None,
) -> ReleaseMetadata:
    if event_name == "workflow_dispatch":
        if client is None:
            raise RuntimeError("workflow_dispatch release resolution requires a GitHub API client")

        if input_release_tag:
            release_payload = client.get_release_by_tag(repository, input_release_tag)
            release_source = "explicit_release_tag"
        elif input_head_sha:
            existing = resolve_existing_tag(repo_root, input_head_sha, channel=None)
            if existing is None:
                raise RuntimeError(
                    f"workflow_dispatch head_sha={input_head_sha} does not point at an existing release tag"
                )
            release_payload = client.get_release_by_tag(repository, existing.tag)
            release_source = "explicit_head_sha"
        else:
            release_payload = client.get_most_recent_published_release(repository)
            release_source = "latest_published_release"

        raw_tag = str(release_payload.get("tag_name", "")).strip()
        parsed_tag = parse_release_tag(raw_tag)
        if parsed_tag is None:
            raise RuntimeError(f"release tag {raw_tag!r} is not a supported OctoRill release tag")

        release_head_sha = resolve_tag_commit(repo_root, parsed_tag.tag)
        return ReleaseMetadata(
            should_release=True,
            reason=f"backfill:{release_source}",
            release_mode="backfill",
            release_head_sha=release_head_sha,
            app_effective_version=parsed_tag.version,
            app_release_tag=parsed_tag.tag,
            app_is_prerelease=parsed_tag.prerelease,
            reuse_existing_tag=True,
            publish_docker=False,
            generate_release_notes=False,
            release_tag_source=release_source,
        )

    if not intent_should_release:
        return ReleaseMetadata(
            should_release=False,
            reason=intent_reason or "should_release=false",
            release_mode="skip",
            release_head_sha=workflow_run_sha,
            app_effective_version="",
            app_release_tag="",
            app_is_prerelease=False,
            reuse_existing_tag=False,
            publish_docker=False,
            generate_release_notes=False,
            release_tag_source="none",
        )

    if not workflow_run_sha:
        raise RuntimeError("workflow_run release resolution requires WORKFLOW_RUN_SHA")

    channel = normalize_channel(intent_channel)
    existing = resolve_existing_tag(repo_root, workflow_run_sha, channel=channel)
    if existing is None:
        if not intent_bump_level:
            raise RuntimeError("release intent must provide bump_level when no existing tag can be reused")
        resolved = compute_next_release_tag(
            repo_root,
            bump_level=intent_bump_level,
            channel=channel,
            commit_sha=workflow_run_sha,
        )
        return ReleaseMetadata(
            should_release=True,
            reason=intent_reason or "intent_release",
            release_mode="publish",
            release_head_sha=workflow_run_sha,
            app_effective_version=resolved.version,
            app_release_tag=resolved.tag,
            app_is_prerelease=resolved.prerelease,
            reuse_existing_tag=False,
            publish_docker=True,
            generate_release_notes=True,
            release_tag_source="computed",
        )

    return ReleaseMetadata(
        should_release=True,
        reason="reuse_existing_tag",
        release_mode="publish",
        release_head_sha=workflow_run_sha,
        app_effective_version=existing.version,
        app_release_tag=existing.tag,
        app_is_prerelease=existing.prerelease,
        reuse_existing_tag=True,
        publish_docker=True,
        generate_release_notes=True,
        release_tag_source="existing_commit_tag",
    )


def write_output(key: str, value: str) -> None:
    output_path = os.environ.get("GITHUB_OUTPUT", "").strip()
    if not output_path:
        return
    with open(output_path, "a", encoding="utf-8") as handle:
        handle.write(f"{key}={value}\n")


def main() -> int:
    repository = os.environ.get("GITHUB_REPOSITORY", "").strip()
    if not repository or "/" not in repository:
        raise SystemExit("release_metadata: missing or invalid GITHUB_REPOSITORY")

    repo_root = Path(os.environ.get("REPO_ROOT", ".")).resolve()
    event_name = os.environ.get("GITHUB_EVENT_NAME", "").strip()
    if event_name not in {"workflow_run", "workflow_dispatch"}:
        raise SystemExit(f"release_metadata: unsupported event {event_name!r}")

    token = os.environ.get("GITHUB_TOKEN", "").strip()
    api_root = os.environ.get("GITHUB_API_URL", "https://api.github.com").strip() or "https://api.github.com"
    client = None
    if event_name == "workflow_dispatch":
        if not token:
            raise SystemExit("release_metadata: missing GITHUB_TOKEN for workflow_dispatch backfill resolution")
        client = GitHubApiClient(api_root=api_root, token=token)

    metadata = resolve_release_metadata(
        repo_root=repo_root,
        repository=repository,
        event_name=event_name,
        workflow_run_sha=os.environ.get("WORKFLOW_RUN_SHA", "").strip(),
        input_head_sha=os.environ.get("INPUT_HEAD_SHA", "").strip(),
        input_release_tag=os.environ.get("INPUT_RELEASE_TAG", "").strip(),
        intent_should_release=parse_bool(os.environ.get("INTENT_SHOULD_RELEASE"), default=False),
        intent_bump_level=os.environ.get("INTENT_BUMP_LEVEL", "").strip(),
        intent_channel=os.environ.get("INTENT_CHANNEL", "").strip(),
        intent_reason=os.environ.get("INTENT_REASON", "").strip(),
        client=client,
    )

    outputs = {
        "should_release": "true" if metadata.should_release else "false",
        "reason": metadata.reason,
        "release_mode": metadata.release_mode,
        "release_head_sha": metadata.release_head_sha,
        "app_effective_version": metadata.app_effective_version,
        "app_release_tag": metadata.app_release_tag,
        "app_is_prerelease": "true" if metadata.app_is_prerelease else "false",
        "reuse_existing_tag": "true" if metadata.reuse_existing_tag else "false",
        "publish_docker": "true" if metadata.publish_docker else "false",
        "generate_release_notes": "true" if metadata.generate_release_notes else "false",
        "release_tag_source": metadata.release_tag_source,
    }

    for key, value in outputs.items():
        write_output(key, value)
        print(f"{key}={value}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
