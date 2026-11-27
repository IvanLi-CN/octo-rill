#!/usr/bin/env python3
"""Exercise real PG HTTP reads. Failures and untested parameterized routes stay visible."""
import argparse
import json
import pathlib
import re
import urllib.error
import urllib.request


def route_query_parameters(route, values):
    """Required query parameters that a bare path cannot express."""
    from urllib.parse import quote
    if route.endswith('/attempt-events'):
        return {'request_id': values['request_id']} if values.get('request_id') else {}
    if route.endswith('/search'):
        return {'q': 'release'}
    if route.endswith('/release-freshness'):
        return {'decision': 'fetch', 'reason': 'validation_probe'}
    if route.endswith('/releases/content'):
        release_id = values.get('release_id')
        return {'release_ids': quote(release_id, safe=''), 'content': 'translated'} if release_id else {}
    if route.endswith('/admin/jobs/ai-records/{record_kind}') or route.endswith('/ai-records/{record_kind}/activity'):
        return {}
    return {}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base-url', required=True)
    p.add_argument('--cookie-file', type=pathlib.Path)
    p.add_argument('--routes-source', type=pathlib.Path, default=pathlib.Path('src/postgres_server.rs'))
    p.add_argument('--parameters', type=pathlib.Path, help='private JSON mapping route parameter names to fixture IDs')
    p.add_argument('--placeholder-reads', type=pathlib.Path, help='JSON list of known read paths among any() placeholders')
    p.add_argument('--output', type=pathlib.Path, required=True)
    p.add_argument(
        '--allow-unrunnable',
        action='store_true',
        help='accept not_run cases (auth routes, absent fixtures) as non-fatal in CI',
    )
    args = p.parse_args()
    cookie = args.cookie_file.read_text().strip() if args.cookie_file else ''
    values = json.loads(args.parameters.read_text()) if args.parameters else {}
    source = args.routes_source.read_text()
    routes = re.findall(r'\.route\(\s*"([^"]+)"\s*,\s*get\(', source)
    placeholders = re.findall(r'\.route\(\s*"([^"]+)"\s*,\s*any\(postgres_capability_unavailable\)', source)
    # any() placeholders must stay failures; absence of a handler is not passing coverage.
    placeholder_reads = json.loads(args.placeholder_reads.read_text()) if args.placeholder_reads else []
    routes = sorted(set(routes + [r for r in placeholders if r in placeholder_reads]))
    uncovered = [r for r in placeholders if r not in placeholder_reads]
    excluded = ('/auth/', '/api/auth/', '/api/webhooks/')
    results = []
    for route in routes:
        if route.startswith(excluded):
            results.append({'route': route, 'result': 'not_run', 'reason': 'auth flow or webhook requires separate validation'})
            continue
        route_values = dict(values)
        # Release freshness is only valid for access-refresh tasks, so that
        # route gets the dedicated task fixture instead of the user's latest one.
        if route.endswith('/release-freshness') and values.get('freshness_task_id'):
            route_values['task_id'] = values['freshness_task_id']
        # A task log only exists for tasks that wrote one.
        if route.endswith('/log') and values.get('log_task_id'):
            route_values['task_id'] = values['log_task_id']
        names = re.findall(r'\{([^}]+)\}', route)
        if any(n not in route_values for n in names):
            results.append({'route': route, 'result': 'not_run', 'reason': 'missing fixture identifiers'})
            continue
        path = route
        for name in names:
            from urllib.parse import quote
            path = path.replace('{'+name+'}', quote(str(route_values[name]), safe=''))
        query_parameters = route_query_parameters(route, route_values)
        if query_parameters:
            from urllib.parse import urlencode
            path += '?' + urlencode(query_parameters)
        request = urllib.request.Request(args.base_url.rstrip('/') + path, headers={'Cookie': cookie})
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                status = response.status
                content_type = response.headers.get('Content-Type', '')
                raw = response.read(1) if 'text/event-stream' in content_type else response.read(32 * 1024 * 1024)
        except urllib.error.HTTPError as exc:
            status = exc.code
            content_type = exc.headers.get('Content-Type', '')
            raw = exc.read(65536)
        except Exception as exc:
            results.append({'route': route, 'result': 'fail', 'reason': type(exc).__name__})
            continue
        result = {'route': route, 'status': status}
        if status >= 500:
            result['result'] = 'fail'
        elif status in (401, 403) and cookie:
            result.update(result='fail', reason='authenticated request rejected')
        elif status in (401, 403) and not cookie:
            result.update(result='pass', contract='anonymous access denied')
        elif status >= 400:
            result.update(result='fail', reason='fixture read failed')
        elif 'json' in content_type:
            try:
                body = json.loads(raw)
                result.update(result='pass', shape=type(body).__name__)
                if isinstance(body, dict):
                    result['keys'] = sorted(body.keys())
                    if body.get('code') in ('postgres_capability_unavailable', 'database_error'):
                        result.update(result='fail', reason='error body')
            except Exception:
                result.update(result='fail', reason='invalid JSON')
        elif 'text/event-stream' in content_type:
            result.update(result='pass', contract='SSE response')
        else:
            result.update(result='fail', reason='unexpected response content type')
        results.append(result)
    report = {'cases': results, 'passed': sum(r['result'] == 'pass' for r in results),
              'failed': sum(r['result'] == 'fail' for r in results),
              'not_run': sum(r['result'] == 'not_run' for r in results),
              'unclassified_placeholder_routes': uncovered}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k not in ('cases', 'unclassified_placeholder_routes')}))
    # Real failures always fail. `not_run` is fatal only when the caller can
    # supply every fixture; CI keeps the auth routes and private-fixture routes
    # visible in the report without letting them mask a green read path.
    if report['failed']:
        return 1
    if report['not_run'] and not args.allow_unrunnable:
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
