#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python3 - <<'PY' \
  "$repo_root/.github/scripts/release_metadata.py" \
  "$repo_root/.github/scripts/check_quality_gates_contract.py" \
  "$repo_root/.github/workflows/release.yml" \
  "$repo_root/.github/workflows/ci.yml"
from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile
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


def git(repo: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=repo,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise AssertionError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout.strip()


module = load_module(script_path, "release_metadata")
contract = load_module(contract_path, "quality_gates_contract")


class FakeClient:
    def __init__(self, *, latest_release=None, releases_by_tag=None):
        self.latest_release = latest_release or {}
        self.releases_by_tag = releases_by_tag or {}

    def get_most_recent_published_release(self, repository: str):
        assert repository == "IvanLi-CN/octo-rill"
        return dict(self.latest_release)

    def get_release_by_tag(self, repository: str, tag: str):
        assert repository == "IvanLi-CN/octo-rill"
        if tag not in self.releases_by_tag:
            raise AssertionError(f"unexpected tag lookup: {tag}")
        return dict(self.releases_by_tag[tag])


with tempfile.TemporaryDirectory() as tmp_dir:
    repo = Path(tmp_dir)
    git(repo, "init")
    git(repo, "config", "user.name", "Test User")
    git(repo, "config", "user.email", "test@example.com")
    (repo / "Cargo.toml").write_text('[package]\nname = "octo-rill"\nversion = "0.1.0"\n', encoding="utf-8")
    (repo / "README.md").write_text("test\n", encoding="utf-8")
    git(repo, "add", "Cargo.toml", "README.md")
    git(repo, "commit", "-m", "init")
    first_sha = git(repo, "rev-parse", "HEAD")

    computed = module.resolve_release_metadata(
        repo_root=repo,
        repository="IvanLi-CN/octo-rill",
        event_name="workflow_run",
        workflow_run_sha=first_sha,
        input_head_sha="",
        input_release_tag="",
        intent_should_release=True,
        intent_bump_level="patch",
        intent_channel="channel:stable",
        intent_reason="intent_release",
        client=None,
    )
    assert computed.should_release is True
    assert computed.app_release_tag == "v0.1.1"
    assert computed.app_effective_version == "0.1.1"
    assert computed.reuse_existing_tag is False
    assert computed.publish_docker is True

    (repo / "README.md").write_text("rerun target\n", encoding="utf-8")
    git(repo, "add", "README.md")
    git(repo, "commit", "-m", "rerun target")
    second_sha = git(repo, "rev-parse", "HEAD")
    git(repo, "tag", "v2.4.3", second_sha)
    git(repo, "tag", "v2.4.4", second_sha)

    reused = module.resolve_release_metadata(
        repo_root=repo,
        repository="IvanLi-CN/octo-rill",
        event_name="workflow_run",
        workflow_run_sha=second_sha,
        input_head_sha="",
        input_release_tag="",
        intent_should_release=True,
        intent_bump_level="patch",
        intent_channel="stable",
        intent_reason="intent_release",
        client=None,
    )
    assert reused.app_release_tag == "v2.4.4"
    assert reused.app_effective_version == "2.4.4"
    assert reused.reuse_existing_tag is True
    assert reused.reason == "reuse_existing_tag"

    reused_after_channel_drift = module.resolve_release_metadata(
        repo_root=repo,
        repository="IvanLi-CN/octo-rill",
        event_name="workflow_run",
        workflow_run_sha=second_sha,
        input_head_sha="",
        input_release_tag="",
        intent_should_release=True,
        intent_bump_level="patch",
        intent_channel="rc",
        intent_reason="intent_release",
        client=None,
    )
    assert reused_after_channel_drift.app_release_tag == "v2.4.4"
    assert reused_after_channel_drift.app_effective_version == "2.4.4"
    assert reused_after_channel_drift.reuse_existing_tag is True
    assert reused_after_channel_drift.reason == "reuse_existing_tag"

    (repo / "README.md").write_text("rc only target\n", encoding="utf-8")
    git(repo, "add", "README.md")
    git(repo, "commit", "-m", "rc only target")
    third_sha = git(repo, "rev-parse", "HEAD")
    git(repo, "tag", f"v2.4.5-rc.{third_sha[:7]}", third_sha)

    stable_promotion = module.resolve_release_metadata(
        repo_root=repo,
        repository="IvanLi-CN/octo-rill",
        event_name="workflow_run",
        workflow_run_sha=third_sha,
        input_head_sha="",
        input_release_tag="",
        intent_should_release=True,
        intent_bump_level="patch",
        intent_channel="stable",
        intent_reason="intent_release",
        client=None,
    )
    assert stable_promotion.app_release_tag == "v2.4.5"
    assert stable_promotion.app_effective_version == "2.4.5"
    assert stable_promotion.app_is_prerelease is False
    assert stable_promotion.reuse_existing_tag is False

    explicit_backfill = module.resolve_release_metadata(
        repo_root=repo,
        repository="IvanLi-CN/octo-rill",
        event_name="workflow_dispatch",
        workflow_run_sha="",
        input_head_sha="",
        input_release_tag="v2.4.4",
        intent_should_release=False,
        intent_bump_level="",
        intent_channel="",
        intent_reason="",
        client=FakeClient(releases_by_tag={"v2.4.4": {"tag_name": "v2.4.4"}}),
    )
    assert explicit_backfill.app_release_tag == "v2.4.4"
    assert explicit_backfill.app_effective_version == "2.4.4"
    assert explicit_backfill.publish_docker is False
    assert explicit_backfill.generate_release_notes is False
    assert explicit_backfill.release_mode == "backfill"
    assert explicit_backfill.reason == "backfill:explicit_release_tag"

    latest_backfill = module.resolve_release_metadata(
        repo_root=repo,
        repository="IvanLi-CN/octo-rill",
        event_name="workflow_dispatch",
        workflow_run_sha="",
        input_head_sha="",
        input_release_tag="",
        intent_should_release=False,
        intent_bump_level="",
        intent_channel="",
        intent_reason="",
        client=FakeClient(latest_release={"tag_name": "v2.4.4"}),
    )
    assert latest_backfill.app_release_tag == "v2.4.4"
    assert latest_backfill.reason == "backfill:latest_published_release"

release_workflow = contract.load_yaml(release_workflow_path)
dispatch_inputs = contract.require_mapping(
    contract.event_config(release_workflow, "workflow_dispatch", "release.yml").get("inputs"),
    "release.yml.on.workflow_dispatch.inputs",
)
release_tag_input = contract.require_mapping(dispatch_inputs.get("release_tag"), "release.yml.on.workflow_dispatch.inputs.release_tag")
assert release_tag_input.get("required") is False
prepare_job = contract.job_config(release_workflow, "prepare", "release.yml")
prepare_checkout = contract.uses_step_config(
    prepare_job,
    "Checkout workflow revision",
    "actions/checkout@v4",
    "release.yml.jobs.prepare",
)
prepare_checkout_with = contract.require_mapping(
    prepare_checkout.get("with"),
    "release.yml.jobs.prepare.steps['Checkout workflow revision'].with",
)
assert prepare_checkout_with.get("path") == "workflow-src"
resolve_step = contract.step_config(prepare_job, "Resolve release metadata", "release.yml.jobs.prepare")
assert "workflow-src/.github/scripts/release_metadata.py" in contract.step_run(
    resolve_step,
    "release.yml.jobs.prepare.steps['Resolve release metadata']",
)
bundle_job = contract.job_config(release_workflow, "bundle-release", "release.yml")
bundle_checkout = contract.uses_step_config(
    bundle_job,
    "Checkout workflow revision",
    "actions/checkout@v4",
    "release.yml.jobs.bundle-release",
)
bundle_checkout_with = contract.require_mapping(
    bundle_checkout.get("with"),
    "release.yml.jobs.bundle-release.steps['Checkout workflow revision'].with",
)
assert bundle_checkout_with.get("path") == "workflow-src"
bundle_build_step = contract.step_config(
    bundle_job,
    "Build release bundle",
    "release.yml.jobs.bundle-release",
)
assert "workflow-src/.github/scripts/build-release-bundle.sh" in contract.step_run(
    bundle_build_step,
    "release.yml.jobs.bundle-release.steps['Build release bundle']",
)
bundle_upload = contract.uses_step_config(
    bundle_job,
    "Upload release bundle artifact",
    "actions/upload-artifact@v4",
    "release.yml.jobs.bundle-release",
)
bundle_upload_with = contract.require_mapping(
    bundle_upload.get("with"),
    "release.yml.jobs.bundle-release.steps['Upload release bundle artifact'].with",
)
assert bundle_upload_with.get("name") == "${{ env.RELEASE_BUNDLE_NAME }}"

publish_job = contract.job_config(release_workflow, "publish-release", "release.yml")
assert set(publish_job.get("needs", [])) == {"prepare", "bundle-release", "docker-release"}
contract.uses_step_config(
    publish_job,
    "Download release bundle artifact",
    "actions/download-artifact@v5",
    "release.yml.jobs.publish-release",
)
publish_step = contract.uses_step_config(
    publish_job,
    "Create or update GitHub Release",
    "softprops/action-gh-release@v2",
    "release.yml.jobs.publish-release",
)
publish_with = contract.require_mapping(
    publish_step.get("with"),
    "release.yml.jobs.publish-release.steps['Create or update GitHub Release'].with",
)
assert "RELEASE_BUNDLE_NAME" in str(publish_with.get("files", ""))

comment_job = contract.job_config(release_workflow, "pr-release-comment", "release.yml")
assert set(comment_job.get("needs", [])) == {"prepare", "publish-release"}

ci_workflow = contract.load_yaml(ci_workflow_path)
build_job = contract.job_config(ci_workflow, "build", "ci.yml")
build_step = contract.step_config(build_job, "Build release bundle", "ci.yml.jobs.build")
assert "build-release-bundle.sh" in contract.step_run(build_step, "ci.yml.jobs.build.steps['Build release bundle']")

print("test-release-metadata: all checks passed")
PY
