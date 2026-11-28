#!/usr/bin/env python3
"""Validate the PostgreSQL admin dashboard read surface on a synthetic database.

Seeds one row per task type for the current local day plus pre-aggregated
rollup rows, then asserts the `/api/admin/dashboard` response keeps the legacy
shape the web admin consumes: KPI summary, today snapshot, status breakdown with
business codes, task share ratios, trend points and the 24h LLM health summary.
"""
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
from urllib.parse import urlsplit, urlunsplit

from pg_seed_common import psql

ALPHABET = '23456789abcdefghjkmnpqrstuvwxyz'
TASK_TYPES = ('translation.generate', 'sync.releases', 'brief.daily_slot')
# Mirrors `ADMIN_JOB_TASK_GROUPS` in `src/admin_job_groups.rs`. The dashboard
# breakdown lists every current PostgreSQL runtime type, not the legacy triple.
RUNTIME_TASK_TYPES = (
    'translation.generate',
    'polish.backfill',
    'translate.release.batch',
    'translate.release',
    'translate.release_detail',
    'translate.notification',
    'sync.starred',
    'sync.releases',
    'sync.notifications',
    'sync.repo_release',
    'sync.all',
    'sync.access_refresh',
    'brief.daily_slot',
    'brief.generate',
    'webhook.push.manage',
    'webhook.push.reconcile',
    'retry.recent_failures',
    'release.reading_backfill',
)
LEGACY_TASK_TYPE = 'sync.subscriptions'


def local_id(seed):
    digest = hashlib.sha256(seed.encode()).digest()
    return ''.join(ALPHABET[digest[i] % len(ALPHABET)] for i in range(16))


def utc_now():
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0)


def iso(moment):
    return moment.isoformat().replace('+00:00', 'Z')


def shell(value):
    return "'" + str(value).replace("'", "''") + "'"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3034)
    args = parser.parse_args()
    name = 'octo_rill_admin_dashboard_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))

    def sql(query, db=url):
        return psql(db, query).strip()

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
        APP_DEFAULT_TIME_ZONE='UTC',
    )
    process = None
    admin = local_id('dashboard-admin')
    user = local_id('dashboard-user')
    failures = []

    def check(name, actual, expected):
        status = 'ok' if actual == expected else 'FAIL'
        print(f'{status:4} {name}: {actual!r} expected {expected!r}')
        if actual != expected:
            failures.append(name)

    try:
        bootstrap_env = dict(env, OCTORILL_PG_BOOTSTRAP_ONLY='1')
        subprocess.run([binary], env=bootstrap_env, check=True, stdout=subprocess.DEVNULL)

        now = utc_now()
        today = now.date().isoformat()
        yesterday = (now.date() - dt.timedelta(days=1)).isoformat()
        yesterday_point = (now.date() - dt.timedelta(days=1)).isoformat()
        # Anchor seeded "today" rows just after the UTC day boundary instead of
        # one hour before `now`: a run shortly after midnight would otherwise
        # date them to the previous day, and the day-scoped dashboard breakdown
        # would find none of them.
        created_today = iso(dt.datetime.combine(now.date(), dt.time(0, 0), tzinfo=dt.timezone.utc)
                                 + dt.timedelta(seconds=1))

        sql(
            "INSERT INTO public.users(id,github_user_id,login,created_at,updated_at,is_admin,is_disabled,"
            "last_active_at,daily_brief_time_zone,include_own_releases,webhook_push_enabled,webhook_push_desired_state) VALUES"
            f"({shell(admin)},101,'admin','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,0,{shell(iso(now))},NULL,0,0,'deleted'),"
            f"({shell(user)},102,'user','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,{shell(iso(now - dt.timedelta(days=3)))},'UTC',0,0,'deleted');"
        )

        # One task per task type for the current day, plus an older one outside
        # the live window to prove the breakdown is day-scoped.
        def task(seed, task_type, status, payload, result, created_at):
            sql(
                "INSERT INTO public.job_tasks(id,task_type,status,source,requested_by,payload_json,"
                "result_json,cancel_requested,created_at,updated_at) VALUES"
                f"({shell(local_id(seed))},{shell(task_type)},{shell(status)},'scheduler',NULL,"
                f"{shell(json.dumps(payload))},{shell(json.dumps(result)) if result is not None else 'NULL'},"
                f"0,{shell(created_at)},{shell(created_at)});"
            )

        translate_ok = {'release_ids': [1, 2]}
        task('translate-ok', TASK_TYPES[0], 'succeeded', translate_ok,
             {'items': [{'status': 'ready'}, {'status': 'ready'}]}, created_today)
        task('translate-failed', TASK_TYPES[0], 'failed', translate_ok,
             {'total': 2, 'error': 2}, created_today)
        task('sync-running', TASK_TYPES[1], 'running', {'repository_count': 3}, None, created_today)
        # Imported SQLite history: kept in the table, but never counted or rolled up.
        task('legacy-failed', LEGACY_TASK_TYPE, 'failed', {}, {'total': 1}, created_today)
        task('brief-ok', TASK_TYPES[2], 'succeeded', {}, {'total': 1, 'succeeded': 1, 'failed': 0, 'canceled': 0},
             created_today)
        task('brief-old', TASK_TYPES[2], 'failed', {},
             {'total': 1, 'succeeded': 0, 'failed': 1, 'canceled': 0}, iso(now - dt.timedelta(days=5)))

        sql(
            "INSERT INTO public.admin_dashboard_daily_rollups(rollup_date,time_zone,task_type,total_users,"
            "active_users,queued_count,running_count,succeeded_count,failed_count,canceled_count,"
            "business_ok_count,business_partial_count,business_failed_count,business_disabled_count,updated_at) VALUES"
            f"({shell(yesterday_point)},'UTC',{shell(TASK_TYPES[0])},2,1,0,0,3,1,0,3,0,1,0,{shell(iso(now))}),"
            f"({shell(yesterday_point)},'UTC',{shell(TASK_TYPES[2])},2,1,0,0,2,0,0,2,0,0,0,{shell(iso(now))}),"
            f"({shell(yesterday_point)},'UTC',{shell(LEGACY_TASK_TYPE)},2,1,0,0,0,5,0,0,0,5,0,{shell(iso(now))});"
        )

        sql(
            "INSERT INTO public.llm_calls(id,status,source,model,created_at,updated_at,finished_at,error_text,"
            "scheduler_wait_ms,duration_ms,max_tokens,attempt_count,prompt_text) VALUES"
            f"({shell(local_id('llm-1'))},'failed','translation','seed',{shell(iso(now - dt.timedelta(minutes=30)))},"
            f"{shell(iso(now - dt.timedelta(minutes=30)))},{shell(iso(now - dt.timedelta(minutes=30)))},'429 rate limit',0,10,100,1,'p'),"
            f"({shell(local_id('llm-2'))},'failed','summary','seed',{shell(iso(now - dt.timedelta(minutes=20)))},"
            f"{shell(iso(now - dt.timedelta(minutes=20)))},{shell(iso(now - dt.timedelta(minutes=20)))},'429 rate limit',0,10,100,1,'p'),"
            f"({shell(local_id('llm-3'))},'succeeded','translation','seed',{shell(iso(now - dt.timedelta(minutes=10)))},"
            f"{shell(iso(now - dt.timedelta(minutes=10)))},{shell(iso(now - dt.timedelta(minutes=10)))},NULL,0,10,100,1,'p');"
        )

        with tempfile.TemporaryDirectory(prefix='octo-rill-admin-dashboard-') as tmp:
            tmp = pathlib.Path(tmp)
            cookie_file = tmp / 'admin.cookie'
            subprocess.run(
                [helper, '--postgres-url', url, '--user-id', admin,
                 '--cookie-file', cookie_file, '--isolated-validation-copy'],
                check=True, stdout=subprocess.DEVNULL,
            )
            cookie = cookie_file.read_text().strip()
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

            def get(path, as_admin=True):
                headers = {'Cookie': cookie} if as_admin else {}
                req = urllib.request.Request(base + path, headers=headers)
                try:
                    with urllib.request.urlopen(req, timeout=10) as response:
                        return response.status, json.load(response)
                except urllib.error.HTTPError as error:
                    return error.code, json.load(error)

            status, body = get('/api/admin/dashboard?window=7d')
            check('status', status, 200)
            missing = [key for key in (
                'generated_at', 'time_zone', 'summary', 'today_live', 'status_breakdown',
                'task_share', 'trend_points', 'llm_health', 'window_meta',
            ) if key not in body]
            check('top_level_keys', missing, [])

            check('time_zone', body.get('time_zone'), 'UTC')
            check('summary.total_users', body['summary']['total_users'], 2)
            check('summary.active_users_today', body['summary']['active_users_today'], 1)
            check('summary.queued_tasks', body['summary']['queued_tasks'], 0)
            check('summary.running_tasks', body['summary']['running_tasks'], 1)
            check('summary.ongoing_by_task', body['summary']['ongoing_by_task'],
                  {'translations': 0, 'sync': 1, 'briefs': 0, 'operations': 0})

            check('today_live.date', body['today_live']['date'], today)
            check('today_live.running_tasks', body['today_live']['running_tasks'], 1)

            items = {item['task_type']: item for item in body['status_breakdown']['items']}
            check('translate.total', items[TASK_TYPES[0]]['total'], 2)
            check('status_item_types', set(items), set(RUNTIME_TASK_TYPES))
            check('legacy.excluded_from_breakdown', LEGACY_TASK_TYPE in items, False)
            check('translate.total', items[TASK_TYPES[0]]['total'], 2)
            check('translate.labels', items[TASK_TYPES[0]]['label'], '翻译生成')
            check('translate.group', items[TASK_TYPES[0]]['task_group'], 'translation')
            check('translate.business_counts', items[TASK_TYPES[0]]['business_counts'],
                  {'ok': 1, 'partial': 0, 'failed': 1, 'disabled': 0})
            check('translate.success_rate', items[TASK_TYPES[0]]['success_rate'], 0.5)
            check('translate.business_success_rate', items[TASK_TYPES[0]]['business_success_rate'], 0.5)
            check('brief.old_task_excluded', items[TASK_TYPES[2]]['total'], 1)
            check('sync.running', items[TASK_TYPES[1]]['running'], 1)
            check('sync.group', items[TASK_TYPES[1]]['task_group'], 'sync')
            check('status_breakdown.total', body['status_breakdown']['total'], 4)
            check('status_breakdown.business_counts', body['status_breakdown']['business_counts'],
                  {'ok': 2, 'partial': 0, 'failed': 1, 'disabled': 0})

            check('task_share_total_ratio', round(sum(item['share_ratio'] for item in body['task_share']), 6), 1.0)

            points = body['trend_points']
            check('trend_point_count', len(points), 7)
            check('trend_dates_ascending', [point['date'] for point in points],
                  sorted(point['date'] for point in points))
            by_date = {point['date']: point for point in points}
            check('trend_yesterday_translations', by_date[yesterday]['translations_total'], 4)
            check('trend_yesterday_translations_failed', by_date[yesterday]['translations_failed'], 1)
            check('trend_yesterday_briefs', by_date[yesterday]['briefs_total'], 2)
            check('trend_today_translations', by_date[today]['translations_total'], 2)
            check('trend_today_sync', by_date[today]['sync_total'], 1)
            check('trend_legacy_excluded', by_date[yesterday]['sync_total'], 0)
            check('trend_today_active_users', by_date[today]['active_users'], 1)

            check('llm.calls_24h', body['llm_health']['calls_24h'], 3)
            check('llm.failed_24h', body['llm_health']['failed_24h'], 2)
            check('llm.top_reason', body['llm_health']['top_failure_reasons'],
                  [{'label': '429 rate limit', 'count': 2}])
            check('llm.top_sources', body['llm_health']['top_failure_sources'],
                  [{'label': 'summary', 'count': 1}, {'label': 'translation', 'count': 1}])
            check('llm.last_failure_at_present', isinstance(body['llm_health']['last_failure_at'], str), True)

            check('window_meta.selected', body['window_meta']['selected_window'], '7d')
            check('window_meta.available', body['window_meta']['available_windows'], ['7d', '30d'])
            check('window_meta.point_count', body['window_meta']['point_count'], 7)

            status, body30 = get('/api/admin/dashboard?window=30d')
            check('30d.status', status, 200)
            check('30d.point_count', len(body30.get('trend_points', [])), 30)
            check('30d.selected', body30.get('window_meta', {}).get('selected_window'), '30d')

            status, _ = get('/api/admin/dashboard?window=90d')
            check('invalid_window.status', status, 400)

            status, _ = get('/api/admin/dashboard?window=7d', as_admin=False)
            check('anonymous.status', status, 401)

            log.seek(0)
            server_log = log.read()
            check('no_server_errors', 'admin dashboard query failed' in server_log, False)
    finally:
        if process is not None:
            process.terminate()
            process.wait(timeout=10)
        sql(f'DROP DATABASE IF EXISTS {name}', args.admin_url)

    if failures:
        raise SystemExit(f'{len(failures)} assertion(s) failed: {", ".join(failures)}')
    print('admin dashboard suite passed')


if __name__ == '__main__':
    main()
