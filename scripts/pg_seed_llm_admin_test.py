#!/usr/bin/env python3
"""Validate PG LLM administration reads using synthetic isolated records."""
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
from urllib.parse import urlsplit, urlunsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3021)
    args = parser.parse_args()
    name = 'octo_rill_llm_seed_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))
    def sql(query, db=args.admin_url):
        return psql(db, query)
    sql('CREATE DATABASE ' + name)
    binary = (args.binary_dir / 'octo-rill').resolve()
    helper = (args.binary_dir / 'octo-rill-validation-session').resolve()
    env = dict(os.environ, DATABASE_URL=url, OCTORILL_ENCRYPTION_KEY_BASE64=base64.b64encode(bytes(32)).decode(), GITHUB_CLIENT_ID='validation', GITHUB_CLIENT_SECRET='validation', GITHUB_OAUTH_REDIRECT_URL=f'http://127.0.0.1:{args.port}/auth/github/callback', OCTORILL_BIND_ADDR=f'127.0.0.1:{args.port}', OCTORILL_PG_WORKER_ENABLED='0', OCTORILL_PG_SCHEDULER_ENABLED='0', OCTORILL_PG_BOOTSTRAP_ONLY='1')
    process = None
    try:
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)
        string_fields = 'id,status,source,model,requested_by,parent_task_id,parent_task_type,finish_reason,provider_request_id,failure_class,final_model,retry_scheduled_at,created_at,started_at,finished_at,updated_at,input_messages_json,output_messages_json,prompt_text,response_text,error_text'.split(',')
        int_fields = 'max_tokens,attempt_count,scheduler_wait_ms,first_token_wait_ms,duration_ms,input_tokens,output_tokens,provider_http_status,cached_input_tokens,total_tokens,fallback_count,recovery_attempt_count'.split(',')
        columns = ','.join(f'{c} TEXT' for c in string_fields) + ',' + ','.join(f'{c} BIGINT' for c in int_fields)
        sql("INSERT INTO public.users(id,github_user_id,login,created_at,updated_at,is_admin) VALUES('seed-admin',101,'admin','2026','2026',1),('seed-user',102,'user','2026','2026',0); DROP TABLE IF EXISTS public.admin_runtime_settings CASCADE; CREATE TABLE public.admin_runtime_settings(id bigint PRIMARY KEY,llm_models_json text,ai_model_context_limit bigint,llm_max_concurrency bigint); INSERT INTO public.admin_runtime_settings VALUES(1,'[\"seed-model\"]',8192,4); DROP TABLE IF EXISTS public.llm_calls CASCADE; CREATE TABLE public.llm_calls(" + columns + "); DROP TABLE IF EXISTS public.llm_call_events CASCADE; CREATE TABLE public.llm_call_events(id text,call_id text,event_type text,status text,model text,attempt bigint,failure_class text,retry_after_ms bigint,from_model text,to_model text,fallback_count bigint,created_at text); DROP TABLE IF EXISTS public.content_attempt_events CASCADE; CREATE TABLE public.content_attempt_events(id text,attempt_no bigint,result_status text,error_summary text,failure_class text); DROP TABLE IF EXISTS public.content_attempt_llm_calls CASCADE; CREATE TABLE public.content_attempt_llm_calls(id text,attempt_event_id text,status text,model text,provider_call_id text,output_tokens bigint,created_at text);", url)
        sql("INSERT INTO public.llm_calls(id,status,source,model,max_tokens,attempt_count,scheduler_wait_ms,duration_ms,prompt_text,created_at,finished_at,updated_at,fallback_count,recovery_attempt_count) VALUES('seed-call','succeeded','validation','seed-model',10,1,12,20,'private synthetic prompt',now()::text,now()::text,now()::text,0,0),('seed-failed','failed','validation','old-model',10,1,8,10,'private synthetic prompt',now()::text,now()::text,now()::text,0,0); INSERT INTO public.llm_call_events VALUES('event-1','seed-call','completed','succeeded','seed-model',1,NULL,NULL,NULL,NULL,0,now()::text); INSERT INTO public.content_attempt_events VALUES('global-event',1,'ready',NULL,NULL); INSERT INTO public.content_attempt_llm_calls VALUES('global-call','global-event','succeeded','seed-model','provider-1',10,now()::text);", url)
        with tempfile.TemporaryDirectory(prefix='octo-rill-llm-test-') as tmp:
            tmp = pathlib.Path(tmp)
            cookies = {}
            for user in ('seed-admin', 'seed-user'):
                file = tmp / user
                subprocess.run([helper, '--postgres-url', url, '--user-id', user, '--cookie-file', file, '--isolated-validation-copy'], check=True, stdout=subprocess.DEVNULL)
                cookies[user] = file.read_text().strip()
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
            def get(path, user='seed-admin'):
                headers = {'Cookie': cookies[user]} if user else {}
                request = urllib.request.Request(base + '/api/admin/jobs/llm/' + path, headers=headers)
                try:
                    with urllib.request.urlopen(request, timeout=10) as response:
                        return response.status, json.load(response)
                except urllib.error.HTTPError as error:
                    return error.code, json.load(error)
            code, body = get('calls?model=seed-model&page_size=1')
            assert code == 200 and body['total'] == 1 and body['items'][0]['id'] == 'seed-call'
            assert 'prompt_text' not in body['items'][0], 'list must not reveal diagnostics'
            assert get('calls?status=invalid')[0] == 400
            assert get('calls?started_from=bad')[0] == 400
            assert get('calls', 'seed-user')[0] == 403
            assert get('calls', None)[0] == 401
            code, body = get('calls/seed-call')
            assert code == 200 and body['prompt_text'] == 'private synthetic prompt' and len(body['attempt_history']) == 1
            code, body = get('calls/global-call')
            assert code == 200 and body['source'] == 'global_content_processing' and body['provider_request_id'] == 'provider-1'
            assert get('calls/missing')[0] == 404
            code, body = get('activity')
            assert code == 200 and len(body['buckets']) == 50
            assert body['models'][0] == {'model': 'seed-model', 'priority': 1, 'configured': True}
            assert sum(c['succeeded'] for b in body['buckets'] for c in b['counts']) == 1
            assert sum(c['failed'] for b in body['buckets'] for c in b['counts']) == 1
            code, body = get('status')
            assert code == 200 and body['calls_24h'] == 2 and body['failed_24h'] == 1
            assert body['model_statuses'][0]['status'] == 'ready' and body['effective_model_input_limit'] == 8192
            sql("INSERT INTO public.llm_model_health(model,relevant_failure_count,cooldown_until,created_at,updated_at) VALUES('seed-model',2,now()+interval '5 minutes',now(),now())", url)
            code, body = get('status')
            assert code == 200 and body['model_statuses'][0]['status'] == 'cooldown'
            assert body['selected_model_for_new_calls'] == 'seed-model', 'all-cooled route preserves earliest cooldown selection'

            print(json.dumps({'calls_filters_and_auth': 'pass', 'detail_and_global_fallback': 'pass', 'activity_50_buckets': 'pass', 'status_aggregates': 'pass'}))
    finally:
        if process is not None:
            process.send_signal(2)
            process.wait(timeout=15)
        sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()
