#!/usr/bin/env python3
"""Validate PostgreSQL admin mutations using a synthetic isolated database."""
import argparse
import base64
import datetime as dt
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
from urllib.parse import urlsplit, urlunsplit

ALPHABET = '23456789abcdefghjkmnpqrstuvwxyz'


def local_id(seed):
    digest = hashlib.sha256(seed.encode()).digest()
    return ''.join(ALPHABET[digest[i] % len(ALPHABET)] for i in range(16))


def iso(days_ago=0):
    moment = dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=days_ago)
    return moment.replace(microsecond=0).isoformat().replace('+00:00', 'Z')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3023)
    args = parser.parse_args()
    name = 'octo_rill_admin_actions_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))

    def sql(query, db=url):
        return psql(db, query)

    sql('CREATE DATABASE ' + name, args.admin_url)
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
    )
    process = None
    admin = local_id('seed-admin')
    user = local_id('seed-user')
    missing_user = local_id('missing-user')
    usage_id = local_id('public-usage')
    call_id = local_id('llm-call')
    missing_call = local_id('missing-call')
    try:
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)
        sql(
            "INSERT INTO public.users(id,github_user_id,login,created_at,updated_at,is_admin,is_disabled,"
            "daily_brief_time_zone,include_own_releases,webhook_push_enabled,webhook_push_desired_state) VALUES"
            f"('{admin}',101,'admin','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,0,NULL,0,0,'deleted'),"
            f"('{user}',102,'user','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,'UTC',0,0,'deleted');"
            "DROP TABLE IF EXISTS public.admin_runtime_settings CASCADE; CREATE TABLE public.admin_runtime_settings(id bigint PRIMARY KEY,"
            "llm_max_concurrency bigint NOT NULL,translation_general_worker_concurrency bigint NOT NULL,"
            "translation_dedicated_worker_concurrency bigint NOT NULL,ai_model_context_limit bigint,"
            "ai_model_context_limit_migrated_at text,llm_models_json text NOT NULL DEFAULT '[]',"
            "daily_brief_schedule_local_time text NOT NULL DEFAULT '06:00',created_at text NOT NULL,updated_at text NOT NULL);"
            "INSERT INTO public.admin_runtime_settings(id,llm_max_concurrency,translation_general_worker_concurrency,"
            "translation_dedicated_worker_concurrency,ai_model_context_limit,llm_models_json,"
            "daily_brief_schedule_local_time,created_at,updated_at) VALUES"
            "(1,1,3,1,NULL,'[\"seed-model\"]','06:00','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
            "DROP TABLE IF EXISTS public.llm_calls CASCADE; CREATE TABLE public.llm_calls(id text PRIMARY KEY,status text,model text,created_at text,"
            "finished_at text,scheduler_wait_ms bigint,duration_ms bigint);"
            f"INSERT INTO public.llm_calls VALUES('{call_id}','succeeded','seed-model',"
            "'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',10,20);"
            "DROP TABLE IF EXISTS public.content_attempt_llm_calls CASCADE; CREATE TABLE public.content_attempt_llm_calls(id text PRIMARY KEY);"
            "DROP TABLE IF EXISTS public.public_repo_release_usage CASCADE; CREATE TABLE public.public_repo_release_usage(id text PRIMARY KEY,repo_id bigint,"
            "owner_login text NOT NULL,repo_name text NOT NULL,full_name text NOT NULL,"
            "full_name_lower text NOT NULL UNIQUE,first_registered_at text NOT NULL,last_requested_at text NOT NULL,"
            "last_list_requested_at text,last_detail_requested_at text,api_list_requests bigint NOT NULL DEFAULT 0,"
            "api_detail_requests bigint NOT NULL DEFAULT 0,page_list_requests bigint NOT NULL DEFAULT 0,"
            "page_detail_requests bigint NOT NULL DEFAULT 0,last_sync_status text NOT NULL DEFAULT 'pending',"
            "last_sync_error text,created_at text NOT NULL,updated_at text NOT NULL);"
            f"INSERT INTO public.public_repo_release_usage VALUES('{usage_id}',700001,'octo','demo','octo/demo',"
            "'octo/demo','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',NULL,NULL,0,0,0,0,'ready',NULL,"
            "'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
            "DROP TABLE IF EXISTS public.repo_releases CASCADE; CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY,repo_id bigint,"
            "published_at text,created_at text,detected_at text,is_draft bigint);"
            "DROP TABLE IF EXISTS public.ai_translations CASCADE; CREATE TABLE public.ai_translations(id text,entity_type text,entity_id text,lang text,status text,"
            "title text,summary text,source_hash text,user_id text,updated_at text);"
            f"INSERT INTO public.ai_translations VALUES('tr-1','release_detail','900001','zh-CN','ready',"
            f"'标题',NULL,'hash-1','{admin}','2026-01-01T00:00:00Z'),"
            f"('tr-2','release_detail','900003','zh-CN','ready','标题',NULL,'hash-2','{admin}',"
            "'2026-01-01T00:00:00Z');"
            "DROP VIEW IF EXISTS public.user_release_visible_repos CASCADE; CREATE TABLE public.user_release_visible_repos(user_id text,repo_id bigint);"
            "DROP TABLE IF EXISTS public.repo_release_work_items CASCADE; CREATE TABLE public.repo_release_work_items(id text,repo_id bigint,repo_full_name text);"
            "DROP TABLE IF EXISTS public.repo_release_sync_state CASCADE; CREATE TABLE public.repo_release_sync_state(repo_id bigint PRIMARY KEY);"
            "DROP TABLE IF EXISTS public.content_legacy_observations CASCADE; CREATE TABLE public.content_legacy_observations(id text PRIMARY KEY,legacy_table text NOT NULL,"
            "legacy_primary_key text NOT NULL,canonical_resource_type text,"
            "canonical_resource_id text,pipeline text,classification text NOT NULL,"
            "observation_basis_json text NOT NULL,observed_at text NOT NULL,"
            "UNIQUE (legacy_table,legacy_primary_key));"
            "DROP TABLE IF EXISTS public.content_identity_upgrade_control CASCADE; CREATE TABLE public.content_identity_upgrade_control(id integer PRIMARY KEY,generation bigint NOT NULL,"
            "status text NOT NULL,phase text NOT NULL,cursor text,last_error_code text,started_at text,"
            "completed_at text,updated_at text NOT NULL);"
            "INSERT INTO public.content_identity_upgrade_control VALUES"
            "(1,1,'running','work_identity_backfill',NULL,NULL,'2026-01-01T00:00:00Z',NULL,'2026-01-01T00:00:00Z');"
            "DROP TABLE IF EXISTS public.content_work_identities CASCADE; CREATE TABLE public.content_work_identities(id text,canonical_resource_type text,"
            "canonical_resource_id text,pipeline text,variant text,target_lang text,source_hash text,"
            "protocol_version text,created_at text,updated_at text);"
            "DROP TABLE IF EXISTS public.content_current_result_projections CASCADE; CREATE TABLE public.content_current_result_projections(identity_id text,work_item_id text,"
            "active_work_item_id text,source_projection_id text,payload_json text,published_at text,updated_at text);"
            "DROP TABLE IF EXISTS public.search_projection_backfill_state CASCADE; CREATE TABLE public.search_projection_backfill_state(id integer PRIMARY KEY,phase text NOT NULL,"
            "cursor bigint NOT NULL DEFAULT 0,status text NOT NULL,last_error text,updated_at text NOT NULL);"
            "INSERT INTO public.search_projection_backfill_state VALUES(1,'releases',0,'failed','boom','2026-01-01T00:00:00Z');"
            "DROP TABLE IF EXISTS public.search_metadata_backfill_queue CASCADE; CREATE TABLE public.search_metadata_backfill_queue(id text);"
            "DROP TABLE IF EXISTS public.search_projection_outbox CASCADE; CREATE TABLE public.search_projection_outbox(id text);"
            "DROP TABLE IF EXISTS public.search_projection_outbox_failed CASCADE; CREATE TABLE public.search_projection_outbox_failed(projection_key text,last_error text,failed_at text);"
            "DROP TABLE IF EXISTS public.release_reading_backfill_runs CASCADE; CREATE TABLE public.release_reading_backfill_runs(id text PRIMARY KEY,started_at text NOT NULL,"
            "window_start_utc text NOT NULL,window_end_utc text NOT NULL,status text NOT NULL,"
            "cursor_published_at text,cursor_release_id bigint,total bigint NOT NULL DEFAULT 0,"
            "succeeded bigint NOT NULL DEFAULT 0,failed bigint NOT NULL DEFAULT 0,waiting bigint NOT NULL DEFAULT 0,"
            "skipped bigint NOT NULL DEFAULT 0,lease_owner text,lease_task_id text,lease_expires_at text,"
            "error_text text,created_at text NOT NULL,updated_at text NOT NULL);"
            "DROP TABLE IF EXISTS public.release_reading_backfill_items CASCADE; CREATE TABLE public.release_reading_backfill_items(run_id text NOT NULL,release_id bigint NOT NULL,"
            "authorized_user_id text,published_at text NOT NULL,status text NOT NULL,"
            "attempt_count bigint NOT NULL DEFAULT 0,error_text text,updated_at text NOT NULL,"
            "PRIMARY KEY(run_id,release_id));"
            "INSERT INTO public.online_migration_runs(migration_id,definition_checksum,status,pause_requested,"
            "created_at,updated_at) VALUES('content-processing-online-v2','checksum','running',0,"
            "'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
            "INSERT INTO public.online_migration_operations(migration_id,operation_id,definition_checksum,"
            "operation_kind,operation_order,status,cursor,rows_processed,updated_at) VALUES"
            "('content-processing-online-v2','op-1','checksum','copy',1,'failed','',0,'2026-01-01T00:00:00Z');"
        )
        # Release rows for the public-usage cache cleanup and the backfill window.
        recent = iso(1)
        older = iso(45)
        sql(
            "INSERT INTO public.repo_releases VALUES"
            f"(900001,700001,'2026-01-05T00:00:00Z','2026-01-05T00:00:00Z','2026-01-05T00:00:00Z',0),"
            f"(900002,700002,'{recent}','{recent}','{recent}',0),"
            f"(900003,700002,'{older}','{older}','{older}',0);"
            f"INSERT INTO public.user_release_visible_repos VALUES('{admin}',700002);"
        )
        with tempfile.TemporaryDirectory(prefix='octo-rill-admin-test-') as tmp:
            tmp = pathlib.Path(tmp)
            cookies = {}
            for seed_user in (admin, user):
                file = tmp / seed_user
                subprocess.run(
                    [helper, '--postgres-url', url, '--user-id', seed_user,
                     '--cookie-file', file, '--isolated-validation-copy'],
                    check=True, stdout=subprocess.DEVNULL,
                )
                cookies[seed_user] = file.read_text().strip()
            env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
            log = (tmp / 'server.log').open('w+')
            process = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
            base = f'http://127.0.0.1:{args.port}'
            for _ in range(100):
                try:
                    urllib.request.urlopen(base + '/health', timeout=1).close()
                    break
                except Exception:
                    if process.poll() is not None:
                        log.seek(0)
                        raise RuntimeError(log.read()[-5000:])
                    time.sleep(.1)
            else:
                raise RuntimeError('validation server did not become ready')

            def request(method, path, body=None, as_user=admin):
                headers = {'Cookie': cookies[as_user]} if as_user else {}
                data = None
                if body is not None:
                    data = json.dumps(body).encode()
                    headers['Content-Type'] = 'application/json'
                req = urllib.request.Request(base + path, data=data, headers=headers, method=method)
                try:
                    with urllib.request.urlopen(req, timeout=10) as response:
                        return response.status, json.load(response)
                except urllib.error.HTTPError as error:
                    payload = json.load(error)
                    if error.code >= 500:
                        print('--- server log tail ---', flush=True)
                        print((tmp / 'server.log').read_text()[-4000:], flush=True)
                    return error.code, payload

            # --- route inventory authentication -------------------------------
            literal_routes = [
                ('DELETE', f'/api/admin/public-release-repos/{usage_id}', None),
                ('POST', '/api/admin/jobs/release-reading/backfill', None),
                ('PATCH', '/api/admin/jobs/llm/runtime-config', {'max_concurrency': 2}),
                ('POST', f'/api/admin/jobs/llm/calls/{call_id}/diagnostic-access', {'action': 'reveal'}),
                ('POST', '/api/admin/jobs/migrations/content-processing-online-v2/pause', None),
                ('POST', '/api/admin/jobs/migrations/content-processing-online-v2/resume', None),
                ('POST', '/api/admin/jobs/content-processing/freeze', {'switch_token': 'switch'}),
                ('POST', '/api/admin/jobs/content-processing/cutover', {'switch_token': 'switch'}),
                ('POST', '/api/admin/jobs/content-processing/identity-upgrade', {'action': 'pause'}),
                ('POST', '/api/admin/jobs/search-index/resume', None),
                ('PATCH', f'/api/admin/users/{user}/profile', {'daily_brief_time_zone': 'UTC'}),
                ('PATCH', '/admin/jobs/translations/runtime-config',
                 {'general_worker_concurrency': 1, 'dedicated_worker_concurrency': 1}),
                ('PATCH', '/api/admin/jobs/translations/runtime-config',
                 {'general_worker_concurrency': 1, 'dedicated_worker_concurrency': 1}),
            ]
            for method, path, body in literal_routes:
                code, payload = request(method, path, body, as_user=None)
                assert code == 401 and payload['code'] == 'admin_required', (method, path, code, payload)
                code, payload = request(method, path, body, as_user=user)
                assert code == 403 and payload['code'] == 'admin_required', (method, path, code, payload)

            # --- public release repo deletion ---------------------------------
            assert request('DELETE', '/api/admin/public-release-repos/not-a-local-id')[0] == 400
            code, body = request('DELETE', f'/api/admin/public-release-repos/{usage_id}')
            assert code == 200, body
            cleanup = body['cache_cleanup']
            assert cleanup == {
                'repo_id': 700001,
                'full_name': 'octo/demo',
                'deleted_release_count': 1,
                'deleted_ai_cache_count': 1,
                'skipped_reason': None,
            }, cleanup
            assert body['total'] == 0
            assert sql(f"SELECT COUNT(*) FROM public.public_repo_release_usage WHERE id='{usage_id}'") == '0'
            assert sql('SELECT COUNT(*) FROM public.repo_releases WHERE repo_id=700001') == '0'
            assert request('DELETE', f'/api/admin/public-release-repos/{usage_id}')[0] == 404
            assert request('DELETE', f'/api/admin/public-release-repos/{local_id("unknown")}')[0] == 404
            code, listed = request('GET', '/api/admin/public-release-repos')
            assert code == 200 and listed['total'] == 0

            # --- release reading backfill -------------------------------------
            code, body = request('POST', '/api/admin/jobs/release-reading/backfill')
            assert code == 200 and body['status'] == 'queued' and body['task_id'], body
            first_task = body['task_id']
            run = sql('SELECT status,total FROM public.release_reading_backfill_runs').split('|')
            assert run == ['queued', '1'], run
            assert sql('SELECT release_id FROM public.release_reading_backfill_items') == '900002'
            code, body = request('POST', '/api/admin/jobs/release-reading/backfill')
            assert code == 200 and body['task_id'] == first_task and body['status'] == 'queued', body
            assert sql(
                "SELECT COUNT(*) FROM public.job_tasks WHERE task_type='release.reading_backfill'"
            ) == '1'

            # --- LLM runtime configuration ------------------------------------
            code, body = request('PATCH', '/api/admin/jobs/llm/runtime-config', {
                'max_concurrency': 3,
                'ai_model_context_limit': 4096,
                'llm_models': ['seed-model', 'other-model'],
                'llm_recovery_enabled': True,
                'llm_recovery_rollout_percent': 42,
            })
            assert code == 200, body
            assert body['max_concurrency'] == 3 and body['ai_model_context_limit'] == 4096
            assert body['llm_models'] == ['seed-model', 'other-model']
            assert body['llm_recovery_enabled'] is True and body['llm_recovery_rollout_percent'] == 42
            assert body['effective_model_input_limit'] == 4096
            code, body = request('GET', '/api/admin/jobs/llm/status')
            assert code == 200 and body['llm_models'] == ['seed-model', 'other-model']
            assert body['ai_model_context_limit'] == 4096 and body['max_concurrency'] == 3
            assert sql(
                "SELECT llm_models_json FROM public.admin_runtime_settings WHERE id=1"
            ) == '["seed-model","other-model"]'
            assert sql(
                "SELECT llm_recovery_rollout_percent FROM public.llm_recovery_flags WHERE id=1"
            ) == '42'
            code, body = request('PATCH', '/api/admin/jobs/llm/runtime-config', {
                'ai_model_context_limit': None,
                'max_concurrency': 3,
            })
            assert code == 200 and body['ai_model_context_limit'] is None, body
            assert body['llm_models'] == ['seed-model', 'other-model'], 'omitted models must be preserved'
            assert request('PATCH', '/api/admin/jobs/llm/runtime-config', {'max_concurrency': 0})[0] == 400
            assert request('PATCH', '/api/admin/jobs/llm/runtime-config',
                           {'max_concurrency': 2, 'llm_models': ['  ']})[0] == 400
            assert request('PATCH', '/api/admin/jobs/llm/runtime-config',
                           {'max_concurrency': 2,
                            'llm_models': ['a-model', ' a-model ']})[0] == 400
            assert request('PATCH', '/api/admin/jobs/llm/runtime-config',
                           {'max_concurrency': 2,
                            'llm_recovery_rollout_percent': 101})[0] == 400

            # --- diagnostic access audit --------------------------------------
            code, body = request('POST', f'/api/admin/jobs/llm/calls/{call_id}/diagnostic-access',
                                 {'action': 'copy'})
            assert code == 200 and body == {'ok': True}, body
            assert sql(
                "SELECT action FROM public.llm_diagnostic_access_audit "
                f"WHERE call_id='{call_id}' AND actor_user_id='{admin}'"
            ) == 'copy'
            assert request('POST', f'/api/admin/jobs/llm/calls/{call_id}/diagnostic-access',
                           {'action': 'download'})[0] == 400
            assert request('POST', f'/api/admin/jobs/llm/calls/{missing_call}/diagnostic-access',
                           {'action': 'reveal'})[0] == 404
            assert request('POST', '/api/admin/jobs/llm/calls/not-a-local-id/diagnostic-access',
                           {'action': 'reveal'})[0] == 400

            # --- online migration pause / resume -------------------------------
            code, body = request('POST', '/api/admin/jobs/migrations/content-processing-online-v2/pause')
            assert code == 200 and body['run']['pause_requested'] is True, body
            assert sql("SELECT pause_requested FROM public.online_migration_runs") == '1'
            code, body = request('POST', '/api/admin/jobs/migrations/content-processing-online-v2/resume')
            assert code == 200 and body['run']['pause_requested'] is False, body
            assert body['run']['status'] == 'running'
            assert body['operations'][0]['status'] == 'pending'
            assert request('POST', '/api/admin/jobs/migrations/other-migration/pause')[0] == 404
            assert request('POST', '/api/admin/jobs/migrations/other-migration/resume')[0] == 404

            # --- content processing mode transitions ---------------------------
            assert request('POST', '/api/admin/jobs/content-processing/freeze',
                           {'switch_token': '  '})[0] == 400
            assert request('POST', '/api/admin/jobs/content-processing/cutover',
                           {'switch_token': 'token'})[0] == 409
            code, body = request('POST', '/api/admin/jobs/content-processing/freeze',
                                 {'switch_token': 'token'})
            assert code == 200 and body == {'mode': 'rollback_freeze'}, body
            assert sql('SELECT mode FROM public.content_processing_control WHERE id=1') == 'rollback_freeze'
            assert request('POST', '/api/admin/jobs/content-processing/freeze',
                           {'switch_token': 'token'})[0] == 409
            code, body = request('POST', '/api/admin/jobs/content-processing/cutover',
                                 {'switch_token': 'token'})
            assert code == 200 and body == {'mode': 'global'}, body
            assert sql('SELECT mode FROM public.content_processing_control WHERE id=1') == 'global'
            assert sql('SELECT COUNT(*) FROM public.content_legacy_observations') == '1'
            assert request('POST', '/api/admin/jobs/content-processing/cutover',
                           {'switch_token': 'token'})[0] == 409

            # --- identity upgrade control --------------------------------------
            code, body = request('GET', '/api/admin/jobs/content-processing/identity-upgrade')
            assert code == 200 and body['status'] == 'running', body
            code, body = request('POST', '/api/admin/jobs/content-processing/identity-upgrade',
                                 {'action': 'pause'})
            assert code == 200 and body['status'] == 'paused', body
            code, body = request('POST', '/api/admin/jobs/content-processing/identity-upgrade',
                                 {'action': 'resume'})
            assert code == 200 and body['status'] == 'running', body
            sql("UPDATE public.content_identity_upgrade_control SET status='completed' WHERE id=1")
            code, body = request('POST', '/api/admin/jobs/content-processing/identity-upgrade',
                                 {'action': 'pause'})
            assert code == 409 and body['code'] == 'content_identity_upgrade_state_conflict', body
            sql("UPDATE public.content_identity_upgrade_control SET status='running' WHERE id=1")

            # --- search index resume -------------------------------------------
            code, body = request('POST', '/api/admin/jobs/search-index/resume')
            assert code == 200 and body['status'] == 'building', body
            assert sql(
                "SELECT status FROM public.search_projection_backfill_state WHERE id=1"
            ) == 'building'
            code, body = request('POST', '/api/admin/jobs/search-index/resume')
            assert code == 409 and body['code'] == 'search_projection_backfill_state_conflict', body

            # --- admin user profile --------------------------------------------
            assert request('PATCH', f'/api/admin/users/{user}/profile',
                           {'daily_brief_time_zone': 'Asia/Kathmandu'})[0] == 400
            assert request('PATCH', f'/api/admin/users/{user}/profile',
                           {'daily_brief_time_zone': 'not a zone'})[0] == 400
            assert request('PATCH', f'/api/admin/users/{missing_user}/profile',
                           {'daily_brief_time_zone': 'UTC'})[0] == 404
            code, body = request('PATCH', f'/api/admin/users/{user}/profile',
                                 {'daily_brief_time_zone': 'Asia/Shanghai',
                                  'include_own_releases': True})
            assert code == 200, body
            assert body['daily_brief_time_zone'] == 'Asia/Shanghai' and body['include_own_releases'] is True
            assert sql(
                f"SELECT daily_brief_time_zone,include_own_releases FROM public.users WHERE id='{user}'"
            ) == 'Asia/Shanghai|1'
            # Disabling "my releases" while the desired webhook state is enabled
            # must be rejected; the legacy conflict code is preserved.
            sql(f"UPDATE public.users SET include_own_releases=0,webhook_push_desired_state='enabled',"
                f"webhook_push_enabled=1 WHERE id='{user}'")
            code, body = request('PATCH', f'/api/admin/users/{user}/profile',
                                 {'daily_brief_time_zone': 'UTC'})
            assert code == 409 and body['code'] == 'my_releases_required', body
            code, body = request('PATCH', f'/api/admin/users/{user}/profile',
                                 {'daily_brief_time_zone': 'Asia/Shanghai',
                                  'include_own_releases': True})
            assert code == 200 and body['include_own_releases'] is True, body
            code, body = request('PATCH', f'/api/admin/users/{user}/profile',
                                 {'daily_brief_time_zone': 'UTC', 'include_own_releases': False})
            assert code == 200 and body['include_own_releases'] is False, body
            assert sql(
                f"SELECT webhook_push_desired_state FROM public.users WHERE id='{user}'"
            ) == 'paused'
            assert sql(
                "SELECT COUNT(*) FROM public.job_tasks "
                f"WHERE task_type='webhook.push.manage' AND requested_by='{user}'"
            ) == '1'
            sql(f"UPDATE public.users SET webhook_push_desired_state='enabled',"
                f"webhook_push_enabled=1,include_own_releases=1 WHERE id='{user}'")
            code, body = request('PATCH', f'/api/admin/users/{user}/profile',
                                 {'daily_brief_time_zone': 'UTC', 'include_own_releases': False})
            assert code == 409 and body['code'] == 'webhook_push_operation_in_progress', body
            code, body = request('GET', f'/api/admin/users/{user}/profile')
            assert code == 200 and body['daily_brief_time_zone'] == 'UTC', body

            # --- translation runtime configuration ------------------------------
            assert request('PATCH', '/admin/jobs/translations/runtime-config',
                           {'general_worker_concurrency': 0,
                            'dedicated_worker_concurrency': 1})[0] == 400
            assert request('PATCH', '/admin/jobs/translations/runtime-config',
                           {'general_worker_concurrency': 64,
                            'dedicated_worker_concurrency': 1})[0] == 400
            code, body = request('PATCH', '/admin/jobs/translations/runtime-config',
                                 {'general_worker_concurrency': 5,
                                  'dedicated_worker_concurrency': 2})
            assert code == 200, body
            assert sql(
                "SELECT translation_general_worker_concurrency,"
                "translation_dedicated_worker_concurrency FROM public.admin_runtime_settings WHERE id=1"
            ) == '5|2'
            code, body = request('PATCH', '/api/admin/jobs/translations/runtime-config',
                                 {'general_worker_concurrency': 4,
                                  'dedicated_worker_concurrency': 1})
            assert code == 200, body
            assert sql(
                "SELECT translation_general_worker_concurrency,"
                "translation_dedicated_worker_concurrency FROM public.admin_runtime_settings WHERE id=1"
            ) == '4|1'

            print(json.dumps({
                'route_authentication': 'pass',
                'public_repo_delete_and_cleanup': 'pass',
                'release_reading_backfill': 'pass',
                'llm_runtime_config': 'pass',
                'diagnostic_access_audit': 'pass',
                'online_migration_control': 'pass',
                'content_processing_transitions': 'pass',
                'identity_upgrade_control': 'pass',
                'search_index_resume': 'pass',
                'admin_user_profile': 'pass',
                'translation_runtime_config': 'pass',
            }))
    finally:
        if process is not None:
            process.send_signal(2)
            process.wait(timeout=15)
        sql('DROP DATABASE ' + name + ' WITH (FORCE)', args.admin_url)


if __name__ == '__main__':
    main()
