#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python3 - <<'PY' \
  "$repo_root/.github/scripts/release_pr_comment.py" \
  "$repo_root/.github/scripts/check_quality_gates_contract.py" \
  "$repo_root/.github/workflows/release.yml" \
  "$repo_root/.github/workflows/ci.yml"
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

script_path = Path(sys.argv[1])
contract_path = Path(sys.argv[2])
release_workflow_path = Path(sys.argv[3])
ci_workflow_path = Path(sys.argv[4])


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


module = load_module(script_path, "release_pr_comment")
contract = load_module(contract_path, "quality_gates_contract")


class FakeClient:
    def __init__(self, comments_by_pr):
        self.comments_by_pr = comments_by_pr
        self.requests = []
        self.next_id = 900

    def paginate(self, path: str, query=None):
        del query
        pull_number = int(path.split("/issues/", 1)[1].split("/", 1)[0])
        return list(self.comments_by_pr.get(pull_number, []))

    def request_json(self, path: str, *, method: str = "GET", body=None, query=None):
        del query
        self.requests.append((method, path, body))
        if method == "POST" and path.endswith("/comments"):
            pull_number = int(path.split("/issues/", 1)[1].split("/", 1)[0])
            response = {
                "id": self.next_id,
                "html_url": f"https://github.com/IvanLi-CN/octo-rill/pull/{pull_number}#issuecomment-{self.next_id}",
                "body": body["body"],
                "user": {"login": "github-actions[bot]"},
            }
            self.next_id += 1
            self.comments_by_pr.setdefault(pull_number, []).append(response)
            return response

        if method == "PATCH" and "/issues/comments/" in path:
            comment_id = int(path.rsplit("/", 1)[-1])
            for comments in self.comments_by_pr.values():
                for comment in comments:
                    if comment["id"] == comment_id:
                        comment["body"] = body["body"]
                        return {
                            "id": comment_id,
                            "html_url": comment["html_url"],
                            "body": comment["body"],
                            "user": comment["user"],
                        }
        raise AssertionError(f"unexpected request_json call: {method} {path}")


release_url = "https://github.com/IvanLi-CN/octo-rill/releases/tag/v1.2.3"
body = module.render_comment_body("1.2.3", "v1.2.3", release_url)
assert module.COMMENT_MARKER in body
assert "- Version: `1.2.3`" in body
assert "- Tag: `v1.2.3`" in body
assert f"- Release: [v1.2.3]({release_url})" in body

create_context = module.CommentContext(
    owner="IvanLi-CN",
    repo="octo-rill",
    should_release=True,
    pull_number=21,
    version="1.2.3",
    tag="v1.2.3",
    server_url="https://github.com",
)
create_client = FakeClient(comments_by_pr={21: []})
create_result = module.execute_release_comment(create_context, create_client)
assert create_result["action"] == "created"
assert create_client.requests == [
    (
        "POST",
        "/repos/IvanLi-CN/octo-rill/issues/21/comments",
        {"body": module.render_comment_body("1.2.3", "v1.2.3", release_url)},
    )
]

existing_comments = {
    22: [
        {
            "id": 100,
            "body": "manual note",
            "html_url": "https://github.com/IvanLi-CN/octo-rill/pull/22#issuecomment-100",
            "user": {"login": "ivan"},
        },
        {
            "id": 101,
            "body": f"{module.COMMENT_MARKER}\nold human marker",
            "html_url": "https://github.com/IvanLi-CN/octo-rill/pull/22#issuecomment-101",
            "user": {"login": "ivan"},
        },
        {
            "id": 102,
            "body": f"{module.COMMENT_MARKER}\nold bot body",
            "html_url": "https://github.com/IvanLi-CN/octo-rill/pull/22#issuecomment-102",
            "user": {"login": "github-actions[bot]"},
        },
    ]
}
update_context = module.CommentContext(
    owner="IvanLi-CN",
    repo="octo-rill",
    should_release=True,
    pull_number=22,
    version="1.2.4",
    tag="v1.2.4-rc.abc1234",
    server_url="https://github.com",
)
update_client = FakeClient(comments_by_pr=existing_comments)
update_result = module.execute_release_comment(update_context, update_client)
assert update_result["action"] == "updated"
assert update_client.requests == [
    (
        "PATCH",
        "/repos/IvanLi-CN/octo-rill/issues/comments/102",
        {
            "body": module.render_comment_body(
                "1.2.4",
                "v1.2.4-rc.abc1234",
                "https://github.com/IvanLi-CN/octo-rill/releases/tag/v1.2.4-rc.abc1234",
            )
        },
    )
]

skip_context = module.CommentContext(
    owner="IvanLi-CN",
    repo="octo-rill",
    should_release=False,
    pull_number=23,
    version="1.2.5",
    tag="v1.2.5",
    server_url="https://github.com",
)
skip_client = FakeClient(comments_by_pr={23: []})
skip_result = module.execute_release_comment(skip_context, skip_client)
assert skip_result == {
    "action": "skipped",
    "reason": "should_release=false",
    "release_url": "https://github.com/IvanLi-CN/octo-rill/releases/tag/v1.2.5",
}
assert skip_client.requests == []

missing_pr_context = module.CommentContext(
    owner="IvanLi-CN",
    repo="octo-rill",
    should_release=True,
    pull_number=None,
    version="1.2.6",
    tag="v1.2.6",
    server_url="https://github.com",
)
missing_pr_result = module.execute_release_comment(missing_pr_context, FakeClient(comments_by_pr={}))
assert missing_pr_result == {
    "action": "skipped",
    "reason": "missing_pr_number",
    "release_url": "https://github.com/IvanLi-CN/octo-rill/releases/tag/v1.2.6",
}

release_workflow = contract.load_yaml(release_workflow_path)
comment_job = contract.job_config(release_workflow, "pr-release-comment", "release.yml")
assert set(comment_job.get("needs", [])) == {"prepare", "docker-release"}
assert "needs.prepare.outputs.should_release == 'true'" in comment_job.get("if", "")
assert "needs.prepare.outputs.pr_number != ''" in comment_job.get("if", "")
permissions = contract.require_mapping(comment_job.get("permissions"), "release.yml.jobs.pr-release-comment.permissions")
assert permissions == {"contents": "read", "issues": "write"}
checkout_step = contract.uses_step_config(
    comment_job,
    "Checkout workflow revision",
    "actions/checkout@v4",
    "release.yml.jobs.pr-release-comment",
)
assert "with" not in checkout_step
comment_step = contract.step_config(comment_job, "Upsert PR release comment", "release.yml.jobs.pr-release-comment")
assert "python3 ./.github/scripts/release_pr_comment.py" in contract.step_run(
    comment_step,
    "release.yml.jobs.pr-release-comment.steps['Upsert PR release comment']",
)

ci_workflow = contract.load_yaml(ci_workflow_path)
lint_job = contract.job_config(ci_workflow, "lint", "ci.yml")
compile_step = contract.step_config(lint_job, "Check quality-gates scripts", "ci.yml.jobs.lint")
compile_run = contract.step_run(compile_step, "ci.yml.jobs.lint.steps['Check quality-gates scripts']")
assert ".github/scripts/release_pr_comment.py" in compile_run
self_tests_step = contract.step_config(lint_job, "Quality gates self-tests", "ci.yml.jobs.lint")
self_tests_run = contract.step_run(self_tests_step, "ci.yml.jobs.lint.steps['Quality gates self-tests']")
assert "bash .github/scripts/test-release-pr-comment.sh" in self_tests_run

print("test-release-pr-comment: all checks passed")
PY
