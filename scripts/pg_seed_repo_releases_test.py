#!/usr/bin/env python3
"""Validate PG release detail, announcement detail, and public release reads."""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import subprocess

import tempfile
import time
import urllib.error
import urllib.request
from pg_seed_common import psql
from urllib.parse import quote, urlsplit, urlunsplit


def sha256_hex(value):
    return hashlib.sha256(value.encode()).hexdigest()


def release_detail_hash(repo_full_name, title, body):
    return sha256_hex(f'v=1\nkind=release_detail\nrepo={repo_full_name}\ntitle={title}\nbody={body}\n')


def announcement_detail_hash(repo_full_name, number, title, body):
    return sha256_hex(
        f'v=1\nkind=announcement_detail\nrepo={repo_full_name.lower()}\ndiscussion_number={number}\ntitle={title}\nbody={body}\n'
    )


SCHEMA = """
DROP TABLE IF EXISTS public.starred_repos CASCADE; CREATE TABLE public.starred_repos (
  id text PRIMARY KEY,
  user_id text NOT NULL,
  repo_id bigint NOT NULL,
  full_name text NOT NULL,
  owner_login text NOT NULL,
  name text NOT NULL,
  description text,
  html_url text NOT NULL,
  stargazed_at text,
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
  is_private bigint,
  initialized_at text NOT NULL,
  updated_at text NOT NULL,
  owner_avatar_url text,
  open_graph_image_url text,
  uses_custom_open_graph_image bigint NOT NULL DEFAULT 0
);
DROP VIEW IF EXISTS public.user_release_visible_repos CASCADE; CREATE TABLE public.user_release_visible_repos (
  user_id text NOT NULL,
  repo_id bigint,
  full_name text NOT NULL,
  owner_login text,
  name text,
  description text,
  html_url text,
  stargazed_at text,
  is_private bigint,
  updated_at text,
  owner_avatar_url text,
  open_graph_image_url text,
  uses_custom_open_graph_image bigint
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
  detected_at text,
  react_plus1 bigint NOT NULL DEFAULT 0,
  react_laugh bigint NOT NULL DEFAULT 0,
  react_heart bigint NOT NULL DEFAULT 0,
  react_hooray bigint NOT NULL DEFAULT 0,
  react_rocket bigint NOT NULL DEFAULT 0,
  react_eyes bigint NOT NULL DEFAULT 0
);
DROP TABLE IF EXISTS public.ai_translations CASCADE; CREATE TABLE public.ai_translations (
  id bigint PRIMARY KEY,
  user_id text NOT NULL,
  entity_type text NOT NULL,
  entity_id text NOT NULL,
  lang text NOT NULL,
  source_hash text NOT NULL,
  title text,
  summary text,
  status text NOT NULL DEFAULT 'ready',
  error_text text,
  active_work_item_id text,
  created_at text NOT NULL,
  updated_at text NOT NULL
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
  discussion_number bigint,
  actor_github_user_id bigint NOT NULL,
  actor_login text NOT NULL,
  actor_avatar_url text,
  actor_html_url text,
  occurred_at text NOT NULL,
  detected_at text NOT NULL,
  created_at text NOT NULL,
  updated_at text NOT NULL
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
  published_by_user_id text,
  published_at text,
  created_at text NOT NULL,
  updated_at text NOT NULL
);
DROP TABLE IF EXISTS public.repo_release_work_items CASCADE; CREATE TABLE public.repo_release_work_items (
  id text PRIMARY KEY,
  repo_id bigint NOT NULL UNIQUE,
  repo_full_name text NOT NULL,
  status text NOT NULL,
  request_origin text NOT NULL,
  deadline_at text NOT NULL,
  created_at text NOT NULL,
  updated_at text NOT NULL
);
DROP TABLE IF EXISTS public.repo_release_sync_state CASCADE; CREATE TABLE public.repo_release_sync_state (
  repo_id bigint PRIMARY KEY,
  updated_at text NOT NULL
);
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3023)
    args = parser.parse_args()

    name = 'octo_rill_repo_releases_seed_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))

    def sql(query, database=args.admin_url):
        return psql(database, query)

    sql('CREATE DATABASE ' + name)
    app = (args.binary_dir / 'octo-rill').resolve()
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
        AI_API_KEY='validation-ai-key',
    )
    server = None
    try:
        subprocess.run([app], env=env, check=True, stdout=subprocess.DEVNULL)
        sql(SCHEMA, url)
        sql(
            "INSERT INTO public.users(id,github_user_id,login,created_at,updated_at,is_admin) VALUES"
            " ('seed-owner',101,'owner','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0),"
            " ('seed-other',102,'other','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0);",
            url,
        )
        # Public repository "seed/repo" (repo_id 1) and empty public "seed/empty" (repo_id 3).
        sql(
            "INSERT INTO public.starred_repos(id,user_id,repo_id,full_name,owner_login,name,html_url,stargazed_at,is_private,updated_at,owner_avatar_url,open_graph_image_url,uses_custom_open_graph_image) VALUES"
            " ('star-1','seed-owner',1,'seed/repo','seed','repo','https://github.com/seed/repo','2026-01-01T00:00:00Z',0,to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),'https://avatars.example/seed','https://og.example/repo.png',1),"
            " ('star-3','seed-owner',3,'seed/empty','seed','empty','https://github.com/seed/empty','2026-01-01T00:00:00Z',0,to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),NULL,NULL,0);"
            "INSERT INTO public.user_release_visible_repos(user_id,repo_id,full_name,owner_login,name,html_url,stargazed_at,is_private,updated_at,owner_avatar_url,open_graph_image_url,uses_custom_open_graph_image) VALUES"
            " ('seed-owner',1,'seed/repo','seed','repo','https://github.com/seed/repo','2026-01-01T00:00:00Z',0,to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),'https://avatars.example/seed','https://og.example/repo.png',1),"
            " ('seed-owner',3,'seed/empty','seed','empty','https://github.com/seed/empty','2026-01-01T00:00:00Z',0,to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),NULL,NULL,0);",
            url,
        )
        sql(
            "INSERT INTO public.repo_releases(id,repo_id,release_id,tag_name,name,body,html_url,published_at,created_at,is_prerelease,is_draft,updated_at,detected_at,react_plus1,react_heart) VALUES"
            " ('rel-101',1,101,'v1.0.0','v1.0.0 release','Hello body','https://github.com/seed/repo/releases/tag/v1.0.0','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,'2026-01-02T00:00:00Z','2026-01-02T00:00:00Z',3,1),"
            " ('rel-102',1,102,'v2.0.0','v2.0.0 release','Hello body','https://github.com/seed/repo/releases/tag/v2.0.0','2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',0,0,'2026-02-02T00:00:00Z','2026-02-02T00:00:00Z',7,2),"
            " ('rel-103',1,103,'v3.0.0-draft','v3.0.0 draft','Draft body','https://github.com/seed/repo/releases/tag/v3.0.0-draft',NULL,'2026-03-01T00:00:00Z',0,1,'2026-03-02T00:00:00Z','2026-03-02T00:00:00Z',0,0),"
            " ('rel-201',2,201,'v0.1.0','private v0.1.0','Private body','https://github.com/seed/private/releases/tag/v0.1.0','2026-04-01T00:00:00Z','2026-04-01T00:00:00Z',0,0,'2026-04-02T00:00:00Z','2026-04-02T00:00:00Z',0,0);",
            url,
        )
        sql(
            "INSERT INTO public.owned_repo_star_baselines(id,user_id,repo_id,repo_full_name,is_private,initialized_at,updated_at) VALUES"
            " ('owned-2','seed-owner',2,'seed/private',1,'2026-01-01T00:00:00Z',to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'));",
            url,
        )
        ready_hash = release_detail_hash('seed/repo', 'v2.0.0 release', 'Hello body')
        queued_hash = release_detail_hash('seed/repo', 'v1.0.0 release', 'Hello body')
        announcement_hash = announcement_detail_hash('seed/repo', 55, 'Announcement 55', 'Body 55')
        sql(
            "INSERT INTO public.ai_translations(id,user_id,entity_type,entity_id,lang,source_hash,title,summary,status,created_at,updated_at) VALUES"
            f" (1,'seed-owner','release_detail','102','zh-CN','{ready_hash}','标题二','摘要二','ready','2026-02-02T00:00:00Z','2026-02-02T00:00:00Z'),"
            f" (2,'seed-owner','announcement_detail','seed/repo#55','zh-CN','{announcement_hash}','公告标题','公告摘要','ready','2026-01-05T00:00:00Z','2026-01-05T00:00:00Z'),"
            f" (3,'seed-owner','release_detail','101','zh-CN','{queued_hash}',NULL,NULL,'queued','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');",
            url,
        )
        sql(
            "INSERT INTO public.translation_requests(id,mode,source,request_origin,requested_by,"
            " scope_user_id,producer_ref,kind,variant,entity_id,target_lang,max_wait_ms,"
            " source_hash,source_blocks_json,target_slots_json,work_item_id,status,result_status,"
            " title_zh,summary_md,body_md,created_at,updated_at) VALUES"
            f" ('req-102-detail','async','postgres','user','seed-owner','seed-owner',"
            f" 'api.release_detail','release_detail','detail','102','zh-CN',15000,'{ready_hash}',"
            " '[]','[]',NULL,'completed','ready','标题二','摘要二','摘要二',"
            " '2026-02-02T00:00:00Z','2026-02-02T00:00:00Z');",
            url,
        )
        sql(
            "INSERT INTO public.social_activity_events(id,user_id,kind,repo_id,repo_full_name,repo_owner_avatar_url,repo_uses_custom_open_graph_image,title,body,html_url,discussion_number,actor_github_user_id,actor_login,actor_html_url,occurred_at,detected_at,created_at,updated_at) VALUES"
            " ('event-announcement','seed-owner','announcement',1,'seed/repo','https://avatars.example/seed',0,'Announcement 55','Body 55','https://github.com/seed/repo/discussions/55',55,999,'octocat','https://github.com/octocat','2026-01-05T00:00:00Z','2026-01-06T00:00:00Z','2026-01-06T00:00:00Z','2026-01-06T00:00:00Z');",
            url,
        )
        with tempfile.TemporaryDirectory(prefix='octo-rill-repo-releases-') as tmp:
            tmp = pathlib.Path(tmp)
            cookies = {}
            for user in ('seed-owner', 'seed-other'):
                cookie = tmp / user
                subprocess.run(
                    [
                        helper,
                        '--postgres-url',
                        url,
                        '--user-id',
                        user,
                        '--cookie-file',
                        cookie,
                        '--isolated-validation-copy',
                    ],
                    check=True,
                    stdout=subprocess.DEVNULL,
                )
                cookies[user] = cookie.read_text().strip()
            env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
            log = (tmp / 'server.log').open('w+')
            server = subprocess.Popen([app], env=env, stdout=log, stderr=subprocess.STDOUT)
            base = f'http://127.0.0.1:{args.port}'
            for _ in range(200):
                try:
                    urllib.request.urlopen(base + '/health', timeout=1).close()
                    break
                except Exception:
                    if server.poll() is not None:
                        log.seek(0)
                        raise RuntimeError(log.read()[-5000:])
                    time.sleep(.1)
            else:
                raise RuntimeError('validation server did not become ready')

            def call(method, path, user=None):
                headers = {'Cookie': cookies[user]} if user else {}
                request = urllib.request.Request(base + path, headers=headers, method=method)
                try:
                    with urllib.request.urlopen(request, timeout=10) as response:
                        body = response.read()
                        return response.status, json.loads(body) if body else None
                except urllib.error.HTTPError as error:
                    return error.code, json.load(error)

            def get(path, user=None):
                return call('GET', path, user)

            detail_path = '/api/repos/seed/repo/releases/tag/v2.0.0/detail'
            code, body = get(detail_path, 'seed-owner')
            assert code == 200, body
            assert body['repo_full_name'] == 'seed/repo' and body['release_id'] == '102'
            assert body['previous_tag_name'] == 'v1.0.0', body['previous_tag_name']
            assert body['translated']['status'] == 'ready' and body['translated']['title'] == '标题二'
            assert body['translated']['summary'] == '摘要二'
            assert body['translated']['request_id'] == 'req-102-detail', body['translated']
            assert 'auto_translate' not in body['translated'], body['translated']
            assert 'request_id' not in body['smart'], body['smart']
            assert body['smart']['status'] == 'missing'

            # The legacy cache row for v1.0.0 is still queued: the payload stays
            # missing but records that translation is in flight.
            code, body = get('/api/repos/seed/repo/releases/tag/v1.0.0/detail', 'seed-owner')
            assert code == 200, body
            assert body['translated']['status'] == 'missing', body['translated']
            assert body['translated']['auto_translate'] is True, body['translated']
            assert 'request_id' not in body['translated'], body['translated']
            assert body['is_prerelease'] == 0 and body['is_draft'] == 0
            assert 'reactions' not in body, 'release detail must not expose cached reaction counts'
            assert get(detail_path, None)[0] == 401
            assert get('/api/repos/seed/repo/releases/tag/v9.9.9/detail', 'seed-owner')[0] == 404

            code, body = get('/api/repos/seed/repo/discussions/55/detail', 'seed-owner')
            assert code == 200, body
            assert body['discussion_key'] == 'seed/repo#55' and body['discussion_number'] == 55
            assert body['title'] == 'Announcement 55' and body['body'] == 'Body 55'
            assert body['actor']['login'] == 'octocat'
            assert body['translated']['status'] == 'ready' and body['translated']['title'] == '公告标题'
            assert body['repo_visual']['owner_avatar_url'] == 'https://avatars.example/seed'
            assert get('/api/repos/seed/repo/discussions/55/detail', None)[0] == 401
            assert get('/api/repos/seed/repo/discussions/not-a-number/detail', 'seed-owner')[0] == 400
            assert get('/api/repos/seed/repo/discussions/56/detail', 'seed-owner')[0] == 404

            public_list = '/api/public/repos/seed/repo/releases'
            code, body = get(public_list)
            assert code == 200, body
            assert body['status'] == 'ready' and body['repo_full_name'] == 'seed/repo'
            assert body['next_cursor'] is None and 'previous_cursor' not in body
            assert [item['tag_name'] for item in body['items']] == ['v2.0.0', 'v1.0.0']
            first, second = body['items']
            assert first['translated']['status'] == 'ready' and first['translated']['summary'] == '摘要二'
            assert first['translated']['request_id'] == 'req-102-detail', first['translated']
            assert 'auto_translate' not in first['translated'], first['translated']
            assert first['smart']['status'] == 'missing'
            assert first['body'] == 'Hello body'
            assert first['is_highlighted'] is False and first['is_active_highlight'] is False
            assert first['repo_visual']['open_graph_image_url'] == 'https://og.example/repo.png'
            assert second['translated']['status'] == 'missing'
            assert second['translated']['auto_translate'] is True, second['translated']
            assert 'request_id' not in second['translated'], second['translated']
            assert 'highlight' not in body and 'segments' not in body and 'gaps' not in body

            code, body = get(public_list + '?content=translated')
            assert code == 200 and body['items'][0]['body'] is None
            code, body = get(public_list + '?content=polished')
            assert code == 200 and body['items'][0]['translated'] is None
            assert body['items'][1]['smart']['status'] == 'missing'

            code, body = get(public_list + '?limit=1')
            assert code == 200 and len(body['items']) == 1
            assert body['items'][0]['tag_name'] == 'v2.0.0'
            assert body['next_cursor'] == '2026-02-01T00:00:00Z|102', body['next_cursor']
            cursor = quote(body['next_cursor'], safe='')
            code, page_two = get(public_list + '?limit=1&cursor=' + cursor)
            assert code == 200 and [item['tag_name'] for item in page_two['items']] == ['v1.0.0']
            assert page_two['next_cursor'] is None
            code, newer = get(
                public_list + '?direction=newer&cursor=2026-01-01T00:00:00Z%7C101'
            )
            assert code == 200 and [item['tag_name'] for item in newer['items']] == ['v2.0.0']

            code, body = get(public_list + '?highlight=tag:v1.0.0')
            assert code == 200 and body['highlight']['status'] == 'complete'
            assert body['highlight']['resolved'][0]['selector'] == 'tag:v1.0.0'
            assert any(item['is_highlighted'] for item in body['items'])
            assert body['segments'] and body['gaps'] is not None
            code, body = get(public_list + '?highlight=id:999')
            assert code == 200 and body['highlight']['status'] == 'partial'
            assert body['highlight']['unresolved'] == ['id:999']

            assert get(public_list + '?content=bogus')[0] == 400
            assert get(public_list + '?lang=en')[0] == 400
            assert get(public_list + '?limit=abc')[0] == 400
            assert get(public_list + '?direction=sideways')[0] == 400
            assert get(public_list + '?refresh=if_stale')[0] == 401

            code, body = get('/api/public/repos/seed/repo/releases/tag/v2.0.0')
            assert code == 200, body
            assert body['translated']['status'] == 'ready' and body['tag_name'] == 'v2.0.0'
            assert body['translated']['request_id'] == 'req-102-detail', body['translated']
            assert body['previous_tag_name'] == 'v1.0.0'
            code, body = get('/api/public/repos/seed/repo/releases/tag/v1.0.0')
            assert code == 200 and body['translated']['status'] == 'missing'
            code, body = get('/api/public/repos/seed/repo/releases/tag/v9.9.9')
            assert code == 404 and body['error']['code'] == 'release_not_found_or_not_cached'

            code, body = get('/api/public/repos/seed/repo/releases/content?release_ids=101,102&content=translated')
            assert code == 200 and len(body['items']) == 2, body
            assert all(item['body'] is None for item in body['items'])
            assert {item['tag_name'] for item in body['items']} == {'v1.0.0', 'v2.0.0'}
            content_items = {item['tag_name']: item for item in body['items']}
            assert (
                content_items['v2.0.0']['translated']['request_id'] == 'req-102-detail'
            ), content_items['v2.0.0']
            assert (
                content_items['v1.0.0']['translated']['auto_translate'] is True
            ), content_items['v1.0.0']
            assert 'request_id' not in content_items['v1.0.0']['translated']
            assert get('/api/public/repos/seed/repo/releases/content?release_ids=101&content=original')[0] == 400
            assert get('/api/public/repos/seed/repo/releases/content?release_ids=0&content=translated')[0] == 400
            assert get('/api/public/repos/seed/repo/releases/content?release_ids=101&content=translated&lang=en')[0] == 400

            code, body = get('/api/public/repos/seed/empty/releases')
            assert code == 202, body
            assert body['status'] == 'pending_sync'
            assert body['reason'] == 'repository_registered_release_sync_pending'

            status_path = '/api/repos/seed/private/public-release'
            assert get(status_path)[0] == 401
            code, body = get(status_path, 'seed-other')
            assert code == 200 and body['publication_state'] == 'not_publishable'
            assert body['can_publish'] is False and body['reason'] == 'not_viewer_owned_private_repo'
            code, body = call('POST', status_path, 'seed-other')
            assert code == 403 and body['error']['code'] == 'repo_not_publishable'
            assert call('POST', status_path)[0] == 401
            code, body = get(status_path, 'seed-owner')
            assert code == 200 and body['publication_state'] == 'private_owner_unpublished'
            assert body['can_publish'] is True and body['can_unpublish'] is False
            assert body['public_path'] == '/seed/private/releases'
            code, body = call('POST', status_path, 'seed-owner')
            assert code == 200, body
            assert body['publication_state'] == 'private_owner_published'
            assert body['visibility'] == 'private' and body['can_unpublish'] is True
            assert body['published_at'] is not None
            assert body['last_sync_status'] == 'ready'
            code, body = get('/api/public/repos/seed/private/releases')
            assert code == 200, body
            assert [item['tag_name'] for item in body['items']] == ['v0.1.0']
            assert call('DELETE', status_path, 'seed-other')[0] == 403
            assert call('DELETE', status_path)[0] == 401
            code, body = call('DELETE', status_path, 'seed-owner')
            assert code == 200, body
            assert body['publication_state'] == 'private_owner_unpublished'
            assert body['can_publish'] is True and body['can_unpublish'] is False
            assert body['cache_cleanup']['skipped_reason'] is None
            assert body['cache_cleanup']['deleted_release_count'] == 1
            assert body['cache_cleanup']['full_name'] == 'seed/private'
            assert sql("SELECT COUNT(*) FROM public.repo_releases WHERE repo_id=2", url) == '0'
            assert sql("SELECT COUNT(*) FROM public.public_repo_release_usage WHERE full_name_lower='seed/private'", url) == '0'

            print(
                json.dumps(
                    {
                        'release_detail': 'pass',
                        'announcement_detail': 'pass',
                        'public_list_pagination_and_highlight': 'pass',
                        'public_detail_and_content': 'pass',
                        'public_release_lifecycle': 'pass',
                        'pending_sync': 'pass',
                    }
                )
            )
    finally:
        if server is not None:
            server.send_signal(2)
            server.wait(timeout=15)
        sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()
