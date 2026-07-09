#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: assemble-pages-site.sh <docs_dir> <storybook_dir> <demo_dir> <output_dir>

Copy the built docs site into the output root, nest Storybook under output_dir/storybook,
and publish the demo app under output_dir/demo.
USAGE
}

if [[ "$#" -ne 4 ]]; then
  usage >&2
  exit 1
fi

docs_dir="$1"
storybook_dir="$2"
demo_dir="$3"
output_dir="$4"

to_abs_path() {
  python3 -c 'import os, sys; print(os.path.abspath(sys.argv[1]))' "$1"
}

is_same_or_parent() {
  local base="$1"
  local candidate="$2"
  [[ "$candidate" == "$base" || "$candidate" == "$base"/* ]]
}

if [[ ! -d "$docs_dir" ]]; then
  echo "docs_dir does not exist: $docs_dir" >&2
  exit 1
fi

if [[ ! -d "$storybook_dir" ]]; then
  echo "storybook_dir does not exist: $storybook_dir" >&2
  exit 1
fi

if [[ ! -d "$demo_dir" ]]; then
  echo "demo_dir does not exist: $demo_dir" >&2
  exit 1
fi

docs_dir_abs="$(to_abs_path "$docs_dir")"
storybook_dir_abs="$(to_abs_path "$storybook_dir")"
demo_dir_abs="$(to_abs_path "$demo_dir")"
output_dir_abs="$(to_abs_path "$output_dir")"

if [[ "$output_dir_abs" == "/" ]]; then
  echo "refusing to use unsafe output_dir: $output_dir" >&2
  exit 1
fi

if is_same_or_parent "$output_dir_abs" "$docs_dir_abs"; then
  echo "refusing to let output_dir contain docs_dir: $output_dir" >&2
  exit 1
fi

if is_same_or_parent "$output_dir_abs" "$storybook_dir_abs"; then
  echo "refusing to let output_dir contain storybook_dir: $output_dir" >&2
  exit 1
fi

if is_same_or_parent "$output_dir_abs" "$demo_dir_abs"; then
  echo "refusing to let output_dir contain demo_dir: $output_dir" >&2
  exit 1
fi

rm -rf "$output_dir"
mkdir -p "$output_dir/storybook" "$output_dir/demo"

cp -R "$docs_dir"/. "$output_dir"/
cp -R "$storybook_dir"/. "$output_dir/storybook"/
cp -R "$demo_dir"/. "$output_dir/demo"/
cp ".github/assets/demo-404-recovery.js" "$output_dir/demo-404-recovery.js"

python3 - "$output_dir/404.html" "${DOCS_BASE:-/}" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
docs_base = sys.argv[2].strip() or "/"
if docs_base != "/":
    if not docs_base.startswith("/"):
        docs_base = f"/{docs_base}"
    if not docs_base.endswith("/"):
        docs_base = f"{docs_base}/"
content = path.read_text(encoding="utf-8")
snippet = f'<script src="{docs_base}demo-404-recovery.js" data-demo-404-recovery="true"></script>'

if snippet in content:
    raise SystemExit(0)

anchor = "</body>"
if anchor not in content:
    raise SystemExit("404.html is missing </body> for demo recovery injection")

content = content.replace(anchor, f"  {snippet}\n{anchor}", 1)
path.write_text(content, encoding="utf-8")
PY

if [[ ! -f "$output_dir/index.html" ]]; then
  echo "assembled site is missing root index.html" >&2
  exit 1
fi

if [[ ! -f "$output_dir/storybook/index.html" ]]; then
  echo "assembled site is missing storybook/index.html" >&2
  exit 1
fi

if [[ ! -f "$output_dir/demo/index.html" ]]; then
  echo "assembled site is missing demo/index.html" >&2
  exit 1
fi

if [[ ! -f "$output_dir/storybook.html" ]]; then
  echo "assembled site is missing storybook.html" >&2
  exit 1
fi

if ! grep -q '正在跳转到 Storybook' "$output_dir/storybook.html"; then
  echo "storybook.html is missing the Storybook redirect copy" >&2
  exit 1
fi

if ! grep -q 'demo-404-recovery.js' "$output_dir/404.html"; then
  echo "404.html is missing the demo recovery shim" >&2
  exit 1
fi

if [[ ! -f "$output_dir/demo/mockServiceWorker.js" ]]; then
  echo "assembled site is missing demo/mockServiceWorker.js" >&2
  exit 1
fi
