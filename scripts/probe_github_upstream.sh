#!/usr/bin/env bash
set -euo pipefail

URL="${1:-https://api.github.com/repos/syncthing/syncthing/releases?per_page=20&page=1}"
ATTEMPTS="${ATTEMPTS:-5}"
CONNECT_TIMEOUT="${CONNECT_TIMEOUT:-10}"
MAX_TIME="${MAX_TIME:-35}"
SLEEP_SECONDS="${SLEEP_SECONDS:-1}"
USER_AGENT="${USER_AGENT:-OctoRill-Probe}"
FAIL_ON_ERROR="${FAIL_ON_ERROR:-1}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

failures=0

for ((attempt=1; attempt<=ATTEMPTS; attempt++)); do
  body_path="$tmpdir/body-${attempt}.bin"
  stderr_path="$tmpdir/stderr-${attempt}.log"
  rc=0
  metrics="$(
    curl -L -sS \
      -o "$body_path" \
      -w "%{http_code}\t%{time_namelookup}\t%{time_connect}\t%{time_appconnect}\t%{time_starttransfer}\t%{time_total}\t%{size_download}" \
      --connect-timeout "$CONNECT_TIMEOUT" \
      --max-time "$MAX_TIME" \
      -H "User-Agent: ${USER_AGENT}" \
      "$URL" \
      2>"$stderr_path"
  )" || rc=$?

  IFS=$'\t' read -r http_code time_namelookup time_connect time_appconnect time_starttransfer time_total size_download <<<"$metrics"
  stderr_text="$(tr '\n' ' ' <"$stderr_path" | sed 's/[[:space:]]\+/ /g; s/^ //; s/ $//')"
  printf \
    'attempt=%s rc=%s http_code=%s dns_s=%s connect_s=%s tls_s=%s ttfb_s=%s total_s=%s downloaded_bytes=%s url=%q stderr=%q\n' \
    "$attempt" \
    "$rc" \
    "${http_code:-0}" \
    "${time_namelookup:-0}" \
    "${time_connect:-0}" \
    "${time_appconnect:-0}" \
    "${time_starttransfer:-0}" \
    "${time_total:-0}" \
    "${size_download:-0}" \
    "$URL" \
    "$stderr_text"

  if [[ "$rc" -ne 0 ]]; then
    failures=$((failures + 1))
  fi

  if [[ "$attempt" -lt "$ATTEMPTS" ]]; then
    sleep "$SLEEP_SECONDS"
  fi
done

if [[ "$FAIL_ON_ERROR" != "0" && "$failures" -gt 0 ]]; then
  exit 1
fi
