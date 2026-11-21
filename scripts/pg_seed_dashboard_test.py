#!/usr/bin/env python3
"""Validate the PostgreSQL dashboard read surface with synthetic isolated data.

Covers /api/feed, HEAD /api/feed, /api/dashboard/feed,
/api/dashboard/feed/sections/{id}/items, and /api/dashboard/updates:
signature token stability, cursor pagination, time-zone day boundaries, brief
visibility/attachment, and anonymous 401 handling.
"""
import argparse
import base64
import datetime
import json
import os
import pathlib
import subprocess

import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pg_seed_common import psql
from urllib.parse import urlsplit, urlunsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3022)
    args = parser.parse_args()
    name = 'octo_rill_dashboard_seed_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))

    def sql(query, db=args.admin_url):
        return psql(db, query)

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
        # 1. Runtime baseline (migrations_pg) without the application schema.
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)

        # 2. Application tables imported from SQLite in production; created here
        #    with the columns the dashboard surface reads.
        sql(
            """
            DROP VIEW IF EXISTS public.user_release_visible_repos CASCADE; CREATE TABLE public.user_release_visible_repos (
              user_id text NOT NULL,
              repo_id bigint NOT NULL,
              full_name text NOT NULL,
              owner_login text NOT NULL,
              name text NOT NULL,
              owner_avatar_url text,
              open_graph_image_url text,
              uses_custom_open_graph_image bigint NOT NULL DEFAULT 0,
              is_private bigint
            );
            DROP VIEW IF EXISTS public.user_following_repos CASCADE; CREATE TABLE public.user_following_repos (
              user_id text NOT NULL,
              repo_id bigint,
              full_name text NOT NULL,
              owner_login text NOT NULL,
              name text NOT NULL,
              owner_avatar_url text,
              open_graph_image_url text,
              uses_custom_open_graph_image bigint NOT NULL DEFAULT 0
            );
            DROP TABLE IF EXISTS public.starred_repos CASCADE; CREATE TABLE public.starred_repos (
              id text PRIMARY KEY,
              user_id text NOT NULL,
              repo_id bigint NOT NULL,
              full_name text NOT NULL,
              owner_login text NOT NULL,
              name text NOT NULL,
              html_url text,
              is_private bigint NOT NULL DEFAULT 0,
              updated_at text NOT NULL,
              owner_avatar_url text,
              open_graph_image_url text,
              uses_custom_open_graph_image bigint NOT NULL DEFAULT 0
            );
            DROP TABLE IF EXISTS public.owned_repo_star_baselines CASCADE; CREATE TABLE public.owned_repo_star_baselines (
              id text PRIMARY KEY,
              user_id text NOT NULL,
              repo_id bigint NOT NULL,
              repo_full_name text NOT NULL,
              initialized_at text NOT NULL,
              updated_at text NOT NULL,
              owner_avatar_url text,
              open_graph_image_url text,
              uses_custom_open_graph_image bigint NOT NULL DEFAULT 0,
              is_private bigint,
              UNIQUE(user_id, repo_id)
            );
            DROP TABLE IF EXISTS public.user_repo_associations CASCADE; CREATE TABLE public.user_repo_associations (
              id text PRIMARY KEY,
              user_id text NOT NULL,
              repo_id bigint,
              repo_full_name text NOT NULL,
              repo_full_name_lower text NOT NULL,
              owner_login text NOT NULL,
              repo_name text NOT NULL,
              html_url text,
              description text,
              is_private bigint,
              owner_avatar_url text,
              open_graph_image_url text,
              uses_custom_open_graph_image bigint NOT NULL DEFAULT 0,
              first_source text NOT NULL,
              first_associated_at text NOT NULL,
              last_seen_at text NOT NULL,
              is_following bigint NOT NULL DEFAULT 0,
              follow_state_source text NOT NULL DEFAULT 'system_default',
              has_personal_owned_source bigint NOT NULL DEFAULT 0,
              has_github_star_source bigint NOT NULL DEFAULT 0,
              has_manual_feed_source bigint NOT NULL DEFAULT 0,
              created_at text NOT NULL,
              updated_at text NOT NULL,
              UNIQUE(user_id, repo_full_name_lower)
            );
            DROP TABLE IF EXISTS public.repo_releases CASCADE; CREATE TABLE public.repo_releases (
              id text PRIMARY KEY,
              repo_id bigint NOT NULL,
              release_id bigint NOT NULL UNIQUE,
              node_id text,
              tag_name text NOT NULL,
              name text,
              body text,
              html_url text NOT NULL,
              published_at text,
              created_at text,
              is_prerelease bigint NOT NULL DEFAULT 0,
              is_draft bigint NOT NULL DEFAULT 0,
              updated_at text NOT NULL,
              react_plus1 bigint NOT NULL DEFAULT 0,
              react_laugh bigint NOT NULL DEFAULT 0,
              react_heart bigint NOT NULL DEFAULT 0,
              react_hooray bigint NOT NULL DEFAULT 0,
              react_rocket bigint NOT NULL DEFAULT 0,
              react_eyes bigint NOT NULL DEFAULT 0
            );
            DROP TABLE IF EXISTS public.social_activity_events CASCADE; CREATE TABLE public.social_activity_events (
              id text PRIMARY KEY,
              user_id text NOT NULL,
              kind text NOT NULL,
              repo_id bigint,
              repo_full_name text,
              repo_owner_avatar_url text,
              repo_open_graph_image_url text,
              repo_uses_custom_open_graph_image bigint,
              title text,
              body text,
              html_url text,
              github_event_id text,
              actor_github_user_id bigint NOT NULL,
              actor_login text NOT NULL,
              actor_avatar_url text,
              actor_html_url text,
              occurred_at text NOT NULL,
              detected_at text NOT NULL,
              created_at text NOT NULL,
              updated_at text NOT NULL,
              discussion_number bigint
            );
            DROP TABLE IF EXISTS public.notifications CASCADE; CREATE TABLE public.notifications (
              id text PRIMARY KEY,
              user_id text NOT NULL,
              thread_id text NOT NULL,
              repo_full_name text,
              subject_title text,
              subject_type text,
              reason text,
              updated_at text,
              unread bigint NOT NULL DEFAULT 1,
              url text,
              html_url text,
              last_seen_at text,
              UNIQUE(user_id, thread_id)
            );
            DROP TABLE IF EXISTS public.ai_translations CASCADE; CREATE TABLE public.ai_translations (
              id text PRIMARY KEY,
              user_id text NOT NULL,
              entity_type text NOT NULL,
              entity_id text NOT NULL,
              lang text NOT NULL,
              source_hash text NOT NULL,
              status text NOT NULL DEFAULT 'ready',
              title text,
              summary text,
              error_text text,
              active_work_item_id text,
              created_at text NOT NULL,
              updated_at text NOT NULL,
              UNIQUE(user_id, entity_type, entity_id, lang)
            );
            DROP TABLE IF EXISTS public.public_repo_release_usage CASCADE; CREATE TABLE public.public_repo_release_usage (
              id text PRIMARY KEY,
              repo_id bigint,
              owner_login text NOT NULL,
              repo_name text NOT NULL,
              full_name text NOT NULL,
              full_name_lower text NOT NULL UNIQUE,
              first_registered_at text NOT NULL,
              last_requested_at text NOT NULL,
              last_list_requested_at text,
              last_detail_requested_at text,
              api_list_requests bigint NOT NULL DEFAULT 0,
              api_detail_requests bigint NOT NULL DEFAULT 0,
              page_list_requests bigint NOT NULL DEFAULT 0,
              page_detail_requests bigint NOT NULL DEFAULT 0,
              last_sync_status text NOT NULL DEFAULT 'pending',
              last_sync_error text,
              access_kind text NOT NULL DEFAULT 'github_public',
              created_at text NOT NULL,
              updated_at text NOT NULL
            );
            """,
            url,
        )

        releases = []

        def release(repo_id, release_id, tag, published, name, body, plus1=0):
            releases.append(
                "('rel-{rid}','{repo}',{rid},'node-{rid}','{tag}','{name}','{body}',"
                "'https://github.com/acme/repo/releases/tag/{tag}','{pub}','{pub}',0,0,'{pub}',{p1},2,3,4,5,6)".format(
                    rid=release_id,
                    repo=repo_id,
                    tag=tag,
                    name=name,
                    body=body,
                    pub=published,
                    p1=plus1,
                )
            )

        # 33 releases on 2026-02-10 UTC (pagination section).
        for i in range(33):
            release(
                10,
                1001 + i,
                f'v0.0.{i}',
                f'2026-02-10T00:{i:02d}:00Z',
                f'Rocket {i}',
                f'release body {i}',
                plus1=i,
            )
        # 4 releases on 2026-02-09 UTC.
        for i in range(4):
            release(
                10,
                1101 + i,
                f'v0.9.{i}',
                f'2026-02-09T12:0{i}:00Z',
                f'Rocket backfill {i}',
                f'backfill body {i}',
            )
        # 5 releases on 2026-02-08 UTC; one is attached to a visible brief.
        for i in range(5):
            release(
                10,
                1201 + i,
                f'v0.8.{i}',
                f'2026-02-08T12:0{i}:00Z',
                f'Rocket brief {i}',
                f'brief body {i}',
            )
        # Owned repository for scope=mine and the HEAD warmup resolution.
        release(
            30,
            3001,
            'v1.2.3',
            '2026-02-09T15:00:00Z',
            'My release',
            'owned body',
            plus1=5,
        )
        # Time-zone boundary repository.
        release(20, 2001, 'v2.0.0', '2026-02-10T20:00:00Z', 'TZ late', 'tz late body')
        release(20, 2002, 'v2.0.1', '2026-02-10T08:00:00Z', 'TZ early', 'tz early body')

        now_iso = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
        sql(
            """
            INSERT INTO public.users (id, github_user_id, login, created_at, updated_at, is_admin, is_disabled, daily_brief_time_zone)
            VALUES
              ('seed-user', 101, 'seed-user', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 0, 0, 'UTC'),
              ('seed-user-tz', 102, 'seed-user-tz', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 0, 0, 'Asia/Shanghai');

            INSERT INTO public.github_connections (id, user_id, github_user_id, login, access_token_ciphertext, access_token_nonce, scopes, linked_at, updated_at)
            VALUES
              ('conn-1', 'seed-user', 101, 'seed-user', '\\x00'::bytea, '\\x00'::bytea, 'repo', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
              ('conn-2', 'seed-user-tz', 102, 'seed-user-tz', '\\x00'::bytea, '\\x00'::bytea, 'repo', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
            """,
            url,
        )
        sql(
            """
            INSERT INTO public.user_release_visible_repos
              (user_id, repo_id, full_name, owner_login, name, owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image, is_private)
            VALUES
              ('seed-user', 10, 'acme/rocket', 'acme', 'rocket', 'https://avatars.example/acme', NULL, 0, 0),
              ('seed-user', 30, 'seed-user/myrepo', 'seed-user', 'myrepo', 'https://avatars.example/seed-user', NULL, 0, 0),
              ('seed-user-tz', 20, 'acme/tzrepo', 'acme', 'tzrepo', NULL, NULL, 0, 0);

            INSERT INTO public.user_following_repos
              (user_id, repo_id, full_name, owner_login, name)
            VALUES
              ('seed-user', 10, 'acme/rocket', 'acme', 'rocket');

            INSERT INTO public.user_repo_associations
              (id, user_id, repo_id, repo_full_name, repo_full_name_lower, owner_login, repo_name, html_url,
               is_private, uses_custom_open_graph_image, first_source, first_associated_at, last_seen_at,
               is_following, follow_state_source, has_personal_owned_source, has_github_star_source,
               has_manual_feed_source, created_at, updated_at)
            VALUES
              ('assoc-1', 'seed-user', 10, 'acme/rocket', 'acme/rocket', 'acme', 'rocket',
               'https://github.com/acme/rocket', 0, 0, 'manual_feed', '2026-01-01T00:00:00Z',
               '2026-01-01T00:00:00Z', 1, 'system_default', 0, 0, 1, '2026-01-01T00:00:00Z',
               '2026-01-01T00:00:00Z');

            INSERT INTO public.owned_repo_star_baselines
              (id, user_id, repo_id, repo_full_name, initialized_at, updated_at, owner_avatar_url,
               open_graph_image_url, uses_custom_open_graph_image, is_private)
            VALUES
              ('owned-30', 'seed-user', 30, 'seed-user/myrepo', '{now}', '{now}',
               'https://avatars.example/seed-user', NULL, 0, 0);

            INSERT INTO public.social_activity_events
              (id, user_id, kind, repo_id, repo_full_name, repo_owner_avatar_url,
               repo_open_graph_image_url, repo_uses_custom_open_graph_image, title, body, html_url,
               github_event_id, actor_github_user_id, actor_login, actor_avatar_url, actor_html_url,
               occurred_at, detected_at, created_at, updated_at)
            VALUES
              ('social-1', 'seed-user', 'repo_star_received', 10, 'acme/rocket', NULL, NULL, 0,
               NULL, NULL, 'https://github.com/acme/rocket', NULL, 900, 'stargazer',
               'https://avatars.example/stargazer', 'https://github.com/stargazer',
               '2026-02-09T13:00:00Z', '2026-02-09T13:05:00Z', '2026-02-09T13:05:00Z',
               '2026-02-09T13:05:00Z');

            INSERT INTO public.notifications
              (id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason, updated_at, unread, html_url)
            VALUES
              ('notif-1', 'seed-user', 'thread-1', 'acme/rocket', 'Release 1', 'Release', 'subscribed', '2026-02-10T00:00:00Z', 1, 'https://github.com/acme/rocket'),
              ('notif-2', 'seed-user', 'thread-2', 'acme/rocket', 'Release 0', 'Release', 'subscribed', '2026-02-09T00:00:00Z', 0, 'https://github.com/acme/rocket');
            """.format(now=now_iso),
            url,
        )
        sql(
            """
            INSERT INTO public.briefs
              (id, user_id, date, window_start_utc, window_end_utc, effective_time_zone,
               effective_local_boundary, generation_source, content_markdown, created_at, updated_at)
            VALUES
              ('brief-feb8', 'seed-user', '2026-02-08', '2026-02-08T00:00:00Z', '2026-02-09T00:00:00Z',
               'UTC', '00:00', 'postgres_deterministic',
               $$# 每日简报

### 获星
- [Rocket brief 0](https://github.com/acme/rocket/releases/tag/v0.8.0) 摘要$$,
               '2026-02-09T01:00:00Z', '2026-02-09T01:00:00Z'),
              ('brief-future', 'seed-user', '2027-01-01', '2027-01-01T00:00:00Z', '2027-01-02T00:00:00Z',
               'UTC', '00:00', 'postgres_deterministic', '# future brief',
               '2027-01-02T01:00:00Z', '2027-01-02T01:00:00Z');

            INSERT INTO public.brief_release_memberships (brief_id, release_id, release_ts_utc, ordinal, created_at)
            VALUES ('brief-feb8', 1201, '2026-02-08T12:00:00Z', 1, '2026-02-09T01:00:00Z');
            """,
            url,
        )
        sql(
            'INSERT INTO public.repo_releases (id, repo_id, release_id, node_id, tag_name, name, body, html_url, published_at, created_at, is_prerelease, is_draft, updated_at, react_plus1, react_laugh, react_heart, react_hooray, react_rocket, react_eyes) VALUES '
            + ','.join(releases),
            url,
        )
        sql(
            "UPDATE public.repo_releases SET html_url = replace(html_url, 'acme/repo', 'seed-user/myrepo') WHERE repo_id = 30; "
            "UPDATE public.repo_releases SET html_url = replace(html_url, 'acme/repo', 'acme/tzrepo') WHERE repo_id = 20;",
            url,
        )

        with tempfile.TemporaryDirectory(prefix='octo-rill-dashboard-test-') as tmp:
            tmp = pathlib.Path(tmp)
            cookies = {}
            for user in ('seed-user', 'seed-user-tz'):
                file = tmp / user
                subprocess.run(
                    [
                        helper,
                        '--postgres-url',
                        url,
                        '--user-id',
                        user,
                        '--cookie-file',
                        file,
                        '--isolated-validation-copy',
                    ],
                    check=True,
                    stdout=subprocess.DEVNULL,
                )
                cookies[user] = file.read_text().strip()
            env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
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
                raise RuntimeError('validation server did not become ready')

            def request(path, user='seed-user', method='GET'):
                headers = {'Cookie': cookies[user]} if user else {}
                req = urllib.request.Request(base + path, headers=headers, method=method)
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

            quote = lambda value: urllib.parse.quote(value, safe='')  # noqa: E731

            # ---- anonymous auth -------------------------------------------------
            for path in ('/api/dashboard/updates', '/api/feed', '/api/dashboard/feed'):
                code, body = request(path, None)
                assert code == 401, (path, code, body)
                assert body['code'] == 'unauthorized', (path, body)
            code, body = request('/api/dashboard/feed/sections/whatever/items', None)
            assert code == 401 and body['code'] == 'unauthorized'
            code, body = request('/api/feed?scope=repo&items=acme/rocket', None, method='HEAD')
            assert code == 401, (code, body)

            # ---- feed projection parity ----------------------------------------
            code, page = request('/api/feed?limit=2')
            assert code == 200, (code, page)
            assert len(page['items']) == 2, page
            assert page['next_cursor'], page
            first = page['items'][0]
            # Legacy ordering is sort_ts DESC: the newest seeded release wins.
            assert first['kind'] == 'release', first
            assert first['id'] == '1033', first
            assert first['ts'] == '2026-02-10T00:32:00Z', first
            assert first['repo_full_name'] == 'acme/rocket', first
            assert first['repo_visual']['owner_avatar_url'] == 'https://avatars.example/acme', first
            assert first['reactions']['status'] == 'ready', first
            assert first['reactions']['counts']['plus1'] == 32, first
            assert first['reactions']['viewer']['plus1'] is False, first
            assert first['translated']['status'] == 'disabled', first
            assert first['smart']['status'] == 'disabled', first
            assert 'discussion_key' not in first, first
            second = page['items'][1]
            assert second['id'] == '1032', second
            assert second['kind'] == 'release', second
            code, page2 = request('/api/feed?limit=2&cursor=' + quote(page['next_cursor']))
            assert code == 200, (code, page2)
            page1_ids = {item['id'] for item in page['items']}
            assert not page1_ids & {item['id'] for item in page2['items']}, page2
            code, body = request('/api/feed?cursor=bogus')
            assert code == 400 and body['code'] == 'bad_request', (code, body)

            # scope=mine resolves through owned repo baselines by viewer login.
            code, mine = request('/api/feed?scope=mine')
            assert code == 200, (code, mine)
            assert {item['id'] for item in mine['items']} >= {'3001'}, mine

            # ---- HEAD /api/feed warmup -----------------------------------------
            code, _ = request('/api/feed', method='HEAD')
            assert code == 400, code
            code, _ = request('/api/feed?scope=org&org=acme', method='HEAD')
            assert code == 400, code
            code, _ = request('/api/feed?scope=repo&items=acme/rocket', method='HEAD')
            assert code == 202, code
            code, _ = request('/api/feed?scope=repo&items=seed-user/myrepo', method='HEAD')
            assert code == 204, code
            status = sql(
                "SELECT last_sync_status || ':' || COALESCE(repo_id::text, '') FROM public.public_repo_release_usage WHERE full_name_lower = 'seed-user/myrepo'",
                url,
            )
            assert status == 'ready:30', status

            # ---- dashboard sections --------------------------------------------
            code, feed = request('/api/dashboard/feed')
            assert code == 200, (code, feed)
            sections = feed['sections']
            dates = [section['display_date'] for section in sections]
            assert dates == ['2026-02-10', '2026-02-09', '2026-02-08'], dates
            assert '2027-01-01' not in dates, 'future brief must stay hidden'
            assert feed['next_cursor'] is None, feed

            raw_section = sections[0]
            assert raw_section['kind'] == 'raw', raw_section
            assert raw_section['activity_count'] == 31, raw_section
            assert len(raw_section['items']) == 30, raw_section
            assert raw_section['items'][0]['id'] == '1033', raw_section['items'][0]
            assert raw_section['brief'] is None
            assert raw_section['can_generate_brief'] is True, raw_section
            assert raw_section['items_next_cursor'], raw_section

            backfill_section = sections[1]
            assert backfill_section['kind'] == 'raw', backfill_section
            assert backfill_section['items'][0]['id'] == '3001', backfill_section['items'][0]
            assert backfill_section['items'][1]['id'] == 'social-1', backfill_section['items']
            assert backfill_section['items'][1]['kind'] == 'repo_star_received'
            assert backfill_section['items'][1]['actor']['login'] == 'stargazer'

            brief_section = sections[2]
            assert brief_section['kind'] == 'brief', brief_section
            assert brief_section['items'] == [], brief_section
            assert brief_section['activity_count'] == 5, brief_section
            assert brief_section['can_generate_brief'] is False, brief_section
            assert brief_section['supplemental_next_cursor'] is None
            assert brief_section['brief']['id'] == 'brief-feb8', brief_section['brief']
            assert brief_section['brief']['release_count'] == 1, brief_section['brief']
            assert brief_section['brief']['covers_repo_stars'] is True, brief_section['brief']
            supplemental_ids = [item['id'] for item in brief_section['supplemental_items']]
            assert len(supplemental_ids) == 4, supplemental_ids
            assert '1201' not in supplemental_ids, supplemental_ids
            assert brief_section['window_start'] == '2026-02-08T00:00:00Z', brief_section

            # ---- section item cursor pagination --------------------------------
            section_path = '/api/dashboard/feed/sections/' + quote(raw_section['id']) + '/items'
            code, items_page = request(section_path)
            assert code == 200, (code, items_page)
            assert len(items_page['items']) == 30, items_page
            assert items_page['next_cursor'], items_page
            code, items_page2 = request(
                section_path + '?cursor=' + quote(items_page['next_cursor'])
            )
            assert code == 200, (code, items_page2)
            assert len(items_page2['items']) == 3, items_page2
            assert items_page2['next_cursor'] is None, items_page2
            assert not {item['id'] for item in items_page['items']} & {
                item['id'] for item in items_page2['items']
            }
            code, body = request(section_path + '?cursor=' + quote('bogus'))
            assert code == 400 and body['code'] == 'bad_request', (code, body)
            code, body = request('/api/dashboard/feed/sections/' + quote('bogus') + '/items')
            assert code == 400 and body['code'] == 'bad_request', (code, body)

            # ---- time zone day boundary ---------------------------------------
            code, tz_feed = request('/api/dashboard/feed', 'seed-user-tz')
            assert code == 200, (code, tz_feed)
            tz_dates = [section['display_date'] for section in tz_feed['sections']]
            assert tz_dates == ['2026-02-11', '2026-02-10'], tz_dates
            assert tz_feed['sections'][0]['items'][0]['id'] == '2001', tz_feed['sections'][0]
            assert tz_feed['sections'][1]['items'][0]['id'] == '2002', tz_feed['sections'][1]

            # ---- dashboard updates token stability -----------------------------
            code, first_update = request('/api/dashboard/updates')
            assert code == 200, (code, first_update)
            assert set(first_update['lists']) == {'feed', 'briefs', 'notifications'}
            assert first_update['lists']['feed']['changed'] is False
            assert first_update['lists']['feed']['new_count'] == 0
            assert first_update['lists']['feed']['latest_keys'][0] == 'release:1033', first_update['lists']['feed']
            assert first_update['lists']['briefs']['latest_keys'] == ['brief:brief-feb8'], first_update['lists']['briefs']
            assert first_update['lists']['notifications']['latest_keys'] == [
                'notification:thread-1',
                'notification:thread-2',
            ], first_update['lists']['notifications']
            token = first_update['token']

            code, repeat_update = request('/api/dashboard/updates?token=' + quote(token))
            assert code == 200, (code, repeat_update)
            assert repeat_update['token'] == token, 'unchanged data must keep the same token'
            for lane in ('feed', 'briefs', 'notifications'):
                assert repeat_update['lists'][lane] == {
                    'changed': False,
                    'new_count': 0,
                    'latest_keys': [],
                }, (lane, repeat_update['lists'][lane])

            sql(
                "INSERT INTO public.notifications (id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason, updated_at, unread, html_url) "
                "VALUES ('notif-3', 'seed-user', 'thread-3', 'acme/rocket', 'Release 2', 'Release', 'subscribed', '2026-02-11T00:00:00Z', 1, 'https://github.com/acme/rocket')",
                url,
            )
            code, changed_update = request('/api/dashboard/updates?token=' + quote(token))
            assert code == 200, (code, changed_update)
            notifications = changed_update['lists']['notifications']
            assert notifications['changed'] is True, notifications
            assert notifications['new_count'] == 1, notifications
            assert notifications['latest_keys'] == ['notification:thread-3'], notifications
            assert changed_update['lists']['feed'] == {
                'changed': False,
                'new_count': 0,
                'latest_keys': [],
            }, changed_update['lists']['feed']
            new_token = changed_update['token']
            assert new_token != token

            code, settled_update = request('/api/dashboard/updates?token=' + quote(new_token))
            assert code == 200, (code, settled_update)
            assert settled_update['token'] == new_token
            assert settled_update['lists']['notifications']['changed'] is False

            # ---- include/scope/feed_type validation ----------------------------
            code, feed_only = request('/api/dashboard/updates?include=feed')
            assert code == 200, (code, feed_only)
            assert set(feed_only['lists']) == {'feed'}, feed_only['lists']
            code, body = request('/api/dashboard/updates?include=unknown')
            assert code == 400 and body['code'] == 'bad_request', (code, body)
            code, body = request('/api/dashboard/updates?token=bogus')
            assert code == 400 and body['code'] == 'bad_request', (code, body)
            code, body = request('/api/dashboard/updates?feed_type=unknown')
            assert code == 400 and body['code'] == 'bad_request', (code, body)
            code, scoped = request(
                '/api/dashboard/updates?scope=org&org=acme&feed_type=releases'
            )
            assert code == 200, (code, scoped)
            scoped_keys = scoped['lists']['feed']['latest_keys']
            assert scoped_keys and all(
                key.startswith('release:') for key in scoped_keys
            ), scoped_keys

            print(
                json.dumps(
                    {
                        'feed_projection': 'pass',
                        'feed_cursor_pagination': 'pass',
                        'head_feed_warmup': 'pass',
                        'dashboard_sections': 'pass',
                        'section_items_cursor': 'pass',
                        'brief_visibility_and_attachment': 'pass',
                        'time_zone_day_boundary': 'pass',
                        'updates_token_stability': 'pass',
                        'updates_change_detection': 'pass',
                        'anonymous_401': 'pass',
                    }
                )
            )
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
