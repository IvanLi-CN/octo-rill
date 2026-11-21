#!/usr/bin/env python3
"""Validate the global content-processing overlay on PostgreSQL read paths.

Seeds imported content-processing state (work identities, current result
projections) with `content_processing_control.mode = 'global'`, then asserts
that the feed, dashboard sections, release/announcement detail, and public
content endpoints serve the projection payloads instead of `ai_translations`.

Also covers the legacy switch (`mode = 'legacy'` serves the `ai_translations`
payloads again and switching back to global restores the projections), the
source-hash mismatch fallback (`active_work_item_id` retained projection), and
rows without any projection staying `missing`.
"""
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
from urllib.parse import urlsplit, urlunsplit

from pg_seed_common import psql

PROTOCOL_VERSION = 'content-processing.v1'
TARGET_LANG = 'zh-CN'
NOW = '2026-03-01T00:00:00Z'

# ---------------------------------------------------------------------------
# Hash helpers. These mirror the Rust implementations exactly:
#   * content_identity_upgrade::identity_id_for
#   * content_processing::source_hash_for_item
#   * api::global_release_source_hash / global_announcement_source_hash
#   * api::release_feed_translation_source_hash (v=5)
#   * api::release_detail_source_hash / announcement_detail_source_hash (v=1)
# ---------------------------------------------------------------------------


def sha256_hex(value):
    return hashlib.sha256(value.encode()).hexdigest()


def dumps(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':'), sort_keys=True)


def identity_id(resource_type, resource_id, pipeline, variant, source_hash):
    key = [
        resource_type,
        resource_id,
        pipeline,
        variant,
        TARGET_LANG,
        source_hash,
        PROTOCOL_VERSION,
    ]
    return 'content-work-' + sha256_hex('content-work-identity.v1\n' + dumps(key))


def source_hash_for_item(kind, variant, entity_id, source_blocks, target_slots):
    item = {
        'kind': kind,
        'variant': variant,
        'entity_id': entity_id,
        'target_lang': TARGET_LANG,
        'source_blocks': source_blocks,
        'target_slots': target_slots,
    }
    return sha256_hex(PROTOCOL_VERSION + '\n' + dumps(item))


def global_source_hash_from_fields(kind, variant, entity_id, metadata, title, body):
    blocks = [
        {'slot': 'metadata', 'text': metadata},
        {'slot': 'title', 'text': title},
    ]
    has_body = bool(body) and bool(body.strip())
    if has_body:
        blocks.append({'slot': 'body_markdown', 'text': body})
    if kind.endswith('_detail') or variant == 'detail':
        target_slots = ['title_zh', 'body_md'] if has_body else ['title_zh']
    else:
        target_slots = ['title_zh', 'summary_md']
    return source_hash_for_item(kind, variant, entity_id, blocks, target_slots)


def global_release_hash(
    release_id, repo_full_name, tag_name, name, body, kind, variant, repo_identity=None
):
    title = (name or '').strip() or tag_name
    identity = repo_full_name if repo_identity is None else repo_identity
    return global_source_hash_from_fields(
        kind,
        variant,
        str(release_id),
        f'{identity}\n{tag_name}',
        title,
        None if body is None else body.replace('\r\n', '\n'),
    )


def global_announcement_hash(discussion_key, repo_full_name, number, title, body, kind, variant):
    return global_source_hash_from_fields(
        kind,
        variant,
        discussion_key,
        f'{repo_full_name.strip()}\ndiscussion_number={number}',
        title,
        None if body is None else body.replace('\r\n', '\n'),
    )


def feed_body(body):
    if body is None:
        return None
    trimmed = body.replace('\r\n', '\n').strip()
    if not trimmed:
        return None
    return trimmed[:3000]


def normalize_body(body):
    return body.replace('\r\n', '\n')


def release_feed_hash(repo_full_name, title, body):
    return sha256_hex(
        f'v=5\nkind=release\nrepo={repo_full_name.strip()}\ntitle={title.strip()}\nbody={(body or "").strip()}\n'
    )


def release_detail_hash(repo_full_name, title, body):
    return sha256_hex(
        f'v=1\nkind=release_detail\nrepo={repo_full_name.strip()}\ntitle={title.strip()}\nbody={normalize_body(body).strip()}\n'
    )


def announcement_feed_hash(repo_full_name, number, title, body):
    return sha256_hex(
        f'v=1\nkind=announcement\nrepo={repo_full_name.strip().lower()}\ndiscussion_number={number}\ntitle={title.strip()}\nbody={(body or "").strip()}\n'
    )


def announcement_detail_hash(repo_full_name, number, title, body):
    return sha256_hex(
        f'v=1\nkind=announcement_detail\nrepo={repo_full_name.strip().lower()}\ndiscussion_number={number}\ntitle={title.strip()}\nbody={normalize_body(body).strip()}\n'
    )


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

REPO = 'acme/rocket'
REPO_ID = 40

RELEASES = {
    4001: {
        'tag': 'v4.0.1',
        'name': 'Rocket 4.0.1',
        'body': '- added warp drive\n- fixed hull',
        'published_at': '2026-03-01T05:00:00Z',
    },
    4002: {
        'tag': 'v4.0.2',
        'name': 'Rocket 4.0.2',
        'body': '- feed only release',
        'published_at': '2026-03-01T06:00:00Z',
    },
    4003: {
        'tag': 'v4.0.3',
        'name': 'Rocket 4.0.3',
        'body': '- retained release',
        'published_at': '2026-03-01T07:00:00Z',
    },
    4004: {
        'tag': 'v4.0.4',
        'name': 'Rocket 4.0.4',
        'body': '- untranslated release',
        'published_at': '2026-03-01T08:00:00Z',
    },
    # Slash tags keep their `/` unescaped in the release URL, so the writer could
    # not parse the repository name and keyed the projection under
    # `unknown/<repo_id>` (production: withastro/astro prereleases).
    4005: {
        'tag': '@scope/pkg@1.0.0-beta.0',
        'name': '@scope/pkg@1.0.0-beta.0',
        'body': '- fixed a thing',
        'published_at': '2026-03-01T10:00:00Z',
        'html_url': 'https://github.com/acme/rocket/releases/tag/%40scope/pkg%401.0.0-beta.0',
        'repo_identity': f'unknown/{REPO_ID}',
    },
    # Only an in-flight work item: the read path must expose auto_translate and
    # must not invent a request id.
    4006: {
        'tag': 'v4.0.6',
        'name': 'Rocket 4.0.6',
        'body': '- in flight release',
        'published_at': '2026-03-01T11:00:00Z',
    },
}

ANNOUNCEMENT = {
    'id': 'ann-77',
    'number': 77,
    'title': 'Rocket announcement 77',
    'body': 'announcement body 77',
    'occurred_at': '2026-03-01T09:00:00Z',
}
ANNOUNCEMENT_KEY = f'{REPO.lower()}#{ANNOUNCEMENT["number"]}'

SCHEMA = """
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
  is_private bigint
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
DROP VIEW IF EXISTS public.user_release_visible_repos CASCADE; CREATE TABLE public.user_release_visible_repos (
  user_id text NOT NULL,
  repo_id bigint,
  full_name text NOT NULL,
  owner_login text,
  name text,
  html_url text,
  stargazed_at text,
  is_private bigint,
  updated_at text,
  owner_avatar_url text,
  open_graph_image_url text,
  uses_custom_open_graph_image bigint
);
DROP TABLE IF EXISTS public.user_following_repos CASCADE; CREATE TABLE public.user_following_repos (
  user_id text NOT NULL,
  repo_id bigint,
  full_name text NOT NULL,
  owner_login text NOT NULL,
  name text NOT NULL,
  owner_avatar_url text,
  open_graph_image_url text,
  uses_custom_open_graph_image bigint NOT NULL DEFAULT 0
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


def content_rows():
    """Work items, identities, members, and current projections for the overlay."""
    work_items = []
    identities = []
    members = []
    projections = []

    def add(resource_type, resource_id, pipeline, variant, source_hash, payload,
            work_id, work_status='ready', current=True, active_work_item_id=None,
            projection_work_id=None):
        identity = identity_id(resource_type, resource_id, pipeline, variant, source_hash)
        work_items.append(
            (work_id, resource_type, resource_id, pipeline, variant, source_hash, work_status)
        )
        identities.append((identity, resource_type, resource_id, pipeline, variant, source_hash))
        members.append(
            (work_id, identity, resource_type, resource_id, pipeline, variant, source_hash)
        )
        if current:
            projections.append(
                (
                    identity,
                    projection_work_id or work_id,
                    active_work_item_id,
                    json.dumps(payload, ensure_ascii=False),
                )
            )
        return identity

    # 4001: feed + detail translations and the public polishing projection.
    release = RELEASES[4001]
    add(
        'release',
        '4001',
        'translation',
        'feed_body',
        global_release_hash(4001, REPO, release['tag'], release['name'], release['body'],
                            'release_summary', 'feed_body'),
        {'title_zh': '摘要 4001', 'body_md': '- 摘要一\n- 摘要二'},
        'wi-4001-feed',
    )
    add(
        'release',
        '4001',
        'translation',
        'detail',
        global_release_hash(4001, REPO, release['tag'], release['name'], release['body'],
                            'release_detail', 'detail'),
        {'title_zh': '详情 4001', 'body_md': '- 详情一\n- 详情二'},
        'wi-4001-detail',
    )
    add(
        'release',
        '4001',
        'polishing',
        'smart_public',
        'polishing-public-4001',
        {'title_zh': '精读 4001', 'summary_md': '精读摘要 4001'},
        'wi-4001-smart',
    )

    # 4002: feed slot only, so the feed translation must come from `feed_body`.
    release = RELEASES[4002]
    add(
        'release',
        '4002',
        'translation',
        'feed_body',
        global_release_hash(4002, REPO, release['tag'], release['name'], release['body'],
                            'release_summary', 'feed_body'),
        {'title_zh': '摘要 4002', 'body_md': '- 仅摘要二'},
        'wi-4002-feed',
    )

    # 4003: the detail identity has no current projection; the newest record
    # keeps `active_work_item_id` on that work item and must be served as the
    # retained payload.
    release = RELEASES[4003]
    add(
        'release',
        '4003',
        'translation',
        'detail',
        global_release_hash(4003, REPO, release['tag'], release['name'], release['body'],
                            'release_detail', 'detail'),
        {},
        'wi-4003-detail',
        current=False,
    )
    add(
        'release',
        '4003',
        'translation',
        'detail',
        'stale-detail-4003',
        {'title_zh': '旧详情 4003', 'body_md': '- 旧详情一'},
        'wi-4003-stale',
        active_work_item_id='wi-4003-detail',
    )

    # 4005: a release whose tag contains `/`. The writer parsed the release URL,
    # failed (six path segments), and keyed the projection under
    # `unknown/<repo_id>`; the live row's visible repository name is
    # `acme/rocket`, so the read path must use the writer identity or the
    # payload is unreachable.
    release = RELEASES[4005]
    add(
        'release',
        '4005',
        'translation',
        'summary',
        global_release_hash(4005, REPO, release['tag'], release['name'], release['body'],
                            'release_summary', 'summary',
                            repo_identity=release['repo_identity']),
        {'title_zh': '斜杠摘要 4005', 'body_md': '- 修复了一个问题'},
        'wi-4005-summary',
        active_work_item_id='wi-4005-summary',
    )

    # 4006: a queued work item without a published projection. The detail read
    # shows it as missing with `auto_translate` on.
    release = RELEASES[4006]
    add(
        'release',
        '4006',
        'translation',
        'detail',
        global_release_hash(4006, REPO, release['tag'], release['name'], release['body'],
                            'release_detail', 'detail'),
        {},
        'wi-4006-detail',
        work_status='queued',
        current=False,
    )

    # 4004 has no identities or projections at all: the feed must report missing.

    # Announcement: feed body, detail, and both polishing variants.
    add(
        'announcement',
        ANNOUNCEMENT_KEY,
        'translation',
        'feed_body',
        global_announcement_hash(ANNOUNCEMENT_KEY, REPO, ANNOUNCEMENT['number'],
                                 ANNOUNCEMENT['title'], ANNOUNCEMENT['body'],
                                 'announcement_summary', 'feed_body'),
        {'title_zh': '公告摘要 77', 'body_md': '公告摘要正文 77'},
        'wi-ann-77-feed',
    )
    add(
        'announcement',
        ANNOUNCEMENT_KEY,
        'translation',
        'detail',
        global_announcement_hash(ANNOUNCEMENT_KEY, REPO, ANNOUNCEMENT['number'],
                                 ANNOUNCEMENT['title'], ANNOUNCEMENT['body'],
                                 'announcement_detail', 'detail'),
        {'title_zh': '公告详情 77', 'body_md': '公告详情正文 77'},
        'wi-ann-77-detail',
    )
    add(
        'announcement',
        ANNOUNCEMENT_KEY,
        'polishing',
        'feed_card',
        global_announcement_hash(ANNOUNCEMENT_KEY, REPO, ANNOUNCEMENT['number'],
                                 ANNOUNCEMENT['title'], ANNOUNCEMENT['body'],
                                 'announcement_smart', 'feed_card'),
        {'title_zh': '公告卡片 77', 'summary_md': '公告卡片摘要 77'},
        'wi-ann-77-card',
    )
    add(
        'announcement',
        ANNOUNCEMENT_KEY,
        'polishing',
        'smart',
        global_announcement_hash(ANNOUNCEMENT_KEY, REPO, ANNOUNCEMENT['number'],
                                 ANNOUNCEMENT['title'], ANNOUNCEMENT['body'],
                                 'announcement_smart', 'smart'),
        {'title_zh': '公告精读 77', 'summary_md': '公告精读摘要 77'},
        'wi-ann-77-smart',
    )

    return work_items, identities, members, projections


def seed_content(sql, url):
    work_items, identities, members, projections = content_rows()

    sql(
        "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id,"
        " pipeline, variant, target_lang, source_hash, protocol_version, model_profile,"
        " source_snapshot_json, configuration_fingerprint, status, created_at, updated_at) VALUES "
        + ','.join(
            "('{0}','{1}','{2}','{3}','{4}','{5}','{6}','{7}','postgres-default','{{}}','fp','{8}','{9}','{9}')".format(
                row[0], row[1], row[2], row[3], row[4], TARGET_LANG, row[5],
                PROTOCOL_VERSION, row[6], NOW,
            )
            for row in work_items
        ),
        url,
    )
    sql(
        "INSERT INTO public.content_work_identities (id, canonical_resource_type,"
        " canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version,"
        " created_at, updated_at) VALUES "
        + ','.join(
            "('{0}','{1}','{2}','{3}','{4}','{5}','{6}','{7}','{8}','{8}')".format(
                row[0], row[1], row[2], row[3], row[4], TARGET_LANG, row[5],
                PROTOCOL_VERSION, NOW,
            )
            for row in identities
        ),
        url,
    )
    sql(
        "INSERT INTO public.content_work_identity_members (work_item_id, identity_id,"
        " canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang,"
        " source_hash, protocol_version, linked_at) VALUES "
        + ','.join(
            "('{0}','{1}','{2}','{3}','{4}','{5}','{6}','{7}','{8}','{9}')".format(
                row[0], row[1], row[2], row[3], row[4], row[5], TARGET_LANG, row[6],
                PROTOCOL_VERSION, NOW,
            )
            for row in members
        ),
        url,
    )
    sql(
        "INSERT INTO public.content_current_result_projections (identity_id, work_item_id,"
        " active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES "
        + ','.join(
            "('{0}','{1}',{2},NULL,$overlay${3}$overlay$,'{4}','{4}')".format(
                row[0],
                row[1],
                'NULL' if row[2] is None else "'" + row[2] + "'",
                row[3],
                NOW,
            )
            for row in projections
        ),
        url,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3026)
    args = parser.parse_args()

    name = 'octo_rill_global_overlay_seed_' + str(os.getpid())
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
        OCTORILL_PG_WORKER_ENABLED='0',
        OCTORILL_PG_SCHEDULER_ENABLED='0',
        OCTORILL_PG_BOOTSTRAP_ONLY='1',
        APP_DEFAULT_TIME_ZONE='UTC',
    )
    # Legacy hides every translation slot when AI is unconfigured, so the read
    # assertions need a configured model. Nothing is called: the probe only
    # reads projections and the ai_translations cache.
    env['AI_BASE_URL'] = 'http://127.0.0.1:9/v1/'
    env['AI_API_KEY'] = 'validation-key'
    env['AI_MODEL'] = 'validation-model'
    process = None
    try:
        # 1. Runtime baseline (migrations_pg) without the application schema.
        subprocess.run([app], env=env, check=True, stdout=subprocess.DEVNULL)

        # 2. Application tables imported from SQLite in production.
        sql(SCHEMA, url)
        sql(
            "INSERT INTO public.users (id, github_user_id, login, created_at, updated_at,"
            " is_admin, is_disabled, daily_brief_time_zone) VALUES"
            " ('seed-user',101,'seed-user','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,'UTC');"
            "INSERT INTO public.github_connections (id, user_id, github_user_id, login,"
            " access_token_ciphertext, access_token_nonce, scopes, linked_at, updated_at) VALUES"
            " ('conn-1','seed-user',101,'seed-user','\\x00'::bytea,'\\x00'::bytea,'repo',"
            " '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');",
            url,
        )
        sql(
            "INSERT INTO public.user_release_visible_repos (user_id, repo_id, full_name,"
            " owner_login, name, html_url, stargazed_at, is_private, updated_at, owner_avatar_url,"
            " open_graph_image_url, uses_custom_open_graph_image) VALUES"
            f" ('seed-user',{REPO_ID},'{REPO}','acme','rocket','https://github.com/{REPO}',"
            f" '2026-01-01T00:00:00Z',0,'{NOW}','https://avatars.example/acme',NULL,0);"
            "INSERT INTO public.public_repo_release_usage (id, repo_id, owner_login, repo_name,"
            " full_name, full_name_lower, first_registered_at, last_requested_at,"
            " last_sync_status, access_kind, created_at, updated_at) VALUES"
            f" ('usage-40',{REPO_ID},'acme','rocket','{REPO}','{REPO}','{NOW}','{NOW}',"
            f" 'ready','github_public','{NOW}','{NOW}');",
            url,
        )
        releases = []
        for release_id, fixture in RELEASES.items():
            html_url = fixture.get(
                'html_url',
                f'https://github.com/{REPO}/releases/tag/{fixture["tag"]}',
            )
            releases.append(
                "('rel-{rid}',{repo},{rid},'node-{rid}','{tag}','{name}',$${body}$$,"
                "'{html_url}','{published}','{published}',"
                "0,0,'{published}','{published}',0,0,0,0,0,0)".format(
                    rid=release_id,
                    repo=REPO_ID,
                    tag=fixture['tag'],
                    name=fixture['name'],
                    body=fixture['body'],
                    html_url=html_url,
                    published=fixture['published_at'],
                )
            )
        sql(
            "INSERT INTO public.repo_releases (id, repo_id, release_id, node_id, tag_name, name,"
            " body, html_url, published_at, created_at, is_prerelease, is_draft, updated_at,"
            " detected_at, react_plus1, react_laugh, react_heart, react_hooray, react_rocket,"
            " react_eyes) VALUES " + ','.join(releases),
            url,
        )
        sql(
            "INSERT INTO public.social_activity_events (id, user_id, kind, repo_id, repo_full_name,"
            " repo_owner_avatar_url, repo_uses_custom_open_graph_image, title, body, html_url,"
            " discussion_number, actor_github_user_id, actor_login, actor_html_url, occurred_at,"
            " detected_at, created_at, updated_at) VALUES"
            f" ('{ANNOUNCEMENT['id']}','seed-user','announcement',{REPO_ID},'{REPO}',"
            f" 'https://avatars.example/acme',0,$${ANNOUNCEMENT['title']}$$,"
            f" $${ANNOUNCEMENT['body']}$$,"
            f" 'https://github.com/{REPO}/discussions/{ANNOUNCEMENT['number']}',"
            f" {ANNOUNCEMENT['number']},999,'octocat','https://github.com/octocat',"
            f" '{ANNOUNCEMENT['occurred_at']}','{ANNOUNCEMENT['occurred_at']}',"
            f" '{ANNOUNCEMENT['occurred_at']}','{ANNOUNCEMENT['occurred_at']}');",
            url,
        )

        # 3. Imported content-processing state in global mode.
        sql(
            f"UPDATE public.content_processing_control SET mode='global', updated_at='{NOW}'"
            " WHERE id=1;",
            url,
        )
        sql(
            "INSERT INTO public.content_identity_upgrade_control (id, generation, status, phase,"
            f" started_at, completed_at, updated_at) VALUES (1,1,'completed','complete','{NOW}',"
            f" '{NOW}','{NOW}')"
            " ON CONFLICT (id) DO UPDATE SET status='completed', phase='complete',"
            " updated_at=EXCLUDED.updated_at;",
            url,
        )
        seed_content(sql, url)
        # One persisted translation request owns release 4001's detail payload.
        # Releases 4003/4004/4006 have no request row, so their payloads must
        # omit `request_id`.
        release_4001 = RELEASES[4001]
        request_hash = global_release_hash(
            4001, REPO, release_4001['tag'], release_4001['name'], release_4001['body'],
            'release_detail', 'detail',
        )
        sql(
            "INSERT INTO public.translation_requests (id, mode, source, request_origin,"
            " requested_by, scope_user_id, producer_ref, kind, variant, entity_id, target_lang,"
            " max_wait_ms, source_hash, source_blocks_json, target_slots_json, work_item_id,"
            " status, result_status, title_zh, summary_md, body_md, created_at, updated_at)"
            " VALUES ('req-4001-detail','async','postgres','user','seed-user','seed-user',"
            " 'api.release_detail','release_detail','detail','4001','zh-CN',15000,"
            f" '{request_hash}','[]','[]',NULL,'completed','ready','详情 4001',"
            ' $body$- 详情一\n- 详情二$body$,$body$- 详情一\n- 详情二$body$,'
            f" '{NOW}','{NOW}');",
            url,
        )

        # Legacy cache rows used by the second phase of the suite.
        release = RELEASES[4001]
        legacy_rows = [
            (
                '1',
                'release',
                '4001',
                release_feed_hash(REPO, release['name'], feed_body(release['body'])),
                '旧摘要 4001',
                '- 旧摘要一\n- 旧摘要二',
            ),
            (
                '2',
                'release_detail',
                '4001',
                release_detail_hash(REPO, release['name'], release['body']),
                '旧详情 4001',
                '- 旧详情一\n- 旧详情二',
            ),
            (
                '3',
                'announcement_detail',
                ANNOUNCEMENT_KEY,
                announcement_detail_hash(
                    REPO, ANNOUNCEMENT['number'], ANNOUNCEMENT['title'], ANNOUNCEMENT['body']
                ),
                '旧公告详情 77',
                '旧公告详情正文 77',
            ),
            (
                '4',
                'announcement',
                ANNOUNCEMENT_KEY,
                announcement_feed_hash(
                    REPO,
                    ANNOUNCEMENT['number'],
                    ANNOUNCEMENT['title'],
                    feed_body(ANNOUNCEMENT['body']),
                ),
                '旧公告摘要 77',
                '旧公告摘要正文 77',
            ),
        ]
        sql(
            "INSERT INTO public.ai_translations (id, user_id, entity_type, entity_id, lang,"
            " source_hash, status, title, summary, created_at, updated_at) VALUES "
            + ','.join(
                "('{0}','seed-user','{1}','{2}','zh-CN','{3}','ready',$${4}$$,$${5}$$,'{6}','{6}')".format(
                    row[0], row[1], row[2], row[3], row[4], row[5], NOW
                )
                for row in legacy_rows
            ),
            url,
        )

        # Pre-identity-upgrade projection table; only read while the identity
        # upgrade is incomplete (phase 4 of the assertions below).
        sql(
            "INSERT INTO public.content_result_projections (id, canonical_resource_type,"
            " canonical_resource_id, pipeline, variant, target_lang, protocol_version,"
            " model_profile, source_hash, work_item_id, active_work_item_id, payload_json,"
            " published_at, updated_at) VALUES"
            " ('proj-4001-detail','release','4001','translation','detail','zh-CN',"
            f" '{PROTOCOL_VERSION}','postgres-default',"
            f" '{global_release_hash(4001, REPO, release['tag'], release['name'], release['body'], 'release_detail', 'detail')}',"
            " 'wi-4001-detail',NULL,"
            ' $overlay${"title_zh": "表详情 4001", "body_md": "- 表详情一\\n- 表详情二"}$overlay$,'
            f" '{NOW}','{NOW}');",
            url,
        )

        with tempfile.TemporaryDirectory(prefix='octo-rill-global-overlay-') as tmp:
            tmp = pathlib.Path(tmp)
            cookie = tmp / 'seed-user'
            subprocess.run(
                [
                    helper,
                    '--postgres-url',
                    url,
                    '--user-id',
                    'seed-user',
                    '--cookie-file',
                    cookie,
                    '--isolated-validation-copy',
                ],
                check=True,
                stdout=subprocess.DEVNULL,
            )
            env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
            log = (tmp / 'server.log').open('w+')
            process = subprocess.Popen([app], env=env, stdout=log, stderr=subprocess.STDOUT)
            base = f'http://127.0.0.1:{args.port}'
            for _ in range(200):
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
            session = cookie.read_text().strip()

            def get(path):
                request = urllib.request.Request(base + path, headers={'Cookie': session})
                try:
                    with urllib.request.urlopen(request, timeout=20) as response:
                        body = response.read()
                        return response.status, json.loads(body) if body else None
                except urllib.error.HTTPError as error:
                    body = error.read()
                    if error.code >= 500:
                        print('--- server log tail ---', flush=True)
                        print((tmp / 'server.log').read_text()[-4000:], flush=True)
                    return error.code, json.loads(body) if body else None

            def feed_items():
                code, body = get('/api/feed?limit=50')
                assert code == 200, body
                return {item['id']: item for item in body['items']}

            def section_items():
                code, body = get('/api/dashboard/feed')
                assert code == 200, body
                items = {}
                for section in body['sections']:
                    for item in section['items'] + section['supplemental_items']:
                        items[item['id']] = item
                return items

            def release_detail(tag='v4.0.1'):
                return get(f'/api/repos/{REPO}/releases/tag/{tag}/detail')

            def release_detail_by_id(release_id):
                return get(f'/api/releases/{release_id}/detail')

            def announcement_detail():
                return get(
                    f'/api/repos/{REPO}/discussions/{ANNOUNCEMENT["number"]}/detail'
                )

            def content(content_kind, release_ids):
                ids = ','.join(str(value) for value in release_ids)
                code, body = get(
                    f'/api/public/repos/{REPO}/releases/content?release_ids={ids}&content={content_kind}'
                )
                assert code == 200, body
                return {str(item['release_id']): item for item in body['items']}

            def assert_global_payloads():
                items = feed_items()
                item = items['4001']
                assert item['translated']['status'] == 'ready', item['translated']
                assert item['translated']['title'] == '详情 4001', item['translated']
                assert item['translated']['summary'] == '- 详情一\n- 详情二', item['translated']
                assert item['smart']['status'] == 'ready', item['smart']
                assert item['smart']['title'] == '精读 4001', item['smart']
                assert item['smart']['summary'] == '精读摘要 4001', item['smart']

                assert items['4002']['translated']['title'] == '摘要 4002', items['4002']
                assert items['4002']['smart']['status'] == 'missing', items['4002']
                assert items['4003']['translated']['title'] == '旧详情 4003', items['4003']
                assert items['4004']['translated']['status'] == 'missing', items['4004']

                # Slash tag: the projection is keyed by the writer identity
                # (`unknown/<repo_id>`), not by the visible repository name.
                assert items['4005']['translated']['status'] == 'ready', items['4005']
                assert items['4005']['translated']['title'] == '斜杠摘要 4005', items['4005']

                code, detail = release_detail_by_id(4005)
                assert code == 200, detail
                assert detail['translated']['status'] == 'ready', detail['translated']
                assert detail['translated']['title'] == '斜杠摘要 4005', detail['translated']
                assert detail['translated']['summary'] == '- 修复了一个问题'

                code, detail = release_detail(
                    urllib.parse.quote(RELEASES[4005]['tag'], safe='')
                )
                assert code == 200, detail
                assert detail['translated']['title'] == '斜杠摘要 4005', detail['translated']

                announcement = items[ANNOUNCEMENT['id']]
                assert announcement['translated']['title'] == '公告详情 77', announcement
                assert announcement['smart']['title'] == '公告卡片 77', announcement

                sections = section_items()
                assert sections['4001']['translated']['title'] == '详情 4001', sections['4001']
                assert sections['4001']['smart']['title'] == '精读 4001', sections['4001']
                assert sections['4005']['translated']['title'] == '斜杠摘要 4005', sections['4005']
                assert (
                    sections[ANNOUNCEMENT['id']]['translated']['title'] == '公告详情 77'
                ), sections[ANNOUNCEMENT['id']]

                code, detail = release_detail()
                assert code == 200, detail
                assert detail['translated']['status'] == 'ready', detail['translated']
                assert detail['translated']['title'] == '详情 4001', detail['translated']
                assert detail['translated']['summary'] == '- 详情一\n- 详情二'
                # The projection is owned by the seeded translation request.
                assert (
                    detail['translated']['request_id'] == 'req-4001-detail'
                ), detail['translated']
                assert 'auto_translate' not in detail['translated'], detail['translated']
                assert detail['smart']['status'] == 'ready', detail['smart']
                assert detail['smart']['title'] == '精读 4001', detail['smart']
                assert 'request_id' not in detail['smart'], detail['smart']

                code, detail = release_detail('v4.0.3')
                assert code == 200, detail
                assert detail['translated']['title'] == '旧详情 4003', detail['translated']
                # No translation request was recorded for this release.
                assert 'request_id' not in detail['translated'], detail['translated']

                code, detail = release_detail('v4.0.6')
                assert code == 200, detail
                assert detail['translated']['status'] == 'missing', detail['translated']
                assert detail['translated']['auto_translate'] is True, detail['translated']
                assert 'request_id' not in detail['translated'], detail['translated']

                code, detail = release_detail('v4.0.4')
                assert code == 200, detail
                assert detail['translated']['status'] == 'missing', detail['translated']
                assert detail['smart']['status'] == 'missing', detail['smart']

                code, detail = announcement_detail()
                assert code == 200, detail
                assert detail['translated']['title'] == '公告详情 77', detail['translated']
                assert detail['smart']['title'] == '公告精读 77', detail['smart']

                # Public content reads are exact: the retained payload of 4003
                # describes a different source hash and must not surface there.
                translated = content('translated', [4001, 4003])
                assert translated['4001']['translated']['title'] == '详情 4001', translated
                assert (
                    translated['4001']['translated']['request_id'] == 'req-4001-detail'
                ), translated['4001']
                assert translated['4003']['translated']['status'] == 'missing', translated
                assert 'request_id' not in translated['4003']['translated'], translated['4003']
                # Public content reads the exact detail variant only. 4005 owns a
                # summary projection, which the legacy public list never read.
                translated = content('translated', [4005])
                assert translated['4005']['translated']['status'] == 'missing', translated
                polished = content('polished', [4001, 4003])
                assert polished['4001']['smart']['title'] == '精读 4001', polished
                assert polished['4003']['smart']['status'] == 'missing', polished

                # Public tag detail carries the same request metadata.
                code, detail = get(f'/api/public/repos/{REPO}/releases/tag/v4.0.1')
                assert code == 200, detail
                assert (
                    detail['translated']['request_id'] == 'req-4001-detail'
                ), detail['translated']
                code, detail = get(f'/api/public/repos/{REPO}/releases/tag/v4.0.6')
                assert code == 200, detail
                assert detail['translated']['status'] == 'missing', detail['translated']
                assert 'request_id' not in detail['translated'], detail['translated']

            def assert_legacy_payloads():
                items = feed_items()
                assert items['4001']['translated']['title'] == '旧详情 4001', items['4001']
                assert items['4001']['translated']['summary'] == '- 旧详情一\n- 旧详情二'
                assert items['4001']['smart']['status'] == 'missing', items['4001']
                assert items['4002']['translated']['status'] == 'missing', items['4002']
                assert items['4005']['translated']['status'] == 'missing', items['4005']
                announcement = items[ANNOUNCEMENT['id']]
                assert announcement['translated']['title'] == '旧公告详情 77', announcement
                assert announcement['smart']['status'] == 'missing', announcement

                sections = section_items()
                assert sections['4001']['translated']['title'] == '旧详情 4001', sections['4001']

                code, detail = release_detail()
                assert code == 200, detail
                assert detail['translated']['title'] == '旧详情 4001', detail['translated']

                code, detail = announcement_detail()
                assert code == 200, detail
                assert detail['translated']['title'] == '旧公告详情 77', detail['translated']

                translated = content('translated', [4001])
                assert translated['4001']['translated']['title'] == '旧详情 4001', translated

            # Phase 1: global mode serves the projections.
            assert_global_payloads()

            # Phase 2: legacy mode serves the ai_translations cache again.
            sql(
                f"UPDATE public.content_processing_control SET mode='legacy', updated_at='{NOW}'"
                " WHERE id=1;",
                url,
            )
            assert_legacy_payloads()

            # Phase 3: switching back restores the projection payloads.
            sql(
                f"UPDATE public.content_processing_control SET mode='global', updated_at='{NOW}'"
                " WHERE id=1;",
                url,
            )
            assert_global_payloads()

            # Phase 4: before the work-identity upgrade completes, the legacy
            # `content_result_projections` table is the source of truth.
            sql(
                "UPDATE public.content_identity_upgrade_control SET status='running',"
                f" phase='projection_backfill', updated_at='{NOW}' WHERE id=1;",
                url,
            )
            items = feed_items()
            assert items['4001']['translated']['title'] == '表详情 4001', items['4001']
            assert items['4001']['smart']['status'] == 'missing', items['4001']
            assert items['4002']['translated']['status'] == 'missing', items['4002']
            sql(
                "UPDATE public.content_identity_upgrade_control SET status='completed',"
                f" phase='complete', updated_at='{NOW}' WHERE id=1;",
                url,
            )
            assert_global_payloads()

            print(
                json.dumps(
                    {
                        'global_feed_and_sections': 'pass',
                        'release_and_announcement_detail': 'pass',
                        'public_release_content': 'pass',
                        'legacy_switch': 'pass',
                        'source_hash_mismatch_fallback': 'pass',
                        'missing_projection': 'pass',
                        'pre_identity_upgrade_projection': 'pass',
                    }
                )
            )
    finally:
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
        sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()
