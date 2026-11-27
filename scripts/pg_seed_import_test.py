#!/usr/bin/env python3
"""Build a synthetic source from SQLite schema; import into a disposable PG database."""
import argparse
import datetime
import json
import os
import sys
import pathlib
import sqlite3
import subprocess

import tempfile
from pg_seed_common import psql
from urllib.parse import urlsplit, urlunsplit


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--schema-source', required=True, type=pathlib.Path)
    p.add_argument('--admin-url', required=True)
    p.add_argument('--binary', type=pathlib.Path, default=pathlib.Path('target/debug/octo-rill-migrate'))
    p.add_argument(
        '--server-binary',
        type=pathlib.Path,
        default=pathlib.Path('target/debug/octo-rill'),
        help='runtime binary used to bootstrap the target schema before importing',
    )
    args = p.parse_args()
    name = 'octo_rill_import_seed_' + str(os.getpid())
    url = urlunsplit(urlsplit(args.admin_url)._replace(path='/' + name))
    def sql(query, database=args.admin_url):
        return psql(database, query)

    bootstrap_env = {
        'OCTORILL_PG_BOOTSTRAP_ONLY': '1',
        'OCTORILL_ENCRYPTION_KEY_BASE64': 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=',
        'GITHUB_CLIENT_ID': 'validation',
        'GITHUB_CLIENT_SECRET': 'validation',
        'GITHUB_OAUTH_REDIRECT_URL': 'http://127.0.0.1/auth/github/callback',
        'OCTORILL_BIND_ADDR': '127.0.0.1:0',
        'APP_DEFAULT_TIME_ZONE': 'UTC',
        'AI_API_KEY': '',
        'AI_BASE_URL': '',
        'AI_MODEL': '',
    }

    def bootstrap_target():
        # The importer writes `public.migration_manifests` and compares row
        # content, so the target schema must exist before the source rows are
        # built. Bootstrap through the runtime, like the other seed suites.
        subprocess.run(
            [args.server_binary.resolve()],
            env={**os.environ, **bootstrap_env, 'DATABASE_URL': url},
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    sql('CREATE DATABASE ' + name)
    try:
        with tempfile.TemporaryDirectory(prefix='octo-rill-import-') as tmp:
            source = pathlib.Path(tmp) / 'seed.db'
            original = sqlite3.connect('file:' + str(args.schema_source) + '?mode=ro', uri=True)
            seed = sqlite3.connect(source)
            schemas = original.execute("SELECT name,sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND sql IS NOT NULL").fetchall()
            virtual = {n for n, ddl in schemas if 'CREATE VIRTUAL TABLE' in ddl.upper()}
            for n, ddl in schemas:
                if n in virtual or any(n.startswith(v + '_') for v in virtual):
                    continue
                seed.execute(ddl)
            for name_, ddl in original.execute("SELECT name,sql FROM sqlite_master WHERE type='view' AND name IN ('user_following_repos','user_release_visible_repos','search_release_metadata')"):
                seed.execute(ddl)
            for index_name, table_name, ddl in original.execute("SELECT name,tbl_name,sql FROM sqlite_master WHERE type='index' AND sql IS NOT NULL"):
                if ddl.upper().startswith('CREATE UNIQUE INDEX'):
                    seed.execute(ddl)
            bootstrap_target()
            seed.execute("INSERT INTO users(id,github_user_id,login,created_at,updated_at) VALUES ('seed-user',101,'seed','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')")
            seed.execute("INSERT INTO starred_repos(id,user_id,repo_id,full_name,owner_login,name,html_url,is_private,updated_at) VALUES ('seed-repo','seed-user',1,'seed/repo','seed','repo','https://example.invalid/seed',0,'2026-01-01T00:00:00Z')")
            seed.execute("INSERT INTO tower_sessions VALUES ('seed-session',?,?)", (bytes([0x80]), '2099-01-01T00:00:00Z'))
            # The runtime migrations seed a few control rows. The importer
            # compares row content, so the synthetic source must carry exactly
            # the values the bootstrapped target already holds; otherwise the
            # seed looks like a source-side delete. Copy them from the target.
            for control_table in ('content_processing_control', 'llm_recovery_flags'):
                columns = [
                    row[0]
                    for row in seed.execute(
                        f"SELECT name FROM pragma_table_info('{control_table}')"
                    )
                ]
                # Timestamps must be compared the way the importer normalizes
                # them, so transport them as epoch seconds and rebuild ISO UTC.
                selects = []
                for column in columns:
                    kind = sql(
                        f"SELECT data_type FROM information_schema.columns "
                        f"WHERE table_schema='public' AND table_name='{control_table}' "
                        f"AND column_name='{column}'",
                        url,
                    )
                    if kind.startswith('timestamp'):
                        selects.append(
                            # Match `to_rfc3339_opts(SecondsFormat::Micros, true)`,
                            # the canonical form the importer compares against.
                            f"to_char(\"{column}\" AT TIME ZONE 'UTC', "
                            f"'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+00:00\"')"
                        )
                    else:
                        # Wrap in a null sentinel: `::text` renders NULL as an
                        # empty string, which is indistinguishable from `''`.
                        selects.append(
                            f"coalesce('v:'||\"{column}\"::text, '\\N')"
                        )
                raw_values = sql(
                    'SELECT ' + ','.join(selects) + f' FROM public.{control_table}',
                    url,
                ).split('|')
                target_values = [
                    None if value == r'\N' else (
                        value[2:] if value.startswith('v:') else value
                    )
                    for value in raw_values
                ]
                placeholders = ','.join('?' for _ in columns)
                seed.execute(
                    f"INSERT INTO {control_table}(" + ','.join(columns) + ')'
                    f' VALUES ({placeholders})',
                    target_values,
                )
            seed.commit()
            seed.close()
            original.close()
            env = dict(os.environ, SQLITE_SOURCE='sqlite:' + str(source), POSTGRES_URL=url,
                       MIGRATION_TARGET_PATH='/', MIGRATION_MANIFEST=str(pathlib.Path(tmp) / 'manifest.json'))
            bootstrap_target()
            first = subprocess.run([args.binary.resolve(), 'import'], env=env,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            if first.returncode:
                print(first.stdout[-1500:], file=sys.stderr)
            first.check_returncode()
            subprocess.run([args.binary.resolve(), 'verify'], env=env, check=True)
            assert sql("SELECT count(*) FROM public.tower_sessions WHERE id='seed-session'", url) == '1' 
            subprocess.run([args.binary.resolve(), 'import'], env=env, check=True)
            sql("UPDATE public.users SET login='tampered' WHERE id='seed-user'", url)
            corrupted = subprocess.run([args.binary.resolve(), 'verify'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            assert corrupted.returncode != 0, 'same-count corruption must fail verification'
            rerun = subprocess.run([args.binary.resolve(), 'import'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            assert rerun.returncode != 0, 'same-count corruption must fail import re-entry'
            sql("UPDATE public.users SET login='seed' WHERE id='seed-user'", url)
            assert sql("SELECT count(*) FROM public.user_release_visible_repos WHERE user_id='seed-user'", url) == '1'
            # Imported `briefs.updated_at` is text; the retry lane must compare it
            # chronologically instead of against a timestamp.
            recent = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
            sql("INSERT INTO public.briefs (id, user_id, date, window_start_utc, window_end_utc, "
                "effective_time_zone, effective_local_boundary, generation_source, content_markdown, "
                "created_at, updated_at) VALUES ('seed-brief','seed-user','2026-01-01',"
                "'2026-01-01T00:00:00Z','2026-01-02T00:00:00Z','UTC','00:00',"
                f"'content_refresh_failed','# failed','{recent}','{recent}')", url)
            retry_rows = sql(
                "SELECT b.id FROM public.briefs b JOIN public.users u ON u.id = b.user_id"
                " WHERE u.is_disabled = 0 AND u.paused_at IS NULL"
                " AND b.generation_source = 'content_refresh_failed'"
                " AND b.window_start_utc IS NOT NULL AND b.window_end_utc IS NOT NULL"
                " AND b.effective_time_zone IS NOT NULL AND b.effective_local_boundary IS NOT NULL"
                " AND pg_input_is_valid(b.updated_at::text, 'timestamptz')"
                " AND b.updated_at::text::timestamptz >= (now() - interval '8 hours')"
                " ORDER BY b.updated_at DESC, b.created_at DESC, b.id DESC LIMIT 20",
                url,
            )
            assert retry_rows.strip() == 'seed-brief', retry_rows
            assert sql("SELECT count(*) FROM public.user_following_repos WHERE user_id='seed-user'", url) == '1'
            sql("DELETE FROM public.starred_repos WHERE user_id='seed-user'", url)
            assert sql("SELECT count(*) FROM public.user_release_visible_repos WHERE user_id='seed-user'", url) == '0'
            print(json.dumps({'synthetic_import': 'pass', 'verify': 'pass', 'live_visibility_views': 'pass', 'idempotent_reentry': 'pass', 'same_count_corruption_rejected': 'pass'}))
    finally:
        sql('DROP DATABASE ' + name + ' WITH (FORCE)')


if __name__ == '__main__':
    main()
