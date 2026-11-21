#!/usr/bin/env python3
"""Seed an isolated PostgreSQL database and verify the translation HTTP contract.

Covers the disabled-AI payloads, a stubbed OpenAI-compatible provider for the
non-streaming and streaming release/notification paths, task_id persistence and
the audit rows its worker writes, request-validation errors, failure
classification, and the LLM audit rows that `/api/admin/jobs/llm/calls`
projects.
"""
import argparse
import base64
import json
import os
import pathlib
import subprocess

import tempfile
import threading
import time
import urllib.error
import urllib.request
from pg_seed_common import psql
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit, urlunsplit

STUB_CALLS = []
STUB_LOCK = threading.Lock()


class StubProvider(BaseHTTPRequestHandler):
    """Minimal OpenAI-compatible chat completions stub."""

    def do_POST(self):
        length = int(self.headers.get('content-length') or 0)
        try:
            payload = json.loads(self.rfile.read(length) or b'{}')
        except json.JSONDecodeError:
            payload = {}
        messages = payload.get('messages') or []
        user_text = messages[-1].get('content', '') if messages else ''
        with STUB_LOCK:
            STUB_CALLS.append(user_text)
            call_no = len(STUB_CALLS)

        if 'trigger-401' in user_text:
            self._write(401, {'error': {'code': 'invalid_api_key'}})
            return
        if 'trigger-empty' in user_text:
            content = ''
        elif '输出中文标题' in user_text:
            content = '发布标题'
        elif 'Inbox 通知' in user_text:
            content = json.dumps({'title_zh': '通知标题', 'summary_md': '- 建议动作'})
        else:
            content = '- 新增功能\n- 修复缺陷'
        self._write(200, {
            'id': f'stub-{call_no}',
            'model': 'stub-model',
            'choices': [{'message': {'role': 'assistant', 'content': content}, 'finish_reason': 'stop'}],
            'usage': {'prompt_tokens': 10, 'completion_tokens': 5, 'total_tokens': 15},
        })

    def _write(self, status, payload):
        data = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header('content-type', 'application/json')
        self.send_header('content-length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3022)
    args = parser.parse_args()

    name = 'octo_rill_translate_seed_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))

    def sql(query, database=args.admin_url):
        return psql(database, query)

    def quote(text):
        return "'" + text.replace("'", "''") + "'"

    sql('CREATE DATABASE ' + name)
    app = (args.binary_dir / 'octo-rill').resolve()
    helper = (args.binary_dir / 'octo-rill-validation-session').resolve()

    stub = ThreadingHTTPServer(('127.0.0.1', 0), StubProvider)
    stub_port = stub.server_address[1]
    threading.Thread(target=stub.serve_forever, daemon=True).start()

    base_env = dict(
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
    servers = []

    def start_server(ai_enabled, log_dir):
        env = dict(base_env)
        env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
        if ai_enabled:
            env['AI_BASE_URL'] = f'http://127.0.0.1:{stub_port}/v1/'
            env['AI_API_KEY'] = 'stub-key'
            env['AI_MODEL'] = 'stub-model'
        else:
            for key in ('AI_BASE_URL', 'AI_API_KEY', 'AI_MODEL'):
                env.pop(key, None)
        log = open(f"/tmp/octo-rill-translate-{'ai' if ai_enabled else 'noai'}.log", 'w+')
        process = subprocess.Popen([app], env=env, stdout=log, stderr=subprocess.STDOUT)
        servers.append((process, log))
        for _ in range(150):
            try:
                with urllib.request.urlopen(f'http://127.0.0.1:{args.port}/health', timeout=1):
                    return process
            except Exception:
                if process.poll() is not None:
                    log.seek(0)
                    raise RuntimeError('server startup failed: ' + log.read()[-4000:])
                time.sleep(.1)
        raise RuntimeError('validation server did not become ready')

    def stop_server():
        process, log = servers.pop()
        try:
            if process.poll() is None:
                process.send_signal(2)
                process.wait(timeout=20)
        finally:
            log.close()

    def start_worker_server():
        """Run a second instance on a spare port that drains the task queue."""
        port = args.port + 1
        env = dict(base_env)
        env.pop('OCTORILL_PG_BOOTSTRAP_ONLY')
        env['OCTORILL_BIND_ADDR'] = f'127.0.0.1:{port}'
        env['OCTORILL_PG_WORKER_ENABLED'] = '1'
        env['OCTORILL_PG_SCHEDULER_ENABLED'] = '0'
        env['AI_BASE_URL'] = f'http://127.0.0.1:{stub_port}/v1/'
        env['AI_API_KEY'] = 'stub-key'
        env['AI_MODEL'] = 'stub-model'
        log = open('/tmp/octo-rill-translate-worker.log', 'w+')
        process = subprocess.Popen([app], env=env, stdout=log, stderr=subprocess.STDOUT)
        servers.append((process, log))
        for _ in range(150):
            try:
                with urllib.request.urlopen(f'http://127.0.0.1:{port}/health', timeout=1):
                    return process
            except Exception:
                if process.poll() is not None:
                    log.seek(0)
                    raise RuntimeError('worker startup failed: ' + log.read()[-4000:])
                time.sleep(.1)
        raise RuntimeError('validation worker did not become ready')

    def wait_for_task(task_id, timeout=60):
        deadline = time.time() + timeout
        while time.time() < deadline:
            status = sql(f"SELECT status FROM public.job_tasks WHERE id='{task_id}'", url)
            if status in ('succeeded', 'failed', 'cancelled'):
                return status
            time.sleep(.2)
        raise AssertionError('task did not finish: ' + task_id)

    try:
        subprocess.run([app], env=base_env, check=True, stdout=subprocess.DEVNULL)

        sql(
            "INSERT INTO public.users(id,github_user_id,login,created_at,updated_at,is_admin) "
            "VALUES('seed-user',101,'seed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0),"
            "('seed-admin',102,'admin','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1)",
            url,
        )
        sql(
            ""
            "DROP VIEW IF EXISTS public.user_release_visible_repos CASCADE; CREATE TABLE public.user_release_visible_repos(user_id text, repo_id bigint, full_name text, is_private bigint);"
            "INSERT INTO public.user_release_visible_repos VALUES('seed-user',1,'seed/repo',0);"
            "DROP TABLE IF EXISTS public.repo_releases CASCADE; CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY, repo_id bigint, tag_name text, name text,"
            " body text, html_url text, updated_at text, detected_at text, created_at text, published_at text,"
            " is_prerelease bigint, is_draft bigint);"
            "INSERT INTO public.repo_releases VALUES"
            "(1,1,'v1.0.0','Release One','# Changes\n\n- Added thing\n- Fixed bug','https://github.com/seed/repo/releases/tag/v1.0.0','2026-01-01T00:00:00Z',NULL,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0),"
            "(2,1,'v1.1.0','Release Two','- Second release body','https://github.com/seed/repo/releases/tag/v1.1.0','2026-01-02T00:00:00Z',NULL,'2026-01-02T00:00:00Z','2026-01-02T00:00:00Z',0,0),"
            "(3,1,'v1.2.0','trigger-401','- rejected body','https://github.com/seed/repo/releases/tag/v1.2.0','2026-01-03T00:00:00Z',NULL,'2026-01-03T00:00:00Z','2026-01-03T00:00:00Z',0,0),"
            "(4,1,'v1.3.0','trigger-empty','- empty body','https://github.com/seed/repo/releases/tag/v1.3.0','2026-01-04T00:00:00Z',NULL,'2026-01-04T00:00:00Z','2026-01-04T00:00:00Z',0,0),"
            "(5,1,'v1.5.0','Live Title Marker','LIVE-BODY-MARKER\n\n- live detail','https://github.com/seed/repo/releases/tag/v1.5.0','2026-01-05T00:00:00Z',NULL,'2026-01-05T00:00:00Z','2026-01-05T00:00:00Z',0,0),"
            "(6,1,'v1.6.0','Fallback Live Title','LIVE-FALLBACK-MARKER\n\n- fallback detail','https://github.com/seed/repo/releases/tag/v1.6.0','2026-01-06T00:00:00Z',NULL,'2026-01-06T00:00:00Z','2026-01-06T00:00:00Z',0,0)",
            url,
        )
        sql(
            "DROP TABLE IF EXISTS public.notifications CASCADE; CREATE TABLE public.notifications(id text PRIMARY KEY, user_id text, thread_id text, repo_full_name text,"
            " subject_title text, reason text, subject_type text, updated_at text, unread bigint, html_url text);"
            "INSERT INTO public.notifications VALUES"
            "('n1','seed-user','t1','seed/repo','New pull request','review_requested','PullRequest','2026-01-01T00:00:00Z',1,'https://github.com/seed/repo/pull/1')",
            url,
        )
        sql(
            "DROP TABLE IF EXISTS public.llm_calls CASCADE; CREATE TABLE public.llm_calls(id text PRIMARY KEY,status text,source text,model text,requested_by text,"
            "parent_task_id text,parent_task_type text,max_tokens bigint,attempt_count bigint,scheduler_wait_ms bigint,"
            "first_token_wait_ms bigint,duration_ms bigint,prompt_text text,input_messages_json text,output_messages_json text,"
            "response_text text,error_text text,input_tokens bigint,output_tokens bigint,finish_reason text,"
            "provider_request_id text,provider_http_status bigint,cached_input_tokens bigint,total_tokens bigint,"
            "failure_class text,final_model text,fallback_count bigint,retry_scheduled_at text,recovery_attempt_count bigint,"
            "created_at text,started_at text,finished_at text,updated_at text,runtime_owner_id text,lease_heartbeat_at text);"
            "DROP TABLE IF EXISTS public.llm_call_events CASCADE; CREATE TABLE public.llm_call_events(id text PRIMARY KEY,call_id text,event_type text,status text,"
            "source text,requested_by text,parent_task_id text,payload_json text,created_at text,failure_class text,"
            "model text,attempt bigint,retry_after_ms bigint,from_model text,to_model text,fallback_count bigint);",
            url,
        )

        with tempfile.TemporaryDirectory(prefix='octo-rill-translate-') as tmp:
            tmp = pathlib.Path(tmp)
            cookies = {}
            for user in ('seed-user', 'seed-admin'):
                file = tmp / user
                subprocess.run(
                    [helper, '--postgres-url', url, '--user-id', user, '--cookie-file', file,
                     '--isolated-validation-copy'],
                    check=True,
                    stdout=subprocess.DEVNULL,
                )
                cookies[user] = file.read_text().strip()

            base = f'http://127.0.0.1:{args.port}'

            def post(path, body, user='seed-user', raw=None):
                data = (raw if raw is not None else json.dumps(body)).encode()
                headers = {'content-type': 'application/json'}
                if user:
                    headers['Cookie'] = cookies[user]
                request = urllib.request.Request(base + path, data=data, headers=headers, method='POST')
                try:
                    with urllib.request.urlopen(request, timeout=30) as response:
                        text = response.read().decode()
                        return response.status, response.headers.get('content-type', ''), text
                except urllib.error.HTTPError as error:
                    return error.code, error.headers.get('content-type', ''), error.read().decode()

            def post_json(path, body, user='seed-user'):
                status, _, text = post(path, body, user)
                return status, json.loads(text)

            def get_json(path, user='seed-admin'):
                request = urllib.request.Request(base + path, headers={'Cookie': cookies[user]})
                try:
                    with urllib.request.urlopen(request, timeout=10) as response:
                        return response.status, json.load(response)
                except urllib.error.HTTPError as error:
                    return error.code, json.load(error)

            # ----- Phase A: AI disabled -----
            start_server(False, tmp)
            code, body = post_json('/api/translate/releases/batch', {'release_ids': ['1', '2']})
            assert code == 200 and [item['status'] for item in body['items']] == ['disabled', 'disabled'], body
            assert body['items'][0]['lang'] == 'zh-CN'
            code, body = post_json('/translate/releases/batch', {'release_ids': ['999']})
            assert code == 200 and body['items'][0]['status'] == 'disabled', body
            code, body = post_json('/api/translate/release', {'release_id': '1'})
            assert code == 200 and body == {'lang': 'zh-CN', 'status': 'disabled', 'title': None, 'summary': None}, body
            code, body = post_json('/api/translate/release/detail', {'release_id': '1'})
            assert code == 200 and body['status'] == 'disabled', body
            code, body = post_json('/api/translate/release/detail/batch', {'release_ids': ['1']})
            assert code == 200 and body['items'][0]['status'] == 'disabled', body
            code, body = post_json('/api/translate/notifications/batch', {'thread_ids': ['t1']})
            assert code == 200 and body['items'][0]['status'] == 'disabled', body
            code, body = post_json('/api/translate/notification', {'thread_id': 't1'})
            assert code == 200 and body['status'] == 'disabled', body
            code, body = post_json('/api/translate/release?return_mode=bogus', {'release_id': '1'})
            assert code == 400 and body['code'] == 'bad_request', body
            code, body = post_json('/api/translate/releases/batch', {'release_ids': []})
            assert code == 400 and body['message'] == 'release_ids is required', body
            code, body = post_json('/api/translate/releases/batch', {'release_ids': [str(i) for i in range(61)]})
            assert code == 400 and 'at most 60' in body['message'], body
            code, body = post_json('/api/translate/release', {'release_id': 'abc'})
            assert code == 400 and body['message'] == 'release_id must be an integer string', body
            code, body = post_json('/api/translate/notification', {'thread_id': '   '})
            assert code == 400 and body['message'] == 'thread_id is required', body
            code, body = post_json('/api/translate/release', {'release_id': '1'}, user=None)
            assert code == 401 and body['code'] == 'unauthorized', body
            stop_server()

            # ----- Phase B: AI enabled against the stub provider -----
            start_server(True, tmp)

            status, body = post_json('/api/translate/release', {'release_id': '1'})
            assert status == 200 and body['status'] == 'ready', body
            assert body['title'] == '发布标题', body
            assert '新增功能' in body['summary'], body

            summary_row = sql(
                "SELECT status || ':' || result_status || ':' || COALESCE(title_zh,'') FROM public.translation_requests "
                "WHERE entity_id='1' AND kind='release_summary'",
                url,
            )
            assert summary_row == 'completed:ready:发布标题', summary_row
            work_item_row = sql(
                "SELECT status || ':' || result_status FROM public.translation_work_items "
                "WHERE entity_id='1' AND kind='release_detail'",
                url,
            )
            assert work_item_row == 'completed:ready', work_item_row
            assert sql("SELECT COUNT(*) FROM public.llm_calls WHERE source='api.translate_release' AND status='succeeded'", url) == '2'
            assert sql(
                "SELECT COUNT(*) FROM public.llm_call_events e JOIN public.llm_calls c ON c.id=e.call_id "
                "WHERE c.source='api.translate_release' AND e.event_type='llm.succeeded'",
                url,
            ) == '2'
            assert sql(
                "SELECT COUNT(*) FROM public.translation_attempt_events WHERE entity_id='1' "
                "AND event_type='attempt_started' AND attempt_no=1",
                url,
            ) == '1'
            assert sql(
                "SELECT result_status || ':' || llm_call_ids_json FROM public.translation_attempt_events "
                "WHERE entity_id='1' AND event_type='attempt_completed'",
                url,
            ).startswith('ready:[')
            assert sql(
                "SELECT COUNT(*) FROM public.translation_attempt_llm_calls l JOIN public.translation_work_items w "
                "ON w.id=l.work_item_id WHERE w.entity_id='1'",
                url,
            ) == '2'
            assert sql(
                "SELECT stage FROM public.translation_attempt_llm_calls l JOIN public.translation_work_items w "
                "ON w.id=l.work_item_id WHERE w.entity_id='1' ORDER BY l.ordinal",
                url,
            ).splitlines() == ['title', 'body']
            calls_after_first = sql("SELECT COUNT(*) FROM public.llm_calls", url)

            # Shared work item cache: the second call must not reach the provider.
            status, body = post_json('/api/translate/release', {'release_id': '1'})
            assert status == 200 and body['status'] == 'ready', body
            assert sql("SELECT COUNT(*) FROM public.llm_calls", url) == calls_after_first
            # Release detail reuses the release_detail work item written by the summary route.
            status, body = post_json('/api/translate/release/detail', {'release_id': '1'})
            assert status == 200 and body['status'] == 'ready', body
            assert sql("SELECT COUNT(*) FROM public.llm_calls", url) == calls_after_first
            # The summary route owns the request row; the detail route reuses
            # the shared work item, so it must not enqueue a second request.
            assert sql(
                "SELECT kind || ':' || status FROM public.translation_requests WHERE entity_id='1'",
                url,
            ) == 'release_summary:completed'
            status, body = post_json('/api/translate/release/detail/batch', {'release_ids': ['1']})
            assert status == 200 and body['items'][0]['status'] == 'ready', body

            status, body = post_json('/api/translate/release', {'release_id': '999'})
            assert status == 404 and body['code'] == 'not_found', body
            status, body = post_json('/api/translate/releases/batch', {'release_ids': ['1', '999']})
            assert status == 200 and [(i['id'], i['status']) for i in body['items']] == [('1', 'ready'), ('999', 'missing')], body
            assert body['items'][1]['error'] == 'release not found'

            # Failure classification: upstream 401 is a configuration failure.
            status, body = post_json('/api/translate/release', {'release_id': '3'})
            assert status == 500 and body['code'] == 'llm_error', body
            assert body['failure_class'] == 'configuration', body
            assert sql(
                "SELECT status || ':' || result_status || ':' || error_text FROM public.translation_work_items WHERE entity_id='3'",
                url,
            ) == 'failed:error:LLM configuration or request rejected'
            assert sql(
                "SELECT failure_class || ':' || retry_eligible FROM public.translation_attempt_events "
                "WHERE entity_id='3' AND event_type='attempt_completed'",
                url,
            ) == 'configuration:0'
            status, body = post_json('/api/translate/releases/batch', {'release_ids': ['3']})
            assert status == 200 and body['items'][0]['status'] == 'error', body
            assert body['items'][0]['failure_class'] == 'configuration', body

            # Empty model output is classified as empty_content.
            status, body = post_json('/api/translate/release', {'release_id': '4'})
            assert status == 500 and body['failure_class'] == 'empty_content', body

            # Notifications.
            status, body = post_json('/api/translate/notification', {'thread_id': 't1'})
            assert status == 200 and body['status'] == 'ready', body
            assert body['title'] == '通知标题' and body['summary'] == '- 建议动作', body
            assert sql("SELECT COUNT(*) FROM public.llm_calls WHERE source='api.translate_notification'", url) == '1'
            status, body = post_json('/api/translate/notifications/batch', {'thread_ids': ['t1', 'nope']})
            assert status == 200 and [(i['id'], i['status']) for i in body['items']] == [('t1', 'ready'), ('nope', 'missing')], body
            assert body['items'][1]['error'] == 'notification not found'
            status, body = post_json('/api/translate/notifications/batch', {'thread_ids': ['nope']})
            assert status == 200 and body['items'][0]['status'] == 'missing', body

            # task_id mode persists the queued request and task without calling
            # the provider until a worker picks the task up.
            calls_before_task = sql("SELECT COUNT(*) FROM public.llm_calls", url)
            status, body = post_json('/api/translate/release?return_mode=task_id', {'release_id': '2'})
            assert status == 200 and body['mode'] == 'task_id', body
            assert body['task_type'] == 'translation.generate' and body['status'] == 'queued', body
            task_id = body['task_id']
            assert sql(
                f"SELECT status FROM public.job_tasks WHERE id='{task_id}' AND task_type='translation.generate'",
                url,
            ) == 'queued'
            assert sql(
                "SELECT status FROM public.translation_requests WHERE entity_id='2' AND kind='release_summary'",
                url,
            ) == 'queued'
            work_item = sql(
                "SELECT work_item_id FROM public.translation_requests WHERE entity_id='2' AND kind='release_summary'",
                url,
            )
            assert work_item and work_item != ''
            payload = json.loads(sql(f"SELECT payload_json FROM public.job_tasks WHERE id='{task_id}'", url))
            assert payload['work_item_id'] == work_item
            assert payload['request_id'] and payload['kind'] == 'release_summary', payload
            assert sql("SELECT COUNT(*) FROM public.llm_calls", url) == calls_before_task

            # The work item persists the source blocks captured at enqueue.
            # Release 2 keeps them (persisted == live), release 5's snapshot
            # deliberately diverges from the live release text to prove the
            # async prompt is built from the persisted blocks
            # (metadata/title/body_markdown order), and release 6 drops the
            # snapshot to prove the live-content fallback.
            status, body = post_json('/api/translate/release?return_mode=task_id', {'release_id': '5'})
            assert status == 200 and body['mode'] == 'task_id', body
            snapshot_task = body['task_id']
            snapshot_work_item = sql(
                "SELECT work_item_id FROM public.translation_requests WHERE entity_id='5' AND kind='release_summary'",
                url,
            )
            assert snapshot_work_item and snapshot_work_item != ''
            snapshot_blocks = json.dumps([
                {'slot': 'source_observed_at', 'text': '2026-01-05T00:00:00Z'},
                {'slot': 'source_revision_tiebreak', 'text': '424242'},
                {'slot': 'metadata', 'text': 'seed/repo\nv5.0.0-persisted'},
                {'slot': 'title', 'text': 'Persisted Title Marker'},
                {'slot': 'body_markdown', 'text': 'PERSISTED-BODY-MARKER\n\n- snapshot detail'},
            ])
            snapshot_slots = json.dumps(['title_zh', 'body_md'])
            sql(
                "UPDATE public.translation_work_items SET source_blocks_json=" + quote(snapshot_blocks)
                + ", target_slots_json=" + quote(snapshot_slots)
                + f" WHERE id='{snapshot_work_item}'",
                url,
            )
            status, body = post_json('/api/translate/release?return_mode=task_id', {'release_id': '6'})
            assert status == 200 and body['mode'] == 'task_id', body
            fallback_task = body['task_id']
            fallback_work_item = sql(
                "SELECT work_item_id FROM public.translation_requests WHERE entity_id='6' AND kind='release_summary'",
                url,
            )
            assert fallback_work_item and fallback_work_item != ''
            sql(
                f"UPDATE public.translation_work_items SET source_blocks_json='[]' WHERE id='{fallback_work_item}'",
                url,
            )
            stub_calls_before_task = len(STUB_CALLS)

            # Draining the task runs the same audited model calls as the
            # synchronous path: llm_calls, llm_call_events and
            # translation_attempt_llm_calls all get rows for this work item.
            start_worker_server()
            task_status = wait_for_task(task_id)
            snapshot_task_status = wait_for_task(snapshot_task)
            fallback_task_status = wait_for_task(fallback_task)
            stop_server()
            assert snapshot_task_status == 'succeeded', sql(
                f"SELECT COALESCE(error_text,'') FROM public.job_tasks WHERE id='{snapshot_task}'", url,
            )
            assert fallback_task_status == 'succeeded', sql(
                f"SELECT COALESCE(error_text,'') FROM public.job_tasks WHERE id='{fallback_task}'", url,
            )
            task_prompts = STUB_CALLS[stub_calls_before_task:]
            assert any('PERSISTED-BODY-MARKER' in prompt for prompt in task_prompts), task_prompts
            assert all('LIVE-BODY-MARKER' not in prompt for prompt in task_prompts), task_prompts
            assert any('Persisted Title Marker' in prompt for prompt in task_prompts), task_prompts
            assert all('Live Title Marker' not in prompt for prompt in task_prompts), task_prompts
            # Persisted == live for the original task fixture.
            assert any('- Second release body' in prompt for prompt in task_prompts), task_prompts
            # No usable snapshot falls back to the live release text.
            assert any('LIVE-FALLBACK-MARKER' in prompt for prompt in task_prompts), task_prompts
            worker_log = pathlib.Path('/tmp/octo-rill-translate-worker.log').read_text()
            assert worker_log.count('falling back to live content') == 1, worker_log[-2000:]
            assert task_status == 'succeeded', sql(
                f"SELECT COALESCE(error_text,'') FROM public.job_tasks WHERE id='{task_id}'", url,
            )
            task_result = json.loads(sql(f"SELECT result_json FROM public.job_tasks WHERE id='{task_id}'", url))
            assert task_result == {'work_item_id': work_item, 'status': 'completed'}, task_result
            assert sql(
                f"SELECT COUNT(*) FROM public.llm_calls WHERE source='api.translate_release' "
                f"AND parent_task_id='{task_id}' AND parent_task_type='translation.generate' "
                f"AND requested_by='seed-user' AND status='succeeded'",
                url,
            ) == '2'
            assert sql("SELECT COUNT(*) FROM public.llm_calls", url) == str(int(calls_before_task) + 6)
            assert sql(
                f"SELECT COUNT(*) FROM public.llm_calls WHERE parent_task_id='{task_id}' "
                "AND provider_request_id LIKE 'stub-%' AND provider_http_status=200 "
                "AND input_tokens=10 AND output_tokens=5 AND total_tokens=15 "
                "AND finish_reason='stop' AND duration_ms IS NOT NULL",
                url,
            ) == '2'
            event_rows = sql(
                "SELECT e.event_type || ':' || e.status FROM public.llm_call_events e "
                "JOIN public.llm_calls c ON c.id=e.call_id "
                f"WHERE c.parent_task_id='{task_id}' ORDER BY e.event_type, e.id",
                url,
            ).splitlines()
            # Both audited calls must emit the full lifecycle; the statement
            # orders by event type, so compare as a multiset.
            assert sorted(event_rows) == sorted(
                ['llm.queued:queued', 'llm.running:running', 'llm.succeeded:succeeded'] * 2
            ), event_rows
            assert sql(
                f"SELECT COUNT(*) FROM public.llm_call_events WHERE parent_task_id='{task_id}'",
                url,
            ) == '6'
            assert sql(
                "SELECT stage FROM public.translation_attempt_llm_calls l "
                "JOIN public.translation_work_items w ON w.id=l.work_item_id "
                "WHERE w.entity_id='2' AND l.attempt_no=1 ORDER BY l.ordinal",
                url,
            ).splitlines() == ['title', 'body']
            attempt_calls = json.loads(sql(
                "SELECT llm_call_ids_json FROM public.translation_attempt_events "
                "WHERE entity_id='2' AND event_type='attempt_completed'",
                url,
            ))
            task_calls = sql(f"SELECT id FROM public.llm_calls WHERE parent_task_id='{task_id}'", url).splitlines()
            assert sorted(attempt_calls) == sorted(task_calls) and len(task_calls) == 2, (attempt_calls, task_calls)
            assert sql(
                "SELECT event_type FROM public.translation_attempt_events WHERE entity_id='2' "
                "AND attempt_no=1 ORDER BY event_type",
                url,
            ).splitlines() == ['attempt_completed', 'attempt_queued', 'attempt_started']
            assert sql(
                "SELECT status || ':' || result_status FROM public.translation_work_items "
                "WHERE entity_id='2' AND kind='release_detail'",
                url,
            ) == 'completed:ready'
            assert sql(
                "SELECT status || ':' || result_status FROM public.translation_requests WHERE entity_id='2'",
                url,
            ) == 'completed:ready'
            # The snapshot fixture keeps the same audited call shape and writes
            # only the persisted source through the worker.
            snapshot_result = json.loads(sql(f"SELECT result_json FROM public.job_tasks WHERE id='{snapshot_task}'", url))
            assert snapshot_result == {'work_item_id': snapshot_work_item, 'status': 'completed'}, snapshot_result
            assert sql(
                f"SELECT COUNT(*) FROM public.llm_calls WHERE source='api.translate_release' "
                f"AND parent_task_id='{snapshot_task}' AND parent_task_type='translation.generate' "
                f"AND requested_by='seed-user' AND status='succeeded'",
                url,
            ) == '2'
            assert sql(
                "SELECT stage FROM public.translation_attempt_llm_calls l "
                "JOIN public.translation_work_items w ON w.id=l.work_item_id "
                "WHERE w.entity_id='5' AND l.attempt_no=1 ORDER BY l.ordinal",
                url,
            ).splitlines() == ['title', 'body']
            assert sql(
                "SELECT status || ':' || result_status FROM public.translation_work_items WHERE entity_id='5'",
                url,
            ) == 'completed:ready'
            assert sql(
                "SELECT status || ':' || result_status FROM public.translation_requests WHERE entity_id='5'",
                url,
            ) == 'completed:ready'
            # The fallback fixture keeps the same audited shape while using the
            # live release text.
            fallback_result = json.loads(sql(f"SELECT result_json FROM public.job_tasks WHERE id='{fallback_task}'", url))
            assert fallback_result == {'work_item_id': fallback_work_item, 'status': 'completed'}, fallback_result
            assert sql(
                f"SELECT COUNT(*) FROM public.llm_calls WHERE source='api.translate_release' "
                f"AND parent_task_id='{fallback_task}' AND parent_task_type='translation.generate' "
                f"AND requested_by='seed-user' AND status='succeeded'",
                url,
            ) == '2'
            assert sql(
                "SELECT stage FROM public.translation_attempt_llm_calls l "
                "JOIN public.translation_work_items w ON w.id=l.work_item_id "
                "WHERE w.entity_id='6' AND l.attempt_no=1 ORDER BY l.ordinal",
                url,
            ).splitlines() == ['title', 'body']
            assert sql(
                "SELECT status || ':' || result_status FROM public.translation_work_items WHERE entity_id='6'",
                url,
            ) == 'completed:ready'
            assert sql(
                "SELECT status || ':' || result_status FROM public.translation_requests WHERE entity_id='6'",
                url,
            ) == 'completed:ready'

            # SSE mode streams the persisted task events in order and terminates.
            status, content_type, text = post('/api/translate/release?return_mode=sse', {'release_id': '1'})
            assert status == 200 and 'text/event-stream' in content_type, (status, content_type, text[:400])
            assert 'event: task.progress' in text and 'event: task.completed' in text, text
            assert text.index('event: task.progress') < text.index('event: task.completed'), text
            assert '"item_status":"ready"' in text, text
            assert '"status":"succeeded"' in text, text
            assert '"ready":1' in text, text

            # The SSE alias path defaults to streaming mode.
            status, content_type, text = post('/translate/release/stream', {'release_id': '1'})
            assert status == 200 and 'text/event-stream' in content_type, (status, content_type, text[:400])
            assert 'event: task.completed' in text, text

            # Batch stream: NDJSON item events in request order and a terminal done line.
            status, content_type, text = post('/api/translate/releases/batch/stream', {'release_ids': ['1', '999']})
            assert status == 200 and 'application/x-ndjson' in content_type, (status, content_type, text[:400])
            lines = [json.loads(line) for line in text.splitlines() if line.strip()]
            assert lines[0]['event'] == 'item' and lines[0]['item']['id'] == '1' and lines[0]['item']['status'] == 'ready', lines
            assert lines[1]['event'] == 'item' and lines[1]['item']['id'] == '999' and lines[1]['item']['status'] == 'missing', lines
            assert lines[-1] == {'event': 'done'}, lines
            assert sql(
                "SELECT COUNT(*) FROM public.job_tasks WHERE task_type='translate.release.batch' AND status='succeeded'",
                url,
            ) == '1'
            assert sql(
                "SELECT COUNT(*) FROM public.job_task_events e JOIN public.job_tasks t ON t.id=e.task_id "
                "WHERE t.task_type='translate.release.batch' AND e.event_type='task.progress'",
                url,
            ) >= '3'

            # The administration LLM audit sees the translation calls from both
            # the synchronous and the task_id path.
            code, body = get_json('/api/admin/jobs/llm/calls?status=succeeded&source=api.translate_release&page_size=10')
            assert code == 200 and body['total'] == 8, body
            assert all(item['id'] and item['provider_request_id'].startswith('stub-') for item in body['items']), body
            assert sum(1 for item in body['items'] if item.get('parent_task_id') == task_id) == 2, body
            assert sum(1 for item in body['items'] if item.get('parent_task_id') == snapshot_task) == 2, body
            assert sum(1 for item in body['items'] if item.get('parent_task_id') == fallback_task) == 2, body
            code, body = get_json(f'/api/admin/jobs/llm/calls?parent_task_id={task_id}')
            assert code == 200 and body['total'] == 2, body
            code, body = get_json(f'/api/admin/jobs/llm/calls?parent_task_id={snapshot_task}')
            assert code == 200 and body['total'] == 2, body
            code, body = get_json(f'/api/admin/jobs/llm/calls?parent_task_id={fallback_task}')
            assert code == 200 and body['total'] == 2, body
            code, body = get_json('/api/admin/jobs/llm/calls?source=api.translate_notification')
            assert code == 200 and body['total'] == 1, body
            code, body = get_json('/api/admin/jobs/llm/calls', user='seed-user')
            assert code == 403, (code, body)

            print(json.dumps({
                'disabled_payloads': 'pass',
                'request_validation': 'pass',
                'sync_release_and_cache': 'pass',
                'failure_classification': 'pass',
                'notification_paths': 'pass',
                'task_id_persistence': 'pass',
                'task_id_llm_audit': 'pass',
                'task_id_persisted_prompt': 'pass',
                'sse_event_order': 'pass',
                'batch_stream_ndjson': 'pass',
                'llm_audit_visibility': 'pass',
            }))
    finally:
        while servers:
            stop_server()
        stub.shutdown()
        if os.environ.get('OCTORILL_KEEP_SEED_DB'):
            print('kept database:', name, flush=True)
        else:
            sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()
