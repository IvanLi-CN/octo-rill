#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Verify that production rolled out the expected OctoRill version.")
    parser.add_argument("--url", required=True, help="Health endpoint URL to query.")
    parser.add_argument("--expected-version", required=True, help="Expected version string, e.g. 2.37.5.")
    parser.add_argument("--timeout-seconds", type=int, default=900, help="Maximum total wait time in seconds.")
    parser.add_argument("--poll-interval", type=int, default=15, help="Seconds between attempts.")
    parser.add_argument("--request-timeout", type=int, default=15, help="Per-request timeout in seconds.")
    return parser.parse_args()


def fetch_version(url: str, timeout: int) -> tuple[int | None, str]:
    request = urllib.request.Request(url, headers={"Accept": "application/json"}, method="GET")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        status = getattr(response, "status", None)
        body = response.read().decode("utf-8")
        return status, body


def main() -> int:
    args = parse_args()
    deadline = time.monotonic() + args.timeout_seconds
    attempt = 0
    last_error = "no attempts"

    while True:
        attempt += 1
        try:
            status, body = fetch_version(args.url, args.request_timeout)
            payload = json.loads(body)
            version = payload.get("version")
            if not isinstance(version, str):
                raise RuntimeError(f"missing string version field in payload: {body}")
            print(
                f"verify_release_rollout: attempt={attempt} status={status} version={version} "
                f"expected={args.expected_version}"
            )
            if version == args.expected_version:
                return 0
            last_error = f"version_mismatch(actual={version}, expected={args.expected_version})"
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", errors="replace")
            last_error = f"http_{exc.code}: {body}"
            print(f"verify_release_rollout: attempt={attempt} {last_error}", file=sys.stderr)
        except urllib.error.URLError as exc:
            last_error = f"url_error: {exc.reason}"
            print(f"verify_release_rollout: attempt={attempt} {last_error}", file=sys.stderr)
        except Exception as exc:  # pragma: no cover - CLI guard
            last_error = str(exc)
            print(f"verify_release_rollout: attempt={attempt} error: {exc}", file=sys.stderr)

        if time.monotonic() >= deadline:
            break
        time.sleep(args.poll_interval)

    print(
        "verify_release_rollout: production did not reach the expected version within the timeout; "
        f"last_error={last_error}",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
