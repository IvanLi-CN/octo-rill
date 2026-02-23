#!/usr/bin/env bash
set -euo pipefail

api_root="${GITHUB_API_URL:-https://api.github.com}"
repo="${GITHUB_REPOSITORY:-}"
token="${GITHUB_TOKEN:-}"
sha="${WORKFLOW_RUN_SHA:-${GITHUB_SHA:-}}"

if [[ -z "${repo}" ]]; then
  echo "release-intent: missing GITHUB_REPOSITORY" >&2
  exit 2
fi

if [[ -z "${sha}" ]]; then
  echo "release-intent: missing WORKFLOW_RUN_SHA (or GITHUB_SHA)" >&2
  exit 2
fi

if [[ -z "${token}" ]]; then
  echo "release-intent: missing GITHUB_TOKEN" >&2
  exit 2
fi

write_output() {
  local key="$1"
  local value="$2"

  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    echo "${key}=${value}" >> "${GITHUB_OUTPUT}"
  fi
}

conservative_skip() {
  local reason="$1"
  echo "should_release=false"
  echo "bump_level="
  echo "channel="
  echo "prerelease=false"
  echo "release_intent_label="
  echo "pr_number="
  echo "pr_url="
  echo "reason=${reason}"

  write_output "should_release" "false"
  write_output "bump_level" ""
  write_output "channel" ""
  write_output "prerelease" "false"
  write_output "release_intent_label" ""
  write_output "pr_number" ""
  write_output "pr_url" ""
  write_output "reason" "${reason}"
}

pulls_json=""
if ! pulls_json="$(
  curl -fsSL \
    --max-time 15 \
    -H "Accept: application/vnd.github+json" \
    -H "Authorization: Bearer ${token}" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    "${api_root}/repos/${repo}/commits/${sha}/pulls?per_page=100"
)"; then
  echo "::warning::release-intent: failed to resolve commit->PR mapping for sha=${sha}; skip release"
  conservative_skip "api_failure:commit_pulls"
  exit 0
fi

export pulls_json
declare count pr_number pr_url
count=""
pr_number=""
pr_url=""
while IFS='=' read -r key value; do
  case "${key}" in
    count) count="${value}" ;;
    pr_number) pr_number="${value}" ;;
    pr_url) pr_url="${value}" ;;
  esac
done < <(
  python3 - <<'PY'
from __future__ import annotations

import json
import os

pulls = json.loads(os.environ["pulls_json"])
if not isinstance(pulls, list):
    print("count=0")
    raise SystemExit(0)

print(f"count={len(pulls)}")
if len(pulls) == 1:
    pr = pulls[0]
    number = pr.get("number")
    if isinstance(number, int):
        print(f"pr_number={number}")
    url = pr.get("html_url") or ""
    print(f"pr_url={url}")
PY
)

if [[ "${count}" != "1" || -z "${pr_number}" ]]; then
  echo "::notice::release-intent: sha=${sha} maps to ${count:-0} PR(s); skip release"
  conservative_skip "ambiguous_or_missing_pr(count=${count:-0})"
  exit 0
fi

labels_json=""
if ! labels_json="$(
  curl -fsSL \
    --max-time 15 \
    -H "Accept: application/vnd.github+json" \
    -H "Authorization: Bearer ${token}" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    "${api_root}/repos/${repo}/issues/${pr_number}/labels?per_page=100"
)"; then
  echo "::warning::release-intent: failed to read PR labels for pr=${pr_number}; skip release"
  conservative_skip "api_failure:pr_labels"
  exit 0
fi

export labels_json
declare status message should_release bump_level channel prerelease release_intent_label reason
status=""
message=""
should_release=""
bump_level=""
channel=""
prerelease=""
release_intent_label=""
reason=""
while IFS='=' read -r key value; do
  case "${key}" in
    status) status="${value}" ;;
    message) message="${value}" ;;
    should_release) should_release="${value}" ;;
    bump_level) bump_level="${value}" ;;
    channel) channel="${value}" ;;
    prerelease) prerelease="${value}" ;;
    release_intent_label) release_intent_label="${value}" ;;
    reason) reason="${value}" ;;
  esac
done < <(
  python3 - <<'PY'
from __future__ import annotations

import json
import os

labels = json.loads(os.environ["labels_json"])
names = [l.get("name", "") for l in labels if isinstance(l, dict)]

allowed_type = {
    "type:docs",
    "type:skip",
    "type:patch",
    "type:minor",
    "type:major",
}
allowed_channel = {
    "channel:stable",
    "channel:rc",
}

type_labels = [n for n in names if n.startswith("type:")]
channel_labels = [n for n in names if n.startswith("channel:")]

unknown_type = sorted({n for n in type_labels if n not in allowed_type})
unknown_channel = sorted({n for n in channel_labels if n not in allowed_channel})

selected_type = sorted({n for n in type_labels if n in allowed_type})
selected_channel = sorted({n for n in channel_labels if n in allowed_channel})

if unknown_type:
    print("status=error")
    print(f"message=unknown_type_labels({','.join(unknown_type)})")
    raise SystemExit(0)

if unknown_channel:
    print("status=error")
    print(f"message=unknown_channel_labels({','.join(unknown_channel)})")
    raise SystemExit(0)

if len(selected_type) != 1:
    print("status=error")
    print(f"message=invalid_type_label_count({len(selected_type)})")
    raise SystemExit(0)

if len(selected_channel) != 1:
    print("status=error")
    print(f"message=invalid_channel_label_count({len(selected_channel)})")
    raise SystemExit(0)

type_label = selected_type[0]
channel_label = selected_channel[0]

if type_label in {"type:docs", "type:skip"}:
    print("status=ok")
    print("should_release=false")
    print("bump_level=")
    print(f"channel={channel_label}")
    print("prerelease=false")
    print(f"release_intent_label={type_label}")
    print("reason=intent_skip")
    raise SystemExit(0)

bump_level = type_label.removeprefix("type:")
is_prerelease = "true" if channel_label == "channel:rc" else "false"

print("status=ok")
print("should_release=true")
print(f"bump_level={bump_level}")
print(f"channel={channel_label}")
print(f"prerelease={is_prerelease}")
print(f"release_intent_label={type_label}")
print("reason=intent_release")
PY
)

if [[ "${status}" != "ok" ]]; then
  echo "::error::release-intent: ${message:-invalid_labels}"
  exit 1
fi

echo "should_release=${should_release}"
echo "bump_level=${bump_level}"
echo "channel=${channel}"
echo "prerelease=${prerelease}"
echo "release_intent_label=${release_intent_label}"
echo "pr_number=${pr_number}"
echo "pr_url=${pr_url}"
echo "reason=${reason}"

write_output "should_release" "${should_release}"
write_output "bump_level" "${bump_level}"
write_output "channel" "${channel}"
write_output "prerelease" "${prerelease}"
write_output "release_intent_label" "${release_intent_label}"
write_output "pr_number" "${pr_number}"
write_output "pr_url" "${pr_url}"
write_output "reason" "${reason}"
