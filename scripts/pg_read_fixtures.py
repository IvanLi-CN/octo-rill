#!/usr/bin/env python3
"""Derive read-acceptance route parameters from an imported PostgreSQL copy.

Values are opaque identifiers taken from the database itself, so the probe
exercises real production rows instead of synthetic fixtures. Output is meant
for ``pg_read_acceptance.py --parameters``; missing identifiers are reported so
unexercised routes stay visible as ``not_run``.
"""
import argparse
import json
import subprocess
import urllib.request


QUERIES = {
    'owner': "WITH latest AS (SELECT r.repo_id FROM public.repo_releases r JOIN public.user_release_visible_repos v ON v.repo_id = r.repo_id WHERE v.user_id = {user_id} AND r.is_draft = 0 ORDER BY r.release_id DESC LIMIT 1) SELECT split_part(v.full_name, '/', 1) FROM public.user_release_visible_repos v JOIN latest ON latest.repo_id = v.repo_id WHERE v.user_id = {user_id} LIMIT 1",
    'repo': "WITH latest AS (SELECT r.repo_id FROM public.repo_releases r JOIN public.user_release_visible_repos v ON v.repo_id = r.repo_id WHERE v.user_id = {user_id} AND r.is_draft = 0 ORDER BY r.release_id DESC LIMIT 1) SELECT split_part(v.full_name, '/', 2) FROM public.user_release_visible_repos v JOIN latest ON latest.repo_id = v.repo_id WHERE v.user_id = {user_id} LIMIT 1",
    'tag': "SELECT r.tag_name FROM public.repo_releases r JOIN public.user_release_visible_repos v ON v.repo_id = r.repo_id WHERE v.user_id = {user_id} AND r.tag_name <> '' ORDER BY r.release_id DESC LIMIT 1",
    'number': "SELECT e.discussion_number FROM public.social_activity_events e JOIN public.user_release_visible_repos v ON v.repo_id = e.repo_id WHERE v.user_id = {user_id} AND e.discussion_number IS NOT NULL ORDER BY e.discussion_number DESC LIMIT 1",
    'release_id': "SELECT r.release_id FROM public.repo_releases r JOIN public.user_release_visible_repos v ON v.repo_id = r.repo_id WHERE v.user_id = {user_id} AND r.is_draft = 0 ORDER BY r.release_id DESC LIMIT 1",
    'task_id': "SELECT id FROM public.job_tasks WHERE requested_by = {user_id} ORDER BY created_at DESC LIMIT 1",
    'freshness_task_id': "SELECT id FROM public.job_tasks WHERE task_type = 'sync.access_refresh' ORDER BY created_at DESC LIMIT 1",
    'log_task_id': "SELECT id FROM public.job_tasks WHERE COALESCE(log_file_path, '') <> '' ORDER BY created_at DESC LIMIT 1",
    'record_kind': "SELECT 'release'",
    'record_id': "SELECT CAST(r.release_id AS TEXT) FROM public.repo_releases r JOIN public.user_release_visible_repos v ON v.repo_id = r.repo_id WHERE v.user_id = {user_id} AND r.is_draft = 0 ORDER BY r.release_id DESC LIMIT 1",
    'call_id': "SELECT id FROM public.llm_calls ORDER BY created_at DESC LIMIT 1",
    'migration_id': "SELECT migration_id FROM public.online_migration_operations ORDER BY operation_order LIMIT 1",
    'usage_id': "SELECT id FROM public.public_repo_release_usage ORDER BY last_requested_at DESC NULLS LAST, id LIMIT 1",
    'request_id': "SELECT id FROM public.translation_requests ORDER BY created_at DESC LIMIT 1",
    'batch_id': "SELECT id FROM public.translation_batches ORDER BY created_at DESC LIMIT 1",
    'brief_id': "SELECT id FROM public.briefs ORDER BY date DESC LIMIT 1",
    'section_id': "SELECT date FROM public.briefs ORDER BY date DESC LIMIT 1",
    'user_id': "SELECT id FROM public.users ORDER BY created_at LIMIT 1",
    'api_key_id': "SELECT id FROM public.user_api_keys LIMIT 1",
    'passkey_id': "SELECT id FROM public.user_passkeys LIMIT 1",
    'connection_id': "SELECT id FROM public.github_connections LIMIT 1",
    'hour_utc': "SELECT hour_utc::text FROM public.daily_brief_hour_slots ORDER BY hour_utc LIMIT 1",
}


def discover_http_fixtures(base_url, cookie_file):
    """Section ids are user-scoped cursors, so they can only come from the service."""
    if not cookie_file:
        return {}
    found = {}
    request = urllib.request.Request(
        base_url.rstrip('/') + '/api/dashboard/feed',
        headers={'Cookie': cookie_file.read().strip()},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            body = json.loads(response.read())
    except Exception:
        return {}
    sections = body.get('sections') or []
    for section in sections:
        if section.get('id'):
            found['section_id'] = section['id']
            break
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--postgres-url', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--user-id', help='session user the probe runs as; scopes task/release fixtures')
    parser.add_argument('--base-url', help='running PostgreSQL service used to discover dashboard section ids')
    parser.add_argument('--cookie-file', type=argparse.FileType('r'))
    args = parser.parse_args()
    values = {}
    missing = []
    for name, query in QUERIES.items():
        if '{user_id}' in query:
            if not args.user_id:
                missing.append({'parameter': name, 'error': ['--user-id required']})
                continue
            query = query.replace('{user_id}', "'" + args.user_id.replace("'", "''") + "'")
        result = subprocess.run(
            ['psql', args.postgres_url, '-XAt', '-v', 'ON_ERROR_STOP=1', '-c', query],
            capture_output=True,
            text=True,
        )
        value = result.stdout.strip()
        if result.returncode != 0 or not value:
            missing.append({'parameter': name, 'error': result.stderr.strip().splitlines()[:1]})
            continue
        values[name] = value
    if args.base_url:
        values.update(discover_http_fixtures(args.base_url, args.cookie_file))
    with open(args.output, 'w', encoding='utf-8') as handle:
        json.dump(values, handle, indent=2, sort_keys=True)
        handle.write('\n')
    print(json.dumps({'parameters': len(values), 'missing': missing}))


if __name__ == '__main__':
    raise SystemExit(main())
