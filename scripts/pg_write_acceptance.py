#!/usr/bin/env python3
"""Probe the PostgreSQL write path with reversible, external-free mutations.

Every case is allowlisted from a handler that only reads or writes PostgreSQL and
queues work for the (disabled) worker. Each mutation snapshots its rows, prints
the rollback SQL, writes through the HTTP API, verifies the persisted effect with
an API read-back plus a direct SQL read, then restores the snapshot and asserts
the restore. Routes whose handlers reach GitHub, an LLM provider, a webhook
receiver or any other outbound service are refused and reported as
`skipped_external`; routes that mutate durable state outside the approved
reversal set are reported as `skipped_scope`.

The worker must be disabled (`OCTORILL_PG_WORKER_ENABLED=0`): the probe asserts
that every task it enqueues stays `queued` and that no task created during the
run ever becomes `running`.
"""
import argparse
import datetime as dt
import json
import os
import pathlib
import re
import sys
import urllib.error
import urllib.parse
import urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from pg_seed_common import psql  # noqa: E402

ONLINE_MIGRATION_ID = 'content-processing-online-v2'

# Routes this probe writes to, with the exact handler-visible contract.
ALLOWED = {
    ('PATCH', '/api/me/profile'): 'profile time zone and webhook eligibility',
    ('POST', '/api/me/api-keys'): 'create then revoke a probe API key',
    ('DELETE', '/api/me/api-keys/{api_key_id}'): 'revoke the probe API key',
    ('PUT', '/api/repos/{owner}/{repo}/following'): 'follow a visible repository',
    ('DELETE', '/api/repos/{owner}/{repo}/following'): 'unfollow it again',
    ('PATCH', '/api/admin/jobs/scheduled/{hour_utc}'): 'flip one daily-brief slot flag',
    ('PATCH', '/api/admin/jobs/llm/runtime-config'): 'change llm_max_concurrency',
    ('PATCH', '/api/admin/users/{user_id}'): 'idempotent patch on the probe admin',
    ('PATCH', '/api/admin/users/{user_id}/profile'): 'probe user daily-brief time zone',
    ('POST', '/api/me/webhook-push/reconcile'): 'enqueue-only webhook reconcile',
    ('PATCH', '/api/me/webhook-push'): 'webhook desired-state flip (enqueue-only)',
    ('POST', '/api/admin/jobs/migrations/{migration_id}/pause'): 'pause the online migration',
    ('POST', '/api/admin/jobs/migrations/{migration_id}/resume'): 'resume the online migration',
    ('POST', '/api/admin/jobs/search-index/resume'): 'resume a failed search backfill',
    ('POST', '/api/me/resume'): 'resume account (enqueue-only)',
    ('POST', '/api/sync/starred'): 'enqueue sync.starred for the cancel probe',
    ('POST', '/api/tasks/{task_id}/cancel'): 'cancel the probe-created task',
}

# Handlers verified by reading src/postgres_server.rs and the modules below.
SKIPPED = [
    {
        'method': 'POST', 'route': '/api/reaction-token/check',
        'classification': 'external',
        'reason': 'postgres_reactions::check_reaction_token calls check_reaction_pat_with_github',
    },
    {
        'method': 'PUT', 'route': '/api/reaction-token',
        'classification': 'external',
        'reason': 'postgres_reactions::upsert_reaction_token validates the PAT through GitHub',
    },
    {
        'method': 'GET', 'route': '/api/reaction-token/status',
        'classification': 'scope',
        'reason': 'verified DB-only token status read; not a write path',
    },
    {
        'method': 'POST', 'route': '/api/release/reactions/toggle',
        'classification': 'external',
        'reason': 'postgres_reactions::toggle_release_reaction mutates reactions through the GitHub API',
    },
    {
        'method': 'POST', 'route': '/api/feed/reactions/refresh',
        'classification': 'external',
        'reason': 'postgres_reactions::refresh_feed_reactions fetches live reactions from GitHub',
    },
    {
        'method': 'POST', 'route': '/translate/requests',
        'classification': 'external', 'reason': 'translation work calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/translate/requests/{request_id}/retry',
        'classification': 'external', 'reason': 'translation retry calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/translate/results',
        'classification': 'external', 'reason': 'translation result resolution calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/api/translate/requests',
        'classification': 'external', 'reason': 'translation work calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/api/translate/requests/{request_id}/retry',
        'classification': 'external', 'reason': 'translation retry calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/api/translate/results',
        'classification': 'external', 'reason': 'translation result resolution calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/translate/releases/batch',
        'classification': 'external', 'reason': 'batch translation calls the LLM provider inline',
    },
    {
        'method': 'POST', 'route': '/translate/releases/batch/stream',
        'classification': 'external', 'reason': 'streaming batch translation calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/translate/release',
        'classification': 'external', 'reason': 'release translation calls the LLM provider inline',
    },
    {
        'method': 'POST', 'route': '/translate/release/stream',
        'classification': 'external', 'reason': 'streaming release translation calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/api/translate/releases/batch',
        'classification': 'external', 'reason': 'batch translation calls the LLM provider inline',
    },
    {
        'method': 'POST', 'route': '/api/translate/releases/batch/stream',
        'classification': 'external', 'reason': 'streaming batch translation calls the LLM provider',
    },
    {
        'method': 'POST', 'route': '/api/translate/release',
        'classification': 'external', 'reason': 'release translation calls the LLM provider inline',
    },
    {
        'method': 'POST', 'route': '/api/translate/release/detail/batch',
        'classification': 'external', 'reason': 'detail translation calls the LLM provider inline',
    },
    {
        'method': 'POST', 'route': '/api/translate/release/detail',
        'classification': 'external', 'reason': 'detail translation calls the LLM provider inline',
    },
    {
        'method': 'POST', 'route': '/api/translate/notifications/batch',
        'classification': 'external', 'reason': 'notification translation calls the LLM provider inline',
    },
    {
        'method': 'POST', 'route': '/api/translate/notification',
        'classification': 'external', 'reason': 'notification translation calls the LLM provider inline',
    },
    {
        'method': 'POST', 'route': '/api/webhooks/github/releases',
        'classification': 'external',
        'reason': 'postgres_webhook::receive is the GitHub webhook receiver',
    },
    {
        'method': 'POST', 'route': '/api/repos/{owner}/{repo}/public-release',
        'classification': 'external',
        'reason': 'publish governs the externally visible public release surface; handler is DB-only today but outside the approved set',
    },
    {
        'method': 'DELETE', 'route': '/api/repos/{owner}/{repo}/public-release',
        'classification': 'external',
        'reason': 'unpublish deletes cached releases and public visibility state; not reversible without a full snapshot',
    },
    {
        'method': 'POST', 'route': '/api/admin/jobs/content-processing/cutover',
        'classification': 'global_mode',
        'reason': 'flips content_processing_control.mode and changes read semantics globally',
    },
    {
        'method': 'POST', 'route': '/api/admin/jobs/content-processing/freeze',
        'classification': 'global_mode',
        'reason': 'flips content_processing_control.mode and changes read semantics globally',
    },
    {
        'method': 'POST', 'route': '/api/admin/jobs/content-processing/identity-upgrade',
        'classification': 'global_mode',
        'reason': 'mutates global identity-upgrade status outside the approved set',
    },
    {
        'method': 'POST', 'route': '/api/admin/jobs/release-reading/backfill',
        'classification': 'scope',
        'reason': 'verified enqueue-only (no upstream call) but creates release_reading_backfill_runs bookkeeping outside the approved set',
    },
    {
        'method': 'POST', 'route': '/api/sync/all',
        'classification': 'scope',
        'reason': 'enqueue-only; worker disabled, covered by the canonical sync.starred enqueue case',
    },
    {
        'method': 'POST', 'route': '/api/sync/releases',
        'classification': 'scope',
        'reason': 'enqueue-only; worker disabled, covered by the canonical sync.starred enqueue case',
    },
    {
        'method': 'POST', 'route': '/api/sync/notifications',
        'classification': 'scope',
        'reason': 'enqueue-only; worker disabled, covered by the canonical sync.starred enqueue case',
    },
    {
        'method': 'POST', 'route': '/api/briefs/generate',
        'classification': 'scope',
        'reason': 'enqueue-only brief.generate task; not in the approved mutation set',
    },
    {
        'method': 'POST', 'route': '/api/admin/jobs/llm/calls/{call_id}/diagnostic-access',
        'classification': 'scope',
        'reason': 'audit-only insert tied to a real LLM call; not in the approved mutation set',
    },
    {
        'method': 'POST', 'route': '/api/admin/jobs/realtime/{task_id}/cancel',
        'classification': 'scope',
        'reason': 'would cancel a production task outside the probe-owned task',
    },
    {
        'method': 'POST', 'route': '/api/admin/jobs/realtime/{task_id}/retry',
        'classification': 'scope',
        'reason': 'would requeue a production task outside the probe-owned task',
    },
    {
        'method': 'PATCH', 'route': '/api/admin/jobs/translations/runtime-config',
        'classification': 'scope',
        'reason': 'translation scheduler settings write; not in the approved mutation set',
    },
    {
        'method': 'PATCH', 'route': '/admin/jobs/translations/runtime-config',
        'classification': 'scope',
        'reason': 'translation scheduler settings write; not in the approved mutation set',
    },
    {
        'method': 'DELETE', 'route': '/api/me/passkeys/{passkey_id}',
        'classification': 'scope',
        'reason': 'deletes a WebAuthn credential blob; outside the approved set',
    },
    {
        'method': 'DELETE', 'route': '/api/me/github-connections/{connection_id}',
        'classification': 'scope',
        'reason': 'deletes an OAuth token row; restoring requires copying secret material',
    },
    {
        'method': 'DELETE', 'route': '/api/me/linuxdo',
        'classification': 'scope',
        'reason': 'deletes an OAuth connection row; restoring requires copying secret material',
    },
    {
        'method': 'DELETE', 'route': '/api/admin/public-release-repos/{usage_id}',
        'classification': 'scope',
        'reason': 'deletes usage and may purge cached releases; not reversible without a full snapshot',
    },
    {
        'method': 'POST', 'route': '/api/auth/passkeys/register/options',
        'classification': 'scope', 'reason': 'WebAuthn ceremony; not a data-path mutation',
    },
    {
        'method': 'POST', 'route': '/api/auth/passkeys/register/verify',
        'classification': 'scope', 'reason': 'WebAuthn ceremony; not a data-path mutation',
    },
    {
        'method': 'POST', 'route': '/api/auth/passkeys/authenticate/options',
        'classification': 'scope', 'reason': 'WebAuthn ceremony; not a data-path mutation',
    },
    {
        'method': 'POST', 'route': '/api/auth/passkeys/authenticate/verify',
        'classification': 'scope', 'reason': 'WebAuthn ceremony; not a data-path mutation',
    },
]


def sql_literal(value):
    if value is None:
        return 'NULL'
    if isinstance(value, bool):
        return 'TRUE' if value else 'FALSE'
    if isinstance(value, int):
        return str(value)
    return "'" + str(value).replace("'", "''") + "'"


def now_iso():
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()


class MutationFailure(Exception):
    pass


class RestoreFailure(Exception):
    pass


class SkipCase(Exception):
    def __init__(self, result, reason):
        super().__init__(reason)
        self.result = result
        self.reason = reason


def expect(condition, message):
    if not condition:
        raise MutationFailure(message)


def expect_status(status, expected, route):
    expect(status == expected, f'{route} returned {status}, expected {expected}')


class Http:
    def __init__(self, base_url, cookie):
        self.base_url = base_url.rstrip('/')
        self.cookie = cookie

    def request(self, method, path, body=None):
        data = None
        headers = {}
        if self.cookie:
            headers['Cookie'] = self.cookie
        if body is not None:
            data = json.dumps(body).encode()
            headers['Content-Type'] = 'application/json'
        request = urllib.request.Request(
            self.base_url + path, data=data, headers=headers, method=method
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read(32 * 1024 * 1024)
                status = response.status
                content_type = response.headers.get('Content-Type', '')
        except urllib.error.HTTPError as exc:
            raw = exc.read(65536)
            status = exc.code
            content_type = exc.headers.get('Content-Type', '')
        parsed = None
        if raw:
            try:
                parsed = json.loads(raw)
            except Exception:
                parsed = None
        return status, parsed, content_type


class Db:
    def __init__(self, url):
        self.url = url

    def sql(self, query):
        return psql(self.url, query)

    def row(self, query):
        output = self.sql(query)
        return json.loads(output) if output else None

    def rows(self, query):
        output = self.sql(query)
        return json.loads(output) if output else []

    def scalar(self, query):
        output = self.sql(query)
        return None if output == '' else output


class Probe:
    def __init__(self, http, db):
        self.http = http
        self.db = db
        self.cases = []
        self.mutations = []
        self.attention = []
        self.probe_task_ids = []
        self.started_at = now_iso()
        self.user_id = None
        self.running_before = int(self.db.scalar(
            "SELECT COUNT(*) FROM public.job_tasks WHERE status = 'running'"
        ) or 0)

    # -- mutation bookkeeping -------------------------------------------------
    def announce(self, name, rollback_sql):
        entry = {
            'mutation': name,
            'rollback_sql': rollback_sql,
            'restored': False,
            'needs_attention': False,
        }
        self.mutations.append(entry)
        print(json.dumps({
            'event': 'mutation.rollback_sql',
            'mutation': name,
            'sql': list(rollback_sql),
        }, ensure_ascii=False))
        return entry

    def announce_appended(self, entry, statements):
        """Record rollback SQL that depends on a server-generated identifier."""
        entry['rollback_sql'].extend(statements)
        print(json.dumps({
            'event': 'mutation.rollback_sql.appended',
            'mutation': entry['mutation'],
            'sql': statements,
        }, ensure_ascii=False))

    def execute_rollback(self, entry):
        for statement in entry['rollback_sql']:
            self.db.sql(statement)

    def mutate(self, name, rollback_sql, apply, verify_restore):
        """Run one reversible mutation and always attempt the restore."""
        entry = self.announce(name, rollback_sql)
        try:
            result = apply()
            extra = result.pop('extra_rollback_sql', None) if isinstance(result, dict) else None
            if extra:
                self.announce_appended(entry, extra)
        except Exception:
            try:
                self.execute_rollback(entry)
                if not verify_restore():
                    raise RuntimeError('restore verification mismatch after failed mutation')
                entry['restored'] = True
            except Exception as restore_error:
                entry['needs_attention'] = True
                entry['restore_error'] = f'{type(restore_error).__name__}: {restore_error}'
                self.attention.append(entry)
            raise
        try:
            self.execute_rollback(entry)
            if not verify_restore():
                raise RuntimeError('restored values do not match the snapshot')
            entry['restored'] = True
        except Exception as restore_error:
            entry['needs_attention'] = True
            entry['restore_error'] = f'{type(restore_error).__name__}: {restore_error}'
            self.attention.append(entry)
            raise RestoreFailure(f'{name}: {restore_error}')
        return result

    def run_case(self, name, method, route, func):
        start = len(self.mutations)
        case = {'case': name, 'method': method, 'route': route}
        try:
            outcome = func() or {}
            case['result'] = 'pass'
            case.update(outcome)
        except SkipCase as exc:
            case.update(result=exc.result, reason=exc.reason)
        except RestoreFailure as exc:
            case.update(result='fail', reason=str(exc))
        except Exception as exc:
            case.update(result='fail', reason=f'{type(exc).__name__}: {exc}')
        entries = self.mutations[start:]
        if entries:
            case['mutations'] = [entry['mutation'] for entry in entries]
            case['restored'] = all(entry['restored'] for entry in entries)
        self.cases.append(case)

    # -- task helpers ---------------------------------------------------------
    def task_rollback(self, task_id):
        return [
            f"DELETE FROM public.job_task_events WHERE task_id = {sql_literal(task_id)};",
            f"DELETE FROM public.job_tasks WHERE id = {sql_literal(task_id)};",
        ]

    def task_cleanup_predicate(self, task_type, source):
        clause = (
            f"requested_by = {sql_literal(self.user_id)} "
            f"AND task_type = {sql_literal(task_type)} "
            f"AND source = {sql_literal(source)} "
            "AND status = 'queued' "
            f"AND created_at::timestamptz >= {sql_literal(self.started_at)}::timestamptz"
        )
        return [
            "DELETE FROM public.job_task_events WHERE task_id IN "
            f"(SELECT id FROM public.job_tasks WHERE {clause});",
            f"DELETE FROM public.job_tasks WHERE {clause};",
        ]

    def task_row(self, task_id):
        return self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT id, task_type, status, requested_by, "
            "source, cancel_requested FROM public.job_tasks WHERE id = "
            f"{sql_literal(task_id)}) t"
        )

    def task_gone(self, task_id):
        return self.task_row(task_id) is None

    # -- cases ---------------------------------------------------------------
    def case_profile(self):
        route = '/api/me/profile'
        before = self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT daily_brief_time_zone, include_own_releases, "
            f"updated_at FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
        )
        expect(before is not None, 'probe user row missing')
        status, body, _ = self.http.request('GET', route)
        expect_status(status, 200, route)
        alternative = self.alternative_zone(body.get('daily_brief_time_zone'))
        rollback = [
            "UPDATE public.users SET "
            f"daily_brief_time_zone = {sql_literal(before['daily_brief_time_zone'])}, "
            f"include_own_releases = {sql_literal(before['include_own_releases'])}, "
            f"updated_at = {sql_literal(before['updated_at'])} "
            f"WHERE id = {sql_literal(self.user_id)};"
        ]

        def apply():
            status, body, _ = self.http.request(
                'PATCH', route, {'daily_brief_time_zone': alternative}
            )
            expect_status(status, 200, route)
            expect(
                body and body.get('daily_brief_time_zone') == alternative,
                f'profile patch response did not echo {alternative}',
            )
            read = self.http.request('GET', route)
            expect_status(read[0], 200, route)
            expect(
                read[1] and read[1].get('daily_brief_time_zone') == alternative,
                'profile read-back did not show the new time zone',
            )
            persisted = self.db.scalar(
                "SELECT daily_brief_time_zone FROM public.users WHERE id = "
                f"{sql_literal(self.user_id)}"
            )
            expect(persisted == alternative, 'database does not hold the new time zone')
            return {'value': alternative}

        def verify_restore():
            row = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT daily_brief_time_zone, include_own_releases, "
                f"updated_at FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
            )
            read = self.http.request('GET', route)
            effective = 'UTC' if before['daily_brief_time_zone'] is None else before['daily_brief_time_zone']
            return (
                row == before
                and read[0] == 200
                and (read[1] or {}).get('daily_brief_time_zone') == effective
            )

        return self.mutate('api.me.profile.patch', rollback, apply, verify_restore)

    def case_api_keys(self):
        route = '/api/me/api-keys'
        name = (
            'pg-write-acceptance-'
            + dt.datetime.now(dt.timezone.utc).strftime('%H%M%S')
            + '-'
            + str(os.getpid())
        )
        holder = {}
        # The row id is server-generated, so the crash-safe rollback is scoped by
        # the unique probe key name; the exact id is appended after the response.
        rollback = [
            "DELETE FROM public.user_api_keys WHERE user_id = "
            f"{sql_literal(self.user_id)} AND name = {sql_literal(name)};"
        ]

        def apply():
            status, body, _ = self.http.request('POST', route, {'name': name})
            expect_status(status, 201, route)
            expect(bool(body and body.get('item', {}).get('id')), 'api key response missing id')
            echo = body.get('item', {})
            holder['key_id'] = echo['id']
            persisted = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT id, name, revoked_at FROM public.user_api_keys "
                f"WHERE id = {sql_literal(echo['id'])}) t"
            )
            expect(persisted is not None, 'api key row was not inserted')
            expect(persisted['name'] == name, 'api key row name mismatch')
            expect(persisted['revoked_at'] is None, 'new api key is already revoked')
            status, listing, _ = self.http.request('GET', route)
            expect_status(status, 200, route)
            ids = {item.get('id') for item in (listing or {}).get('items', [])}
            expect(echo['id'] in ids, 'api key listing does not contain the new key')
            status, _, _ = self.http.request('DELETE', f'{route}/{echo["id"]}')
            expect_status(status, 204, f'{route}/{{api_key_id}}')
            persisted = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT id, name, revoked_at FROM public.user_api_keys "
                f"WHERE id = {sql_literal(echo['id'])}) t"
            )
            expect(persisted and persisted['revoked_at'] is not None, 'api key was not revoked')
            status, listing, _ = self.http.request('GET', route)
            expect_status(status, 200, route)
            ids = {item.get('id') for item in (listing or {}).get('items', [])}
            expect(echo['id'] not in ids, 'revoked api key is still listed')
            return {
                'key_id': echo['id'],
                'masked_key': echo.get('masked_key'),
                'extra_rollback_sql': [
                    f"DELETE FROM public.user_api_keys WHERE id = {sql_literal(echo['id'])};"
                ],
            }

        def verify_restore():
            if holder.get('key_id') is None:
                return True
            return self.db.scalar(
                "SELECT COUNT(*) FROM public.user_api_keys WHERE id = "
                f"{sql_literal(holder['key_id'])}"
            ) == '0'

        return self.mutate('api.me.api_keys.create_and_delete', rollback, apply, verify_restore)

    def case_following(self):
        status, body, _ = self.http.request('GET', '/api/repos/following')
        expect_status(status, 200, '/api/repos/following')
        candidates = (body or {}).get('associated_items') or (body or {}).get('items') or []
        if not candidates:
            raise SkipCase(
                'skipped_fixture',
                'no user_repo_associations row is available to follow',
            )
        target = candidates[0]
        full_name = target.get('full_name')
        expect(bool(full_name and '/' in full_name), 'repo candidate has no owner/name')
        owner, repo = full_name.split('/', 1)
        before = self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT is_following, follow_state_source, updated_at "
            "FROM public.user_repo_associations WHERE user_id = "
            f"{sql_literal(self.user_id)} AND repo_full_name_lower = "
            f"{sql_literal(full_name.lower())}) t"
        )
        expect(before is not None, 'repo association disappeared before the probe')
        rollback = [
            "UPDATE public.user_repo_associations SET "
            f"is_following = {sql_literal(before['is_following'])}, "
            f"follow_state_source = {sql_literal(before['follow_state_source'])}, "
            f"updated_at = {sql_literal(before['updated_at'])} "
            f"WHERE user_id = {sql_literal(self.user_id)} AND "
            f"repo_full_name_lower = {sql_literal(full_name.lower())};"
        ]
        path = (
            '/api/repos/'
            + urllib.parse.quote(owner, safe='')
            + '/'
            + urllib.parse.quote(repo, safe='')
            + '/following'
        )

        def follows():
            status, listing, _ = self.http.request('GET', '/api/repos/following')
            expect_status(status, 200, '/api/repos/following')
            return full_name.lower() in {
                item.get('full_name', '').lower()
                for item in (listing or {}).get('items', [])
            }

        def apply():
            status, body, _ = self.http.request('PUT', path)
            expect_status(status, 200, path)
            expect(bool(body and body.get('is_following') is True), 'PUT did not report is_following')
            expect(body.get('follow_state_source') == 'user_explicit', 'PUT did not record explicit follow')
            expect(follows(), 'followed repo is absent from GET /api/repos/following')
            status, body, _ = self.http.request('DELETE', path)
            expect_status(status, 200, path)
            expect(bool(body and body.get('is_following') is False), 'DELETE did not clear is_following')
            expect(not follows(), 'unfollowed repo still appears in GET /api/repos/following')
            return {'repo': full_name}

        def verify_restore():
            row = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT is_following, follow_state_source, updated_at "
                "FROM public.user_repo_associations WHERE user_id = "
                f"{sql_literal(self.user_id)} AND repo_full_name_lower = "
                f"{sql_literal(full_name.lower())}) t"
            )
            return row == before and follows() == (before['is_following'] != 0)

        return self.mutate('api.repos.following.follow_then_unfollow', rollback, apply, verify_restore)

    def case_scheduled_slot(self):
        before_rows = self.db.rows(
            "SELECT COALESCE(json_agg(to_jsonb(s)), '[]'::json) FROM (SELECT hour_utc, enabled, "
            "last_dispatch_at, updated_at FROM public.daily_brief_hour_slots "
            "ORDER BY hour_utc) s"
        )
        if not before_rows:
            raise SkipCase('skipped_fixture', 'daily_brief_hour_slots is empty')
        slot = before_rows[0]
        new_enabled = 0 if slot['enabled'] else 1
        rollback = [
            "UPDATE public.daily_brief_hour_slots SET "
            f"enabled = {sql_literal(slot['enabled'])}, "
            f"last_dispatch_at = {sql_literal(slot['last_dispatch_at'])}, "
            f"updated_at = {sql_literal(slot['updated_at'])} "
            f"WHERE hour_utc = {sql_literal(slot['hour_utc'])};"
        ]
        route = f"/api/admin/jobs/scheduled/{slot['hour_utc']}"

        def apply():
            status, body, _ = self.http.request('PATCH', route, {'enabled': bool(new_enabled)})
            expect_status(status, 200, route)
            expect(body and body.get('enabled') is bool(new_enabled), 'slot response did not reflect the flip')
            listing = self.http.request('GET', '/api/admin/jobs/scheduled')
            expect_status(listing[0], 200, '/api/admin/jobs/scheduled')
            match = [
                item for item in (listing[1] or {}).get('items', [])
                if item.get('hour_utc') == slot['hour_utc']
            ]
            expect(match and match[0].get('enabled') is bool(new_enabled), 'slot listing did not reflect the flip')
            persisted = self.db.scalar(
                "SELECT enabled FROM public.daily_brief_hour_slots WHERE hour_utc = "
                f"{sql_literal(slot['hour_utc'])}"
            )
            expect(persisted == str(new_enabled), 'database did not persist the slot flip')
            return {'hour_utc': slot['hour_utc'], 'enabled': bool(new_enabled)}

        def verify_restore():
            row = self.db.row(
                "SELECT to_jsonb(s) FROM (SELECT hour_utc, enabled, last_dispatch_at, updated_at "
                "FROM public.daily_brief_hour_slots WHERE hour_utc = "
                f"{sql_literal(slot['hour_utc'])}) s"
            )
            return row == slot

        return self.mutate('api.admin.jobs.scheduled.patch', rollback, apply, verify_restore)

    def case_llm_runtime_config(self):
        before = self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT llm_max_concurrency, ai_model_context_limit, "
            "ai_model_context_limit_migrated_at, llm_models_json, updated_at "
            "FROM public.admin_runtime_settings WHERE id = 1) t"
        )
        recovery = self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT llm_recovery_enabled, llm_recovery_rollout_percent, "
            "updated_at FROM public.llm_recovery_flags WHERE id = 1) t"
        )
        rollback = []
        if before is None:
            rollback.append("DELETE FROM public.admin_runtime_settings WHERE id = 1;")
            new_max = 1
        else:
            rollback.append(
                "UPDATE public.admin_runtime_settings SET "
                f"llm_max_concurrency = {sql_literal(before['llm_max_concurrency'])}, "
                f"ai_model_context_limit = {sql_literal(before['ai_model_context_limit'])}, "
                f"ai_model_context_limit_migrated_at = {sql_literal(before['ai_model_context_limit_migrated_at'])}, "
                f"llm_models_json = {sql_literal(before['llm_models_json'])}, "
                f"updated_at = {sql_literal(before['updated_at'])} WHERE id = 1;"
            )
            new_max = 2 if before['llm_max_concurrency'] == 1 else 1
        if recovery is None:
            rollback.append("DELETE FROM public.llm_recovery_flags WHERE id = 1;")
        else:
            rollback.append(
                "UPDATE public.llm_recovery_flags SET "
                f"llm_recovery_enabled = {sql_literal(recovery['llm_recovery_enabled'])}, "
                f"llm_recovery_rollout_percent = {sql_literal(recovery['llm_recovery_rollout_percent'])}, "
                f"updated_at = {sql_literal(recovery['updated_at'])} WHERE id = 1;"
            )

        def apply():
            status, body, _ = self.http.request(
                'PATCH', '/api/admin/jobs/llm/runtime-config', {'max_concurrency': new_max}
            )
            expect_status(status, 200, '/api/admin/jobs/llm/runtime-config')
            expect(body and body.get('max_concurrency') == new_max, 'runtime config response did not echo max_concurrency')
            read = self.http.request('GET', '/api/admin/jobs/llm/status')
            expect_status(read[0], 200, '/api/admin/jobs/llm/status')
            expect((read[1] or {}).get('max_concurrency') == new_max, 'llm status did not show the new concurrency')
            persisted = self.db.scalar(
                "SELECT llm_max_concurrency FROM public.admin_runtime_settings WHERE id = 1"
            )
            expect(persisted == str(new_max), 'database did not persist max_concurrency')
            return {'max_concurrency': new_max}

        def verify_restore():
            row = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT llm_max_concurrency, ai_model_context_limit, "
                "ai_model_context_limit_migrated_at, llm_models_json, updated_at "
                "FROM public.admin_runtime_settings WHERE id = 1) t"
            )
            recovery_row = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT llm_recovery_enabled, llm_recovery_rollout_percent, "
                "updated_at FROM public.llm_recovery_flags WHERE id = 1) t"
            )
            return row == before and recovery_row == recovery

        return self.mutate('api.admin.jobs.llm.runtime_config', rollback, apply, verify_restore)

    def case_admin_user_patch(self):
        route = f'/api/admin/users/{self.user_id}'
        detail = self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT is_admin, is_disabled, updated_at "
            f"FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
        )
        expect(detail is not None, 'probe user row missing')
        expect(detail['is_admin'] != 0, 'probe session user is not an admin')
        rollback = [
            "UPDATE public.users SET "
            f"is_admin = {sql_literal(detail['is_admin'])}, "
            f"is_disabled = {sql_literal(detail['is_disabled'])}, "
            f"updated_at = {sql_literal(detail['updated_at'])} "
            f"WHERE id = {sql_literal(self.user_id)};"
        ]

        def apply():
            # The handler only exposes is_admin/is_disabled. Sending the current
            # values keeps the probe harm-free: the row is written, then restored.
            status, body, _ = self.http.request(
                'PATCH', route, {'is_admin': True, 'is_disabled': False}
            )
            expect_status(status, 200, route)
            expect(body and body.get('is_admin') is True, 'admin patch response mismatch')
            expect(body.get('is_disabled') is False, 'admin patch disabled the probe user')
            read = self.http.request('GET', route)
            expect_status(read[0], 200, route)
            expect((read[1] or {}).get('is_admin') is True, 'admin detail read-back mismatch')
            expect((read[1] or {}).get('is_disabled') is False, 'admin detail reports disabled')
            return {'contract': 'idempotent is_admin/is_disabled patch; only updated_at changes'}

        def verify_restore():
            row = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT is_admin, is_disabled, updated_at "
                f"FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
            )
            return row == detail

        return self.mutate('api.admin.users.patch', rollback, apply, verify_restore)

    def case_admin_user_profile(self):
        route = f'/api/admin/users/{self.user_id}/profile'
        before = self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT daily_brief_time_zone, updated_at "
            f"FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
        )
        expect(before is not None, 'probe user row missing')
        rollback = [
            "UPDATE public.users SET "
            f"daily_brief_time_zone = {sql_literal(before['daily_brief_time_zone'])}, "
            f"updated_at = {sql_literal(before['updated_at'])} "
            f"WHERE id = {sql_literal(self.user_id)};"
        ]

        def apply():
            current = (before['daily_brief_time_zone'] or '').lower()
            candidates = [
                zone for zone in ('UTC', 'Asia/Shanghai', 'Europe/Berlin')
                if zone.lower() != current
            ]
            last_error = None
            for zone in candidates:
                status, body, _ = self.http.request(
                    'PATCH', route, {'daily_brief_time_zone': zone}
                )
                if status == 400:
                    last_error = body
                    continue
                expect_status(status, 200, route)
                expect(
                    body and body.get('daily_brief_time_zone') == zone,
                    'admin profile response did not echo the new time zone',
                )
                read = self.http.request('GET', route)
                expect_status(read[0], 200, route)
                expect(
                    (read[1] or {}).get('daily_brief_time_zone') == zone,
                    'admin profile read-back did not show the new time zone',
                )
                persisted = self.db.scalar(
                    "SELECT daily_brief_time_zone FROM public.users WHERE id = "
                    f"{sql_literal(self.user_id)}"
                )
                expect(persisted == zone, 'database does not hold the new time zone')
                return {'time_zone': zone}
            raise SkipCase(
                'skipped_fixture',
                'scheduler slots do not permit an hour-aligned probe time zone: '
                + json.dumps(last_error, ensure_ascii=False),
            )

        def verify_restore():
            row = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT daily_brief_time_zone, updated_at "
                f"FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
            )
            read = self.http.request('GET', route)
            return (
                row == before
                and read[0] == 200
                and (read[1] or {}).get('daily_brief_time_zone') == before['daily_brief_time_zone']
            )

        return self.mutate('api.admin.users.profile.patch', rollback, apply, verify_restore)

    def user_row(self):
        return self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT daily_brief_time_zone, include_own_releases, "
            "webhook_push_desired_state, webhook_push_enabled, updated_at FROM public.users "
            f"WHERE id = {sql_literal(self.user_id)}) t"
        )

    def user_restore_sql(self, snapshot):
        return (
            "UPDATE public.users SET "
            f"daily_brief_time_zone = {sql_literal(snapshot['daily_brief_time_zone'])}, "
            f"include_own_releases = {sql_literal(snapshot['include_own_releases'])}, "
            f"webhook_push_desired_state = {sql_literal(snapshot['webhook_push_desired_state'])}, "
            f"webhook_push_enabled = {sql_literal(snapshot['webhook_push_enabled'])}, "
            f"updated_at = {sql_literal(snapshot['updated_at'])} "
            f"WHERE id = {sql_literal(self.user_id)};"
        )

    def case_webhook_reconcile(self):
        route = '/api/me/webhook-push/reconcile'
        snapshot = self.user_row()
        expect(snapshot is not None, 'probe user row missing')
        rollback = [self.user_restore_sql(snapshot)]
        rollback.extend(self.task_cleanup_predicate('webhook.push.manage', 'webhook_push'))
        holder = {}

        def apply():
            status, body, _ = self.http.request('POST', route, {})
            if status == 409 and (body or {}).get('code') == 'my_releases_disabled':
                # Precondition: desired_state=enabled with include_own_releases=0.
                status, flipped, _ = self.http.request(
                    'PATCH', '/api/me/profile',
                    {'daily_brief_time_zone': 'UTC', 'include_own_releases': True},
                )
                expect_status(status, 200, '/api/me/profile')
                expect(flipped and flipped.get('include_own_releases') is True, 'include_own_releases flip failed')
                status, body, _ = self.http.request('POST', route, {})
            expect_status(status, 202, route)
            expect(body and body.get('mode') == 'task_id', 'reconcile response missing task mode')
            expect(body.get('task_type') == 'webhook.push.manage', 'reconcile response task_type mismatch')
            expect(body.get('status') == 'queued', 'reconcile response status is not queued')
            holder['task_id'] = body.get('task_id')
            expect(bool(holder['task_id']), 'reconcile response missing task_id')
            task = self.task_row(holder['task_id'])
            expect(task is not None, 'reconcile task row was not inserted')
            expect(task['status'] == 'queued', f'reconcile task status is {task["status"]}')
            expect(task['task_type'] == 'webhook.push.manage', 'reconcile task row type mismatch')
            self.probe_task_ids.append(holder['task_id'])
            return {
                'task_id': holder['task_id'],
                'extra_rollback_sql': self.task_rollback(holder['task_id']),
            }

        def verify_restore():
            if holder.get('task_id') and not self.task_gone(holder['task_id']):
                return False
            return self.user_row() == snapshot

        return self.mutate('api.me.webhook_push.reconcile', rollback, apply, verify_restore)

    def case_webhook_settings(self):
        route = '/api/me/webhook-push'
        snapshot = self.user_row()
        expect(snapshot is not None, 'probe user row missing')
        rollback = [self.user_restore_sql(snapshot)]
        rollback.extend(self.task_cleanup_predicate('webhook.push.reconcile', 'api.webhook_push'))
        target_state = 'paused' if snapshot['webhook_push_desired_state'] == 'enabled' else 'enabled'
        holder = {}

        def apply():
            if target_state == 'enabled' and snapshot['include_own_releases'] == 0:
                status, body, _ = self.http.request(
                    'PATCH', '/api/me/profile',
                    {'daily_brief_time_zone': 'UTC', 'include_own_releases': True},
                )
                expect_status(status, 200, '/api/me/profile')
                expect(body and body.get('include_own_releases') is True, 'include_own_releases precondition failed')
            status, body, _ = self.http.request(
                'PATCH', route, {'desired_state': target_state}
            )
            expect_status(status, 202, route)
            expect(body and body.get('desired_state') == target_state, 'webhook patch response mismatch')
            expect(body.get('status') == 'queued', 'webhook patch response status is not queued')
            holder['task_id'] = body.get('task_id')
            expect(bool(holder['task_id']), 'webhook patch response missing task_id')
            self.probe_task_ids.append(holder['task_id'])
            task = self.task_row(holder['task_id'])
            expect(task is not None, 'webhook reconcile task row was not inserted')
            expect(task['status'] == 'queued', f'webhook task status is {task["status"]}')
            expect(task['task_type'] == 'webhook.push.reconcile', 'webhook task type mismatch')
            read = self.http.request('GET', route)
            expect_status(read[0], 200, route)
            expect((read[1] or {}).get('desired_state') == target_state, 'webhook settings read-back mismatch')
            expect(
                (read[1] or {}).get('enabled') is (target_state == 'enabled'),
                'webhook enabled flag does not follow desired_state',
            )
            persisted = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT webhook_push_desired_state, webhook_push_enabled "
                f"FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
            )
            expect(persisted and persisted['webhook_push_desired_state'] == target_state, 'database desired_state mismatch')
            return {
                'desired_state': target_state,
                'task_id': holder['task_id'],
                'extra_rollback_sql': self.task_rollback(holder['task_id']),
            }

        def verify_restore():
            if holder.get('task_id') and not self.task_gone(holder['task_id']):
                return False
            read = self.http.request('GET', route)
            return (
                self.user_row() == snapshot
                and read[0] == 200
                and (read[1] or {}).get('desired_state') == snapshot['webhook_push_desired_state']
            )

        return self.mutate('api.me.webhook_push.settings', rollback, apply, verify_restore)

    def case_online_migration(self):
        run = self.db.row(
            "SELECT to_jsonb(r) FROM (SELECT migration_id, status, pause_requested, owner_id, "
            "lease_heartbeat_at, last_error, started_at, completed_at, updated_at "
            "FROM public.online_migration_runs WHERE migration_id = "
            f"{sql_literal(ONLINE_MIGRATION_ID)}) r"
        )
        failed_operations = self.db.rows(
            "SELECT COALESCE(json_agg(to_jsonb(o) ORDER BY o.operation_id), '[]'::json) FROM ("
            "SELECT operation_id, status, last_error, owner_id, lease_heartbeat_at, updated_at "
            "FROM public.online_migration_operations WHERE migration_id = "
            f"{sql_literal(ONLINE_MIGRATION_ID)} AND status = 'failed') o"
        )
        rollback = []
        if run is None:
            pause_route = f'/api/admin/jobs/migrations/{ONLINE_MIGRATION_ID}/pause'
            resume_route = f'/api/admin/jobs/migrations/{ONLINE_MIGRATION_ID}/resume'

            def apply():
                for route in (pause_route, resume_route):
                    status, body, _ = self.http.request('POST', route)
                    expect_status(status, 200, route)
                    expect(isinstance(body, dict) and 'run' in body, 'migrations response missing run')
                return {'migration_row': 'absent', 'contract': 'no persisted effect'}

            return self.mutate('api.admin.jobs.migrations.pause_resume', rollback, apply, lambda: True)
        rollback.append(
            "UPDATE public.online_migration_runs SET "
            f"status = {sql_literal(run['status'])}, "
            f"pause_requested = {sql_literal(run['pause_requested'])}, "
            f"owner_id = {sql_literal(run['owner_id'])}, "
            f"lease_heartbeat_at = {sql_literal(run['lease_heartbeat_at'])}, "
            f"last_error = {sql_literal(run['last_error'])}, "
            f"started_at = {sql_literal(run['started_at'])}, "
            f"completed_at = {sql_literal(run['completed_at'])}, "
            f"updated_at = {sql_literal(run['updated_at'])} "
            f"WHERE migration_id = {sql_literal(ONLINE_MIGRATION_ID)};"
        )
        for operation in failed_operations:
            rollback.append(
                "UPDATE public.online_migration_operations SET "
                f"status = {sql_literal(operation['status'])}, "
                f"last_error = {sql_literal(operation['last_error'])}, "
                f"owner_id = {sql_literal(operation['owner_id'])}, "
                f"lease_heartbeat_at = {sql_literal(operation['lease_heartbeat_at'])}, "
                f"updated_at = {sql_literal(operation['updated_at'])} "
                f"WHERE migration_id = {sql_literal(ONLINE_MIGRATION_ID)} AND "
                f"operation_id = {sql_literal(operation['operation_id'])};"
            )
        pause_route = f'/api/admin/jobs/migrations/{ONLINE_MIGRATION_ID}/pause'
        resume_route = f'/api/admin/jobs/migrations/{ONLINE_MIGRATION_ID}/resume'

        def pause_requested():
            value = self.db.scalar(
                "SELECT pause_requested FROM public.online_migration_runs WHERE migration_id = "
                f"{sql_literal(ONLINE_MIGRATION_ID)}"
            )
            return value

        def apply():
            status, body, _ = self.http.request('POST', pause_route)
            expect_status(status, 200, pause_route)
            run_body = (body or {}).get('run')
            expect(isinstance(run_body, dict), 'pause response missing run')
            expect(run_body.get('pause_requested') is True, 'pause response did not report pause_requested')
            expect(pause_requested() == '1', 'pause_requested was not persisted')
            status, body, _ = self.http.request('POST', resume_route)
            expect_status(status, 200, resume_route)
            run_body = (body or {}).get('run')
            expect(isinstance(run_body, dict), 'resume response missing run')
            expect(run_body.get('pause_requested') is False, 'resume response still reports pause_requested')
            expect(pause_requested() == '0', 'pause_requested was not cleared')
            return {
                'migration_id': ONLINE_MIGRATION_ID,
                'original_status': run['status'],
                'failed_operations_snapshotted': len(failed_operations),
            }

        def verify_restore():
            row = self.db.row(
                "SELECT to_jsonb(r) FROM (SELECT migration_id, status, pause_requested, owner_id, "
                "lease_heartbeat_at, last_error, started_at, completed_at, updated_at "
                "FROM public.online_migration_runs WHERE migration_id = "
                f"{sql_literal(ONLINE_MIGRATION_ID)}) r"
            )
            operations = self.db.rows(
                "SELECT COALESCE(json_agg(to_jsonb(o) ORDER BY o.operation_id), '[]'::json) FROM ("
                "SELECT operation_id, status, last_error, owner_id, lease_heartbeat_at, updated_at "
                "FROM public.online_migration_operations WHERE migration_id = "
                f"{sql_literal(ONLINE_MIGRATION_ID)} AND status = 'failed') o"
            )
            return row == run and operations == failed_operations

        return self.mutate('api.admin.jobs.migrations.pause_resume', rollback, apply, verify_restore)

    def case_search_index_resume(self):
        before = self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT phase, cursor, status, last_error, updated_at "
            "FROM public.search_projection_backfill_state WHERE id = 1) t"
        )
        if before is None:
            raise SkipCase('skipped_fixture', 'search_projection_backfill_state row is missing')
        if before['status'] != 'failed':
            status, body, _ = self.http.request('POST', '/api/admin/jobs/search-index/resume')
            expect_status(status, 409, '/api/admin/jobs/search-index/resume')
            expect(
                (body or {}).get('code') == 'search_projection_backfill_state_conflict',
                'resume conflict code mismatch',
            )
            return {
                'contract': 'not in failed state; handler returned the documented conflict',
                'status_before': before['status'],
            }
        rollback = [
            "UPDATE public.search_projection_backfill_state SET "
            f"phase = {sql_literal(before['phase'])}, "
            f"cursor = {sql_literal(before['cursor'])}, "
            f"status = {sql_literal(before['status'])}, "
            f"last_error = {sql_literal(before['last_error'])}, "
            f"updated_at = {sql_literal(before['updated_at'])} WHERE id = 1;"
        ]

        def apply():
            status, body, _ = self.http.request('POST', '/api/admin/jobs/search-index/resume')
            expect_status(status, 200, '/api/admin/jobs/search-index/resume')
            expect(isinstance(body, dict) and 'status' in body, 'search status response shape mismatch')
            persisted = self.db.scalar(
                "SELECT status FROM public.search_projection_backfill_state WHERE id = 1"
            )
            expect(persisted == 'building', f'backfill status is {persisted}, expected building')
            return {'status_after_resume': persisted}

        def verify_restore():
            row = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT phase, cursor, status, last_error, updated_at "
                "FROM public.search_projection_backfill_state WHERE id = 1) t"
            )
            return row == before

        return self.mutate('api.admin.jobs.search_index.resume', rollback, apply, verify_restore)

    def case_me_resume(self):
        route = '/api/me/resume'
        before = self.db.row(
            "SELECT to_jsonb(t) FROM (SELECT paused_at, last_active_at, updated_at "
            f"FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
        )
        expect(before is not None, 'probe user row missing')
        holder = {}
        rollback = [
            "UPDATE public.users SET "
            f"paused_at = {sql_literal(before['paused_at'])}, "
            f"last_active_at = {sql_literal(before['last_active_at'])}, "
            f"updated_at = {sql_literal(before['updated_at'])} "
            f"WHERE id = {sql_literal(self.user_id)};"
        ]
        rollback.extend(self.task_cleanup_predicate('sync.access_refresh', 'api.me_resume'))

        def apply():
            status, body, _ = self.http.request('POST', route)
            expect_status(status, 200, route)
            expect(body and body.get('status') == 'enabled', 'resume response status mismatch')
            access_sync = (body or {}).get('access_sync') or {}
            expect(access_sync.get('task_type') == 'sync.access_refresh', 'resume response task_type mismatch')
            holder['task_id'] = access_sync.get('task_id')
            expect(bool(holder['task_id']), 'resume response missing task_id')
            task = self.task_row(holder['task_id'])
            expect(task is not None, 'resume task row was not inserted')
            expect(task['status'] == 'queued', f'resume task status is {task["status"]}')
            expect(task['task_type'] == 'sync.access_refresh', 'resume task type mismatch')
            persisted = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT paused_at, last_active_at, updated_at "
                f"FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
            )
            expect(persisted and persisted['paused_at'] is None, 'resume did not clear paused_at')
            expect(persisted['last_active_at'] is not None, 'resume did not bump last_active_at')
            read = self.http.request('GET', '/api/tasks/' + urllib.parse.quote(holder['task_id'], safe=''))
            expect_status(read[0], 200, '/api/tasks/{task_id}')
            expect((read[1] or {}).get('status') == 'queued', 'task read-back is not queued')
            self.probe_task_ids.append(holder['task_id'])
            return {
                'task_id': holder['task_id'],
                'extra_rollback_sql': self.task_rollback(holder['task_id']),
            }

        def verify_restore():
            if holder.get('task_id') and not self.task_gone(holder['task_id']):
                return False
            row = self.db.row(
                "SELECT to_jsonb(t) FROM (SELECT paused_at, last_active_at, updated_at "
                f"FROM public.users WHERE id = {sql_literal(self.user_id)}) t"
            )
            return row == before

        return self.mutate('api.me.resume', rollback, apply, verify_restore)

    def case_sync_cancel(self):
        holder = {}
        rollback = self.task_cleanup_predicate('sync.starred', 'api.sync.starred')

        def apply():
            status, body, _ = self.http.request('POST', '/api/sync/starred')
            expect_status(status, 202, '/api/sync/starred')
            expect(body and body.get('mode') == 'task_id', 'sync response missing task mode')
            expect(body.get('task_type') == 'sync.starred', 'sync response task_type mismatch')
            expect(body.get('status') == 'queued', 'sync response status is not queued')
            holder['task_id'] = body.get('task_id')
            expect(bool(holder['task_id']), 'sync response missing task_id')
            task = self.task_row(holder['task_id'])
            expect(task is not None, 'sync task row was not inserted')
            expect(task['status'] == 'queued', f'sync task status is {task["status"]}')
            read = self.http.request(
                'GET', '/api/tasks/' + urllib.parse.quote(holder['task_id'], safe='')
            )
            expect_status(read[0], 200, '/api/tasks/{task_id}')
            expect((read[1] or {}).get('status') == 'queued', 'sync task read-back is not queued')
            cancel_route = (
                '/api/tasks/' + urllib.parse.quote(holder['task_id'], safe='') + '/cancel'
            )
            status, body, _ = self.http.request('POST', cancel_route)
            expect_status(status, 200, cancel_route)
            expect(body and body.get('cancel_requested') is True, 'cancel response mismatch')
            task = self.task_row(holder['task_id'])
            expect(task and task['status'] == 'queued', 'cancel moved the task out of queued')
            expect(task['cancel_requested'] == 1, 'cancel_requested was not persisted')
            read = self.http.request(
                'GET', '/api/tasks/' + urllib.parse.quote(holder['task_id'], safe='')
            )
            expect_status(read[0], 200, '/api/tasks/{task_id}')
            expect((read[1] or {}).get('cancel_requested') is True, 'cancel read-back mismatch')
            expect((read[1] or {}).get('status') == 'queued', 'cancelled task left queued state')
            self.probe_task_ids.append(holder['task_id'])
            return {
                'task_id': holder['task_id'],
                'extra_rollback_sql': self.task_rollback(holder['task_id']),
            }

        def verify_restore():
            return not holder.get('task_id') or self.task_gone(holder['task_id'])

        return self.mutate('api.sync.starred.then_cancel', rollback, apply, verify_restore)

    # -- final guard ----------------------------------------------------------
    def task_status_guard(self):
        created = int(self.db.scalar(
            "SELECT COUNT(*) FROM public.job_tasks WHERE status = 'running' "
            f"AND created_at::timestamptz >= {sql_literal(self.started_at)}::timestamptz"
        ) or 0)
        started = int(self.db.scalar(
            "SELECT COUNT(*) FROM public.job_tasks WHERE started_at IS NOT NULL "
            f"AND started_at::timestamptz >= {sql_literal(self.started_at)}::timestamptz"
        ) or 0)
        running_after = int(self.db.scalar(
            "SELECT COUNT(*) FROM public.job_tasks WHERE status = 'running'"
        ) or 0)
        queued_probe_tasks = []
        for task_id in self.probe_task_ids:
            task = self.task_row(task_id)
            queued_probe_tasks.append({
                'task_id': task_id,
                'present_after_cleanup': task is not None,
            })
        result = (
            'pass'
            if created == 0 and started == 0 and running_after == self.running_before
            else 'fail'
        )
        self.cases.append({
            'case': 'task.status.guard',
            'method': 'SQL',
            'route': 'public.job_tasks',
            'result': result,
            'running_before': self.running_before,
            'running_after': running_after,
            'running_created_since_start': created,
            'tasks_started_since_start': started,
            'probe_tasks': queued_probe_tasks,
            'reason': None if result == 'pass' else 'a task became running while the worker was disabled',
        })

    # -- route audit ----------------------------------------------------------
    @staticmethod
    def alternative_zone(current):
        current = (current or '').strip().lower()
        for zone in ('Asia/Shanghai', 'UTC', 'Europe/Berlin'):
            if zone.lower() != current:
                return zone
        return 'UTC'

    def source_write_routes(self, source_path):
        source = pathlib.Path(source_path).read_text()
        if '.route(' not in source or '.with_state(state)' not in source:
            return []
        body = source[source.index('.route('):source.index('.with_state(state)')]
        routes = []
        for chunk in body.split('.route(')[1:]:
            match = re.match(r'\s*"([^"]+)"', chunk)
            if not match:
                continue
            rest = chunk[match.end():]
            for method in ('post', 'put', 'patch', 'delete'):
                if re.search(r'\b' + method + r'\(', rest):
                    routes.append((method.upper(), match.group(1)))
        return sorted(set(routes))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-url', required=True)
    parser.add_argument('--cookie-file', type=pathlib.Path)
    parser.add_argument('--postgres-url', required=True)
    parser.add_argument('--output', type=pathlib.Path, required=True)
    parser.add_argument(
        '--routes-source', type=pathlib.Path,
        default=pathlib.Path(__file__).resolve().parent.parent / 'src/postgres_server.rs',
    )
    args = parser.parse_args()

    cookie = args.cookie_file.read_text().strip() if args.cookie_file else ''
    probe = Probe(Http(args.base_url, cookie), Db(args.postgres_url))

    status, body, _ = probe.http.request('GET', '/api/me')
    if status != 200 or not isinstance(body, dict) or not body.get('user', {}).get('id'):
        report = {
            'cases': [{
                'case': 'session',
                'route': '/api/me',
                'result': 'fail',
                'reason': f'admin session unavailable (status {status})',
            }],
            'passed': 0,
            'failed': 1,
            'skipped_external': 0,
            'skipped_scope': 0,
            'skipped_fixture': 0,
            'restored': False,
            'needs_attention': [],
            'generated_at': now_iso(),
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'error': 'admin session unavailable', 'status': status}))
        return 2
    probe.user_id = body['user']['id']
    if not body['user'].get('is_admin'):
        print(json.dumps({'error': 'probe session user is not an admin'}))
        return 2
    account = probe.db.row(
        "SELECT to_jsonb(t) FROM (SELECT paused_at, is_disabled FROM public.users WHERE id = "
        f"{sql_literal(probe.user_id)}) t"
    )
    if account is None or account['is_disabled'] != 0:
        print(json.dumps({'error': 'probe session user is disabled or missing'}))
        return 2
    if account['paused_at'] is not None:
        print(json.dumps({
            'error': 'probe session user is paused; resume the account before running the write probe',
            'paused_at': account['paused_at'],
        }))
        return 2

    cases = [
        ('profile', 'PATCH', '/api/me/profile', probe.case_profile),
        ('api_keys', 'POST', '/api/me/api-keys', probe.case_api_keys),
        ('following', 'PUT', '/api/repos/{owner}/{repo}/following', probe.case_following),
        ('scheduled_slot', 'PATCH', '/api/admin/jobs/scheduled/{hour_utc}', probe.case_scheduled_slot),
        ('llm_runtime_config', 'PATCH', '/api/admin/jobs/llm/runtime-config', probe.case_llm_runtime_config),
        ('admin_user_patch', 'PATCH', '/api/admin/users/{user_id}', probe.case_admin_user_patch),
        ('admin_user_profile', 'PATCH', '/api/admin/users/{user_id}/profile', probe.case_admin_user_profile),
        ('webhook_reconcile', 'POST', '/api/me/webhook-push/reconcile', probe.case_webhook_reconcile),
        ('webhook_settings', 'PATCH', '/api/me/webhook-push', probe.case_webhook_settings),
        ('online_migration', 'POST', '/api/admin/jobs/migrations/{migration_id}/pause', probe.case_online_migration),
        ('search_index_resume', 'POST', '/api/admin/jobs/search-index/resume', probe.case_search_index_resume),
        ('me_resume', 'POST', '/api/me/resume', probe.case_me_resume),
        ('sync_cancel', 'POST', '/api/tasks/{task_id}/cancel', probe.case_sync_cancel),
    ]
    for name, method, route, func in cases:
        probe.run_case(name, method, route, func)
    probe.task_status_guard()

    write_routes = probe.source_write_routes(args.routes_source)
    classified = set(ALLOWED) | {(entry['method'], entry['route']) for entry in SKIPPED}
    unclassified = [
        {'method': method, 'route': route}
        for method, route in write_routes
        if (method, route) not in classified
    ]

    for entry in SKIPPED:
        probe.cases.append({
            'case': 'refused.' + entry['route'],
            'method': entry['method'],
            'route': entry['route'],
            'result': 'skipped_external' if entry['classification'] == 'external' else 'skipped_scope',
            'classification': entry['classification'],
            'reason': entry['reason'],
        })

    failed = sum(case['result'] == 'fail' for case in probe.cases)
    passed = sum(case['result'] == 'pass' for case in probe.cases)
    skipped_external = sum(case['result'] == 'skipped_external' for case in probe.cases)
    skipped_scope = sum(case['result'] == 'skipped_scope' for case in probe.cases)
    skipped_fixture = sum(case['result'] == 'skipped_fixture' for case in probe.cases)
    restored = all(entry['restored'] for entry in probe.mutations) and not probe.attention

    report = {
        'cases': probe.cases,
        'allowlist': [
            {'method': method, 'route': route, 'contract': contract}
            for (method, route), contract in sorted(ALLOWED.items())
        ],
        'skip_table': SKIPPED,
        'unclassified_write_routes': unclassified,
        'mutations': probe.mutations,
        'passed': passed,
        'failed': failed,
        'skipped_external': skipped_external,
        'skipped_scope': skipped_scope,
        'skipped_fixture': skipped_fixture,
        'restored': restored,
        'needs_attention': probe.attention,
        'started_at': probe.started_at,
        'generated_at': now_iso(),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    marker = pathlib.Path(str(args.output) + '.needs-attention.json')
    marker.write_text(json.dumps({
        'generated_at': report['generated_at'],
        'needs_attention': probe.attention,
    }, indent=2) + '\n')
    print(json.dumps({
        key: report[key] for key in (
            'passed', 'failed', 'skipped_external', 'skipped_scope',
            'skipped_fixture', 'restored', 'started_at',
        )
    }))
    print(json.dumps({
        'mutations': len(probe.mutations),
        'not_restored': [
            entry['mutation'] for entry in probe.mutations if not entry['restored']
        ],
        'unclassified_write_routes': len(unclassified),
        'marker': str(marker),
    }))
    return 1 if failed or not restored else 0


if __name__ == '__main__':
    raise SystemExit(main())
