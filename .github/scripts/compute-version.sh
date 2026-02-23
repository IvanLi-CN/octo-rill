#!/usr/bin/env bash
set -euo pipefail

# Compute release version using PR intent labels.
# Inputs:
# - BUMP_LEVEL: major|minor|patch
# - RELEASE_CHANNEL: stable|rc|channel:stable|channel:rc
# - COMMIT_SHA: required for rc channel (fallback to GITHUB_SHA)
# Outputs:
# - APP_EFFECTIVE_VERSION: semver without prefix
# - APP_RELEASE_TAG: tag with leading v, maybe rc suffix
# - APP_IS_PRERELEASE: true|false

root_dir="$(git rev-parse --show-toplevel)"

git fetch --tags --force >/dev/null 2>&1 || true

cargo_ver="$(
  grep -m1 '^version[[:space:]]*=[[:space:]]*"' "$root_dir/Cargo.toml" \
    | sed -E 's/.*"([0-9]+\.[0-9]+\.[0-9]+)".*/\1/'
)"

if [[ -z "${cargo_ver:-}" ]]; then
  echo "Failed to detect version from Cargo.toml" >&2
  exit 1
fi

if [[ -z "${BUMP_LEVEL:-}" ]]; then
  echo "Missing BUMP_LEVEL (expected: major|minor|patch)" >&2
  exit 1
fi

case "${BUMP_LEVEL}" in
  major|minor|patch) ;;
  *)
    echo "Invalid BUMP_LEVEL=${BUMP_LEVEL} (expected: major|minor|patch)" >&2
    exit 1
    ;;
esac

if [[ -z "${RELEASE_CHANNEL:-}" ]]; then
  echo "Missing RELEASE_CHANNEL (expected: stable|rc|channel:stable|channel:rc)" >&2
  exit 1
fi

channel=""
case "${RELEASE_CHANNEL}" in
  stable|channel:stable)
    channel="stable"
    ;;
  rc|channel:rc)
    channel="rc"
    ;;
  *)
    echo "Invalid RELEASE_CHANNEL=${RELEASE_CHANNEL}" >&2
    exit 1
    ;;
esac

commit_sha="${COMMIT_SHA:-${GITHUB_SHA:-}}"
if [[ "${channel}" == "rc" && -z "${commit_sha}" ]]; then
  echo "Missing COMMIT_SHA (or GITHUB_SHA) for rc release" >&2
  exit 1
fi

max_stable_tag="$(
  git tag -l \
    | grep -E '^v?[0-9]+\.[0-9]+\.[0-9]+$' \
    | sed -E 's/^v//' \
    | sort -Vu \
    | tail -n 1 \
    || true
)"

base_ver="${max_stable_tag:-$cargo_ver}"

base_major="$(echo "$base_ver" | cut -d. -f1)"
base_minor="$(echo "$base_ver" | cut -d. -f2)"
base_patch="$(echo "$base_ver" | cut -d. -f3)"

case "${BUMP_LEVEL}" in
  major)
    next_major="$((base_major + 1))"
    next_minor="0"
    next_patch="0"
    ;;
  minor)
    next_major="${base_major}"
    next_minor="$((base_minor + 1))"
    next_patch="0"
    ;;
  patch)
    next_major="${base_major}"
    next_minor="${base_minor}"
    next_patch="$((base_patch + 1))"
    ;;
esac

candidate_patch="${next_patch}"
while \
  git rev-parse -q --verify "refs/tags/${next_major}.${next_minor}.${candidate_patch}" >/dev/null \
  || git rev-parse -q --verify "refs/tags/v${next_major}.${next_minor}.${candidate_patch}" >/dev/null; do
  candidate_patch="$((candidate_patch + 1))"
done

app_effective_version="${next_major}.${next_minor}.${candidate_patch}"
app_release_tag=""
app_is_prerelease="false"

if [[ "${channel}" == "stable" ]]; then
  app_release_tag="v${app_effective_version}"
else
  short_sha="$(echo "${commit_sha}" | cut -c1-7)"
  app_release_tag="v${app_effective_version}-rc.${short_sha}"
  app_is_prerelease="true"
fi

{
  echo "APP_EFFECTIVE_VERSION=${app_effective_version}"
  echo "APP_RELEASE_TAG=${app_release_tag}"
  echo "APP_IS_PRERELEASE=${app_is_prerelease}"
} >> "${GITHUB_ENV:-/dev/stdout}"

echo "Computed release version"
echo "  base_version=${base_ver} (max_stable_tag=${max_stable_tag:-<none>}, cargo=${cargo_ver})"
echo "  bump_level=${BUMP_LEVEL}"
echo "  channel=${channel}"
echo "  APP_EFFECTIVE_VERSION=${app_effective_version}"
echo "  APP_RELEASE_TAG=${app_release_tag}"
echo "  APP_IS_PRERELEASE=${app_is_prerelease}"
