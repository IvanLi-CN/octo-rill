#!/usr/bin/env python3
"""Prove a fresh PostgreSQL database no longer needs a SQLite snapshot import.

Creates a throwaway database, bootstraps it with OCTORILL_PG_BOOTSTRAP_ONLY=1
(no import step), asserts that migration 0015 created every table and view it
declares, seeds one user with a visible repository, a release and a
dashboard-visible activity row, and reads the authenticated surfaces that depend
on the translated legacy schema.
"""
import argparse
import base64
import datetime
import json
import os
import pathlib
import re
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit, urlunsplit

from pg_seed_common import psql


MIGRATION = (
    pathlib.Path(__file__).resolve().parent.parent
    / 'migrations_pg'
    / '0015_application_tables.sql'
)


def migration_objects():
    """Table and view names 0015 promises to create, in file order."""
    text = MIGRATION.read_text()
    tables = re.findall(r'^CREATE TABLE IF NOT EXISTS public\.(\w+) \(', text, re.M)
    views = re.findall(r'^CREATE OR REPLACE VIEW public\.(\w+) AS', text, re.M)
    return tables, views


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3024)
    args = parser.parse_args()
    name = 'octo_rill_fresh_schema_seed_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))

    def sql(query, db=args.admin_url):
        return psql(db, query)

    tables, views = migration_objects()
    assert len(tables) >= 50, len(tables)
    assert views, '0015 must create the read views'

    sql('CREATE DATABASE ' + name)
    binary = (args.binary_dir / 'octo-rill').resolve()
    helper = (args.binary_dir / 'octo-rill-validation-session').resolve()
    env = dict(
        os.environ,
        DATABASE_URL=url,
        OCTORILL_ENCRYPTION_KEY_BASE64=base64.b64encode(bytes(32)).decode(),
        GITHUB_CLIENT_ID='validation',
        GITHUB_CLIENT_SECRET='validation',
        GITHUB_OAUTH_REDIRECT_URL=f'http://127.0.0.1:{args.port}/auth/github/callback',
        OCTORILL_BIND_ADDR=f'127.0.0.1:{args.port}',
        OCTORILL_PG_WORKER_ENABLED='0', OCTORILL_PG_SCHEDULER_ENABLED='0',
        OCTORILL_PG_BOOTSTRAP_ONLY='1',
        APP_DEFAULT_TIME_ZONE='UTC',
    )
    env.pop('AI_MODEL', None)
    env.pop('AI_API_KEY', None)
    process = None
    try:
        # 1. Runtime baseline plus the translated application schema, no import.
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)

        # 2. Every object the migration declares must exist in public. This is
        #    the whole point of the suite: no SQLite snapshot was imported.
        existing_tables = set(
            sql("SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'", url).splitlines()
        )
        existing_views = set(
            sql("SELECT viewname FROM pg_catalog.pg_views WHERE schemaname = 'public'", url).splitlines()
        )
        missing_tables = [table for table in tables if table not in existing_tables]
        missing_views = [view for view in views if view not in existing_views]
        assert not missing_tables, missing_tables
        assert not missing_views, missing_views
        for table in ('repo_releases', 'user_repo_associations', 'social_activity_events'):
            assert table in existing_tables, table
        assert 'user_release_visible_repos' in existing_views

        # 3. Serve the application against the freshly built schema.
        env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
        with tempfile.TemporaryDirectory(prefix='octo-rill-fresh-schema-test-') as tmp:
            tmp = pathlib.Path(tmp)
            log = (tmp / 'server.log').open('w+')
            process = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
            base = f'http://127.0.0.1:{args.port}'
            for _ in range(150):
                try:
                    urllib.request.urlopen(base + '/health', timeout=1).close()
                    break
                except Exception:
                    if process.poll() is not None:
                        log.seek(0)
                        raise RuntimeError(log.read()[-8000:])
                    time.sleep(.1)
            else:
                raise RuntimeError('fresh-schema server did not become ready')

            now = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
            sql(
                """
                INSERT INTO public.users
                  (id, github_user_id, login, name, created_at, updated_at, is_admin,
                   is_disabled, daily_brief_time_zone)
                VALUES
                  ('fresh-user', 4242, 'fresh-user', 'Fresh User', '{now}', '{now}', 1, 0, 'UTC');

                INSERT INTO public.github_connections
                  (id, user_id, github_user_id, login, access_token_ciphertext,
                   access_token_nonce, scopes, linked_at, updated_at)
                VALUES
                  ('fresh-conn', 'fresh-user', 4242, 'fresh-user', '\\x00'::bytea,
                   '\\x00'::bytea, 'repo', '{now}', '{now}');

                INSERT INTO public.user_repo_associations
                  (id, user_id, repo_id, repo_full_name, repo_full_name_lower, owner_login,
                   repo_name, html_url, is_private, owner_avatar_url, open_graph_image_url,
                   uses_custom_open_graph_image, first_source, first_associated_at,
                   last_seen_at, is_following, follow_state_source, has_personal_owned_source,
                   has_github_star_source, has_manual_feed_source, created_at, updated_at)
                VALUES
                  ('fresh-assoc', 'fresh-user', 424242, 'fresh/rocket', 'fresh/rocket', 'fresh',
                   'rocket', 'https://github.com/fresh/rocket', 0,
                   'https://avatars.example/fresh', NULL, 0, 'manual_feed', '{now}',
                   '{now}', 1, 'user_explicit', 0, 0, 1, '{now}', '{now}');

                INSERT INTO public.repo_releases
                  (id, repo_id, release_id, node_id, tag_name, name, body, html_url,
                   published_at, created_at, is_prerelease, is_draft, updated_at,
                   react_plus1, react_laugh, react_heart, react_hooray, react_rocket, react_eyes)
                VALUES
                  ('fresh-release', 424242, 7001, 'node-fresh-1', 'v1.0.0', 'Fresh release',
                   'First release on a database that was never imported.',
                   'https://github.com/fresh/rocket/releases/tag/v1.0.0', '{now}', '{now}',
                   0, 0, '{now}', 1, 0, 0, 0, 0, 0);

                INSERT INTO public.social_activity_events
                  (id, user_id, kind, repo_id, repo_full_name, repo_owner_avatar_url,
                   repo_open_graph_image_url, repo_uses_custom_open_graph_image, title, body,
                   html_url, github_event_id, actor_github_user_id, actor_login,
                   actor_avatar_url, actor_html_url, occurred_at, detected_at, created_at,
                   updated_at, discussion_number)
                VALUES
                  ('fresh-activity', 'fresh-user', 'repo_star_received', 424242, 'fresh/rocket',
                   'https://avatars.example/fresh', NULL, 0, NULL, NULL,
                   'https://github.com/fresh/rocket', NULL, 900, 'stargazer',
                   'https://avatars.example/stargazer', 'https://github.com/stargazer',
                   '{now}', '{now}', '{now}', '{now}', NULL);
                """.format(now=now),
                url,
            )

            cookie_file = tmp / 'session'
            subprocess.run(
                [
                    helper,
                    '--postgres-url',
                    url,
                    '--user-id',
                    'fresh-user',
                    '--cookie-file',
                    cookie_file,
                    '--isolated-validation-copy',
                ],
                check=True,
                stdout=subprocess.DEVNULL,
            )
            cookie = cookie_file.read_text().strip()

            def request(path):
                req = urllib.request.Request(
                    base + path, headers={'Cookie': cookie}, method='GET'
                )
                try:
                    with urllib.request.urlopen(req, timeout=20) as response:
                        raw = response.read()
                        return response.status, (json.loads(raw) if raw else None)
                except urllib.error.HTTPError as error:
                    raw = error.read()
                    if error.code >= 500:
                        print('--- server log tail ---', flush=True)
                        print((tmp / 'server.log').read_text()[-4000:], flush=True)
                    return error.code, (json.loads(raw) if raw else None)

            checks = {}

            code, body = request('/health')
            assert code == 200, (code, body)
            assert body and body.get('ok') is True, body
            checks['health'] = 'pass'

            code, body = request('/api/me')
            assert code == 200, (code, body)
            assert body['user']['id'] == 'fresh-user', body
            assert body['user']['is_admin'] is True, body
            checks['me'] = 'pass'

            code, body = request('/api/feed')
            assert code == 200, (code, body)
            assert body['items'], body
            assert any(item['id'] == '7001' for item in body['items']), body
            checks['feed'] = 'pass'

            code, body = request('/api/dashboard/feed')
            assert code == 200, (code, body)
            assert body['sections'], body
            section_ids = {
                item['id'] for section in body['sections'] for item in section['items']
            }
            assert '7001' in section_ids or 'fresh-activity' in section_ids, body['sections']
            checks['dashboard_feed'] = 'pass'

            code, body = request('/api/releases/7001/detail')
            assert code == 200, (code, body)
            assert body['release_id'] == '7001', body
            assert body['repo_full_name'] == 'fresh/rocket', body
            assert body['tag_name'] == 'v1.0.0', body
            checks['release_detail'] = 'pass'

            code, body = request('/api/admin/jobs/overview')
            assert code == 200, (code, body)
            assert set(body) == {
                'queued',
                'running',
                'failed_24h',
                'succeeded_24h',
                'enabled_scheduled_slots',
                'total_scheduled_slots',
            }, body
            assert all(isinstance(value, int) for value in body.values()), body
            assert body['enabled_scheduled_slots'] == 24, body
            assert body['total_scheduled_slots'] == 24, body
            checks['admin_jobs_overview'] = 'pass'

            # The imported shape carries the translation retry columns; a fresh
            # database must expose them too or the AI-records reads 500.
            columns = set(sql(
                "SELECT column_name FROM information_schema.columns"
                " WHERE table_schema = 'public' AND table_name = 'translation_work_items'",
                url,
            ).splitlines())
            for column in ('attempt_count', 'retry_disposition', 'next_attempt_trigger',
                           'error_code', 'failure_class', 'provider_status'):
                assert column in columns, (column, sorted(columns))
            checks['translation_work_columns'] = 'pass'

            code, body = request('/api/admin/jobs/ai-records/release')
            assert code == 200, (code, body)
            checks['admin_ai_records'] = 'pass'

            # The daily-brief executor upserts on (user_id, date); the fresh
            # schema must carry the matching arbiter.
            brief_upsert = (
                "INSERT INTO public.briefs (id, user_id, date, window_start_utc,"
                " window_end_utc, effective_time_zone, effective_local_boundary,"
                " generation_source, content_markdown, created_at, updated_at)"
                " VALUES ('{brief_id}', 'fresh-user', '2026-11-20', '2026-11-20T00:00:00Z',"
                " '2026-11-21T00:00:00Z', 'UTC', '00:00', 'postgres_deterministic',"
                " '{content}', '{now}', '{now}')"
                " ON CONFLICT (user_id, date) DO UPDATE SET"
                " content_markdown = EXCLUDED.content_markdown,"
                " generation_source = EXCLUDED.generation_source, updated_at = EXCLUDED.updated_at"
            )
            sql(brief_upsert.format(brief_id='fresh-user:2026-11-20', content='first', now=now), url)
            sql(brief_upsert.format(brief_id='fresh-user:2026-11-20', content='second', now=now), url)
            briefs = sql(
                "SELECT count(*) || '|' || max(content_markdown) FROM public.briefs"
                " WHERE user_id = 'fresh-user' AND date = '2026-11-20'",
                url,
            )
            assert briefs == '1|second', briefs
            checks['brief_upsert_arbiter'] = 'pass'

            print(json.dumps(checks))
    finally:
        if process is not None:
            process.send_signal(2)
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
        sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()
