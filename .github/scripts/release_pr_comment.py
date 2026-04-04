#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any

API_VERSION = "2022-11-28"
COMMENT_MARKER = "<!-- octo-rill:release-version -->"
DEFAULT_AUTHOR_LOGIN = "github-actions[bot]"


@dataclass(frozen=True)
class CommentContext:
    owner: str
    repo: str
    should_release: bool
    pull_number: int | None
    version: str
    tag: str
    server_url: str
    author_login: str = DEFAULT_AUTHOR_LOGIN

    @property
    def release_url(self) -> str:
        return build_release_url(self.server_url, self.owner, self.repo, self.tag)


class GitHubApiClient:
    def __init__(self, api_root: str, token: str) -> None:
        self.api_root = api_root.rstrip("/")
        self.token = token

    def request_json(
        self,
        path: str,
        *,
        method: str = "GET",
        body: dict[str, Any] | None = None,
        query: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
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
            detail = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"GitHub API {method} {path} failed: {exc.code} {detail}") from exc

        if not payload:
            return {}
        decoded = json.loads(payload)
        if not isinstance(decoded, dict):
            raise RuntimeError(f"GitHub API {method} {path} returned non-object JSON")
        return decoded

    def paginate(self, path: str, *, query: dict[str, Any] | None = None) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        page = 1
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

    def _build_url(self, path: str, query: dict[str, Any] | None) -> str:
        params = urllib.parse.urlencode(query or {}, doseq=True)
        if not params:
            return f"{self.api_root}{path}"
        return f"{self.api_root}{path}?{params}"


def build_release_url(server_url: str, owner: str, repo: str, tag: str) -> str:
    trimmed = server_url.rstrip("/")
    return f"{trimmed}/{owner}/{repo}/releases/tag/{urllib.parse.quote(tag)}"


def render_comment_body(version: str, tag: str, release_url: str) -> str:
    return "\n".join(
        [
            COMMENT_MARKER,
            "Release automation published a version for this PR.",
            "",
            f"- Version: `{version}`",
            f"- Tag: `{tag}`",
            f"- Release: [{tag}]({release_url})",
        ]
    )


def find_managed_comment(
    comments: list[dict[str, Any]],
    *,
    author_login: str,
    marker: str = COMMENT_MARKER,
) -> dict[str, Any] | None:
    managed: list[dict[str, Any]] = []
    for comment in comments:
        body = comment.get("body")
        user = comment.get("user")
        if not isinstance(body, str) or marker not in body:
            continue
        if not isinstance(user, dict) or user.get("login") != author_login:
            continue
        managed.append(comment)

    if not managed:
        return None
    return max(managed, key=lambda item: int(item.get("id", 0)))


def execute_release_comment(context: CommentContext, client: Any) -> dict[str, str]:
    if not context.should_release:
        return {
            "action": "skipped",
            "reason": "should_release=false",
            "release_url": context.release_url,
        }
    if context.pull_number is None:
        return {
            "action": "skipped",
            "reason": "missing_pr_number",
            "release_url": context.release_url,
        }

    body = render_comment_body(context.version, context.tag, context.release_url)
    comments = client.paginate(f"/repos/{context.owner}/{context.repo}/issues/{context.pull_number}/comments")
    existing = find_managed_comment(comments, author_login=context.author_login)

    if existing is None:
        response = client.request_json(
            f"/repos/{context.owner}/{context.repo}/issues/{context.pull_number}/comments",
            method="POST",
            body={"body": body},
        )
        return {
            "action": "created",
            "comment_url": str(response.get("html_url", "")),
            "release_url": context.release_url,
        }

    response = client.request_json(
        f"/repos/{context.owner}/{context.repo}/issues/comments/{existing['id']}",
        method="PATCH",
        body={"body": body},
    )
    return {
        "action": "updated",
        "comment_url": str(response.get("html_url", "")),
        "release_url": context.release_url,
    }


def write_output(key: str, value: str) -> None:
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as handle:
            handle.write(f"{key}={value}\n")


def parse_context_from_env() -> CommentContext:
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    if not repository or "/" not in repository:
        raise SystemExit("release_pr_comment: missing or invalid GITHUB_REPOSITORY")
    owner, repo = repository.split("/", 1)

    should_release = os.environ.get("SHOULD_RELEASE", "true").strip().lower() == "true"
    pr_number_raw = os.environ.get("PR_NUMBER", "").strip()
    pull_number = int(pr_number_raw) if pr_number_raw else None

    version = os.environ.get("APP_EFFECTIVE_VERSION", "").strip()
    tag = os.environ.get("APP_RELEASE_TAG", "").strip()
    if should_release and pull_number is not None:
        if not version:
            raise SystemExit("release_pr_comment: APP_EFFECTIVE_VERSION is required when commenting")
        if not tag:
            raise SystemExit("release_pr_comment: APP_RELEASE_TAG is required when commenting")

    return CommentContext(
        owner=owner,
        repo=repo,
        should_release=should_release,
        pull_number=pull_number,
        version=version,
        tag=tag,
        server_url=os.environ.get("GITHUB_SERVER_URL", "https://github.com").strip() or "https://github.com",
        author_login=os.environ.get("COMMENT_AUTHOR_LOGIN", DEFAULT_AUTHOR_LOGIN).strip() or DEFAULT_AUTHOR_LOGIN,
    )


def main() -> int:
    context = parse_context_from_env()
    token = os.environ.get("GITHUB_TOKEN", "").strip()
    if context.should_release and context.pull_number is not None and not token:
        raise SystemExit("release_pr_comment: missing GITHUB_TOKEN")

    result: dict[str, str]
    if context.should_release and context.pull_number is not None:
        api_root = os.environ.get("GITHUB_API_URL", "https://api.github.com").strip() or "https://api.github.com"
        client = GitHubApiClient(api_root=api_root, token=token)
        result = execute_release_comment(context, client)
    else:
        result = execute_release_comment(context, client=None)

    comment_url = result.get("comment_url", "")
    release_url = result.get("release_url", context.release_url)
    reason = result.get("reason", "")

    write_output("comment_action", result["action"])
    write_output("comment_url", comment_url)
    write_output("release_url", release_url)
    write_output("comment_reason", reason)

    print(f"release_pr_comment: action={result['action']}")
    if reason:
        print(f"release_pr_comment: reason={reason}")
    if comment_url:
        print(f"release_pr_comment: comment_url={comment_url}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
