#!/usr/bin/env python3
"""Seed an isolated PG18 database and verify the GitHub release webhook receiver.

The suite runs in two phases against one database:

* receiver phase - the task worker is disabled, so every assertion covers the
  webhook contract itself (signature, delivery idempotency, single-flight
  `sync.repo_release` enqueue, delivery -> task association);
* worker phase - the same database is served with `OCTORILL_PG_WORKER_ENABLED=1`
  and `OCTORILL_GITHUB_API_BASE` pointing at a local GitHub stub, so the queued
  `sync.repo_release` task executes without any real upstream traffic.
"""
import argparse
import base64
import hashlib
import hmac
import json
import os
import pathlib
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pg_seed_common import psql
from urllib.parse import urlsplit, urlunsplit

SECRET = "synthetic-webhook-secret"
CALLBACK_KEY = "synthetic-callback-key"
ACCESS_TOKEN = "ghs_synthetic_release_token"
HOOK_ID = 777
REPO_ID = 4242
REPO_FULL_NAME = "owner/repo"
RELEASE_ID = 9001
TASK_TYPE = "sync.repo_release"
PENDING_MARKER = "content_processing_enqueue_pending"

STUB = {
    "etag": '"stub-releases-v1"',
    "fail_status": 0,
    "requests": [],
    "releases": [],
}


def signature(body):
    return "sha256=" + hmac.new(SECRET.encode(), body, hashlib.sha256).hexdigest()


def release_payload(action="published", draft=False, repo_id=REPO_ID, full_name=REPO_FULL_NAME):
    return {
        "action": action,
        "release": {"id": RELEASE_ID, "draft": draft},
        "repository": {"id": repo_id, "full_name": full_name},
    }


def github_release(tag, body, updated_at, release_id=RELEASE_ID):
    return {
        "id": release_id,
        "node_id": "N_" + str(release_id),
        "tag_name": tag,
        "name": tag,
        "body": body,
        "html_url": f"https://github.com/{REPO_FULL_NAME}/releases/tag/{tag}",
        "published_at": updated_at,
        "created_at": updated_at,
        "updated_at": updated_at,
        "draft": False,
        "prerelease": False,
        "reactions": {"+1": 1, "laugh": 0, "heart": 0, "hooray": 0, "rocket": 0, "eyes": 0},
    }


class StubGithubHandler(BaseHTTPRequestHandler):
    """Serve one repository's release list, like the GitHub REST endpoint."""

    def do_GET(self):
        parsed = urlsplit(self.path)
        if parsed.path != f"/repos/{REPO_FULL_NAME}/releases":
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        STUB["requests"].append(
            {
                "path": self.path,
                "authorization": self.headers.get("Authorization"),
                "if_none_match": self.headers.get("If-None-Match"),
            }
        )
        if STUB["fail_status"]:
            payload = json.dumps({"message": "synthetic upstream failure"}).encode()
            self.send_response(STUB["fail_status"])
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        if self.headers.get("If-None-Match") == STUB["etag"]:
            self.send_response(304)
            self.send_header("ETag", STUB["etag"])
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        payload = json.dumps(STUB["releases"]).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("ETag", STUB["etag"])
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--admin-url", required=True)
    parser.add_argument("--binary-dir", type=pathlib.Path, default=pathlib.Path("target/debug"))
    parser.add_argument("--port", type=int, default=3022)
    parser.add_argument("--stub-port", type=int, default=0)
    args = parser.parse_args()

    name = "octo_rill_webhook_seed_" + str(os.getpid())
    database_url = urlunsplit(urlsplit(args.admin_url)._replace(path="/" + name))

    def sql(query, database=args.admin_url):
        return psql(database, query)

    stub = ThreadingHTTPServer(("127.0.0.1", args.stub_port), StubGithubHandler)
    stub_thread = threading.Thread(target=stub.serve_forever, daemon=True)
    stub_thread.start()
    stub_base = f"http://127.0.0.1:{stub.server_address[1]}"

    sql("CREATE DATABASE " + name)
    binary = (args.binary_dir / "octo-rill").resolve()
    env = dict(
        os.environ,
        DATABASE_URL=database_url,
        OCTORILL_ENCRYPTION_KEY_BASE64=base64.b64encode(bytes(32)).decode(),
        GITHUB_CLIENT_ID="validation",
        GITHUB_CLIENT_SECRET="validation",
        GITHUB_OAUTH_REDIRECT_URL=f"http://127.0.0.1:{args.port}/auth/github/callback",
        OCTORILL_BIND_ADDR=f"127.0.0.1:{args.port}",
        OCTORILL_PG_WORKER_ENABLED="0",
        OCTORILL_PG_SCHEDULER_ENABLED="0",
        OCTORILL_PG_BOOTSTRAP_ONLY="1",
    )
    process = None
    log = None
    tmp = None
    try:
        subprocess.run([binary], env=env, check=True, stdout=subprocess.DEVNULL)

        from cryptography.hazmat.primitives.ciphers.aead import AESGCM

        key = bytes(32)
        nonce = b"0123456789ab"
        ciphertext = AESGCM(key).encrypt(nonce, SECRET.encode(), None)
        token_ciphertext = AESGCM(key).encrypt(nonce, ACCESS_TOKEN.encode(), None)

        sql(
            "DROP TABLE IF EXISTS public.reaction_pat_tokens CASCADE; CREATE TABLE public.reaction_pat_tokens ("
            "user_id text PRIMARY KEY, token_ciphertext bytea NOT NULL, token_nonce bytea NOT NULL,"
            "masked_token text NOT NULL, last_check_state text NOT NULL DEFAULT 'unknown',"
            "last_check_message text, last_checked_at text, updated_at text NOT NULL,"
            "owner_github_connection_id text, owner_github_user_id bigint, owner_login text,"
            "webhook_push_allows_private_repos bigint);"
            "DROP TABLE IF EXISTS public.owned_repo_star_baselines CASCADE; CREATE TABLE public.owned_repo_star_baselines ("
            "id text PRIMARY KEY, user_id text NOT NULL, repo_id bigint NOT NULL,"
            "repo_full_name text NOT NULL, initialized_at text NOT NULL, updated_at text NOT NULL,"
            "is_private bigint);"
            "DROP TABLE IF EXISTS public.webhook_push_repos CASCADE; CREATE TABLE public.webhook_push_repos ("
            "user_id text NOT NULL, repo_id bigint NOT NULL, owner_login text NOT NULL,"
            "repo_name text NOT NULL, repo_full_name text NOT NULL, hook_id bigint,"
            "callback_url text NOT NULL, status text NOT NULL DEFAULT 'unknown', error_kind text,"
            "error_message text, permission_paused bigint NOT NULL DEFAULT 0, last_checked_at text,"
            "last_registered_at text, updated_at text NOT NULL, owner_github_user_id bigint,"
            "PRIMARY KEY (user_id, repo_id));"
            "CREATE UNIQUE INDEX idx_webhook_push_repos_hook_id ON public.webhook_push_repos(hook_id) WHERE hook_id IS NOT NULL;"
            "DROP TABLE IF EXISTS public.webhook_push_deliveries CASCADE; CREATE TABLE public.webhook_push_deliveries ("
            "delivery_id text PRIMARY KEY, hook_id bigint, repo_id bigint, event text NOT NULL,"
            "action text, received_at text NOT NULL, queued_task_id text,"
            "processing_state text NOT NULL DEFAULT 'pending', processing_started_at text);",
            database_url,
        )
        sql(
            "INSERT INTO public.users(id, github_user_id, login, created_at, updated_at, is_admin,"
            " include_own_releases, webhook_push_enabled, webhook_push_desired_state,"
            " webhook_push_secret_ciphertext, webhook_push_secret_nonce, webhook_push_callback_key)"
            " VALUES ('seed-hook', 501, 'owner', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 0,"
            f" 1, 1, 'enabled', decode('{ciphertext.hex()}', 'hex'), decode('{nonce.hex()}', 'hex'), '{CALLBACK_KEY}');"
            "INSERT INTO public.reaction_pat_tokens(user_id, token_ciphertext, token_nonce, masked_token,"
            " updated_at, owner_github_user_id, owner_login, webhook_push_allows_private_repos)"
            " VALUES ('seed-hook', decode('00', 'hex'), decode('00', 'hex'), 'masked',"
            " '2026-01-01T00:00:00Z', 501, 'owner', 1);"
            "INSERT INTO public.owned_repo_star_baselines(id, user_id, repo_id, repo_full_name,"
            " initialized_at, updated_at, is_private)"
            " VALUES ('baseline-1', 'seed-hook', 4242, 'owner/repo',"
            " '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 0);"
            "INSERT INTO public.webhook_push_repos(user_id, repo_id, owner_login, repo_name,"
            " repo_full_name, hook_id, callback_url, status, permission_paused, updated_at,"
            " owner_github_user_id)"
            " VALUES ('seed-hook', 4242, 'owner', 'repo', 'owner/repo', 777,"
            " 'http://127.0.0.1/api/webhooks/github/releases?key=synthetic-callback-key',"
            " 'registered', 0, '2026-01-01T00:00:00Z', 501);"
            "DELETE FROM public.github_connections WHERE user_id = 'seed-hook';"
            "INSERT INTO public.github_connections(id, user_id, github_user_id, login,"
            " access_token_ciphertext, access_token_nonce, scopes, linked_at, updated_at)"
            " VALUES ('seed-connection', 'seed-hook', 501, 'owner',"
            f" decode('{token_ciphertext.hex()}', 'hex'), decode('{nonce.hex()}', 'hex'),"
            " 'repo', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
            database_url,
        )

        def start_server(extra_env=None):
            server_env = dict(env)
            server_env.pop("OCTORILL_PG_BOOTSTRAP_ONLY")
            if extra_env:
                server_env.update(extra_env)
            tmp = tempfile.TemporaryDirectory(prefix="octo-rill-webhook-")
            server_log = open(pathlib.Path(tmp.name) / "server.log", "w+")
            server = subprocess.Popen(
                [binary], env=server_env, stdout=server_log, stderr=subprocess.STDOUT
            )
            base = f"http://127.0.0.1:{args.port}"
            for _ in range(100):
                try:
                    urllib.request.urlopen(base + "/health", timeout=1).close()
                    return server, server_log, tmp, base
                except Exception:
                    if server.poll() is not None:
                        server_log.flush()
                        server_log.seek(0)
                        raise RuntimeError(server_log.read()[-4000:])
                    time.sleep(0.1)
            raise RuntimeError("validation server did not become ready")

        def stop_server(server):
            server.send_signal(2)
            server.wait(timeout=15)

        def request(body, delivery="delivery-1", event="release", key=CALLBACK_KEY,
                    signature_value=None, hook_id=str(HOOK_ID), method="POST",
                    omit_delivery=False, base=None):
            headers = {
                "Content-Type": "application/json",
                "X-GitHub-Event": event,
                "X-GitHub-Hook-ID": hook_id,
            }
            if not omit_delivery:
                headers["X-GitHub-Delivery"] = delivery
            if signature_value is not None:
                headers["X-Hub-Signature-256"] = signature_value
            request_obj = urllib.request.Request(
                f"{base}/api/webhooks/github/releases?key={key}",
                data=body,
                headers=headers,
                method=method,
            )
            try:
                with urllib.request.urlopen(request_obj, timeout=10) as response:
                    return response.status, json.load(response)
            except urllib.error.HTTPError as error:
                raw = error.read()
                try:
                    return error.code, json.loads(raw) if raw else {}
                except json.JSONDecodeError:
                    return error.code, {"raw": raw.decode(errors="replace")}

        def post(payload, base, **kwargs):
            body = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
            # Callers may override (or omit) the signature explicitly.
            kwargs.setdefault("signature_value", signature(body))
            return request(body, base=base, **kwargs)

        def delivery_state(delivery_id):
            row = sql(
                "SELECT processing_state, COALESCE(queued_task_id, '') FROM public.webhook_push_deliveries"
                f" WHERE delivery_id = '{delivery_id}'",
                database_url,
            )
            return row.split("|") if row else None

        def queued_tasks():
            rows = sql(
                f"SELECT id FROM public.job_tasks WHERE task_type = '{TASK_TYPE}'"
                " AND source = 'webhook.release' ORDER BY created_at, id",
                database_url,
            )
            return [row for row in rows.splitlines() if row]

        def task_row(task_id):
            return sql(
                "SELECT status, COALESCE(error_message, '') FROM public.job_tasks"
                f" WHERE id = '{task_id}'",
                database_url,
            ).split("|", 1)

        def wait_for_task(task_id, timeout=30):
            deadline = time.time() + timeout
            row = task_row(task_id)
            while time.time() < deadline:
                row = task_row(task_id)
                if row[0] in ("succeeded", "failed", "cancelled"):
                    return row
                time.sleep(0.2)
            raise RuntimeError(f"task {task_id} did not settle: {row}")

        # --- receiver phase: no worker, so the queued task stays observable ---
        process, log, tmp, base = start_server()
        try:
            code, body = post(release_payload(), base, delivery="missing-signature", signature_value=None)
            assert code == 401 and body["error"]["code"] == "invalid_signature", (code, body)
            code, body = post(release_payload(), base, delivery="malformed-signature", signature_value="sha256=abc")
            assert code == 401 and body["error"]["code"] == "invalid_signature", (code, body)
            body_bytes = json.dumps(release_payload()).encode()
            code, body = request(body_bytes, delivery="wrong-signature", signature_value="sha256=" + "00" * 32, base=base)
            assert code == 401 and body["error"]["code"] == "invalid_signature", (code, body)
            code, body = post(release_payload(), base, delivery="unknown-key", key="unknown-callback-key")
            assert code == 200 and body == {"accepted": True, "queued": False, "reason": "unknown_hook"}, (code, body)
            code, body = post(release_payload(), base, delivery="missing-delivery", omit_delivery=True)
            assert code == 400 and body["error"]["code"] == "bad_request", (code, body)
            code, body = post({"zen": "synthetic"}, base, delivery="ping-delivery", event="ping")
            assert code == 200 and body["reason"] == "ping", (code, body)
            code, body = post(b"{not-json", base, delivery="malformed-payload")
            assert code == 400 and body["error"]["code"] == "bad_request", (code, body)

            code, body = post(release_payload(), base, delivery="ignored-event", event="issues")
            assert code == 200 and body == {"accepted": True, "queued": False, "reason": "ignored"}, (code, body)
            assert delivery_state("ignored-event") == ["ignored", ""], delivery_state("ignored-event")

            code, body = post(release_payload(), base, delivery="valid-release")
            assert code == 200 and body == {
                "accepted": True,
                "queued": True,
                "reason": "release_sync_queued",
                "release_id": RELEASE_ID,
            }, (code, body)
            tasks = queued_tasks()
            assert len(tasks) == 1, tasks
            state = delivery_state("valid-release")
            assert state == ["queued", tasks[0]], state
            task_type, status, requested_by, payload_json = sql(
                "SELECT task_type, status, requested_by, payload_json FROM public.job_tasks"
                f" WHERE id = '{tasks[0]}'",
                database_url,
            ).split("|", 3)
            assert task_type == TASK_TYPE, task_type
            assert status == "queued" and requested_by == "seed-hook", (status, requested_by)
            task_payload = json.loads(payload_json)
            assert task_payload["user_id"] == "seed-hook", task_payload
            assert task_payload["repo_id"] == REPO_ID, task_payload
            assert task_payload["repo_full_name"] == REPO_FULL_NAME, task_payload
            assert task_payload["reason"] == "webhook_release", task_payload
            assert task_payload["delivery_id"] == "valid-release", task_payload
            # The receiver must not fall back to the account-wide sync any more.
            assert sql(
                "SELECT COUNT(*) FROM public.job_tasks WHERE task_type = 'sync.releases'",
                database_url,
            ) == "0"

            code, body = post(release_payload(), base, delivery="valid-release")
            assert code == 200 and body == {"accepted": True, "queued": False, "reason": "duplicate"}, (code, body)
            assert len(queued_tasks()) == 1, queued_tasks()
            assert delivery_state("valid-release") == ["queued", tasks[0]]

            code, body = post(release_payload(), base, delivery="second-release")
            assert code == 200 and body["queued"] is True, (code, body)
            assert len(queued_tasks()) == 1, queued_tasks()
            assert delivery_state("second-release") == ["queued", tasks[0]], delivery_state("second-release")

            code, body = post(release_payload(draft=True), base, delivery="draft-release")
            assert code == 200 and body["reason"] == "ignored", (code, body)
            code, body = post(release_payload(action="edited"), base, delivery="edited-release")
            assert code == 200 and body["reason"] == "ignored", (code, body)
            code, body = post(release_payload(repo_id=9999), base, delivery="other-repo")
            assert code == 200 and body["reason"] == "ignored", (code, body)

            code, body = request(b"{}", method="GET", signature_value=signature(b"{}"), base=base)
            assert code == 405, (code, body)
            queued_task_id = tasks[0]
        finally:
            stop_server(process)
            process = None
            log.close()
            log = None
            tmp.cleanup()
            tmp = None

        # --- worker phase: real execution against the local GitHub stub ---
        STUB["requests"].clear()
        STUB["releases"] = [
            github_release("v1.0.0", "synthetic release body v1", "2026-11-20T00:00:00Z")
        ]
        process, log, tmp, base = start_server(
            {
                "OCTORILL_PG_WORKER_ENABLED": "1",
                "OCTORILL_GITHUB_API_BASE": stub_base,
            }
        )
        try:
            status, error_message = wait_for_task(queued_task_id)
            assert status == "succeeded", (status, error_message)
            work_item = sql(
                "SELECT status, last_inserted_count, last_updated_count, last_unchanged_count,"
                " last_pages_fetched, last_stopped_reason, COALESCE(last_new_release_ids_json, ''),"
                " COALESCE(error_text, ''), COALESCE(last_success_at, '')"
                f" FROM public.repo_release_work_items WHERE repo_id = {REPO_ID}",
                database_url,
            ).split("|")
            assert work_item[0] == "succeeded", work_item
            assert work_item[1] == "1" and work_item[2] == "0", work_item
            assert work_item[3] == "0", work_item
            assert work_item[4] == "1", work_item
            assert work_item[5] == "short_page", work_item
            assert json.loads(work_item[6]) == [RELEASE_ID], work_item
            assert work_item[7] == PENDING_MARKER, work_item
            assert work_item[8] != "", work_item

            watcher = sql(
                "SELECT status, user_id, origin, reason, COALESCE(error_text, '')"
                " FROM public.repo_release_watchers"
                f" WHERE task_id = '{queued_task_id}'",
                database_url,
            ).split("|")
            assert watcher[0] == "succeeded", watcher
            assert watcher[1] == "seed-hook" and watcher[2] == "interactive", watcher
            assert watcher[3] == "webhook_release" and watcher[4] == "", watcher

            sync_state = sql(
                "SELECT COALESCE(etag, ''), COALESCE(last_success_at, ''),"
                " COALESCE(backoff_until, ''), COALESCE(last_error_text, ''),"
                " last_inserted_count, last_updated_count, last_stopped_reason"
                f" FROM public.repo_release_sync_state WHERE repo_id = {REPO_ID}",
                database_url,
            ).split("|")
            assert sync_state[0] == STUB["etag"], sync_state
            assert sync_state[1] != "" and sync_state[2] == "" and sync_state[3] == "", sync_state
            assert sync_state[4] == "1" and sync_state[5] == "0", sync_state
            assert sync_state[6] == "short_page", sync_state

            cached = sql(
                "SELECT tag_name, body, is_draft, react_plus1 FROM public.repo_releases"
                f" WHERE release_id = {RELEASE_ID}",
                database_url,
            ).split("|")
            assert cached == ["v1.0.0", "synthetic release body v1", "0", "1"], cached
            assert delivery_state("valid-release") == ["queued", queued_task_id]
            assert len(STUB["requests"]) == 1, STUB["requests"]
            assert STUB["requests"][0]["authorization"] == f"Bearer {ACCESS_TOKEN}", STUB["requests"]
            assert STUB["requests"][0]["if_none_match"] is None, STUB["requests"]

            # A newer release payload updates the cached row instead of
            # reporting a brand new release.
            STUB["etag"] = '"stub-releases-v2"'
            STUB["releases"] = [
                github_release("v1.0.1", "synthetic release body v2", "2026-11-21T00:00:00Z")
            ]
            code, body = post(release_payload(), base, delivery="delivery-refresh")
            assert code == 200 and body["queued"] is True, (code, body)
            refresh_state = delivery_state("delivery-refresh")
            assert refresh_state[0] == "queued" and refresh_state[1], refresh_state
            refreshed_task_id = refresh_state[1]
            assert refreshed_task_id != queued_task_id, refreshed_task_id
            status, error_message = wait_for_task(refreshed_task_id)
            assert status == "succeeded", (status, error_message)
            work_item = sql(
                "SELECT last_inserted_count, last_updated_count, COALESCE(error_text, ''),"
                " COALESCE(last_new_release_ids_json, '')"
                f" FROM public.repo_release_work_items WHERE repo_id = {REPO_ID}",
                database_url,
            ).split("|")
            assert work_item[0] == "0" and work_item[1] == "1", work_item
            assert work_item[2] == "" and work_item[3] == "[]", work_item
            cached = sql(
                f"SELECT tag_name, body FROM public.repo_releases WHERE release_id = {RELEASE_ID}",
                database_url,
            ).split("|")
            assert cached == ["v1.0.1", "synthetic release body v2"], cached
            assert STUB["requests"][-1]["if_none_match"] == '"stub-releases-v1"', STUB["requests"][-1]

            # An unchanged repository costs one conditional request and no writes.
            code, body = post(release_payload(), base, delivery="delivery-not-modified")
            assert code == 200 and body["queued"] is True, (code, body)
            not_modified_state = delivery_state("delivery-not-modified")
            assert not_modified_state[0] == "queued" and not_modified_state[1], not_modified_state
            not_modified_task_id = not_modified_state[1]
            status, error_message = wait_for_task(not_modified_task_id)
            assert status == "succeeded", (status, error_message)
            not_modified = sql(
                "SELECT last_stopped_reason, COALESCE(last_not_modified_at, '')"
                f" FROM public.repo_release_sync_state WHERE repo_id = {REPO_ID}",
                database_url,
            ).split("|")
            assert not_modified[0] == "not_modified" and not_modified[1] != "", not_modified
            result = json.loads(
                sql(
                    "SELECT COALESCE(result_json, '{}') FROM public.job_tasks"
                    f" WHERE id = '{not_modified_task_id}'",
                    database_url,
                )
            )
            assert result["not_modified"] is True and result["new_release_ids"] == [], result
            assert STUB["requests"][-1]["if_none_match"] == '"stub-releases-v2"', STUB["requests"][-1]

            # Upstream failures are classified and persisted with a retry backoff.
            STUB["fail_status"] = 500
            code, body = post(release_payload(), base, delivery="delivery-failing")
            assert code == 200 and body["queued"] is True, (code, body)
            failing_state = delivery_state("delivery-failing")
            assert failing_state[0] == "queued" and failing_state[1], failing_state
            failing_task_id = failing_state[1]
            status, error_message = wait_for_task(failing_task_id)
            assert status == "failed", (status, error_message)
            assert error_message.startswith("rate_limited:"), error_message
            failure_state = sql(
                "SELECT COALESCE(backoff_until, ''), COALESCE(last_error_text, '')"
                f" FROM public.repo_release_sync_state WHERE repo_id = {REPO_ID}",
                database_url,
            ).split("|")
            assert failure_state[0] != "", failure_state
            assert failure_state[1].startswith("rate_limited:"), failure_state
            failed_item = sql(
                "SELECT status, COALESCE(error_text, '') FROM public.repo_release_work_items"
                f" WHERE repo_id = {REPO_ID}",
                database_url,
            ).split("|")
            assert failed_item[0] == "failed", failed_item
            assert failed_item[1].startswith("rate_limited:"), failed_item
            failed_watcher = sql(
                "SELECT status FROM public.repo_release_watchers"
                f" WHERE task_id = '{failing_task_id}'",
                database_url,
            )
            assert failed_watcher == "failed", failed_watcher

            # The persisted backoff window must be honoured: a delivery that
            # arrives during it waits in the queue instead of being dropped.
            STUB["fail_status"] = 0
            requests_before = len(STUB["requests"])
            code, body = post(release_payload(), base, delivery="delivery-backed-off")
            assert code == 200 and body["queued"] is True, (code, body)
            backed_off_task = sql(
                "SELECT status || '|' || COALESCE(available_at::text, '') FROM public.job_tasks"
                " WHERE task_type = 'sync.repo_release'"
                " AND payload_json LIKE '%delivery-backed-off%'",
                database_url,
            ).split("|")
            assert backed_off_task[0] == "queued", backed_off_task
            assert backed_off_task[1] != "", backed_off_task
            time.sleep(1.5)
            assert len(STUB["requests"]) == requests_before, STUB["requests"][requests_before:]
            still_queued = sql(
                "SELECT status FROM public.job_tasks"
                " WHERE payload_json LIKE '%delivery-backed-off%'",
                database_url,
            )
            assert still_queued == "queued", still_queued
            # Once the window elapses the deferred delivery is executed.
            sql(
                "UPDATE public.job_tasks SET available_at = now()"
                " WHERE payload_json LIKE '%delivery-backed-off%'",
                database_url,
            )
            backed_off_task_id = sql(
                "SELECT id FROM public.job_tasks"
                " WHERE payload_json LIKE '%delivery-backed-off%'",
                database_url,
            )
            status, error_message = wait_for_task(backed_off_task_id)
            assert status == "succeeded", (status, error_message)
            deferred_release = sql(
                f"SELECT COALESCE(result_json, '{{}}') FROM public.job_tasks"
                f" WHERE id = '{backed_off_task_id}'",
                database_url,
            )
            assert json.loads(deferred_release).get("repo_id") == REPO_ID, deferred_release
            assert len(STUB["requests"]) > requests_before, "deferred refresh must reach GitHub"

            # A completed manage pass must acknowledge its reconcile demand;
            # otherwise the scheduler re-enqueues it on every tick forever.
            sql(
                "INSERT INTO public.users(id, github_user_id, login, created_at, updated_at,"
                " is_admin, include_own_releases, webhook_push_enabled, webhook_push_desired_state,"
                " webhook_push_secret_ciphertext, webhook_push_secret_nonce,"
                " webhook_push_callback_key)"
                " VALUES ('seed-ack', 502, 'ack-owner', '2026-01-01T00:00:00Z',"
                " '2026-01-01T00:00:00Z', 0, 1, 1, 'enabled',"
                f" decode('{ciphertext.hex()}', 'hex'), decode('{nonce.hex()}', 'hex'),"
                " 'synthetic-ack-key');"
                "INSERT INTO public.reaction_pat_tokens(user_id, token_ciphertext, token_nonce,"
                " masked_token, updated_at, owner_github_user_id, owner_login,"
                " webhook_push_allows_private_repos)"
                f" VALUES ('seed-ack', decode('{token_ciphertext.hex()}', 'hex'),"
                f" decode('{nonce.hex()}', 'hex'), 'masked', '2026-01-01T00:00:00Z', 502, 'ack-owner', 1);"
                "INSERT INTO public.webhook_push_reconcile_demands(user_id,"
                " requested_generation, completed_generation, requested_at, updated_at)"
                " VALUES ('seed-ack', 3, 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');"
                "INSERT INTO public.job_tasks(id, task_type, status, source, requested_by,"
                " payload_json, cancel_requested, created_at, updated_at)"
                " VALUES ('task-reconcile-ack', 'webhook.push.manage', 'queued', 'validation',"
                " 'seed-ack', '{\"user_id\":\"seed-ack\",\"operation\":\"reconcile\","
                "\"repo_id\":null,\"demand_generation\":3,\"retry_repo_ids\":[],"
                "\"scheduled\":false,\"retry_count\":0}', 0, now(), now());",
                database_url,
            )
            status, error_message = wait_for_task("task-reconcile-ack")
            assert status == "succeeded", (status, error_message)
            demand = sql(
                "SELECT requested_generation || '|' || completed_generation"
                " FROM public.webhook_push_reconcile_demands WHERE user_id = 'seed-ack'",
                database_url,
            )
            assert demand == "3|3", demand
            completed_at = sql(
                "SELECT COALESCE(webhook_push_last_completed_check_at, '')"
                " FROM public.users WHERE id = 'seed-ack'",
                database_url,
            )
            assert completed_at != "", completed_at

            print(
                json.dumps(
                    {
                        "reconcile_generation_acknowledged": "pass",
                        "signature_verification": "pass",
                        "unknown_key_acknowledged": "pass",
                        "payload_validation": "pass",
                        "ignored_events": "pass",
                        "delivery_idempotency": "pass",
                        "repo_release_task_contract": "pass",
                        "repo_release_single_flight": "pass",
                        "repo_release_worker_persistence": "pass",
                        "repo_release_conditional_refresh": "pass",
                        "repo_release_failure_classification": "pass",
                        "repo_release_backoff_honoured": "pass",
                    }
                )
            )
        finally:
            STUB["fail_status"] = 0
            stop_server(process)
            process = None
            log.close()
            log = None
            tmp.cleanup()
            tmp = None
    finally:
        if process is not None:
            process.send_signal(2)
            process.wait(timeout=15)
        stub.shutdown()
        stub.server_close()
        sql("DROP DATABASE " + name + " WITH (FORCE)")


if __name__ == "__main__":
    main()
