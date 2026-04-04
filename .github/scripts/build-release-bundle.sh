#!/usr/bin/env bash
set -euo pipefail

repo_root="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
bundle_name="${BUNDLE_NAME:-octo-rill-linux-x86_64}"
bundle_root="${BUNDLE_ROOT:-${repo_root}/.tmp/release-bundle}"
archive_dir="${ARCHIVE_DIR:-${repo_root}/.tmp/release-archives}"
binary_name="${BINARY_NAME:-octo-rill}"
web_dir="${WEB_DIR:-${repo_root}/web}"

bundle_dir="${bundle_root}/${bundle_name}"
archive_path="${archive_dir}/${bundle_name}.tar.gz"

rm -rf "${bundle_dir}" "${archive_path}"
mkdir -p "${bundle_dir}/web" "${archive_dir}"

pushd "${web_dir}" >/dev/null
bun install --frozen-lockfile
bun run build
popd >/dev/null

cargo build --release --locked

cp "${repo_root}/target/release/${binary_name}" "${bundle_dir}/${binary_name}"
cp "${repo_root}/.env.example" "${bundle_dir}/.env.example"
cp -R "${web_dir}/dist" "${bundle_dir}/web/dist"

chmod 755 "${bundle_dir}/${binary_name}"

tar -C "${bundle_root}" -czf "${archive_path}" "${bundle_name}"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "bundle_name=${bundle_name}"
    echo "bundle_dir=${bundle_dir}"
    echo "archive_path=${archive_path}"
  } >> "${GITHUB_OUTPUT}"
fi

echo "release bundle ready: ${archive_path}"
