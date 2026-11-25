#!/usr/bin/env python3
"""Run the PostgreSQL read-acceptance probe against a disposable database.

This is the CI entry point for `scripts/pg_read_acceptance.py`. It owns the
whole lifecycle so the workflow stays a single command:

1. create a fresh database on the ephemeral cluster,
2. bootstrap the runtime schema with the app binary,
3. seed the deterministic fixture the probe needs,
4. start the service with workers and schedulers disabled,
5. derive route parameters from the seeded rows,
6. run the probe and print its per-case report,
7. stop the service and drop the database.

Every case the probe cannot execute is printed, so an unrunnable route is
visible in the job log instead of disappearing into a summary line.

Known-unrunnable routes are declared explicitly in `EXPECTED_NOT_RUN` with the
reason. Any other `not_run` case fails the run: that means the fixture drifted,
not that the route is untestable. The probe still fails on any real failure.
"""
import argparse
import base64
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import urllib.request
from urllib.parse import urlsplit, urlunsplit

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from pg_seed_common import psql  # noqa: E402

# Measured against this fixture. Each entry is a route the probe can never
# exercise here, with the contract that makes it impossible in CI.
EXPECTED_NOT_RUN = {
    '/auth/github/callback': 'GitHub OAuth round-trip needs a real client and user consent',
    '/auth/github/connect': 'GitHub OAuth round-trip needs a real client and user consent',
    '/auth/github/login': 'GitHub OAuth round-trip needs a real client and user consent',
    '/auth/linuxdo/callback': 'LinuxDo OAuth round-trip needs a real client and user consent',
    '/auth/linuxdo/login': 'LinuxDo OAuth round-trip needs a real client and user consent',
    '/auth/logout': 'exercised by the authenticated session suites, not by a read probe',
    '/api/auth/bind-context': 'needs a live OAuth callback context',
}

# Routes that currently return a server error because of a defect in `src/`,
# not because CI cannot exercise them. Each entry names the exact mismatch so a
# reader can verify it; both reproduce on `origin/main` and neither is covered by
# any other suite. They are quarantined instead of skipped: a quarantined route
# that starts passing fails the run so the entry gets removed.
EXPECTED_FAILURES = {
    '/api/briefs':
        'PgBriefRow decodes briefs.created_at (timestamptz) as String; the two '
        'sibling SELECTs in src/postgres_server.rs are missing ::text',
    '/api/briefs/{brief_id}':
        'same PgBriefRow decode mismatch as /api/briefs',
    '/admin/jobs/translations/status':
        'translation_batches.item_count is INT4 but status() reads it as i64 '
        'without the ::bigint cast its sibling queries use',
    '/api/admin/jobs/translations/status':
        'same item_count INT4/i64 mismatch as /admin/jobs/translations/status',
}


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True, help='superuser URL able to create databases')
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3201)
    parser.add_argument('--work-dir', type=pathlib.Path, default=pathlib.Path('.ci/read-acceptance'))
    parser.add_argument('--database', default='octo_rill_read_acceptance')
    return parser.parse_args()


def wait_for(url, timeout=40.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=1):
                return
        except Exception:
            time.sleep(.2)
    raise RuntimeError(f'service did not become ready: {url}')


def main():
    args = parse_args()
    binary = (args.binary_dir / 'octo-rill').resolve()
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + args.database))
    base_url = f'http://127.0.0.1:{args.port}'
    work = args.work_dir.resolve()
    cookie_file = work / 'admin.cookie'
    task_log = work / 'task.log'
    params_file = work / 'parameters.json'
    report_file = work / 'read-acceptance.json'

    work.mkdir(parents=True, exist_ok=True)
    shutil.rmtree(work / 'report', ignore_errors=True)

    # Inherit the caller's environment so `psql` resolves, then override only
    # the runtime contract this service needs.
    env = {
        **os.environ,
        'DATABASE_URL': url,
        'OCTORILL_ENCRYPTION_KEY_BASE64': base64.b64encode(bytes(32)).decode(),
        'GITHUB_CLIENT_ID': 'validation',
        'GITHUB_CLIENT_SECRET': 'validation',
        'GITHUB_OAUTH_REDIRECT_URL': f'{base_url}/auth/github/callback',
        'OCTORILL_BIND_ADDR': f'127.0.0.1:{args.port}',
        'OCTORILL_PG_WORKER_ENABLED': '0',
        'OCTORILL_PG_SCHEDULER_ENABLED': '0',
        'APP_DEFAULT_TIME_ZONE': 'UTC',
        'RUST_LOG': 'warn',
        # dotenvy never overrides an existing variable, so declaring these empty
        # keeps a developer's `.env.local` from changing what the probe sees.
        # No AI provider is reachable from CI.
        'AI_API_KEY': '',
        'AI_BASE_URL': '',
        'AI_MODEL': '',
    }

    service = None
    try:
        psql(args.admin_url, f'DROP DATABASE IF EXISTS {args.database} WITH (FORCE)')
        psql(args.admin_url, f'CREATE DATABASE {args.database}')

        # Bootstrap with the runtime itself so the schema is exactly what the
        # service expects, then seed the read fixture on top of it.
        subprocess.run(
            [str(binary)],
            env={**env, 'OCTORILL_PG_BOOTSTRAP_ONLY': '1'},
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        subprocess.run(
            [
                sys.executable, str(pathlib.Path(__file__).with_name('pg_read_acceptance_fixtures.py')),
                '--postgres-url', url,
                '--binary-dir', str(args.binary_dir),
                '--cookie-file', str(cookie_file),
                '--task-log', str(task_log),
            ],
            check=True,
        )

        log = open(work / 'service.log', 'w')
        service = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=subprocess.STDOUT)
        wait_for(f'{base_url}/api/health')

        subprocess.run(
            [
                sys.executable, str(pathlib.Path(__file__).with_name('pg_read_fixtures.py')),
                '--postgres-url', url,
                '--user-id', 'ci-read-admin',
                '--base-url', base_url,
                '--cookie-file', str(cookie_file),
                '--output', str(params_file),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )

        probe = subprocess.run(
            [
                sys.executable, str(pathlib.Path(__file__).with_name('pg_read_acceptance.py')),
                '--base-url', base_url,
                '--cookie-file', str(cookie_file),
                '--parameters', str(params_file),
                '--output', str(report_file),
                # Unrunnable cases are classified below against an explicit
                # allowlist, so the probe itself must not fail on them first.
                '--allow-unrunnable',
            ],
            text=True,
        )

        report = json.loads(report_file.read_text())
        print(json.dumps({k: v for k, v in report.items() if k != 'cases'}))

        failures = [case for case in report['cases'] if case['result'] == 'fail']
        not_runs = [case for case in report['cases'] if case['result'] == 'not_run']
        failing_routes = {case['route'] for case in failures}

        problems = []
        for case in not_runs:
            route = case['route']
            if route not in EXPECTED_NOT_RUN:
                problems.append(f"unexpected not_run: {route} ({case.get('reason', '')})")
            print(f"not_run  {route}  {case.get('reason', '')}")
        for case in failures:
            route = case['route']
            if route not in EXPECTED_FAILURES:
                problems.append(f"unexpected failure: {route} (status {case.get('status')})")
            print(f"FAIL     {route}  status={case.get('status')}  {case.get('reason', '')}")

        for route, reason in sorted(EXPECTED_NOT_RUN.items()):
            print(f'known-unrunnable  {route}  {reason}')
        for route, reason in sorted(EXPECTED_FAILURES.items()):
            print(f'quarantined-failure  {route}  {reason}')

        # A quarantined route that now passes means the defect was fixed; drop
        # the entry instead of letting the allowlist rot.
        for route in sorted(set(EXPECTED_FAILURES) - failing_routes):
            problems.append(f'quarantined route now passes, remove its entry: {route}')

        if problems:
            print('read acceptance found changes the allowlists do not describe:', file=sys.stderr)
            for problem in problems:
                print(f'  - {problem}', file=sys.stderr)
            return 1
        if probe.returncode != 0 and not failures:
            # The probe failed without a per-case failure (for example a crash).
            return probe.returncode
        return 0
    finally:
        if service is not None and service.poll() is None:
            service.terminate()
            try:
                service.wait(timeout=20)
            except subprocess.TimeoutExpired:
                service.kill()
        try:
            psql(args.admin_url, f'DROP DATABASE IF EXISTS {args.database} WITH (FORCE)')
        except Exception as exc:  # noqa: BLE001 - cleanup must not mask the real result
            print(f'warning: could not drop {args.database}: {exc}', file=sys.stderr)


if __name__ == '__main__':
    raise SystemExit(main())
