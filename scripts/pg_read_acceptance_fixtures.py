#!/usr/bin/env python3
"""Seed the deterministic PostgreSQL fixture the read-acceptance probe needs.

`scripts/pg_read_acceptance.py` enumerates every GET route in
`src/postgres_server.rs` and needs one row per route parameter. In CI there is
no imported production copy, so this script writes the smallest complete data
set that makes each parameterized route exercisable: one admin, one visible
repository with a release, a brief, a finished task, an LLM call, a migration
operation, a translation request/batch and the cron/config singleton rows.

Opaque identifiers are deterministic so a failure is reproducible; the session
cookie is created by `octo-rill-validation-session` against the same database.

Usage:
  python3 scripts/pg_read_acceptance_fixtures.py \
      --postgres-url postgres://…/octo_read_acceptance \
      --binary-dir target/debug \
      --cookie-file .ci/read-acceptance.cookie
"""
import argparse
import base64
import json
import os
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from pg_seed_common import psql  # noqa: E402

ADMIN_ID = 'ci-read-admin'
GITHUB_USER_ID = 900001
REPO_ID = 910001
RELEASE_ID = 920001
BRIEF_ID = 'ci-read-brief'
TASK_ID = 'ci-read-task'
CALL_ID = 'ci-read-call'
REQUEST_ID = 'ci-read-request'
BATCH_ID = 'ci-read-batch'
MIGRATION_ID = 'content-processing-online-v2'
USAGE_ID = 'ci-read-usage'
API_KEY_ID = 'ci-read-api-key'
PASSKEY_ID = 'ci-read-passkey'
CONNECTION_ID = 'ci-read-connection'
ACTIVITY_ID = 'ci-read-activity'
HOUR_UTC = 9
OWNER = 'ci-read'
REPO = 'acceptance'
FULL_NAME = f'{OWNER}/{REPO}'
TAG = 'v1.0.0'
DISCUSSION_NUMBER = 1

# Deterministic stand-ins for encrypted material. The probe only reads these
# rows through "does the row exist" contracts, so the ciphertext is never
# decrypted by any route it exercises.
# AES-256-GCM ciphertext of 'ci-validation-token' under the all-zero 32-byte
# validation key with a 12-byte all-zero nonce, so decrypting succeeds.
CIPHERTEXT = "decode('adce6d4b2c0c020a663aacbcd4dee97719056d2fc7c6a5a8ca6c7610f6a0f7fe11a8d3', 'hex')"
NONCE = "decode('000000000000000000000000', 'hex')"


def scalar(query, url):
    return psql(url, query)


def seed(url, log_path):
    statements = []

    statements.append(
        "INSERT INTO public.users (id, github_user_id, login, created_at, updated_at,"
        " is_admin, is_disabled, last_active_at, daily_brief_time_zone)"
        f" VALUES ('{ADMIN_ID}', {GITHUB_USER_ID}, 'ci-read-admin',"
        " '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, 0,"
        " '2026-11-20T00:00:00Z', 'UTC')"
        " ON CONFLICT (id) DO UPDATE SET is_admin = 1, is_disabled = 0"
    )

    # Followed repository: `user_release_visible_repos` requires is_following
    # and a non-personal first_source, so the probe's owner/repo/tag fixtures
    # resolve through the production view instead of a bespoke query.
    statements.append(
        "INSERT INTO public.user_repo_associations (id, user_id, repo_id, repo_full_name,"
        " repo_full_name_lower, owner_login, repo_name, html_url, is_private, first_source,"
        " first_associated_at, last_seen_at, is_following, has_github_star_source,"
        " created_at, updated_at)"
        f" VALUES ('{ADMIN_ID}-{REPO_ID}', '{ADMIN_ID}', {REPO_ID}, '{FULL_NAME}',"
        f" '{FULL_NAME}', '{OWNER}', '{REPO}', 'https://github.com/{FULL_NAME}', 0,"
        " 'github_star', '2026-01-01T00:00:00Z', '2026-11-20T00:00:00Z', 1, 1,"
        " '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        " ON CONFLICT (id) DO UPDATE SET is_following = 1"
    )

    statements.append(
        "INSERT INTO public.repo_releases (id, repo_id, release_id, tag_name, name, body,"
        " html_url, published_at, created_at, is_prerelease, is_draft, updated_at, detected_at)"
        f" VALUES ('{ADMIN_ID}-release', {REPO_ID}, {RELEASE_ID}, '{TAG}', 'CI release',"
        " '- acceptance body',"
        f" 'https://github.com/{FULL_NAME}/releases/tag/{TAG}',"
        " '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z', 0, 0,"
        " '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )

    # The announcement route first tries a live GitHub GraphQL read and only
    # falls back to this cached row when that call fails. CI has no GitHub
    # credentials, so the cached `announcement` row is the deterministic path.
    statements.append(
        "INSERT INTO public.social_activity_events (id, user_id, kind, repo_id,"
        " repo_full_name, title, body, html_url, actor_github_user_id, actor_login,"
        " discussion_number, occurred_at, detected_at, created_at, updated_at)"
        f" VALUES ('{ACTIVITY_ID}', '{ADMIN_ID}', 'announcement', {REPO_ID},"
        f" '{FULL_NAME}', 'CI 公告', '# 公告内容',"
        f" 'https://github.com/{FULL_NAME}/discussions/{DISCUSSION_NUMBER}',"
        f" 900002, 'ci-read-actor', {DISCUSSION_NUMBER}, '2026-11-19T00:00:00Z',"
        " '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.briefs (id, user_id, date, generation_source, content_markdown,"
        " created_at, updated_at)"
        f" VALUES ('{BRIEF_ID}', '{ADMIN_ID}', '2026-11-20', 'legacy', '# CI brief',"
        " now(), now())"
        " ON CONFLICT (id) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by,"
        " payload_json, cancel_requested, log_file_path, created_at, updated_at)"
        f" VALUES ('{TASK_ID}', 'sync.access_refresh', 'succeeded', 'ci',"
        f" '{ADMIN_ID}', '{{}}', 0, '{log_path}', now(), now())"
        " ON CONFLICT (id) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.llm_calls (id, status, source, model, requested_by, max_tokens,"
        " prompt_text, created_at, updated_at)"
        f" VALUES ('{CALL_ID}', 'succeeded', 'ci', 'ci-model', '{ADMIN_ID}', 10,"
        " 'ci prompt', '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.translation_requests (id, mode, source, request_origin,"
        " requested_by, scope_user_id, producer_ref, kind, variant, entity_id, target_lang,"
        " max_wait_ms, source_hash, source_blocks_json, target_slots_json, status,"
        " result_status, title_zh, created_at, updated_at)"
        f" VALUES ('{REQUEST_ID}', 'async', 'ci', 'system', '{ADMIN_ID}', '{ADMIN_ID}',"
        f" 'ci-ref', 'release_summary', 'default', '{RELEASE_ID}', 'zh-CN', 1000,"
        " 'ci-hash', '[]', '[]', 'completed', 'ready', 'CI 标题', now(), now())"
        " ON CONFLICT (id) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.translation_batches (id, partition_key, protocol_version,"
        " model_profile, target_lang, trigger_reason, worker_slot, request_count,"
        " item_count, estimated_input_tokens, status, created_at, updated_at)"
        f" VALUES ('{BATCH_ID}', 'ci-partition', 'translation-request.v1',"
        " 'ci-model', 'zh-CN', 'ci', 0, 1, 1, 10, 'completed', now(), now())"
        " ON CONFLICT (id) DO NOTHING"
    )

    # `online_migration_operations` references the run row, so the parent must
    # exist before the operation the probe resolves as its fixture.
    statements.append(
        "INSERT INTO public.online_migration_runs (migration_id, definition_checksum,"
        " status, pause_requested, created_at, updated_at)"
        f" VALUES ('{MIGRATION_ID}', 'ci-checksum', 'running', 0,"
        " '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z')"
        " ON CONFLICT (migration_id) DO NOTHING"
    )
    statements.append(
        "INSERT INTO public.online_migration_operations (migration_id, operation_id,"
        " definition_checksum, operation_kind, operation_order, status, cursor,"
        " rows_processed, updated_at)"
        f" VALUES ('{MIGRATION_ID}', 'ci-operation', 'ci-checksum', 'backfill', 1,"
        " 'pending', '', 0, '2026-11-19T00:00:00Z')"
        " ON CONFLICT (migration_id, operation_id) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.public_repo_release_usage (id, repo_id, owner_login, repo_name,"
        " full_name, full_name_lower, first_registered_at, last_requested_at, api_list_requests,"
        " api_detail_requests, page_list_requests, page_detail_requests, last_sync_status,"
        " created_at, updated_at, access_kind)"
        f" VALUES ('{USAGE_ID}', {REPO_ID}, '{OWNER}', '{REPO}', '{FULL_NAME}', '{FULL_NAME}',"
        " '2026-01-01T00:00:00Z', '2026-11-20T00:00:00Z', 1, 0, 1, 0, 'ready',"
        " '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'github_public')"
        " ON CONFLICT (full_name_lower) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.daily_brief_hour_slots (hour_utc, enabled, updated_at)"
        f" VALUES ({HOUR_UTC}, 1, '2026-01-01T00:00:00Z')"
        " ON CONFLICT (hour_utc) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.user_api_keys (id, user_id, name, key_hash, key_ciphertext,"
        " key_nonce, key_prefix, masked_key, created_at)"
        f" VALUES ('{API_KEY_ID}', '{ADMIN_ID}', 'ci key', 'ci-key-hash',"
        f" {CIPHERTEXT}, {NONCE}, 'octo_ci', 'octo_ci****', '2026-01-01T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.user_passkeys (id, user_id, credential_id, label, passkey_json,"
        " created_at)"
        f" VALUES ('{PASSKEY_ID}', '{ADMIN_ID}', 'ci-credential', 'ci passkey',"
        " '{}', '2026-01-01T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )

    statements.append(
        "INSERT INTO public.github_connections (id, user_id, github_user_id, login,"
        " access_token_ciphertext, access_token_nonce, scopes, linked_at, updated_at)"
        f" VALUES ('{CONNECTION_ID}', '{ADMIN_ID}', {GITHUB_USER_ID}, 'ci-read-admin',"
        f" {CIPHERTEXT}, {NONCE}, 'read:user repo', '2026-01-01T00:00:00Z',"
        " '2026-01-01T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )

    # The read surfaces `fetch_one` these singletons, so an empty table aborts
    # the whole response instead of returning empty state.
    statements.append(
        "INSERT INTO public.content_identity_upgrade_control (id, generation, status,"
        " phase, updated_at)"
        " VALUES (1, 1, 'completed', 'complete', '2026-11-19T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )
    statements.append(
        "INSERT INTO public.search_projection_backfill_state (id, phase, cursor, status,"
        " updated_at)"
        " VALUES (1, 'complete', 0, 'ready', '2026-11-19T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )

    # The runtime settings singleton is required by the LLM and scheduler reads;
    # an empty model list is intentional because no upstream provider is reachable
    # from CI.
    statements.append(
        "INSERT INTO public.admin_runtime_settings (id, llm_max_concurrency,"
        " translation_general_worker_concurrency, translation_dedicated_worker_concurrency,"
        " llm_models_json, created_at, updated_at)"
        " VALUES (1, 1, 1, 1, '[]', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')"
        " ON CONFLICT (id) DO NOTHING"
    )

    psql(url, ';'.join(statements) + ';')


def task_log(path):
    """The realtime log route streams a file, so the fixture must exist on disk."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('{"level":"INFO","message":"ci read acceptance task log"}\n')


def cookie_file(path, url, binary_dir):
    helper = (binary_dir / 'octo-rill-validation-session').resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.unlink(missing_ok=True)
    subprocess.run(
        [
            str(helper),
            '--postgres-url', url,
            '--user-id', ADMIN_ID,
            '--cookie-file', str(path),
            '--isolated-validation-copy',
        ],
        check=True,
        stdout=subprocess.DEVNULL,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--postgres-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--cookie-file', required=True, type=pathlib.Path)
    parser.add_argument('--task-log', type=pathlib.Path, default=pathlib.Path('/tmp/ci-read-task.log'))
    args = parser.parse_args()

    task_log(args.task_log)
    seed(args.postgres_url, args.task_log)
    cookie_file(args.cookie_file, args.postgres_url, args.binary_dir)
    print(json.dumps({
        'seeded': True,
        'admin_id': ADMIN_ID,
        'release_id': RELEASE_ID,
        'task_id': TASK_ID,
        'cookie_file': str(args.cookie_file),
    }))


if __name__ == '__main__':
    main()
