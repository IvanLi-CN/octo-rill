#!/usr/bin/env python3
"""Seed an isolated PG18 database and verify the disabled-worker HTTP contract."""
import argparse
import base64
import json
import os
import pathlib
import subprocess

import tempfile
import time
import urllib.error
import urllib.request
from pg_seed_common import psql


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--admin-url', required=True)
    p.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    p.add_argument('--port', type=int, default=3019)
    args = p.parse_args()
    name = 'octo_rill_seed_' + str(os.getpid())
    from urllib.parse import urlsplit, urlunsplit
    parts = urlsplit(args.admin_url)
    url = urlunsplit(parts._replace(path='/' + name))
    def sql(query, database=args.admin_url):
        return psql(database, query)
    sql('CREATE DATABASE ' + name)
    env = dict(os.environ, DATABASE_URL=url, OCTORILL_ENCRYPTION_KEY_BASE64=base64.b64encode(bytes(32)).decode(),
               GITHUB_CLIENT_ID='validation', GITHUB_CLIENT_SECRET='validation',
               GITHUB_OAUTH_REDIRECT_URL=f'http://127.0.0.1:{args.port}/auth/github/callback',
               OCTORILL_BIND_ADDR=f'127.0.0.1:{args.port}', OCTORILL_PG_WORKER_ENABLED='0', OCTORILL_PG_SCHEDULER_ENABLED='0',
               OCTORILL_PG_BOOTSTRAP_ONLY='1')
    app = (args.binary_dir / 'octo-rill').resolve()
    helper = (args.binary_dir / 'octo-rill-validation-session').resolve()
    server = None
    try:
        subprocess.run([app], env=env, check=True, stdout=subprocess.DEVNULL)
        sql("INSERT INTO public.users(id,github_user_id,login,created_at,updated_at,is_admin) VALUES ('seed-user',101,'seed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1);"
            "INSERT INTO public.github_connections(id,user_id,github_user_id,login,access_token_ciphertext,access_token_nonce,scopes,linked_at,updated_at) VALUES ('seed-connection','seed-user',101,'seed','\\x00','\\x00','','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
            "INSERT INTO public.job_tasks(id,task_type,status,source,requested_by,payload_json,created_at,updated_at) VALUES ('seed-task','sync.starred','queued','validation','seed-user','{}',now(),now());", url)
        with tempfile.TemporaryDirectory(prefix='octo-rill-seed-') as tmp:
            cookie = pathlib.Path(tmp) / 'cookie'
            subprocess.run([helper, '--postgres-url', url, '--user-id', 'seed-user', '--cookie-file', cookie, '--isolated-validation-copy'], check=True)
            env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
            log = open('/tmp/octo-rill-worker-server.log', 'w+')
            server = subprocess.Popen([app], env=env, stdout=log, stderr=subprocess.STDOUT)
            base = f'http://127.0.0.1:{args.port}'
            for _ in range(100):
                try:
                    with urllib.request.urlopen(base + '/health', timeout=1) as r:
                        assert json.load(r)['ok'] is True
                    break
                except Exception:
                    if server.poll() is not None:
                        log.seek(0)
                        raise RuntimeError('server startup failed: ' + log.read()[-4000:])
                    time.sleep(.1)
            else:
                raise RuntimeError('server startup timed out')
            sql("DROP TABLE IF EXISTS public.admin_runtime_settings CASCADE; CREATE TABLE public.admin_runtime_settings(id bigint PRIMARY KEY, daily_brief_schedule_local_time text NOT NULL,ai_model_context_limit bigint,webhook_push_audit_interval_days bigint,webhook_push_audit_last_started_at text); INSERT INTO public.admin_runtime_settings VALUES(1,'07:30',32768,7,'2026-01-01T00:00:00Z')", url)
            for path in ('/api/me/profile', '/api/admin/users/seed-user/profile'):
                request = urllib.request.Request(base + path, headers={'Cookie': cookie.read_text().strip()})
                with urllib.request.urlopen(request, timeout=5) as response:
                    assert json.load(response)['daily_brief_schedule_local_time'] == '07:30'
            sql("DROP VIEW IF EXISTS public.user_release_visible_repos CASCADE; CREATE TABLE public.user_release_visible_repos(user_id text,repo_id bigint); INSERT INTO public.user_release_visible_repos VALUES('seed-user',1)", url)
            sql("INSERT INTO public.search_documents(id,repo_id,resource_type,resource_id,title,created_at,updated_at) VALUES('seed-document',1,'release','1','release fixture','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')", url)
            request = urllib.request.Request(base + '/api/search?q=release', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert len(json.load(response)['items']) == 1
            sql("DELETE FROM public.user_release_visible_repos", url)
            with urllib.request.urlopen(request, timeout=5) as response:
                assert json.load(response)['items'] == []
            sql("DROP TABLE IF EXISTS public.owned_repo_star_baselines CASCADE; CREATE TABLE public.owned_repo_star_baselines(user_id text,repo_id bigint,repo_full_name text,updated_at text,owner_avatar_url text,open_graph_image_url text,uses_custom_open_graph_image bigint); DROP TABLE IF EXISTS public.repo_releases CASCADE; CREATE TABLE public.repo_releases(repo_id bigint,is_draft bigint,release_id bigint); INSERT INTO public.owned_repo_star_baselines VALUES('seed-user',1,'seed/repo','2026-01-01T00:00:00Z',NULL,NULL,0); INSERT INTO public.repo_releases VALUES(1,0,1),(1,0,2),(1,1,3)", url)
            request = urllib.request.Request(base + '/api/me/personal-repos', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                result = json.load(response)
                assert result['repos'][0]['release_count'] == 2
            sql("INSERT INTO public.translation_requests(id,mode,source,request_origin,requested_by,scope_user_id,producer_ref,kind,variant,entity_id,target_lang,max_wait_ms,source_hash,source_blocks_json,target_slots_json,status,created_at,updated_at) VALUES('seed-request','async','validation','user','seed-user','seed-user','seed','release_detail','default','1','zh-CN',0,'seed','[]','[]','queued',now(),now()); INSERT INTO public.translation_batches(id,partition_key,protocol_version,model_profile,target_lang,trigger_reason,worker_slot,request_count,item_count,estimated_input_tokens,status,created_at,updated_at) VALUES('seed-batch','seed','v1','seed','zh-CN','validation',1,1,0,0,'queued',now(),now())", url)
            for path in ('/admin/jobs/translations/requests','/api/admin/jobs/translations/requests','/admin/jobs/translations/batches','/api/admin/jobs/translations/batches'):
                request = urllib.request.Request(base + path, headers={'Cookie': cookie.read_text().strip()})
                try:
                    with urllib.request.urlopen(request, timeout=5) as response:
                        body = json.load(response)
                        assert body['total'] == 1 and len(body['items']) == 1
                except Exception:
                    log.flush(); log.seek(0); print(log.read()[-8000:]); raise
            sql("DROP TABLE IF EXISTS public.llm_calls CASCADE; CREATE TABLE public.llm_calls(id text PRIMARY KEY,status text,source text,model text,scheduler_wait_ms bigint,duration_ms bigint,created_at text,parent_task_id text,parent_translation_batch_id text); INSERT INTO public.llm_calls VALUES('seed-call','succeeded','validation','seed',0,1,'2026-01-01T00:00:00Z',NULL,NULL); INSERT INTO public.translation_work_items(id,dedupe_key,scope_user_id,kind,variant,entity_id,target_lang,protocol_version,model_profile,source_hash,source_blocks_json,target_slots_json,token_estimate,deadline_at,status,created_at,updated_at) VALUES('seed-work','seed','seed-user','release_detail','default','1','zh-CN','v1','seed','seed','[]','[]',1,now(),'queued',now(),now()); UPDATE public.translation_requests SET work_item_id='seed-work' WHERE id='seed-request'; INSERT INTO public.translation_attempt_events(id,work_item_id,request_id,scope_user_id,kind,variant,entity_id,target_lang,attempt_no,trigger,event_type,retry_eligible,llm_call_ids_json,created_at) VALUES('1','seed-work','seed-request','seed-user','release_detail','default','1','zh-CN',1,'initial','attempt_queued',0,'[\"seed-call\"]',now())", url)
            request = urllib.request.Request(base + '/admin/jobs/translations/requests/seed-request', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                body = json.load(response)
                assert body['request']['id'] == 'seed-request' and len(body['attempts']) == 1 and body['attempts'][0]['llm_calls'][0]['id'] == 'seed-call'
            request = urllib.request.Request(base + '/admin/jobs/translations/attempt-events?request_id=seed-request', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert json.load(response)['total'] == 1
            request = urllib.request.Request(base + '/admin/jobs/translations/status', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                status = json.load(response)
                assert status['queued_requests'] == 1 and status['queued_work_items'] == 1
                assert status['worker_concurrency'] == 0 and status['effective_model_input_limit'] == 32768
            request = urllib.request.Request(base + '/api/admin/jobs/migrations', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert json.load(response) == {'run': None, 'operations': []}
            request = urllib.request.Request(base + '/api/admin/jobs/webhook-push/runtime-config', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert json.load(response)['next_started_at'] == '2026-01-08T00:00:00+00:00'
            sql("DROP TABLE IF EXISTS public.public_repo_release_usage CASCADE; CREATE TABLE public.public_repo_release_usage(id text,repo_id bigint,full_name text,full_name_lower text,first_registered_at text,last_requested_at text,last_list_requested_at text,last_detail_requested_at text,api_list_requests bigint,api_detail_requests bigint,page_list_requests bigint,page_detail_requests bigint,last_sync_status text,last_sync_error text,created_at text,updated_at text); DROP TABLE IF EXISTS public.ai_translations CASCADE; CREATE TABLE public.ai_translations(entity_type text,entity_id text,lang text,status text,updated_at text); INSERT INTO public.public_repo_release_usage VALUES('seed-usage',1,'seed/repo','seed/repo','2026','2026',NULL,NULL,1,0,0,0,'ready',NULL,'2026','2026')", url)
            request = urllib.request.Request(base + '/api/admin/public-release-repos', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                body = json.load(response)
                assert body['total'] == 1 and body['items'][0]['release_count'] == 3
            sql("DROP TABLE IF EXISTS public.search_projection_backfill_state CASCADE; CREATE TABLE public.search_projection_backfill_state(id bigint PRIMARY KEY,phase text,cursor bigint,status text,last_error text,updated_at text); DROP TABLE IF EXISTS public.search_metadata_backfill_queue CASCADE; CREATE TABLE public.search_metadata_backfill_queue(id text); DROP TABLE IF EXISTS public.search_projection_outbox CASCADE; CREATE TABLE public.search_projection_outbox(id text); DROP TABLE IF EXISTS public.search_projection_outbox_failed CASCADE; CREATE TABLE public.search_projection_outbox_failed(projection_key text,last_error text,failed_at text); INSERT INTO public.search_projection_backfill_state VALUES(1,'complete',5,'ready',NULL,'2026'); INSERT INTO public.search_projection_outbox VALUES('seed')", url)
            request = urllib.request.Request(base + '/api/admin/jobs/search-index', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert json.load(response)['status'] == 'building'
            sql("DROP TABLE IF EXISTS public.content_identity_upgrade_control CASCADE; CREATE TABLE public.content_identity_upgrade_control(id bigint,generation bigint,status text,phase text,cursor text,last_error_code text,started_at text,completed_at text,updated_at text); DROP TABLE IF EXISTS public.content_work_identities CASCADE; CREATE TABLE public.content_work_identities(id text,canonical_resource_type text,canonical_resource_id text,pipeline text,variant text,target_lang text,source_hash text,protocol_version text); DROP TABLE IF EXISTS public.content_current_result_projections CASCADE; CREATE TABLE public.content_current_result_projections(identity_id text); INSERT INTO public.content_identity_upgrade_control VALUES(1,1,'completed','complete','{}',NULL,NULL,NULL,'2026')", url)
            request = urllib.request.Request(base + '/api/admin/jobs/content-processing/identity-upgrade', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert json.load(response)['waiting_blocked_config_identities'] == 0
            sql("ALTER TABLE public.admin_runtime_settings ADD COLUMN sync_auto_fetch_interval_minutes bigint DEFAULT 60,ADD COLUMN sync_auto_fetch_effective_at text,ADD COLUMN star_sync_delta_interval_minutes bigint DEFAULT 30,ADD COLUMN star_sync_full_sweep_interval_minutes bigint DEFAULT 1440,ADD COLUMN retry_recent_failures_interval_minutes bigint DEFAULT 10,ADD COLUMN repo_release_worker_concurrency bigint DEFAULT 5,ADD COLUMN repo_refresh_system_budget_per_window bigint DEFAULT 1000,ADD COLUMN dashboard_release_freshness_profile text DEFAULT 'balanced'; DROP TABLE IF EXISTS public.star_sync_epochs CASCADE; CREATE TABLE public.star_sync_epochs(github_connection_id text,processed_pages bigint,processed_items bigint,total_count_at_start bigint,next_slice_not_before text,status text,created_at text,id text,completed_at text)", url)
            request = urllib.request.Request(base + '/api/admin/jobs/sync/runtime-config', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                body = json.load(response)
                assert body['sync_auto_fetch_interval_minutes'] == 60 and body['star_sync']['active_epochs'] == []
            sql("DROP TABLE IF EXISTS public.repo_refresh_governance_snapshots CASCADE; CREATE TABLE public.repo_refresh_governance_snapshots(repo_id bigint,repo_full_name text,watcher_user_count bigint,watcher_repo_total_sum bigint,cached_stargazer_count bigint,priority_rank bigint,target_window bigint,target_interval_minutes bigint,urgency_score float8,urgency_bucket text,system_last_selected_at text,system_last_success_at text,system_last_attempt_at text,system_last_attempt_status text,system_last_attempt_error text,actual_last_success_at text,actual_last_success_source text); INSERT INTO public.repo_refresh_governance_snapshots VALUES(1,'seed/repo',1,1,1,1,2,60,1.5,'warm',NULL,NULL,NULL,NULL,NULL,NULL,NULL)", url)
            request = urllib.request.Request(base + '/api/admin/repos?aging=missing&target_windows=2', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                body = json.load(response)
                assert body['total'] == 1 and body['items'][0]['repo_id'] == 1
            sql("DROP TABLE IF EXISTS public.repo_refresh_governance_cycles CASCADE; CREATE TABLE public.repo_refresh_governance_cycles(id text,status text,started_at text,completed_at text,updated_at text,frozen_repo_count bigint,completed_repo_count bigint,window_minutes bigint,window_budget bigint,last_selection_window_index bigint)", url)
            request = urllib.request.Request(base + '/api/admin/repos/overview', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                body = json.load(response)
                assert body['summary']['dedup_repo_count'] == 1 and len(body['grid_cells']) == 1
            request = urllib.request.Request(base + '/api/admin/jobs/translations/batches/seed-batch', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                body = json.load(response)
                assert body['batch']['id'] == 'seed-batch' and body['items'] == []
            log_fixture = pathlib.Path(tmp) / 'task.ndjson'
            log_fixture.write_text('{"event":"seed"}\n')
            sql("UPDATE public.job_tasks SET log_file_path='" + str(log_fixture).replace("'", "''") + "' WHERE id='seed-task'", url)
            request = urllib.request.Request(base + '/api/admin/jobs/realtime/seed-task/log', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert response.headers['Content-Type'] == 'application/x-ndjson' and response.read() == log_fixture.read_bytes()
            sql("INSERT INTO public.job_tasks(id,task_type,status,source,requested_by,payload_json,created_at,updated_at) VALUES('freshness-task','sync.access_refresh','succeeded','validation','seed-user','{}',now(),now()); DROP TABLE IF EXISTS public.repo_release_work_items CASCADE; CREATE TABLE public.repo_release_work_items(id text,repo_id bigint,repo_full_name text,last_success_at text); DROP TABLE IF EXISTS public.repo_release_watchers CASCADE; CREATE TABLE public.repo_release_watchers(id text,task_id text,work_item_id text,status text,reused_fresh bigint,freshness_window_minutes bigint,freshness_decision text,freshness_assessment_json text,error_text text); INSERT INTO public.repo_release_work_items VALUES('seed-wi',1,'seed/repo',NULL); INSERT INTO public.repo_release_watchers VALUES('seed-watcher','freshness-task','seed-wi','ready',1,60,'reused_fresh','{}',NULL)", url)
            request = urllib.request.Request(base + '/api/admin/jobs/realtime/freshness-task/release-freshness', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert json.load(response)['items'][0]['reused_fresh'] is True
            sql("DROP TABLE IF EXISTS public.reaction_pat_tokens CASCADE; CREATE TABLE public.reaction_pat_tokens(user_id text PRIMARY KEY, masked_token text NOT NULL, last_check_state text NOT NULL, last_check_message text, last_checked_at text, owner_github_connection_id text, owner_github_user_id bigint, owner_login text)", url)
            def token_status():
                request = urllib.request.Request(base + '/api/reaction-token/status', headers={'Cookie': cookie.read_text().strip()})
                with urllib.request.urlopen(request, timeout=5) as response:
                    return json.load(response)
            assert token_status() == {'configured': False, 'masked_token': None, 'check': {'state': 'idle', 'message': None, 'checked_at': None}, 'owner': None}
            sql("INSERT INTO public.reaction_pat_tokens VALUES ('seed-user','ghp_****','valid',NULL,'2026-01-01T00:00:00Z','seed-connection',101,'seed')", url)
            status = token_status()
            assert status['configured'] and status['check']['state'] == 'valid' and status['owner']['github_user_id'] == 101
            req = urllib.request.Request(base + '/api/me', headers={'Cookie': cookie.read_text().strip()})
            with urllib.request.urlopen(req, timeout=5) as r:
                assert json.load(r)['user']['id'] == 'seed-user'
            try:
                urllib.request.urlopen(base + '/api/me', timeout=5)
                raise AssertionError('anonymous me must be rejected')
            except urllib.error.HTTPError as e:
                assert e.code == 401
            time.sleep(1)
            assert sql("SELECT status FROM public.job_tasks WHERE id='seed-task'", url) == 'queued'
            assert sql("SELECT count(*) FROM public.job_task_events WHERE task_id='seed-task'", url) == '0'
            print(json.dumps({'seeded_pg': True, 'health': 'pass', 'authenticated_me': 'pass', 'anonymous_me': 'pass', 'disabled_worker_queue_unchanged': 'pass', 'reaction_token_status': 'pass', 'profile_runtime_schedule': 'pass', 'search_text_query': 'pass', 'personal_repo_release_count': 'pass', 'admin_translation_request_list': 'pass'}))
    finally:
        if server is not None:
            server.send_signal(2)
            server.wait(timeout=15)
        sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()
