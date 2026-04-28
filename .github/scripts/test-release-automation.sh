#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

tmp_repo="$tmp_root/compute-version"
mkdir -p "$tmp_repo"
pushd "$tmp_repo" >/dev/null
git init -q
git config user.name "Test User"
git config user.email "test@example.com"
cat > Cargo.toml <<'EOF'
[package]
name = "octo-rill"
version = "2.10.0"
edition = "2021"
EOF
git add Cargo.toml
git commit -q -m "init"
git tag v2.10.0

echo "second" >> version.txt
git add version.txt
git commit -q -m "second"
commit_two="$(git rev-parse HEAD)"

env_file_one="$tmp_root/compute-stable.env"
env_file_two="$tmp_root/compute-reuse.env"
env_file_three="$tmp_root/compute-rc.env"
env_file_four="$tmp_root/compute-stable-no-commit.env"
env_file_five="$tmp_root/compute-stable-from-rc.env"

GITHUB_ENV="$env_file_one" \
  BUMP_LEVEL=minor \
  RELEASE_CHANNEL=channel:stable \
  COMMIT_SHA="$commit_two" \
  bash "$repo_root/.github/scripts/compute-version.sh" >/dev/null
grep -qx 'APP_EFFECTIVE_VERSION=2.11.0' "$env_file_one"
grep -qx 'APP_RELEASE_TAG=v2.11.0' "$env_file_one"
grep -qx 'APP_IS_PRERELEASE=false' "$env_file_one"

GITHUB_ENV="$env_file_four" \
  BUMP_LEVEL=minor \
  RELEASE_CHANNEL=channel:stable \
  bash "$repo_root/.github/scripts/compute-version.sh" >/dev/null
grep -qx 'APP_EFFECTIVE_VERSION=2.11.0' "$env_file_four"
grep -qx 'APP_RELEASE_TAG=v2.11.0' "$env_file_four"
grep -qx 'APP_IS_PRERELEASE=false' "$env_file_four"

git tag v2.11.0 "$commit_two"
GITHUB_ENV="$env_file_two" \
  BUMP_LEVEL=minor \
  RELEASE_CHANNEL=channel:stable \
  COMMIT_SHA="$commit_two" \
  bash "$repo_root/.github/scripts/compute-version.sh" >/dev/null
grep -qx 'APP_EFFECTIVE_VERSION=2.11.0' "$env_file_two"
grep -qx 'APP_RELEASE_TAG=v2.11.0' "$env_file_two"
grep -qx 'APP_IS_PRERELEASE=false' "$env_file_two"

echo "third" >> version.txt
git add version.txt
git commit -q -m "third"
commit_three="$(git rev-parse HEAD)"
short_three="$(git rev-parse --short=7 "$commit_three")"
git tag "v2.12.0-rc.${short_three}" "$commit_three"
GITHUB_ENV="$env_file_three" \
  BUMP_LEVEL=minor \
  RELEASE_CHANNEL=channel:rc \
  COMMIT_SHA="$commit_three" \
  bash "$repo_root/.github/scripts/compute-version.sh" >/dev/null
grep -qx 'APP_EFFECTIVE_VERSION=2.12.0' "$env_file_three"
grep -qx "APP_RELEASE_TAG=v2.12.0-rc.${short_three}" "$env_file_three"
grep -qx 'APP_IS_PRERELEASE=true' "$env_file_three"

GITHUB_ENV="$env_file_five" \
  BUMP_LEVEL=minor \
  RELEASE_CHANNEL=channel:stable \
  COMMIT_SHA="$commit_three" \
  bash "$repo_root/.github/scripts/compute-version.sh" >/dev/null
grep -qx 'APP_EFFECTIVE_VERSION=2.12.0' "$env_file_five"
grep -qx 'APP_RELEASE_TAG=v2.12.0' "$env_file_five"
grep -qx 'APP_IS_PRERELEASE=false' "$env_file_five"
popd >/dev/null

fake_bin="$tmp_root/fake-bin"
mkdir -p "$fake_bin"
cat > "$fake_bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
url="${*: -1}"
case "$url" in
  *"/commits/sha-push-001/pulls?per_page=100")
    printf '[{"number":63,"html_url":"https://github.com/IvanLi-CN/octo-rill/pull/63"}]'
    ;;
  *"/issues/63/labels?per_page=100")
    printf '[{"name":"type:minor"},{"name":"channel:stable"}]'
    ;;
  *"/commits/sha-skip-002/pulls?per_page=100")
    printf '[{"number":64,"html_url":"https://github.com/IvanLi-CN/octo-rill/pull/64"}]'
    ;;
  *"/issues/64/labels?per_page=100")
    printf '[{"name":"type:skip"},{"name":"channel:stable"}]'
    ;;
  *)
    echo "unexpected url: $url" >&2
    exit 1
    ;;
esac
EOF
chmod +x "$fake_bin/curl"

release_output_one="$tmp_root/release-intent-one.out"
release_output_two="$tmp_root/release-intent-two.out"
PATH="$fake_bin:$PATH" \
  GITHUB_REPOSITORY="IvanLi-CN/octo-rill" \
  GITHUB_TOKEN="test-token" \
  RELEASE_HEAD_SHA="sha-push-001" \
  GITHUB_OUTPUT="$release_output_one" \
  bash "$repo_root/.github/scripts/release-intent.sh" >/dev/null
grep -qx 'should_release=true' "$release_output_one"
grep -qx 'bump_level=minor' "$release_output_one"
grep -qx 'channel=channel:stable' "$release_output_one"
grep -qx 'pr_number=63' "$release_output_one"
grep -qx 'reason=intent_release' "$release_output_one"

PATH="$fake_bin:$PATH" \
  GITHUB_REPOSITORY="IvanLi-CN/octo-rill" \
  GITHUB_TOKEN="test-token" \
  RELEASE_HEAD_SHA="sha-skip-002" \
  GITHUB_OUTPUT="$release_output_two" \
  bash "$repo_root/.github/scripts/release-intent.sh" >/dev/null
grep -qx 'should_release=false' "$release_output_two"
grep -qx 'release_intent_label=type:skip' "$release_output_two"
grep -qx 'pr_number=64' "$release_output_two"
grep -qx 'reason=intent_skip' "$release_output_two"

python3 - <<'PY' \
  "$repo_root/.github/scripts/release_backfill.py" \
  "$repo_root/.github/scripts/check_quality_gates_contract.py" \
  "$repo_root/.github/workflows/release.yml" \
  "$repo_root/.github/workflows/ci.yml"
from __future__ import annotations

import importlib.util
import io
import sys
from pathlib import Path

backfill_path = Path(sys.argv[1])
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


module = load_module(backfill_path, "release_backfill")
contract = load_module(contract_path, "quality_gates_contract")


class FakeResponse:
    def __init__(self, payload: bytes) -> None:
        self.payload = payload

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, traceback) -> bool:
        return False

    def read(self) -> bytes:
        return self.payload


def http_error(
    url: str,
    code: int,
    body: bytes = b"",
    headers: dict[str, str] | None = None,
) -> module.urllib.error.HTTPError:
    return module.urllib.error.HTTPError(
        url,
        code,
        "error",
        headers or {},
        io.BytesIO(body),
    )


original_urlopen = module.urllib.request.urlopen
original_sleep = module.time.sleep
sleep_calls: list[int] = []
module.time.sleep = lambda delay: sleep_calls.append(delay)

try:
    transient_calls = {"count": 0}

    def flaky_commit_pulls(request, *, timeout):
        assert timeout == module.DEFAULT_GITHUB_API_TIMEOUT_SECONDS
        transient_calls["count"] += 1
        if transient_calls["count"] == 1:
            raise http_error(request.full_url, 500, b"temporary", {"Retry-After": "3"})
        return FakeResponse(b'[{"number":128,"html_url":"https://github.com/IvanLi-CN/octo-rill/pull/128"}]')

    module.urllib.request.urlopen = flaky_commit_pulls
    retry_client = module.GitHubApiClient("https://api.github.test", "token")
    assert retry_client.pull_for_commit("IvanLi-CN/octo-rill", "sha-transient") == (
        128,
        "https://github.com/IvanLi-CN/octo-rill/pull/128",
    )
    assert transient_calls["count"] == 2
    assert sleep_calls == [3]
    sleep_calls.clear()

    network_calls = {"count": 0}

    def flaky_network(request, *, timeout):
        assert timeout == module.DEFAULT_GITHUB_API_TIMEOUT_SECONDS
        network_calls["count"] += 1
        if network_calls["count"] == 1:
            raise module.urllib.error.URLError("connection reset")
        return FakeResponse(b'{"ok":true}')

    module.urllib.request.urlopen = flaky_network
    assert retry_client.request_json("/repos/IvanLi-CN/octo-rill/actions/runs/1") == {"ok": True}
    assert network_calls["count"] == 2
    assert sleep_calls == [1]

    allow_404_calls = {"count": 0}

    def missing_release(request, *, timeout):
        assert timeout == module.DEFAULT_GITHUB_API_TIMEOUT_SECONDS
        allow_404_calls["count"] += 1
        raise http_error(request.full_url, 404, b"not found")

    module.urllib.request.urlopen = missing_release
    assert retry_client.request_json("/repos/IvanLi-CN/octo-rill/releases/tags/v0.0.0", allow_404=True) is None
    assert allow_404_calls["count"] == 1

    forbidden_calls = {"count": 0}

    def forbidden_request(request, *, timeout):
        assert timeout == module.DEFAULT_GITHUB_API_TIMEOUT_SECONDS
        forbidden_calls["count"] += 1
        raise http_error(request.full_url, 403, b"forbidden")

    module.urllib.request.urlopen = forbidden_request
    try:
        retry_client.request_json_list("/repos/IvanLi-CN/octo-rill/commits/sha/pulls")
    except RuntimeError as exc:
        assert "failed: 403 forbidden" in str(exc)
    else:
        raise AssertionError("expected non-retryable 403 to fail")
    assert forbidden_calls["count"] == 1

    post_calls = {"count": 0}

    def failing_post(request, *, timeout):
        assert timeout == module.DEFAULT_GITHUB_API_TIMEOUT_SECONDS
        post_calls["count"] += 1
        raise http_error(request.full_url, 500, b"server error")

    module.urllib.request.urlopen = failing_post
    try:
        retry_client.request_json(
            "/repos/IvanLi-CN/octo-rill/actions/workflows/release.yml/dispatches",
            method="POST",
            body={"ref": "main"},
        )
    except RuntimeError as exc:
        assert "failed: 500 server error" in str(exc)
    else:
        raise AssertionError("expected non-GET 500 to fail without retry")
    assert post_calls["count"] == 1
finally:
    module.urllib.request.urlopen = original_urlopen
    module.time.sleep = original_sleep

stable_intent = module.parse_release_intent(["type:minor", "channel:stable"])
assert stable_intent.should_release is True
assert stable_intent.bump_level == "minor"
assert stable_intent.channel == "stable"
assert stable_intent.prerelease is False

skip_intent = module.parse_release_intent(["type:skip", "channel:stable"])
assert skip_intent.should_release is False
assert skip_intent.reason == "intent_skip"

assert module.select_matching_tag(["v2.11.0", "v2.11.0-rc.28c3ff8"], "stable") == "v2.11.0"
assert module.select_matching_tag(["v2.11.0", "v2.11.0-rc.28c3ff8"], "rc") == "v2.11.0-rc.28c3ff8"

legacy_repair_candidate = module.ReleaseCandidate(
    sha="legacy00000000000000000000000000000000000",
    pr_number=6,
    pr_url="https://github.com/IvanLi-CN/octo-rill/pull/6",
    intent=module.ReleaseIntent(
        should_release=True,
        bump_level="patch",
        channel="stable",
        prerelease=False,
        release_intent_label="type:patch",
        reason="intent_release",
    ),
    matching_tag="v0.1.4",
    release_exists=True,
    comment_exists=False,
)
latest_published_candidate = module.ReleaseCandidate(
    sha="published000000000000000000000000000000000",
    pr_number=61,
    pr_url="https://github.com/IvanLi-CN/octo-rill/pull/61",
    intent=stable_intent,
    matching_tag="v2.10.0",
    release_exists=True,
    comment_exists=True,
)
recent_repair_candidate = module.ReleaseCandidate(
    sha="repair000000000000000000000000000000000000",
    pr_number=64,
    pr_url="https://github.com/IvanLi-CN/octo-rill/pull/64",
    intent=stable_intent,
    matching_tag="v2.10.1",
    release_exists=True,
    comment_exists=False,
)
missing_sixty_three = module.ReleaseCandidate(
    sha="28c3ff8f919d881f8e4bdc63c9bb9aae1543cfe9",
    pr_number=63,
    pr_url="https://github.com/IvanLi-CN/octo-rill/pull/63",
    intent=stable_intent,
    matching_tag=None,
    release_exists=False,
    comment_exists=False,
)
missing_sixty_two = module.ReleaseCandidate(
    sha="991bee71b861c7b1be0038fc0909928186c369e2",
    pr_number=62,
    pr_url="https://github.com/IvanLi-CN/octo-rill/pull/62",
    intent=stable_intent,
    matching_tag=None,
    release_exists=False,
    comment_exists=False,
)

candidates = [legacy_repair_candidate, latest_published_candidate, missing_sixty_three, missing_sixty_two]
selected = module.select_release_candidate(
    candidates,
    default_sha=missing_sixty_two.sha,
    requested_sha=None,
    exclude_shas=set(),
)
assert selected.selected_sha == missing_sixty_three.sha
assert selected.selected_candidate == missing_sixty_three
assert selected.next_candidate == missing_sixty_two
assert selected.unpublished_count == 2
assert selected.repair_count == 0

repair_only = module.select_release_candidate(
    [legacy_repair_candidate, latest_published_candidate, recent_repair_candidate],
    default_sha=recent_repair_candidate.sha,
    requested_sha=None,
    exclude_shas=set(),
)
assert repair_only.selected_sha == recent_repair_candidate.sha
assert repair_only.selected_candidate == recent_repair_candidate
assert repair_only.selection_reason == "oldest_repair"
assert repair_only.unpublished_count == 0
assert repair_only.repair_count == 1

explicit = module.select_release_candidate(
    candidates,
    default_sha=missing_sixty_two.sha,
    requested_sha=missing_sixty_two.sha,
    exclude_shas=set(),
)
assert explicit.selected_sha == missing_sixty_two.sha
assert explicit.selection_reason == "explicit_head_sha"
assert explicit.next_candidate == missing_sixty_three

release_workflow = contract.load_yaml(release_workflow_path)
release_workflow_text = release_workflow_path.read_text(encoding="utf-8")
on_section = contract.require_mapping(contract.mapping_get(release_workflow, "on"), "release.yml.on")
assert "workflow_run" not in on_section
push_config = contract.event_config(release_workflow, "push", "release.yml")
contract.assert_event_branches(push_config, {"main"}, "release.yml.on.push")
workflow_dispatch = contract.event_config(release_workflow, "workflow_dispatch", "release.yml")
inputs = contract.require_mapping(workflow_dispatch.get("inputs"), "release.yml.on.workflow_dispatch.inputs")
head_sha_input = contract.require_mapping(inputs.get("head_sha"), "release.yml.on.workflow_dispatch.inputs.head_sha")
assert head_sha_input.get("required") is True
assert head_sha_input.get("type") == "string"

plan_job = contract.job_config(release_workflow, "plan", "release.yml")
plan_step = contract.step_config(plan_job, "Determine release target", "release.yml.jobs.plan")
assert "python3 ./.github/scripts/release_backfill.py plan" in contract.step_run(
    plan_step,
    "release.yml.jobs.plan.steps['Determine release target']",
)

prepare_job = contract.job_config(release_workflow, "prepare", "release.yml")
await_ci_job = contract.job_config(release_workflow, "await-ci", "release.yml")
await_ci_checkout = contract.step_config(await_ci_job, "Checkout workflow revision", "release.yml.jobs.await-ci")
await_ci_checkout_with = contract.require_mapping(
    await_ci_checkout.get("with"),
    "release.yml.jobs.await-ci.steps['Checkout workflow revision'].with",
)
assert await_ci_checkout.get("uses") == "actions/checkout@v4"
assert await_ci_checkout_with.get("ref") == "${{ github.workflow_sha }}"
await_ci_step = contract.step_config(await_ci_job, "Gate current push release against CI", "release.yml.jobs.await-ci")
assert "python3 ./.github/scripts/release_backfill.py await-ci" in contract.step_run(
    await_ci_step,
    "release.yml.jobs.await-ci.steps['Gate current push release against CI']",
)

assert prepare_job.get("needs") == ["plan", "await-ci"]
intent_step = contract.step_config(prepare_job, "Determine release intent", "release.yml.jobs.prepare")
intent_env = contract.require_mapping(
    prepare_job.get("env"),
    "release.yml.jobs.prepare.env",
)
assert intent_env.get("RELEASE_HEAD_SHA") == "${{ needs.plan.outputs.release_head_sha }}"
assert "bash ./.github/scripts/release-intent.sh" in contract.step_run(
    intent_step,
    "release.yml.jobs.prepare.steps['Determine release intent']",
)
docker_release_job = contract.job_config(release_workflow, "docker-release", "release.yml")
docker_release_checkout = contract.step_config(
    docker_release_job,
    "Checkout release revision",
    "release.yml.jobs.docker-release",
)
docker_release_checkout_with = contract.require_mapping(
    docker_release_checkout.get("with"),
    "release.yml.jobs.docker-release.steps['Checkout release revision'].with",
)
assert docker_release_checkout.get("uses") == "actions/checkout@v4"
assert docker_release_checkout_with.get("ref") == "${{ env.RELEASE_HEAD_SHA }}"
docker_overlay_step = contract.step_config(
    docker_release_job,
    "Overlay workflow build infrastructure for historical backfills",
    "release.yml.jobs.docker-release",
)
assert docker_overlay_step.get("if") == "${{ env.RELEASE_HEAD_SHA != github.workflow_sha }}"
docker_overlay_run = contract.step_run(
    docker_overlay_step,
    "release.yml.jobs.docker-release.steps['Overlay workflow build infrastructure for historical backfills']",
)
assert 'git fetch --no-tags --depth=1 origin "${workflow_sha}"' in docker_overlay_run
assert '"Dockerfile"' in docker_overlay_run
assert '"web/vite.config.ts"' in docker_overlay_run
assert '"web/config/embeddedVersion.ts"' in docker_overlay_run

release_existing_step = contract.step_config(
    prepare_job,
    "Create GitHub Release for existing tag",
    "release.yml.jobs.prepare",
)
release_existing_with = contract.require_mapping(
    release_existing_step.get("with"),
    "release.yml.jobs.prepare.steps['Create GitHub Release for existing tag'].with",
)
release_missing_step = contract.step_config(
    prepare_job,
    "Create GitHub Release for missing tag",
    "release.yml.jobs.prepare",
)
release_missing_with = contract.require_mapping(
    release_missing_step.get("with"),
    "release.yml.jobs.prepare.steps['Create GitHub Release for missing tag'].with",
)
auth_step = contract.step_config(
    prepare_job,
    "Validate historical release credentials",
    "release.yml.jobs.prepare",
)
auth_run = contract.step_run(
    auth_step,
    "release.yml.jobs.prepare.steps['Validate historical release credentials']",
)
assert 'git rev-parse -q --verify "refs/tags/${tag}"' in auth_run
assert "needs RELEASE_TOKEN to create missing tag" in auth_run
assert release_existing_with.get("tag_name") == "${{ steps.export.outputs.app_release_tag }}"
assert "target_commitish" not in release_existing_with
assert release_existing_with.get("token") == "${{ secrets.RELEASE_TOKEN != '' && secrets.RELEASE_TOKEN || github.token }}"
assert release_missing_with.get("tag_name") == "${{ steps.export.outputs.app_release_tag }}"
assert release_missing_with.get("target_commitish") == "${{ env.RELEASE_HEAD_SHA }}"
assert release_missing_with.get("token") == "${{ secrets.RELEASE_TOKEN != '' && secrets.RELEASE_TOKEN || github.token }}"
assert 'git push origin "refs/tags/${tag}"' not in release_workflow_text

audit_job = contract.job_config(release_workflow, "audit-backfill", "release.yml")
assert "github.event_name == 'push'" in audit_job.get("if", "")
assert "needs.await-ci.result == 'success'" in audit_job.get("if", "")
audit_permissions = contract.require_mapping(
    audit_job.get("permissions"),
    "release.yml.jobs.audit-backfill.permissions",
)
assert audit_permissions == {
    "contents": "read",
    "issues": "read",
    "pull-requests": "read",
    "actions": "write",
}
dispatch_step = contract.step_config(audit_job, "Dispatch next release backfill", "release.yml.jobs.audit-backfill")
assert "python3 ./.github/scripts/release_backfill.py dispatch" in contract.step_run(
    dispatch_step,
    "release.yml.jobs.audit-backfill.steps['Dispatch next release backfill']",
)

ci_workflow = contract.load_yaml(ci_workflow_path)
lint_job = contract.job_config(ci_workflow, "lint", "ci.yml")
compile_step = contract.step_config(lint_job, "Check quality-gates scripts", "ci.yml.jobs.lint")
compile_run = contract.step_run(compile_step, "ci.yml.jobs.lint.steps['Check quality-gates scripts']")
assert ".github/scripts/release_backfill.py" in compile_run
self_tests_step = contract.step_config(lint_job, "Quality gates self-tests", "ci.yml.jobs.lint")
self_tests_run = contract.step_run(self_tests_step, "ci.yml.jobs.lint.steps['Quality gates self-tests']")
assert "bash .github/scripts/test-release-automation.sh" in self_tests_run

print("test-release-automation: all checks passed")
PY
