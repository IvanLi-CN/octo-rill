#!/usr/bin/env python3
"""Validate the PostgreSQL job center read surface with synthetic isolated data.

Covers `GET /api/admin/jobs/overview` (six KPI counters and their 24h windows),
`GET /api/admin/jobs/realtime` (`task_group` semantics, status/task_type
filters, exact totals, page metadata, the complete item shape and subscription
diagnostics) and `GET /api/admin/jobs/events` (real `text/event-stream`
response, `Last-Event-ID` resume, admin-only access).
"""
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
from urllib.parse import urlencode, urlsplit, urlunsplit

from pg_seed_common import psql


TASK_FIELDS = {
    'id',
    'task_type',
    'task_group',
    'legacy',
    'status',
    'source',
    'skipped',
    'requested_by',
    'parent_task_id',
    'cancel_requested',
    'error_message',
    'created_at',
    'started_at',
    'finished_at',
    'updated_at',
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3033)
    args = parser.parse_args()
    name = 'octo_rill_jobs_center_seed_' + str(os.getpid())
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
        OCTORILL_PG_WORKER_ENABLED='0',
        OCTORILL_PG_SCHEDULER_ENABLED='0',
        OCTORILL_PG_BOOTSTRAP_ONLY='1',
    )
    env.pop('AI_MODEL', None)
    env.pop('AI_API_KEY', None)
    process = None
    try:
        # Runtime baseline (migrations_pg), including job_tasks, its events,
        # the daily brief slots and the application tables.
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)
        sql(
            """
            INSERT INTO public.users
              (id, github_user_id, login, created_at, updated_at, is_admin, is_disabled)
            VALUES
              ('seed-admin', 101, 'seed-admin', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, 0),
              ('seed-user', 102, 'seed-user', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 0, 0);
            """,
            url,
        )
        # Every row below is created relative to NOW() so the 24h KPI windows
        # stay meaningful whenever the suite runs. `rt-old` is deliberately
        # outside the window.
        sql(
            """
            INSERT INTO public.job_tasks
              (id, task_type, status, source, requested_by, payload_json, result_json,
               error_message, cancel_requested, created_at, started_at, finished_at, updated_at)
            VALUES
         			  ('rt-translate', 'translation.generate', 'succeeded', 'api.translate',
			   'seed-user', '{"user_id":"seed-user"}', '{"lang":"zh"}', NULL, 0,
               now() - interval '60 minutes', now() - interval '59 minutes',
               now() - interval '59 minutes', now() - interval '59 minutes'),
         			  ('rt-batch', 'translate.release.batch', 'succeeded', 'api.translate', NULL,
			   '{"release_ids":[1]}', '{"items":[{"status":"ready"}]}', NULL, 0,
               now() - interval '50 minutes', now() - interval '49 minutes',
               now() - interval '49 minutes', now() - interval '49 minutes'),
         			  ('rt-repo', 'sync.repo_release', 'running', 'api.webhook', 'seed-user',
               '{}', NULL, NULL, 1,
               now() - interval '40 minutes', now() - interval '39 minutes',
               NULL, now() - interval '39 minutes'),
         			  ('rt-notify', 'sync.notifications', 'running', 'scheduler', NULL,
			   '{}', NULL, NULL, 0,
			   now() - interval '35 minutes', now() - interval '34 minutes',
			   NULL, now() - interval '34 minutes'),
         			  ('rt-old', 'translation.generate', 'succeeded', 'api.translate', NULL,
			   '{}', '{"lang":"zh"}', NULL, 0,
               now() - interval '48 hours', now() - interval '48 hours',
               now() - interval '48 hours', now() - interval '48 hours'),
         			  ('sch-brief-ok', 'brief.daily_slot', 'succeeded', 'scheduler', NULL,
               '{"hour_utc":6}', '{"generated":1}', NULL, 0,
               now() - interval '30 minutes', now() - interval '29 minutes',
               now() - interval '29 minutes', now() - interval '29 minutes'),
         			  ('sch-brief-queued', 'brief.daily_slot', 'queued', 'scheduler', NULL,
               '{"hour_utc":7}', NULL, NULL, 0,
               now() - interval '20 minutes', NULL, NULL,
               now() - interval '20 minutes'),
         			  ('sch-retry', 'retry.recent_failures', 'succeeded', 'scheduler', NULL,
               '{}', '{"retried":0}', NULL, 0,
               now() - interval '10 minutes', now() - interval '9 minutes',
               now() - interval '9 minutes', now() - interval '9 minutes'),
         			  ('sch-webhook', 'webhook.push.manage', 'succeeded', 'scheduler', NULL,
			   '{}', '{"managed":0}', NULL, 0,
               now() - interval '5 minutes', now() - interval '4 minutes',
               now() - interval '4 minutes', now() - interval '4 minutes'),
         			  ('us-running', 'sync.starred', 'running', 'scheduler', 'seed-user',
			   '{"user_id":"seed-user"}', NULL, NULL, 0,
               now() - interval '55 minutes', now() - interval '54 minutes',
			   NULL, now() - interval '54 minutes'),
         			  ('us-failed', 'sync.starred', 'failed', 'scheduler', 'seed-user',
			   '{"user_id":"seed-user"}', NULL, 'star boom', 0,
               now() - interval '45 minutes', now() - interval '44 minutes',
               now() - interval '44 minutes', now() - interval '44 minutes'),
         			  -- Imported SQLite history: readable, legacy-marked, never current.
         			  ('legacy-summarize', 'summarize.release.smart.batch', 'failed', 'scheduler', NULL,
               '{}', NULL, 'summarize boom', 0,
               now() - interval '50 minutes', now() - interval '49 minutes',
               now() - interval '49 minutes', now() - interval '49 minutes'),
         			  ('legacy-delta', 'sync.starred.delta', 'succeeded', 'scheduler', 'seed-user',
               '{"user_id":"seed-user"}', '{"skipped":false}', NULL, 0,
			   now() - interval '54 minutes', now() - interval '54 minutes',
               now() - interval '54 minutes', now() - interval '54 minutes'),
         			  ('legacy-audit', 'webhook.push.audit', 'succeeded', 'scheduler', NULL,
               '{}', '{"audited":0}', NULL, 0,
			   now() - interval '4 minutes', now() - interval '4 minutes',
               now() - interval '4 minutes', now() - interval '4 minutes'),
              ('sub-1', 'sync.subscriptions', 'succeeded', 'scheduler', NULL,
               '{"trigger":"scheduler","schedule_key":"daily-06"}',
               '{"skipped":true,"skip_reason":"previous_run_active"}', NULL, 0,
               now() - interval '35 minutes', now() - interval '34 minutes',
               now() - interval '34 minutes', now() - interval '34 minutes'),
              ('sub-2', 'sync.subscriptions', 'failed', 'scheduler', NULL,
               '{"trigger":"manual"}',
               '{"release":{"total_repos":4,"succeeded_repos":1,"failed_repos":1},'
               '"social":{"total_users":2,"succeeded_users":2},"critical_events":1}',
               'subscription boom', 0,
               now() - interval '25 minutes', now() - interval '24 minutes',
               now() - interval '24 minutes', now() - interval '24 minutes'),
              ('sub-3', 'sync.subscriptions', 'running', 'scheduler', NULL,
               '{"trigger":"scheduler"}', NULL, NULL, 0,
               now() - interval '15 minutes', now() - interval '14 minutes',
               NULL, now() - interval '14 minutes');
            INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at)
            VALUES
              ('sub-1-collect', 'sub-1', 'task.progress',
               '{"stage":"collect","total_users":7}', now() - interval '34 minutes'),
              ('sub-1-release', 'sub-1', 'task.progress',
               '{"stage":"release_summary","total_repos":3,"succeeded_repos":3}',
               now() - interval '33 minutes');
            INSERT INTO public.sync_subscription_events
              (id, task_id, stage, event_type, severity, recoverable, attempt,
               user_id, repo_id, repo_full_name, payload_json, created_at)
            VALUES
              ('sub-ev-1', 'sub-1', 'collect', 'subscription.progress', 'info', 1, 1,
               'seed-user', 10, 'acme/rocket', '{"message":"collected"}',
               '2026-11-01T00:00:00Z');
            -- 30 progress events on `sub-3` exceed the 20-row window, while the
            -- oldest row is the only `collect` summary and must still be
            -- retained, so the bounded reads stay discriminating.
            INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at)
            SELECT 'sub-3-progress-' || lpad(g::text, 2, '0'), 'sub-3', 'task.progress',
                   CASE WHEN g = 1
                        THEN '{"stage":"collect","total_users":11}'
                        ELSE json_build_object('stage', 'release_progress',
                                               'releases_written', g)::text
                   END,
                   now() - interval '14 minutes' + (g || ' seconds')::interval
            FROM generate_series(1, 30) AS g;
            -- 25 subscription events on `sub-2` prove the newest-20 window.
            INSERT INTO public.sync_subscription_events
              (id, task_id, stage, event_type, severity, recoverable, attempt,
               user_id, repo_id, repo_full_name, payload_json, created_at)
            SELECT 'sub-ev-2-' || lpad(g::text, 2, '0'), 'sub-2', 'release',
                   'subscription.progress', 'info', 1, 1, 'seed-user', 10,
                   'acme/rocket',
                   json_build_object('message', 'event-' || g)::text,
                   (now() - interval '20 minutes' + (g || ' seconds')::interval)::text
            FROM generate_series(1, 25) AS g;
            """,
            url,
        )

        with tempfile.TemporaryDirectory(prefix='octo-rill-jobs-center-test-') as tmp:
            tmp = pathlib.Path(tmp)
            cookies = {}
            for user in ('seed-admin', 'seed-user'):
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

            def request(path, user='seed-admin'):
                headers = {'Cookie': cookies[user]} if user else {}
                req = urllib.request.Request(base + path, headers=headers)
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

            def realtime(params, user='seed-admin'):
                return request('/api/admin/jobs/realtime?' + urlencode(params), user)

            # ---- authentication -------------------------------------------------
            for path in (
                '/api/admin/jobs/overview',
                '/api/admin/jobs/realtime?status=all',
                '/api/admin/jobs/events',
            ):
                code, body = request(path, None)
                assert code == 401, (path, code, body)
                code, body = request(path, 'seed-user')
                assert code == 403, (path, code, body)

            # ---- overview -------------------------------------------------------
            code, overview = request('/api/admin/jobs/overview')
            assert code == 200, (code, overview)
            assert set(overview) == {
                'queued',
                'running',
                'failed_24h',
                'succeeded_24h',
                'enabled_scheduled_slots',
                'total_scheduled_slots',
            }, overview
            assert overview['queued'] == 1, overview
            assert overview['queued'] == 1, overview
            assert overview['running'] == 3, overview
            # `legacy-summarize`, `legacy-delta` and `sub-2` failed inside 24h but
            # are imported SQLite rows, so only `us-failed` counts.
            assert overview['failed_24h'] == 1, overview
            # rt-old finished 48h ago and must stay outside the window.
            assert overview['succeeded_24h'] == 5, overview
            assert overview['enabled_scheduled_slots'] == 24, overview
            assert overview['total_scheduled_slots'] == 24, overview

            # ---- task_group semantics ------------------------------------------
            code, realtime_page = realtime(
                {'status': 'all', 'task_group': 'realtime', 'page': 1, 'page_size': 2}
            )
            assert code == 200, (code, realtime_page)
            assert realtime_page['total'] == 4, realtime_page
            assert realtime_page['page'] == 1 and realtime_page['page_size'] == 2
            ids = [item['id'] for item in realtime_page['items']]
            assert ids == ['rt-repo', 'rt-batch'], ids
            item = realtime_page['items'][0]
            assert TASK_FIELDS <= set(item), item
            assert 'task_id' not in item, item
            assert item['status'] == 'running' and item['cancel_requested'] is True, item
            assert item['created_at'] and item['finished_at'] is None, item
            assert item['task_type'] == 'sync.repo_release', item
            assert item['task_group'] == 'sync' and item['legacy'] is False, item
            assert 'diagnostics' not in item, item
            assert realtime_page['items'][1]['task_group'] == 'translation', realtime_page

            code, second_page = realtime(
                {'status': 'all', 'task_group': 'realtime', 'page': 2, 'page_size': 2}
            )
            assert code == 200, (code, second_page)
            assert [entry['id'] for entry in second_page['items']] == ['rt-translate', 'rt-old']
            assert second_page['total'] == 4, second_page

            code, scheduled_page = realtime({'status': 'all', 'task_group': 'scheduled'})
            assert code == 200, (code, scheduled_page)
            scheduled_ids = [entry['id'] for entry in scheduled_page['items']]
            # Legacy `sync.subscriptions` rows are history, not scheduled work.
            assert scheduled_page['total'] == 5, scheduled_page
            assert scheduled_ids == [
                'sch-webhook',
                'sch-retry',
                'sch-brief-queued',
                'sch-brief-ok',
                'rt-notify',
            ], scheduled_ids
            assert 'us-running' not in scheduled_ids
            scheduled_types = {entry['task_type'] for entry in scheduled_page['items']}
            assert 'sync.starred' not in scheduled_types, scheduled_types
            assert 'sync.subscriptions' not in scheduled_types, scheduled_types
            scheduled_by_id = {entry['id']: entry for entry in scheduled_page['items']}
            assert 'diagnostics' not in scheduled_by_id['sch-brief-ok'], scheduled_by_id

            code, user_sync_page = realtime({'status': 'all', 'task_group': 'user_sync'})
            assert code == 200, (code, user_sync_page)
            assert user_sync_page['total'] == 2, user_sync_page
            assert [entry['task_type'] for entry in user_sync_page['items']] == [
                'sync.starred',
                'sync.starred',
            ], user_sync_page
            assert [entry['id'] for entry in user_sync_page['items']] == [
                'us-failed',
                'us-running',
            ], user_sync_page
            assert user_sync_page['items'][0]['error_message'] == 'star boom'

            code, legacy_page = realtime({'status': 'all', 'task_group': 'legacy'})
            assert code == 200, (code, legacy_page)
            assert legacy_page['total'] == 6, legacy_page
            assert all(entry['legacy'] is True for entry in legacy_page['items']), legacy_page
            assert all(entry['task_group'] is None for entry in legacy_page['items']), legacy_page

            code, sync_group_page = realtime({'status': 'all', 'task_group': 'sync'})
            assert code == 200, (code, sync_group_page)
            assert sync_group_page['total'] == 4, sync_group_page
            assert all(
                entry['task_group'] == 'sync' for entry in sync_group_page['items']
            ), sync_group_page

            # ---- status / task_type / exclude filters --------------------------
            code, failed_page = realtime({'status': 'failed', 'task_group': 'user_sync'})
            assert code == 200 and failed_page['total'] == 1, failed_page
            assert [entry['id'] for entry in failed_page['items']] == ['us-failed'], failed_page

            code, type_page = realtime(
                {'status': 'all', 'task_type': 'brief.daily_slot'}
            )
            assert code == 200 and type_page['total'] == 2, type_page

            code, excluded_page = realtime(
                {'status': 'all', 'exclude_task_type': 'brief.daily_slot'}
            )
            assert code == 200, excluded_page
            excluded_types = {entry['task_type'] for entry in excluded_page['items']}
            assert 'brief.daily_slot' not in excluded_types, excluded_page
            assert excluded_page['total'] == 15, excluded_page

            # ---- subscription tab and its diagnostics --------------------------
            code, subscription_page = realtime(
                {'status': 'all', 'task_type': 'sync.subscriptions'}
            )
            assert code == 200, (code, subscription_page)
            assert subscription_page['total'] == 3, subscription_page
            assert [entry['id'] for entry in subscription_page['items']] == [
                'sub-3',
                'sub-2',
                'sub-1',
            ], subscription_page
            by_id = {entry['id']: entry for entry in subscription_page['items']}
            assert by_id['sub-1']['skipped'] is True, by_id['sub-1']
            diagnostics = by_id['sub-1']['diagnostics']
            assert diagnostics['business_outcome']['code'] == 'disabled', diagnostics
            subscriptions = diagnostics['sync_subscriptions']
            assert subscriptions['skipped'] is True
            assert subscriptions['skip_reason'] == 'previous_run_active'
            assert subscriptions['trigger'] == 'scheduler'
            assert subscriptions['schedule_key'] == 'daily-06'
            # result_json has no collect object, so the progress event fills it.
            assert subscriptions['collect']['total_users'] == 7, subscriptions['collect']
            assert subscriptions['release']['total_repos'] == 3, subscriptions['release']
            assert subscriptions['log_available'] is False
            assert [event['id'] for event in subscriptions['recent_events']] == ['sub-ev-1']
            assert subscriptions['recent_events'][0]['message'] == 'collected'
            assert by_id['sub-1']['diagnostics']['business_outcome']['label'] == '已跳过'

            failed_diagnostics = by_id['sub-2']['diagnostics']
            assert failed_diagnostics['business_outcome']['code'] == 'failed', failed_diagnostics
            assert failed_diagnostics['business_outcome']['message'] == 'subscription boom'
            failed_release = failed_diagnostics['sync_subscriptions']['release']
            assert failed_release['total_repos'] == 4, failed_release
            assert failed_release['succeeded_repos'] == 1, failed_release
            assert failed_release['failed_repos'] == 1, failed_release
            assert failed_diagnostics['sync_subscriptions']['critical_events'] == 1
            # 25 seeded events must collapse to the newest 20, newest first.
            failed_events = failed_diagnostics['sync_subscriptions']['recent_events']
            assert len(failed_events) == 20, failed_events
            assert failed_events[0]['id'] == 'sub-ev-2-25', failed_events[0]
            assert failed_events[0]['message'] == 'event-25', failed_events[0]
            assert failed_events[-1]['id'] == 'sub-ev-2-06', failed_events[-1]

            running_diagnostics = by_id['sub-3']['diagnostics']
            assert running_diagnostics['business_outcome']['code'] == 'unknown', running_diagnostics
            running_subscriptions = running_diagnostics['sync_subscriptions']
            # 30 progress events: the newest 20 fill `releases_written`, while the
            # oldest `collect` summary is retained beyond that window.
            assert running_subscriptions['collect']['total_users'] == 11, running_subscriptions
            assert running_subscriptions['releases_written'] == 30, running_subscriptions
            assert running_subscriptions['recent_events'] == [], running_subscriptions

            # ---- migration read plans ------------------------------------------
            indexes = set(
                sql(
                    "SELECT indexname FROM pg_indexes WHERE schemaname = 'public'",
                    url,
                ).splitlines()
            )
            for index_name in (
                'idx_job_tasks_pg_realtime_created',
                'idx_job_tasks_pg_task_type_created',
                'idx_job_tasks_pg_finished_status',
                'idx_job_task_events_pg_stream',
                'idx_sync_subscription_events_pg_task_created',
            ):
                assert index_name in indexes, (index_name, sorted(indexes))
            # Ordering-driven reads must actually be index-served; the 24h
            # counters are checked by existence because a 13-row fixture lets
            # the planner prefer the other status-leading index.
            for index, statement in (
                (
                    'idx_sync_subscription_events_pg_task_created',
                    "SELECT id FROM public.sync_subscription_events WHERE task_id = 'sub-2' "
                    'ORDER BY created_at DESC, id DESC LIMIT 20',
                ),
                (
                    'idx_job_tasks_pg_realtime_created',
                    'SELECT id FROM public.job_tasks ORDER BY created_at DESC, id DESC LIMIT 20',
                ),
                (
                    'idx_job_task_events_pg_stream',
                    'SELECT id FROM public.job_task_events ORDER BY created_at, id LIMIT 200',
                ),
            ):
                plan = sql(
                    'SET enable_seqscan = off; EXPLAIN ' + statement, url
                )
                assert index in plan, (index, plan)

            # ---- pagination guards ---------------------------------------------
            code, clamped = realtime({'status': 'all', 'task_group': 'realtime', 'page_size': 500})
            assert code == 200 and clamped['page_size'] == 100, clamped
            code, overflow = realtime(
                {'status': 'all', 'page': 9223372036854775807, 'page_size': 100}
            )
            assert code == 400 and overflow['code'] == 'bad_request', (code, overflow)

            # ---- SSE stream ------------------------------------------------------
            stream = open_stream(base, cookies['seed-admin'], None)
            try:
                content_type = stream.headers.get('Content-Type', '')
                assert content_type.startswith('text/event-stream'), content_type
                assert read_frame(stream, lambda text: 'stream-ready' in text) is not None, (
                    'stream-ready comment missing'
                )
                sql(
                    "INSERT INTO public.job_tasks (id, task_type, status, source, payload_json, created_at, updated_at) "
                    "VALUES ('sse-task', 'translate.release.batch', 'queued', 'validation', '{}', now(), now())",
                    url,
                )
                sql(
                    "INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at) "
                    "VALUES ('sse-event-1', 'sse-task', 'task.created', '{\"task_id\":\"sse-task\"}', now())",
                    url,
                )
                frame = read_frame(stream, lambda text: 'sse-event-1' in text)
                assert frame is not None, 'no job event frame within the deadline'
                assert 'event: job.event' in frame, frame
                assert 'id: job-sse-event-1' in frame, frame
                payload = json.loads(
                    next(line[len('data: '):] for line in frame.splitlines() if line.startswith('data: '))
                )
                assert payload['event_id'] == 'sse-event-1', payload
                assert payload['task_id'] == 'sse-task', payload
                assert payload['task_type'] == 'translate.release.batch', payload
                assert payload['status'] == 'queued', payload
                assert payload['event_type'] == 'task.created', payload
            finally:
                stream.close()

            # Last-Event-ID resumes after the delivered event instead of
            # replaying it. The newer event exists before the stream opens, so a
            # resume that ignores the header and starts at the newest cursor
            # can only block here; only a real resume delivers it.
            sql(
                "INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at) "
                "VALUES ('sse-event-2', 'sse-task', 'task.started', '{\"task_id\":\"sse-task\"}', now())",
                url,
            )
            stream = open_stream(base, cookies['seed-admin'], 'job-sse-event-1')
            try:
                frame, seen = read_frame_with_history(
                    stream, lambda text: 'sse-event-2' in text
                )
                assert frame is not None, 'no resumed job event frame within the deadline'
                assert 'sse-event-1' not in seen, 'Last-Event-ID must not replay the acknowledged event'
            finally:
                stream.close()

            # ---- task detail contract -----------------------------------------
            code, detail = request('/api/admin/jobs/realtime/sub-1')
            assert code == 200, (code, detail)
            assert set(detail) == {'task', 'events', 'event_meta', 'diagnostics'}, detail
            task = detail['task']
            assert task['id'] == 'sub-1', task
            assert task['task_type'] == 'sync.subscriptions', task
            assert task['task_group'] is None and task['legacy'] is True, task
            assert isinstance(task['payload_json'], str), task
            assert json.loads(task['payload_json'])['trigger'] == 'scheduler', task
            assert isinstance(task['result_json'], str), task
            assert task['cancel_requested'] is False, task
            assert 'log_file_path' not in task, task
            assert 'payload' not in task and 'result' not in task, task
            assert [event['id'] for event in detail['events']] == [
                'sub-1-release',
                'sub-1-collect',
            ], detail['events']
            assert set(detail['events'][0]) == {
                'id',
                'event_type',
                'payload_json',
                'created_at',
            }, detail['events'][0]
            assert detail['event_meta'] == {
                'returned': 2,
                'total': 2,
                'limit': 200,
                'truncated': False,
            }, detail['event_meta']
            assert detail['diagnostics']['sync_subscriptions']['skipped'] is True
            assert detail['diagnostics']['business_outcome']['code'] == 'disabled'

            code, quiet_detail = request('/api/admin/jobs/realtime/rt-3')
            code, quiet_detail = request('/api/admin/jobs/realtime/rt-notify')
            assert code == 200, (code, quiet_detail)
            assert quiet_detail['task']['task_group'] == 'sync', quiet_detail
            assert quiet_detail['task']['legacy'] is False, quiet_detail
            assert quiet_detail['events'] == [], quiet_detail
            assert quiet_detail['event_meta'] == {
                'returned': 0,
                'total': 0,
                'limit': 200,
                'truncated': False,
            }, quiet_detail['event_meta']
            assert quiet_detail['diagnostics'] is None, quiet_detail

            code, body = request('/api/admin/jobs/realtime/does-not-exist')
            assert code == 404 and body['code'] == 'not_found', (code, body)
            code, _ = request('/api/admin/jobs/realtime/sub-1', 'seed-user')
            assert code == 403, code
            code, _ = request('/api/admin/jobs/realtime/sub-1', None)
            assert code == 401, code

            sql(
                "INSERT INTO public.job_tasks (id, task_type, status, source, payload_json, created_at, updated_at) "
                "VALUES ('dt-many', 'translate.release.batch', 'succeeded', 'validation', '{}', now(), now()); "
                "INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at) "
                "SELECT 'dt-many-' || g, 'dt-many', 'task.progress', '{}', now() + (g || ' seconds')::interval "
                "FROM generate_series(1, 205) AS g;",
                url,
            )
            code, truncated = request('/api/admin/jobs/realtime/dt-many')
            assert code == 200, (code, truncated)
            assert truncated['event_meta'] == {
                'returned': 200,
                'total': 205,
                'limit': 200,
                'truncated': True,
            }, truncated['event_meta']
            assert truncated['events'][0]['id'] == 'dt-many-205', truncated['events'][0]
            assert truncated['diagnostics'] is None, truncated

            print(
                json.dumps(
                    {
                        'overview_kpis': 'pass',
                        'task_group_semantics': 'pass',
                        'filters_and_pagination': 'pass',
                        'item_shape': 'pass',
                        'subscription_diagnostics': 'pass',
                        'bounded_diagnostics_and_plans': 'pass',
                        'sse_event_stream': 'pass',
                        'sse_last_event_id_resume': 'pass',
                        'task_detail_contract': 'pass',
                        'admin_only_access': 'pass',
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


def open_stream(base, cookie, last_event_id):
    headers = {'Cookie': cookie, 'Accept': 'text/event-stream'}
    if last_event_id:
        headers['Last-Event-ID'] = last_event_id
    request = urllib.request.Request(base + '/api/admin/jobs/events', headers=headers)
    return urllib.request.urlopen(request, timeout=30)


def read_frame(stream, predicate, timeout=15.0):
    frame, _ = read_frame_with_history(stream, predicate, timeout)
    return frame


def read_frame_with_history(stream, predicate, timeout=15.0):
    """Read SSE frames until one matches `predicate`.

    Returns the matching frame and every frame read on the way, so callers can
    assert which frames were *not* delivered.
    """
    deadline = time.monotonic() + timeout
    lines = []
    seen = []
    while time.monotonic() < deadline:
        try:
            line = stream.readline()
        except (TimeoutError, OSError):
            return None, ''.join(seen)
        if not line:
            break
        text = line.decode('utf-8', 'replace')
        if text in ('\n', '\r\n'):
            frame = ''.join(lines)
            lines = []
            if frame:
                seen.append(frame)
                if predicate(frame):
                    return frame, ''.join(seen)
            continue
        lines.append(text)
    return None, ''.join(seen)


if __name__ == '__main__':
    main()
