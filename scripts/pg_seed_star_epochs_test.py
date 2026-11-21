#!/usr/bin/env python3
"""Validate PostgreSQL Star reconciliation epoch slices on a disposable database.

The suite drives the real task worker against a local stub GraphQL endpoint
(`OCTORILL_GITHUB_API_BASE`) so no request reaches GitHub. It proves:

1. the first execution creates an epoch and advances exactly one page,
2. a slice that is not due is skipped, and a due slice continues the cursor,
3. a failing slice records the error plus a retry time without succeeding,
4. the terminal slice finalizes the epoch and prunes vanished memberships.
"""
import argparse
import base64
import datetime
import json
import os
import pathlib
import subprocess
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit, urlunsplit

from pg_seed_common import psql

PAGE_SIZE = 2
DATABASE_NAME_PREFIX = "octo_rill_star_epochs_"
USER_ID = "star-epoch-user"
CONNECTION_ID = "star-epoch-connection"
TOKEN = "synthetic-star-token"
BACKDATED = "2020-01-01T00:00:00+00:00"

REPOSITORIES = [
    {
        "id": 101,
        "full_name": "octo/first",
        "starred_at": "2026-11-01T00:00:00Z",
    },
    {
        "id": 102,
        "full_name": "octo/second",
        "starred_at": "2026-10-20T00:00:00Z",
    },
    {
        "id": 103,
        "full_name": "octo/third",
        "starred_at": "2026-10-10T00:00:00Z",
    },
    {
        "id": 104,
        "full_name": "octo/fourth",
        "starred_at": "2026-10-01T00:00:00Z",
    },
    {
        "id": 105,
        "full_name": "octo/fifth",
        "starred_at": "2026-09-20T00:00:00Z",
    },
]


class StarStub:
    """Mutable GitHub GraphQL `starredRepositories` stub."""

    def __init__(self):
        self.repos = list(REPOSITORIES)
        self.fail = False
        self.over_limit = False
        self.requests = []
        self.lock = threading.Lock()

    def snapshot(self):
        with self.lock:
            return self.fail, self.over_limit, list(self.repos)

    def request_count(self):
        with self.lock:
            return len(self.requests)


def build_handler(stub):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):  # keep the suite output readable
            pass

        def _write(self, status, raw, content_type="application/json"):
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length) if length else b"{}"
            if self.path != "/graphql":
                self._write(404, b"{}")
                return
            payload = json.loads(body or b"{}")
            with stub.lock:
                stub.requests.append(payload)
            fail, over_limit, repos = stub.snapshot()
            if fail:
                self._write(500, b'{"message":"stub upstream failure"}')
                return
            variables = payload.get("variables") or {}
            after = variables.get("after")
            start = int(after) if after is not None else 0
            page = repos[start : start + PAGE_SIZE]
            has_next_page = start + PAGE_SIZE < len(repos)
            response = {
                "data": {
                    "viewer": {
                        "starredRepositories": {
                            "totalCount": len(repos),
                            "isOverLimit": over_limit,
                            "pageInfo": {
                                "hasNextPage": has_next_page,
                                "endCursor": str(start + PAGE_SIZE),
                            },
                            "edges": [starred_edge(repo) for repo in page],
                        }
                    }
                }
            }
            self._write(200, json.dumps(response).encode())

    return Handler


def starred_edge(repo):
    owner_login = repo["full_name"].split("/")[0]
    return {
        "starredAt": repo["starred_at"],
        "node": {
            "databaseId": repo["id"],
            "nameWithOwner": repo["full_name"],
            "name": repo["full_name"].split("/")[1],
            "description": "stub repository",
            "url": f"https://github.com/{repo['full_name']}",
            "isPrivate": False,
            "stargazerCount": 7,
            "openGraphImageUrl": "https://example.invalid/open-graph.png",
            "usesCustomOpenGraphImage": False,
            "owner": {
                "login": owner_login,
                "avatarUrl": "https://example.invalid/avatar.png",
            },
        },
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--admin-url", required=True)
    parser.add_argument("--binary-dir", type=pathlib.Path, default=pathlib.Path("target/debug"))
    parser.add_argument("--port", type=int, default=3141)
    args = parser.parse_args()

    name = DATABASE_NAME_PREFIX + str(os.getpid())
    database_url = urlunsplit(urlsplit(args.admin_url)._replace(path="/" + name))
    binary = (args.binary_dir / "octo-rill").resolve()

    def sql(query, database=database_url):
        return psql(database, query)

    def row(query):
        raw = sql(query)
        if not raw:
            raise AssertionError(f"expected one row from: {query}")
        return json.loads(raw)

    def expect(actual, expected, message):
        if actual != expected:
            raise AssertionError(f"{message}: expected {expected!r}, got {actual!r}")

    def connection_epoch():
        return row(
            "SELECT json_build_object("
            "'id', id, 'status', status, 'processed_pages', processed_pages,"
            "'processed_items', processed_items,"
            "'total_count_at_start', total_count_at_start, 'next_cursor', next_cursor,"
            "'next_slice_not_before', next_slice_not_before,"
            "'failure_reason', failure_reason, 'completed_at', completed_at,"
            "'started_at', started_at, 'user_id', user_id"
            f")::text FROM public.star_sync_epochs WHERE github_connection_id = '{CONNECTION_ID}'"
            " ORDER BY created_at DESC, id DESC LIMIT 1"
        )

    def epoch_count():
        return int(
            sql(
                "SELECT COUNT(*) FROM public.star_sync_epochs"
                f" WHERE github_connection_id = '{CONNECTION_ID}'"
            )
        )

    def star_ids(query):
        raw = sql(query)
        return [] if not raw else [int(value) for value in raw.splitlines()]

    def starred_repo_ids():
        return star_ids(
            f"SELECT repo_id FROM public.starred_repos WHERE user_id = '{USER_ID}' ORDER BY repo_id"
        )

    def member_repo_ids():
        return star_ids(
            "SELECT repo_id FROM public.repo_star_current_members"
            f" WHERE user_id = '{USER_ID}' ORDER BY repo_id"
        )

    def backdate_next_slice():
        sql(
            "UPDATE public.star_sync_epochs"
            f" SET next_slice_not_before = '{BACKDATED}'"
            f" WHERE github_connection_id = '{CONNECTION_ID}'"
            " AND status IN ('queued', 'running')"
        )

    task_counter = iter(range(1, 1000))

    def run_task():
        task_id = f"star-epoch-task-{next(task_counter)}"
        sql(
            "INSERT INTO public.job_tasks"
            "(id, task_type, status, source, requested_by, payload_json, created_at, updated_at)"
            f" VALUES ('{task_id}', 'sync.starred', 'queued', 'validation', '{USER_ID}',"
            f" '{{\"user_id\": \"{USER_ID}\"}}', now(), now())"
        )
        deadline = time.time() + 90
        while time.time() < deadline:
            state = row(
                "SELECT json_build_object("
                "'status', status, 'result', result_json, 'error', error_message"
                f")::text FROM public.job_tasks WHERE id = '{task_id}'"
            )
            if state["status"] in ("succeeded", "failed", "cancelled"):
                result = json.loads(state["result"]) if state["result"] else None
                return state["status"], result, state["error"]
            time.sleep(0.1)
        raise AssertionError(f"task {task_id} did not settle in time")

    def is_past(value):
        return datetime.datetime.fromisoformat(value) < datetime.datetime.now(
            datetime.timezone.utc
        )

    sql("CREATE DATABASE " + name, database=args.admin_url)
    server = None
    stub = StarStub()
    stub_http = ThreadingHTTPServer(("127.0.0.1", 0), build_handler(stub))
    stub_thread = threading.Thread(target=stub_http.serve_forever, daemon=True)
    stub_thread.start()

    env = dict(
        os.environ,
        DATABASE_URL=database_url,
        OCTORILL_ENCRYPTION_KEY_BASE64=base64.b64encode(bytes(32)).decode(),
        GITHUB_CLIENT_ID="validation",
        GITHUB_CLIENT_SECRET="validation",
        GITHUB_OAUTH_REDIRECT_URL=f"http://127.0.0.1:{args.port}/auth/github/callback",
        OCTORILL_BIND_ADDR=f"127.0.0.1:{args.port}",
        OCTORILL_PG_SCHEDULER_ENABLED="0",
        OCTORILL_PG_BOOTSTRAP_ONLY="1",
        OCTORILL_GITHUB_API_BASE=f"http://127.0.0.1:{stub_http.server_address[1]}",
        APP_DEFAULT_TIME_ZONE="UTC",
    )

    try:
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)

        from cryptography.hazmat.primitives.ciphers.aead import AESGCM

        nonce = b"0123456789ab"
        ciphertext = AESGCM(bytes(32)).encrypt(nonce, TOKEN.encode(), None)
        sql(
            "INSERT INTO public.users"
            "(id, github_user_id, login, created_at, updated_at, is_admin, is_disabled,"
            " daily_brief_time_zone, paused_at)"
            f" VALUES ('{USER_ID}', 601, 'starepoch', '2026-01-01T00:00:00Z',"
            " '2026-01-01T00:00:00Z', 0, 0, 'UTC', NULL);"
            "INSERT INTO public.github_connections"
            "(id, user_id, github_user_id, login, avatar_url, access_token_ciphertext,"
            " access_token_nonce, scopes, linked_at, updated_at)"
            f" VALUES ('{CONNECTION_ID}', '{USER_ID}', 601, 'starepoch',"
            f" 'https://example.invalid/avatar.png', decode('{ciphertext.hex()}', 'hex'),"
            f" decode('{nonce.hex()}', 'hex'), '', '2026-01-01T00:00:00Z',"
            " '2026-01-01T00:00:00Z');"
            "INSERT INTO public.admin_runtime_settings"
            "(id, llm_max_concurrency, translation_general_worker_concurrency,"
            " translation_dedicated_worker_concurrency, created_at, updated_at,"
            " star_sync_delta_interval_minutes, star_sync_full_sweep_interval_minutes)"
            " VALUES (1, 1, 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 30, 60)"
        )

        env.pop("OCTORILL_PG_BOOTSTRAP_ONLY")
        with tempfile.TemporaryDirectory(prefix="octo-rill-star-epochs-") as tmp:
            log = open(pathlib.Path(tmp) / "server.log", "w+")
            server = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
            base = f"http://127.0.0.1:{args.port}"
            for _ in range(200):
                try:
                    urllib.request.urlopen(base + "/health", timeout=1).close()
                    break
                except Exception:
                    if server.poll() is not None:
                        log.flush()
                        log.seek(0)
                        raise RuntimeError(log.read()[-4000:])
                    time.sleep(0.1)
            else:
                raise RuntimeError("validation server did not become ready")

            # -- 1. first execution: one page only ---------------------------
            status, result, error = run_task()
            expect(status, "succeeded", f"first slice task status ({error})")
            expect(result["status"], "queued", "first slice epoch state")
            expect(result["processed_pages"], 1, "first slice processed_pages")
            expect(result["processed_items"], 2, "first slice processed_items")
            expect(result["has_next_page"], True, "first slice has_next_page")
            expect(result["total_count_at_start"], 5, "first slice total_count_at_start")
            expect(result["github_connection_id"], CONNECTION_ID, "first slice connection")
            epoch = connection_epoch()
            expect(epoch["status"], "queued", "epoch status after first slice")
            expect(epoch["processed_pages"], 1, "epoch processed_pages after first slice")
            expect(epoch["processed_items"], 2, "epoch processed_items after first slice")
            expect(epoch["total_count_at_start"], 5, "epoch total_count_at_start")
            expect(epoch["next_cursor"], str(PAGE_SIZE), "epoch cursor after first slice")
            expect(epoch["completed_at"], None, "epoch must stay open after first slice")
            expect(
                is_past(epoch["next_slice_not_before"]),
                False,
                "epoch must schedule a future slice",
            )
            expect(epoch_count(), 1, "exactly one epoch after the first slice")
            expect(starred_repo_ids(), [101, 102], "aggregate Star rows after first slice")
            expect(member_repo_ids(), [101, 102], "current members after first slice")

            # -- 2a. same slice still not due --------------------------------
            requests_before = stub.request_count()
            status, result, error = run_task()
            expect(status, "succeeded", f"not-due task status ({error})")
            expect(result["status"], "skipped", "not-due decision")
            expect(result["reason"], "star_epoch_not_due", "not-due reason")
            expect(stub.request_count(), requests_before, "not-due slice must not call GitHub")
            epoch = connection_epoch()
            expect(epoch["processed_pages"], 1, "not-due slice must not advance pages")
            expect(epoch["processed_items"], 2, "not-due slice must not advance items")
            expect(epoch["next_cursor"], str(PAGE_SIZE), "not-due slice must keep the cursor")

            # -- 2b. due slice continues the cursor --------------------------
            backdate_next_slice()
            status, result, error = run_task()
            expect(status, "succeeded", f"second slice task status ({error})")
            expect(result["status"], "queued", "second slice epoch state")
            expect(result["processed_pages"], 2, "second slice processed_pages")
            expect(result["processed_items"], 4, "second slice processed_items")
            expect(result["has_next_page"], True, "second slice has_next_page")
            epoch = connection_epoch()
            expect(epoch["next_cursor"], str(PAGE_SIZE * 2), "cursor after second slice")
            expect(starred_repo_ids(), [101, 102, 103, 104], "aggregate rows after second slice")

            # -- 2c. final page completes the epoch --------------------------
            backdate_next_slice()
            status, result, error = run_task()
            expect(status, "succeeded", f"final slice task status ({error})")
            expect(result["status"], "succeeded", "final slice epoch state")
            expect(result["processed_pages"], 3, "final slice processed_pages")
            expect(result["processed_items"], 5, "final slice processed_items")
            expect(result["has_next_page"], False, "final slice has_next_page")
            expect(result["membership_removed"], 0, "no membership removed on a clean sweep")
            epoch = connection_epoch()
            expect(epoch["status"], "succeeded", "epoch status after final slice")
            expect(epoch["processed_pages"], 3, "epoch processed_pages after final sweep")
            expect(epoch["processed_items"], 5, "epoch processed_items after final sweep")
            expect(epoch["total_count_at_start"], 5, "epoch total count after final sweep")
            expect(epoch["next_slice_not_before"], None, "succeeded epoch has no next slice")
            expect(epoch["completed_at"] is None, False, "succeeded epoch has completed_at")
            expect(starred_repo_ids(), [101, 102, 103, 104, 105], "all Star rows after sweep")
            expect(member_repo_ids(), [101, 102, 103, 104, 105], "all members after sweep")

            # -- 3. failing slice records the error, never success -----------
            sql(
                "UPDATE public.star_sync_epochs"
                f" SET completed_at = '{BACKDATED}', started_at = '{BACKDATED}'"
                f" WHERE github_connection_id = '{CONNECTION_ID}' AND status = 'succeeded'"
            )
            stub.fail = True
            status, result, error = run_task()
            expect(status, "failed", "failing slice task status")
            expect("HTTP 500" in (error or ""), True, f"task error records the failure ({error})")
            epoch = connection_epoch()
            expect(epoch["status"], "queued", "failed slice must not finish the epoch")
            expect(epoch["completed_at"], None, "failed slice must not set completed_at")
            expect(epoch["processed_pages"], 0, "failed slice must not advance pages")
            expect(epoch["processed_items"], 0, "failed slice must not advance items")
            expect(epoch["total_count_at_start"], None, "failed slice must not record the total")
            expect("HTTP 500" in (epoch["failure_reason"] or ""), True, "epoch error is recorded")
            expect(
                is_past(epoch["next_slice_not_before"]),
                False,
                "epoch schedules a retry time",
            )
            expect(epoch_count(), 2, "the retry keeps a single new epoch")
            failed_epoch_id = epoch["id"]
            expect(starred_repo_ids(), [101, 102, 103, 104, 105], "failure prunes nothing")

            # -- 4. retry resumes the same epoch and prunes lost members ------
            stub.fail = False
            stub.repos = list(REPOSITORIES[:4])
            backdate_next_slice()
            status, result, error = run_task()
            expect(status, "succeeded", f"resumed slice task status ({error})")
            expect(result["epoch_id"], failed_epoch_id, "retry resumes the deferred epoch")
            expect(result["status"], "queued", "resumed slice epoch state")
            expect(result["processed_pages"], 1, "resumed slice processed_pages")
            expect(result["processed_items"], 2, "resumed slice processed_items")
            expect(starred_repo_ids(), [101, 102, 103, 104, 105], "prune only at finalize")

            backdate_next_slice()
            status, result, error = run_task()
            expect(status, "succeeded", f"pruning slice task status ({error})")
            expect(result["status"], "succeeded", "pruning slice epoch state")
            expect(result["epoch_id"], failed_epoch_id, "pruning slice stays on the epoch")
            expect(result["processed_pages"], 2, "pruning slice processed_pages")
            expect(result["processed_items"], 4, "pruning slice processed_items")
            expect(result["has_next_page"], False, "pruning slice has_next_page")
            expect(result["membership_removed"], 1, "one unstarred membership removed")
            epoch = connection_epoch()
            expect(epoch["id"], failed_epoch_id, "epoch identity after retry")
            expect(epoch["status"], "succeeded", "retried epoch succeeds")
            expect(epoch["failure_reason"], None, "success clears the recorded failure")
            expect(epoch["total_count_at_start"], 4, "retried epoch records the new total")
            expect(starred_repo_ids(), [101, 102, 103, 104], "unstarred repo leaves the aggregate")
            expect(member_repo_ids(), [101, 102, 103, 104], "unstarred repo leaves the members")

            print(
                json.dumps(
                    {
                        "single_page_slice": "pass",
                        "slice_due_gate": "pass",
                        "slice_completion": "pass",
                        "slice_failure_retry": "pass",
                        "membership_diff": "pass",
                    }
                )
            )
    finally:
        if server is not None:
            server.send_signal(2)
            server.wait(timeout=15)
        stub_http.shutdown()
        stub_http.server_close()
        sql("DROP DATABASE " + name + " WITH (FORCE)", database=args.admin_url)


if __name__ == "__main__":
    main()
