#!/usr/bin/env python3
"""Validate the PostgreSQL scheduler lanes against a disposable database.

Every lane is ticked through `OCTORILL_PG_SCHEDULER_TICK` with an explicit
`OCTORILL_PG_SCHEDULER_NOW`, so the suite proves enqueue counts, single-flight
dedupe and daily idempotence without waiting on tokio timers. The task worker
stays disabled for the whole run.
"""
import argparse
import base64
import os
import pathlib
import subprocess
from urllib.parse import urlsplit, urlunsplit

from pg_seed_common import psql

ACTIVE_USERS = "is_disabled = 0 AND paused_at IS NULL"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--admin-url', required=True)
    parser.add_argument('--binary-dir', type=pathlib.Path, default=pathlib.Path('target/debug'))
    parser.add_argument('--port', type=int, default=3122)
    args = parser.parse_args()

    name = 'octo_rill_scheduler_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))
    binary = (args.binary_dir / 'octo-rill').resolve()
    psql(args.admin_url, 'CREATE DATABASE ' + name)

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

    def sql(query):
        return psql(url, query)

    def tick(lane, now):
        tick_env = dict(env, OCTORILL_PG_SCHEDULER_TICK=lane, OCTORILL_PG_SCHEDULER_NOW=now)
        result = subprocess.run([binary], env=tick_env, capture_output=True, text=True)
        assert result.returncode == 0, (lane, now, result.stderr[-2000:])

    def count(query):
        return int(sql(query))

    def task_count(task_type, clause='TRUE'):
        return count(
            f"SELECT COUNT(*) FROM public.job_tasks WHERE task_type='{task_type}' AND {clause}"
        )

    def expect(actual, expected, message):
        if actual != expected:
            raise AssertionError(f'{message}: expected {expected}, got {actual}')

    try:
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)

        # ---------------------------------------------------------------- seed
        sql(
            "INSERT INTO public.users (id, github_user_id, login, created_at, updated_at, "
            "is_admin, is_disabled, daily_brief_time_zone, paused_at) VALUES"
            "('star-user',101,'star','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,"
            "'America/New_York',NULL),"
            "('star-paused',102,'star_paused','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,"
            "'UTC','2026-05-01T00:00:00Z'),"
            "('star-disabled',103,'star_disabled','2026-01-01T00:00:00Z',"
            "'2026-01-01T00:00:00Z',0,1,'UTC',NULL);"
            "INSERT INTO public.github_connections (id, user_id, github_user_id, login, "
            "access_token_ciphertext, access_token_nonce, scopes, linked_at, updated_at) VALUES"
            "('conn-star','star-user',101,'star','\\x00','\\x00','','2026-01-01T00:00:00Z',"
            "'2026-01-01T00:00:00Z'),"
            "('conn-paused','star-paused',102,'star_paused','\\x00','\\x00','',"
            "'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'),"
            "('conn-disabled','star-disabled',103,'star_disabled','\\x00','\\x00','',"
            "'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');"
            "INSERT INTO public.admin_runtime_settings (id, llm_max_concurrency, "
            "translation_general_worker_concurrency, translation_dedicated_worker_concurrency, "
            "created_at, updated_at) VALUES"
            "(1,1,3,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z') "
            "ON CONFLICT (id) DO NOTHING;"
            "UPDATE public.admin_runtime_settings SET sync_auto_fetch_interval_minutes = 60, "
            "sync_auto_fetch_effective_at = NULL, star_sync_delta_interval_minutes = 30, "
            "retry_recent_failures_interval_minutes = 10, daily_brief_schedule_local_time = "
            "'04:00' WHERE id = 1;"
        )

        # ------------------------------------------------------- star sync lane
        tick('star_sync', '2026-06-01T04:00:00Z')
        expect(task_count('sync.starred'), 1, 'star sync enqueued one snapshot')
        expect(
            task_count('sync.starred', "requested_by IS NULL AND payload_json LIKE '%star-user%'"),
            1,
            'star sync targets the connected active user',
        )
        expect(
            task_count('sync.starred', "payload_json LIKE '%star-paused%'"), 0,
            'star sync skips paused users',
        )
        expect(
            task_count('sync.starred', "payload_json LIKE '%star-disabled%'"), 0,
            'star sync skips disabled users',
        )
        tick('star_sync', '2026-06-01T04:00:30Z')
        expect(task_count('sync.starred'), 1, 'star sync dedupes inside the same bucket')
        expect(
            count(
                "SELECT COUNT(*) FROM public.scheduled_task_dispatch_state "
                "WHERE schedule_name = 'sync.starred.delta:star-user' "
                "AND last_dispatch_key LIKE 'interval:30:%'"
            ),
            1,
            'star sync records the delta bucket',
        )

        # --------------------------------------------- star epoch slice probe
        # A slice scheduled between two delta buckets must not wait for the
        # bucket: the lane probes `star_sync_epochs` and enqueues immediately.
        def seed_epoch(epoch_id, user_id, connection_id, next_slice_not_before,
                       lease_expires_at=None):
            lease = f"'{lease_expires_at}'" if lease_expires_at else 'NULL'
            sql(
                "INSERT INTO public.star_sync_epochs (id, user_id, github_connection_id, status, "
                "started_at, next_cursor, total_count_at_start, processed_pages, processed_items, "
                "next_slice_not_before, lease_owner_id, lease_expires_at, completed_at, "
                "failure_reason, created_at, updated_at) VALUES "
                f"('{epoch_id}','{user_id}','{connection_id}','queued',"
                "'2026-06-01T03:30:00Z',NULL,500,2,200,"
                f"'{next_slice_not_before}',NULL,{lease},NULL,NULL,"
                "'2026-06-01T03:30:00Z','2026-06-01T03:30:00Z');"
            )

        sql("DELETE FROM public.job_tasks WHERE task_type = 'sync.starred';")
        seed_epoch('epoch-due', 'star-user', 'conn-star', '2026-06-01T03:59:00Z')
        seed_epoch('epoch-paused', 'star-paused', 'conn-paused', '2026-06-01T03:59:00Z')
        # 04:05 is still inside the delta bucket claimed at 04:00, so only the
        # due-epoch probe can enqueue here.
        tick('star_sync', '2026-06-01T04:05:00Z')
        expect(task_count('sync.starred'), 1, 'due epoch enqueues exactly one star snapshot')
        expect(
            task_count(
                'sync.starred',
                "payload_json LIKE '%star-user%' AND payload_json LIKE '%epoch_due%'",
            ),
            1,
            'due epoch enqueues the slice for its own user',
        )
        expect(
            task_count('sync.starred', "payload_json LIKE '%star-paused%'"), 0,
            'due epoch probe skips paused users',
        )
        tick('star_sync', '2026-06-01T04:05:20Z')
        star_rows = sql(
            "SELECT source || '|' || payload_json FROM public.job_tasks "
            "WHERE task_type = 'sync.starred' ORDER BY created_at, id"
        )
        expect(
            task_count('sync.starred'), 1,
            f'a second tick enqueues nothing while the task waits -> {star_rows}',
        )

        sql(
            "DELETE FROM public.job_tasks WHERE task_type = 'sync.starred';"
            "UPDATE public.star_sync_epochs SET next_slice_not_before = '2026-06-01T05:00:00Z' "
            "WHERE id = 'epoch-due';"
        )
        tick('star_sync', '2026-06-01T04:06:00Z')
        expect(task_count('sync.starred'), 0, 'an epoch whose slice is not due enqueues nothing')

        sql(
            "UPDATE public.star_sync_epochs SET next_slice_not_before = '2026-06-01T03:00:00Z', "
            "lease_expires_at = '2026-06-01T04:30:00Z' WHERE id = 'epoch-due';"
        )
        tick('star_sync', '2026-06-01T04:07:00Z')
        expect(task_count('sync.starred'), 0, 'a live slice lease keeps the probe quiet')

        sql(
            "UPDATE public.star_sync_epochs SET lease_expires_at = NULL "
            "WHERE id = 'epoch-due';"
            "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, "
            "payload_json, created_at, updated_at) VALUES "
            "('existing-star-task','sync.starred','queued','scheduler',NULL,"
            "'{\"user_id\":\"star-user\",\"trigger\":\"schedule\"}',"
            "'2026-06-01T04:08:00Z','2026-06-01T04:08:00Z');"
        )
        tick('star_sync', '2026-06-01T04:08:00Z')
        expect(task_count('sync.starred'), 1, 'an already queued star task absorbs the probe')
        expect(
            count(
                "SELECT COUNT(*) FROM public.job_tasks "
                "WHERE id = 'existing-star-task' AND status = 'queued'"
            ),
            1,
            'the probe never replaces the queued snapshot',
        )

        # --------------------------------------------------- subscription lane
        sql(
            "INSERT INTO public.users (id, github_user_id, login, created_at, updated_at, "
            "is_admin, is_disabled, daily_brief_time_zone, paused_at) VALUES"
            "('sub-user',104,'sub','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,"
            "'Asia/Tokyo',NULL);"
        )
        tick('subscriptions', '2026-06-01T04:00:00Z')
        for user_id in ('star-user', 'sub-user'):
            for task_type in ('sync.releases', 'sync.notifications'):
                expect(
                    task_count(task_type, f"payload_json LIKE '%{user_id}%'"), 1,
                    f'{task_type} enqueued once for {user_id}',
                )
        expect(
            task_count('sync.releases', "payload_json LIKE '%star-paused%'"), 0,
            'subscription lane skips paused users',
        )
        expect(
            task_count('sync.notifications', "payload_json LIKE '%star-disabled%'"), 0,
            'subscription lane skips disabled users',
        )
        tick('subscriptions', '2026-06-01T04:00:40Z')
        expect(task_count('sync.releases'), 2, 'subscription lane dedupes inside the bucket')
        expect(task_count('sync.notifications'), 2, 'subscription lane dedupes inside the bucket')
        expect(
            count(
                "SELECT COUNT(*) FROM public.scheduled_task_dispatch_state "
                "WHERE schedule_name = 'sync.subscriptions' "
                "AND last_dispatch_key LIKE 'interval:60:%'"
            ),
            1,
            'subscription lane records the interval bucket',
        )

        # ------------------------------------------------------ daily brief lane
        sql(
            "INSERT INTO public.users (id, github_user_id, login, created_at, updated_at, "
            "is_admin, is_disabled, daily_brief_time_zone, paused_at) VALUES"
            "('brief-user',105,'brief','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,'UTC',"
            "NULL);"
            "UPDATE public.daily_brief_hour_slots SET enabled = 1, last_dispatch_at = NULL "
            "WHERE hour_utc IN (4, 5);"
        )
        tick('daily_brief', '2026-06-01T04:00:30Z')
        expect(task_count('brief.daily_slot'), 1, 'enabled hour enqueues one brief slot task')
        expect(
            task_count('brief.daily_slot', "payload_json LIKE '%brief-user%'"), 1,
            'brief slot targets the user whose local time is in this UTC hour',
        )
        tick('daily_brief', '2026-06-01T04:10:00Z')
        expect(task_count('brief.daily_slot'), 1, 'the same hour slot is never dispatched twice')

        sql(
            "UPDATE public.admin_runtime_settings SET daily_brief_schedule_local_time = '05:00' "
            "WHERE id = 1;"
            "UPDATE public.daily_brief_hour_slots SET enabled = 0, last_dispatch_at = NULL "
            "WHERE hour_utc = 5;"
        )
        tick('daily_brief', '2026-06-01T05:05:00Z')
        expect(task_count('brief.daily_slot'), 1, 'disabled hour enqueues nothing')

        # --------------------------------------------- recent failure retry lane
        sql(
            "INSERT INTO public.users (id, github_user_id, login, created_at, updated_at, "
            "is_admin, is_disabled, daily_brief_time_zone, paused_at) VALUES"
            "('retry-user',110,'retry','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,'UTC',"
            "NULL);"
            "INSERT INTO public.briefs (id, user_id, date, window_start_utc, window_end_utc, "
            "effective_time_zone, effective_local_boundary, generation_source, content_markdown, "
            "created_at, updated_at) VALUES"
            "('retry-brief','retry-user','2026-05-31','2026-05-31T00:00:00Z',"
            "'2026-06-01T00:00:00Z','UTC','00:00','content_refresh_failed','# failed brief',"
            "'2026-05-31T00:05:00Z','2026-06-01T03:00:00Z');"
        )
        tick('recent_failures', '2026-06-01T06:00:00Z')
        expect(task_count('brief.generate'), 1, 'recent failure retry enqueues one brief task')
        expect(
            task_count('brief.generate', "payload_json LIKE '%retry-user%'"), 1,
            'recent failure retry targets the failed brief owner',
        )
        tick('recent_failures', '2026-06-01T06:05:00Z')
        expect(task_count('brief.generate'), 1, 'recent failure retry dedupes inside the bucket')
        expect(
            count(
                "SELECT COUNT(*) FROM public.scheduled_task_dispatch_state "
                "WHERE schedule_name = 'retry.recent_failures' "
                "AND last_dispatch_key LIKE 'interval:10:%'"
            ),
            1,
            'recent failure retry records the retry bucket',
        )

        # ----------------------------------------------------- webhook push lane
        sql(
            "INSERT INTO public.users (id, github_user_id, login, created_at, updated_at, "
            "is_admin, is_disabled, daily_brief_time_zone, paused_at, include_own_releases, "
            "webhook_push_enabled, webhook_push_desired_state) VALUES"
            "('hook-demand',106,'hook_demand','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,"
            "'UTC',NULL,1,1,'enabled'),"
            "('hook-audit',107,'hook_audit','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,"
            "'UTC',NULL,1,1,'enabled');"
            "INSERT INTO public.webhook_push_reconcile_demands (user_id, requested_generation, "
            "completed_generation, requested_at, updated_at) VALUES"
            "('hook-demand',1,0,'2026-06-02T03:00:00Z','2026-06-02T03:00:00Z');"
            "UPDATE public.admin_runtime_settings SET webhook_push_audit_interval_days = 7, "
            "webhook_push_audit_last_started_at = '2026-06-02T00:00:00Z' WHERE id = 1;"
        )
        tick('webhook_push', '2026-06-02T04:00:00Z')
        expect(
            task_count(
                'webhook.push.manage',
                "source = 'webhook_reconcile_demand' AND payload_json LIKE '%hook-demand%'",
            ),
            1,
            'reconcile demand enqueues one manage task',
        )
        expect(
            task_count('webhook.push.manage', "source = 'webhook-audit'"), 0,
            'a recent audit does not enqueue again',
        )
        expect(
            task_count(
                'webhook.push.manage',
                "payload_json LIKE '%\"demand_generation\":1%'",
            ),
            1,
            'reconcile demand task carries its generation',
        )
        tick('webhook_push', '2026-06-02T04:00:30Z')
        expect(task_count('webhook.push.manage'), 1, 'reconcile demand is single flight')

        sql(
            "UPDATE public.webhook_push_reconcile_demands SET completed_generation = "
            "requested_generation WHERE user_id = 'hook-demand';"
            "UPDATE public.admin_runtime_settings SET webhook_push_audit_last_started_at = "
            "'2026-01-01T00:00:00Z' WHERE id = 1;"
        )
        tick('webhook_push', '2026-06-02T04:10:00Z')
        expect(
            task_count('webhook.push.manage', "source = 'webhook-audit'"), 1,
            'due audit enqueues one reconcile per eligible user',
        )
        expect(
            task_count(
                'webhook.push.manage',
                "source = 'webhook-audit' AND payload_json LIKE '%hook-audit%'",
            ),
            1,
            'due audit targets eligible users',
        )
        expect(
            count(
                "SELECT COUNT(*) FROM public.admin_runtime_settings "
                "WHERE id = 1 AND webhook_push_audit_last_started_at LIKE '2026-06-02T04:10:00%'"
            ),
            1,
            'audit records its start so the interval advances',
        )
        tick('webhook_push', '2026-06-02T04:20:00Z')
        expect(
            task_count('webhook.push.manage'), 2,
            'audit does not duplicate queued reconcile work',
        )

        # -------------------------------------------------- admin rollup lane
        sql(
            "UPDATE public.users SET last_active_at = '2026-06-04T01:00:00Z' "
            "WHERE id = 'hook-audit';"
        )
        tick('admin_rollup', '2026-06-04T04:00:00Z')
        expect(
            count(
                "SELECT COUNT(*) FROM public.admin_dashboard_daily_rollups "
                "WHERE rollup_date = '2026-06-04' AND time_zone = 'UTC'"
            ),
            3,
            'rollup writes one row per dashboard task type',
        )
        expect(
            count(
                "SELECT COUNT(*) FROM public.admin_dashboard_daily_rollups "
                "WHERE rollup_date = '2026-06-04' AND time_zone = 'UTC' "
                "AND total_users > 0 AND active_users >= 1"
            ),
            3,
            'rollup counts users and their activity window',
        )
        expect(
            count(
                "SELECT COUNT(*) FROM public.admin_dashboard_daily_rollups "
                "WHERE rollup_date = '2026-06-04' AND time_zone = 'UTC' "
                "AND task_type = 'brief.daily_slot' AND queued_count = 0"
            ),
            1,
            'rollup only counts tasks created inside the local day',
        )
        tick('admin_rollup', '2026-06-04T04:15:00Z')
        expect(
            count(
                "SELECT COUNT(*) FROM public.admin_dashboard_daily_rollups "
                "WHERE rollup_date = '2026-06-04' AND time_zone = 'UTC'"
            ),
            3,
            'rollup refresh is idempotent',
        )

        # ---------------------------------------------------- account pause lane
        sql(
            "INSERT INTO public.users (id, github_user_id, login, created_at, updated_at, "
            "is_admin, is_disabled, daily_brief_time_zone, paused_at, last_active_at) VALUES"
            "('pause-old',108,'pause_old','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,"
            "'UTC',NULL,'2026-04-01T00:00:00Z'),"
            "('pause-fresh',109,'pause_fresh','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',0,0,"
            "'UTC',NULL,'2026-06-03T04:00:00Z');"
        )
        tick('account_pause', '2026-06-03T03:00:00Z')
        expect(
            count("SELECT COUNT(*) FROM public.users WHERE id = 'pause-old' AND paused_at IS NULL"),
            1,
            'account pause waits for its local time',
        )
        tick('account_pause', '2026-06-03T04:00:00Z')
        expect(
            count(
                "SELECT COUNT(*) FROM public.users "
                "WHERE id = 'pause-old' AND paused_at LIKE '2026-06-03%'"
            ),
            1,
            'account pause sets paused_at for the inactive window',
        )
        expect(
            count(
                "SELECT COUNT(*) FROM public.users WHERE id = 'pause-fresh' AND paused_at IS NULL"
            ),
            1,
            'account pause keeps recently active users',
        )
        paused_updated_at = sql(
            "SELECT updated_at FROM public.users WHERE id = 'pause-old'"
        )
        tick('account_pause', '2026-06-03T04:30:00Z')
        expect(
            sql("SELECT updated_at FROM public.users WHERE id = 'pause-old'"),
            paused_updated_at,
            'account pause runs at most once per local day',
        )

        print('pg scheduler seed suite passed')
    finally:
        psql(args.admin_url, f'DROP DATABASE IF EXISTS {name} WITH (FORCE)')


if __name__ == '__main__':
    main()
