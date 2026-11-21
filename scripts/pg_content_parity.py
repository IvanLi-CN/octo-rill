#!/usr/bin/env python3
"""Sample global content-processing projections and check the API serves them.

Production runs `content_processing_control.mode='global'`, so a release whose
translation exists only in `content_result_projections` must still come back
translated. The probe therefore samples releases that own a global projection
and asserts the release-detail endpoint does not report them as missing.

The service under test must be started with an AI model configured
(`AI_BASE_URL`/`AI_API_KEY`/`AI_MODEL`); legacy hides translations entirely when
AI is disabled, which would make every case look missing.
"""
import argparse
import json
import subprocess
import urllib.error
import urllib.request


def sql(db, query):
    result = subprocess.run(
        ['psql', db, '-XAt', '-v', 'ON_ERROR_STOP=1', '-c', query],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip())
    return result.stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-url', required=True)
    parser.add_argument('--cookie-file', required=True)
    parser.add_argument('--postgres-url', required=True)
    parser.add_argument('--sample', type=int, default=30)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()

    cookie = open(args.cookie_file, encoding='utf-8').read().strip()
    user_id = sql(
        args.postgres_url,
        "SELECT id FROM public.users WHERE is_admin = 1 ORDER BY created_at LIMIT 1",
    )
    mode = sql(
        args.postgres_url,
        "SELECT mode FROM public.content_processing_control WHERE id = 1",
    )
    release_ids = sql(
        args.postgres_url,
        "SELECT DISTINCT r.release_id "
        "FROM public.content_result_projections p "
        "JOIN public.repo_releases r ON r.release_id::text = p.canonical_resource_id "
        "JOIN public.user_release_visible_repos v ON v.repo_id = r.repo_id "
        f"WHERE p.canonical_resource_type = 'release' AND p.pipeline = 'translation' "
        f"AND v.user_id = '{user_id}' "
        f"ORDER BY r.release_id DESC LIMIT {int(args.sample)}",
    ).splitlines()
    release_ids = [value for value in release_ids if value]

    cases = []
    for release_id in release_ids:
        request = urllib.request.Request(
            f"{args.base_url.rstrip('/')}/api/releases/{release_id}/detail",
            headers={'Cookie': cookie},
        )
        status = None
        body = {}
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                status = response.status
                body = json.loads(response.read())
        except urllib.error.HTTPError as error:
            status = error.code
            try:
                body = json.loads(error.read())
            except Exception:
                body = {}
        translated = body.get('translated') if isinstance(body, dict) else None
        translated_status = (translated or {}).get('status')
        cases.append(
            {
                'release_id': release_id,
                'http_status': status,
                'translated_status': translated_status,
                'has_title': bool((translated or {}).get('title')),
                'result': 'pass'
                if status == 200 and translated_status in ('ready', 'stale')
                else 'fail',
            }
        )

    passed = sum(case['result'] == 'pass' for case in cases)
    report = {
        'mode': mode,
        'sample': len(cases),
        'passed': passed,
        'failed': len(cases) - passed,
        'distinct_translated_status': sorted(
            {(case['translated_status'] or 'absent') for case in cases}
        ),
        'cases': cases,
    }
    with open(args.output, 'w', encoding='utf-8') as handle:
        json.dump(report, handle, indent=2)
        handle.write('\n')
    print(
        json.dumps(
            {
                'mode': mode,
                'sample': len(cases),
                'passed': passed,
                'failed': len(cases) - passed,
                'statuses': report['distinct_translated_status'],
            }
        )
    )
    if cases and passed == 0 and set(report['distinct_translated_status']) == {'disabled'}:
        print('hint: start the service with AI_BASE_URL/AI_API_KEY/AI_MODEL configured')
    return 1 if cases and passed != len(cases) else 0


if __name__ == '__main__':
    raise SystemExit(main())
