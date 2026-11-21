#!/usr/bin/env python3
"""Seed an isolated PG18 database and verify repository-release credential fallback.

The stub GitHub API serves public repositories to anonymous callers and private
repositories only to the tokens named for them, which makes the three access
paths observable:

* the payload user cannot read the repository but a follower token can
  (candidate rotation, `via=candidate`);
* only a `github_public` usage row exists and nobody holds a usable token
  (unauthenticated fallback, `via=public`);
* a private repository with no candidate and no public row fails with the
  original upstream classification (`repo_inaccessible`).

`public_repo_release_usage.last_sync_status` is asserted for every outcome, and
the account-level `sync.releases` lane must keep going when one of the user's
visible repositories cannot be read.
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
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pg_seed_common import psql
from urllib.parse import urlsplit, urlunsplit

PAYLOAD_USER = "seed-payload"
FOLLOWER_USER = "seed-follower"
ACCOUNT_USER = "seed-account"
PACE_USER = "seed-pace"
PACE_REPOS = [
    (5000 + index, f"public/pace{index:02d}") for index in range(8)
]
PAYLOAD_TOKEN = "ghs_payload_stale_token"
FOLLOWER_TOKEN = "ghs_follower_valid_token"

ALPHA_REPO = 4242
ALPHA_FULL_NAME = "follow/alpha"
BETA_REPO = 4343
BETA_FULL_NAME = "public/beta"
GAMMA_REPO = 4444
GAMMA_FULL_NAME = "private/gamma"
DELTA_REPO = 4545
DELTA_FULL_NAME = "private/delta"
EPSILON_REPO = 4646
EPSILON_FULL_NAME = "public/epsilon"

ETAG = '"stub-releases-v1"'

STUB = {
    "public": {BETA_FULL_NAME},
    "tokens": {
        ALPHA_FULL_NAME: {FOLLOWER_TOKEN},
        BETA_FULL_NAME: set(),
        GAMMA_FULL_NAME: set(),
        DELTA_FULL_NAME: set(),
    },
    "releases": {},
    "requests": [],
    # Concurrency probe: every `/pace` read sleeps so overlapping requests show
    # up as `max_in_flight`.
    # Repositories whose authenticated read is throttled; the anonymous read
    # must still be attempted.
    "rate_limited": set(),
    "delay_seconds": 0.25,
    "in_flight": 0,
    "max_in_flight": 0,
    "lock": threading.Lock(),
}


def github_release(repo_full_name, tag, release_id, updated_at):
    return {
        "id": release_id,
        "node_id": "N_" + str(release_id),
        "tag_name": tag,
        "name": tag,
        "body": f"synthetic release body for {repo_full_name}",
        "html_url": f"https://github.com/{repo_full_name}/releases/tag/{tag}",
        "published_at": updated_at,
        "created_at": updated_at,
        "updated_at": updated_at,
        "draft": False,
        "prerelease": False,
        "reactions": {"+1": 1, "laugh": 0, "heart": 0, "hooray": 0, "rocket": 0, "eyes": 0},
    }


class StubGithubHandler(BaseHTTPRequestHandler):
    """Serve `/repos/{owner}/{repo}/releases` with token-gated private reads."""

    def do_GET(self):
        parsed = urlsplit(self.path)
        prefix = "/repos/"
        suffix = "/releases"
        if not parsed.path.startswith(prefix) or not parsed.path.endswith(suffix):
            self.respond(404, {"message": "not found"})
            return
        full_name = parsed.path[len(prefix) : -len(suffix)]
        authorization = self.headers.get("Authorization")
        token = authorization[len("Bearer ") :] if authorization else None
        STUB["requests"].append(
            {
                "repo": full_name,
                "path": self.path,
                "authorization": authorization,
                "if_none_match": self.headers.get("If-None-Match"),
            }
        )
        if full_name in STUB["rate_limited"] and authorization:
            self.send_response(403)
            self.send_header("Content-Type", "application/json")
            self.send_header("X-RateLimit-Remaining", "0")
            self.send_header("X-RateLimit-Reset", "9999999999")
            self.end_headers()
            self.wfile.write(b'{"message":"API rate limit exceeded"}')
            return
        with STUB["lock"]:
            STUB["in_flight"] += 1
            STUB["max_in_flight"] = max(STUB["max_in_flight"], STUB["in_flight"])
        try:
            if STUB["delay_seconds"] and full_name.startswith("public/pace"):
                time.sleep(STUB["delay_seconds"])
            if full_name not in STUB["tokens"]:
                self.respond(404, {"message": "Not Found"})
                return
            if full_name not in STUB["public"] and token not in STUB["tokens"][full_name]:
                # GitHub answers 404 for a private repository the token cannot see.
                self.respond(404, {"message": "Not Found"})
                return
            self.respond(200, STUB["releases"].get(full_name, []))
        finally:
            with STUB["lock"]:
                STUB["in_flight"] -= 1

    def respond(self, status, body):
        payload = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        if status == 200:
            self.send_header("ETag", ETAG)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--admin-url", required=True)
    parser.add_argument("--binary-dir", type=pathlib.Path, default=pathlib.Path("target/debug"))
    parser.add_argument("--port", type=int, default=3033)
    parser.add_argument("--stub-port", type=int, default=0)
    args = parser.parse_args()

    name = "octo_rill_release_fallback_seed_" + str(os.getpid())
    database_url = urlunsplit(urlsplit(args.admin_url)._replace(path="/" + name))

    def sql(query, database=args.admin_url):
        return psql(database, query)

    stub = ThreadingHTTPServer(("127.0.0.1", args.stub_port), StubGithubHandler)
    threading.Thread(target=stub.serve_forever, daemon=True).start()
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
        payload_ciphertext = AESGCM(key).encrypt(nonce, PAYLOAD_TOKEN.encode(), None)
        follower_ciphertext = AESGCM(key).encrypt(nonce, FOLLOWER_TOKEN.encode(), None)

        def association(user_id, repo_id, full_name):
            owner, repo = full_name.split("/", 1)
            return (
                "INSERT INTO public.user_repo_associations(id, user_id, repo_id, repo_full_name,"
                " repo_full_name_lower, owner_login, repo_name, html_url, is_private,"
                " first_source, first_associated_at, last_seen_at, is_following, created_at, updated_at)"
                f" VALUES ('{user_id}-{repo_id}', '{user_id}', {repo_id}, '{full_name}',"
                f" '{full_name}', '{owner}', '{repo}', 'https://github.com/{full_name}', 0,"
                " 'manual_feed', '2026-01-01T00:00:00Z', '2026-11-20T00:00:00Z', 1,"
                " '2026-01-01T00:00:00Z', '2026-11-20T00:00:00Z');"
            )

        def usage(repo_id, full_name, access_kind, published_by=None, status="pending"):
            owner, repo = full_name.split("/", 1)
            published = "NULL" if published_by is None else f"'{published_by}'"
            return (
                "INSERT INTO public.public_repo_release_usage(id, repo_id, owner_login, repo_name,"
                " full_name, full_name_lower, first_registered_at, last_requested_at,"
                " last_sync_status, access_kind, published_by_user_id, created_at, updated_at)"
                f" VALUES ('usage-{repo_id}', {repo_id}, '{owner}', '{repo}', '{full_name}',"
                f" '{full_name}', '2026-01-01T00:00:00Z', '2026-11-20T00:00:00Z', '{status}',"
                f" '{access_kind}', {published}, '2026-01-01T00:00:00Z', '2026-11-20T00:00:00Z');"
            )

        def task(task_id, task_type, payload):
            body = json.dumps(payload).replace("'", "''")
            return (
                "INSERT INTO public.job_tasks(id, task_type, status, source, requested_by,"
                " payload_json, cancel_requested, created_at, updated_at)"
                f" VALUES ('{task_id}', '{task_type}', 'queued', 'validation', '{PAYLOAD_USER}',"
                f" '{body}', 0, now(), now());"
            )

        statements = [
            "INSERT INTO public.users(id, github_user_id, login, created_at, updated_at,"
            " last_active_at, is_disabled, paused_at)"
            f" VALUES ('{PAYLOAD_USER}', 601, 'payload-user', '2026-01-01T00:00:00Z',"
            " '2026-01-01T00:00:00Z', '2026-11-20T00:00:00Z', 0, NULL);",
            "INSERT INTO public.users(id, github_user_id, login, created_at, updated_at,"
            " last_active_at, is_disabled, paused_at)"
            f" VALUES ('{FOLLOWER_USER}', 602, 'follower-user', '2026-01-01T00:00:00Z',"
            " '2026-01-01T00:00:00Z', '2026-11-10T00:00:00Z', 0, NULL);",
            # The account lane viewer holds no GitHub connection at all, which
            # used to fail the whole task before the shared decision existed.
            "INSERT INTO public.users(id, github_user_id, login, created_at, updated_at,"
            " last_active_at, is_disabled, paused_at)"
            f" VALUES ('{ACCOUNT_USER}', 603, 'account-user', '2026-01-01T00:00:00Z',"
            " '2026-01-01T00:00:00Z', '2026-11-01T00:00:00Z', 0, NULL);",
            "INSERT INTO public.github_connections(id, user_id, github_user_id, login,"
            " access_token_ciphertext, access_token_nonce, scopes, linked_at, updated_at)"
            f" VALUES ('conn-payload', '{PAYLOAD_USER}', 601, 'payload-user',"
            f" decode('{payload_ciphertext.hex()}', 'hex'), decode('{nonce.hex()}', 'hex'),"
            " 'repo', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
            "INSERT INTO public.github_connections(id, user_id, github_user_id, login,"
            " access_token_ciphertext, access_token_nonce, scopes, linked_at, updated_at)"
            f" VALUES ('conn-follower', '{FOLLOWER_USER}', 602, 'follower-user',"
            f" decode('{follower_ciphertext.hex()}', 'hex'), decode('{nonce.hex()}', 'hex'),"
            " 'repo', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
            # The payload user follows alpha (so it is candidate number one and
            # its stale token is tried first) and the two private repositories.
            association(PAYLOAD_USER, ALPHA_REPO, ALPHA_FULL_NAME),
            association(PAYLOAD_USER, GAMMA_REPO, GAMMA_FULL_NAME),
            association(PAYLOAD_USER, DELTA_REPO, DELTA_FULL_NAME),
            # The follower is the only credential that can read alpha.
            association(FOLLOWER_USER, ALPHA_REPO, ALPHA_FULL_NAME),
            # The account viewer sees alpha and the unreadable gamma. beta stays
            # without any candidate user so its public fallback is unambiguous.
            association(ACCOUNT_USER, ALPHA_REPO, ALPHA_FULL_NAME),
            association(ACCOUNT_USER, GAMMA_REPO, GAMMA_FULL_NAME),
            # beta has no candidate user at all: only the public usage row.
            usage(ALPHA_REPO, ALPHA_FULL_NAME, "github_public"),
            usage(BETA_REPO, BETA_FULL_NAME, "github_public"),
            # delta stays registered but nobody can read it any more.
            usage(DELTA_REPO, DELTA_FULL_NAME, "github_public"),
            task(
                "task-alpha",
                "sync.repo_release",
                {
                    "user_id": PAYLOAD_USER,
                    "repo_id": ALPHA_REPO,
                    "repo_full_name": ALPHA_FULL_NAME,
                    "reason": "fallback_probe",
                },
            ),
            task(
                "task-beta",
                "sync.repo_release",
                {
                    "user_id": PAYLOAD_USER,
                    "repo_id": BETA_REPO,
                    "repo_full_name": BETA_FULL_NAME,
                    "reason": "fallback_probe",
                },
            ),
            task(
                "task-gamma",
                "sync.repo_release",
                {
                    "user_id": PAYLOAD_USER,
                    "repo_id": GAMMA_REPO,
                    "repo_full_name": GAMMA_FULL_NAME,
                    "reason": "fallback_probe",
                },
            ),
            task(
                "task-delta",
                "sync.repo_release",
                {
                    "user_id": PAYLOAD_USER,
                    "repo_id": DELTA_REPO,
                    "repo_full_name": DELTA_FULL_NAME,
                    "reason": "fallback_probe",
                },
            ),
            "INSERT INTO public.users(id, github_user_id, login, created_at, updated_at,"
            " last_active_at, is_disabled, paused_at)"
            f" VALUES ('{PACE_USER}', 604, 'pace-user', '2026-01-01T00:00:00Z',"
            " '2026-01-01T00:00:00Z', '2026-11-01T00:00:00Z', 0, NULL);",
            "INSERT INTO public.admin_runtime_settings (id, llm_max_concurrency,"
            " translation_general_worker_concurrency, translation_dedicated_worker_concurrency,"
            " created_at, updated_at, repo_release_worker_concurrency)"
            " VALUES (1, 2, 2, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 4)"
            " ON CONFLICT (id) DO UPDATE SET repo_release_worker_concurrency = 4;",
            "".join(
                usage(repo_id, full_name, "github_public", status="ready")
                for repo_id, full_name in PACE_REPOS
            ),
            "".join(
                association(PACE_USER, repo_id, full_name)
                for repo_id, full_name in PACE_REPOS
            ),
            association(PAYLOAD_USER, EPSILON_REPO, EPSILON_FULL_NAME),
            usage(EPSILON_REPO, EPSILON_FULL_NAME, "github_public", status="ready"),
            task(
                "task-epsilon",
                "sync.repo_release",
                {
                    "user_id": PAYLOAD_USER,
                    "repo_id": EPSILON_REPO,
                    "repo_full_name": EPSILON_FULL_NAME,
                    "reason": "fallback_probe",
                },
            ),
            task("task-account", "sync.releases", {"user_id": ACCOUNT_USER}),
            task("task-pace", "sync.releases", {"user_id": PACE_USER}),
        ]
        sql("".join(statements), database_url)

        STUB["releases"] = {
            ALPHA_FULL_NAME: [
                github_release(ALPHA_FULL_NAME, "v1.0.0", 9001, "2026-11-18T00:00:00Z")
            ],
            BETA_FULL_NAME: [
                github_release(BETA_FULL_NAME, "v2.0.0", 9002, "2026-11-18T00:00:00Z")
            ],
            GAMMA_FULL_NAME: [
                github_release(GAMMA_FULL_NAME, "v3.0.0", 9003, "2026-11-18T00:00:00Z")
            ],
            DELTA_FULL_NAME: [
                github_release(DELTA_FULL_NAME, "v4.0.0", 9004, "2026-11-18T00:00:00Z")
            ],
        }
        STUB["rate_limited"].add(EPSILON_FULL_NAME)
        STUB["public"].add(EPSILON_FULL_NAME)
        STUB["tokens"][EPSILON_FULL_NAME] = set()
        STUB["releases"][EPSILON_FULL_NAME] = [
            github_release(EPSILON_FULL_NAME, "v5.0.0", 9005, "2026-11-18T00:00:00Z")
        ]
        for pace_repo_id, pace_full_name in PACE_REPOS:
            STUB["public"].add(pace_full_name)
            STUB["tokens"][pace_full_name] = set()
            STUB["releases"][pace_full_name] = [
                github_release(pace_full_name, "v1.0.0", pace_repo_id * 10, "2026-11-18T00:00:00Z")
            ]

        server_env = dict(env)
        server_env.pop("OCTORILL_PG_BOOTSTRAP_ONLY")
        server_env.update(
            {
                "OCTORILL_PG_WORKER_ENABLED": "1",
                "OCTORILL_GITHUB_API_BASE": stub_base,
            }
        )
        tmp = tempfile.TemporaryDirectory(prefix="octo-rill-release-fallback-")
        log = open(pathlib.Path(tmp.name) / "server.log", "w+")
        process = subprocess.Popen(
            [binary], env=server_env, stdout=log, stderr=subprocess.STDOUT
        )
        base = f"http://127.0.0.1:{args.port}"
        try:
            for _ in range(100):
                try:
                    urllib.request.urlopen(base + "/health", timeout=1).close()
                    break
                except Exception:
                    if process.poll() is not None:
                        log.flush()
                        log.seek(0)
                        raise RuntimeError(log.read()[-4000:])
                    time.sleep(0.1)
            else:
                raise RuntimeError("validation server did not become ready")

            def task_state(task_id):
                row = sql(
                    "SELECT status, COALESCE(error_message, ''), COALESCE(result_json, '')"
                    f" FROM public.job_tasks WHERE id = '{task_id}'",
                    database_url,
                )
                return row.split("|", 2) if row else None

            def wait_for(task_id, timeout=30):
                deadline = time.time() + timeout
                state = task_state(task_id)
                while time.time() < deadline:
                    state = task_state(task_id)
                    if state and state[0] in ("succeeded", "failed", "cancelled"):
                        return state
                    time.sleep(0.2)
                raise RuntimeError(f"task {task_id} did not settle: {state}")

            def usage_status(repo_id):
                return sql(
                    "SELECT COALESCE(last_sync_status, ''), COALESCE(last_sync_error, '')"
                    f" FROM public.public_repo_release_usage WHERE repo_id = {repo_id}",
                    database_url,
                )

            def work_item(repo_id):
                row = sql(
                    "SELECT status, COALESCE(error_text, ''), last_inserted_count,"
                    " COALESCE(last_stopped_reason, '')"
                    f" FROM public.repo_release_work_items WHERE repo_id = {repo_id}",
                    database_url,
                )
                return row.split("|") if row else None

            def requests_for(full_name):
                return [r for r in STUB["requests"] if r["repo"] == full_name]

            # --- (a) payload user cannot read alpha, the follower can -------
            status, error_message, result_json = wait_for("task-alpha")
            assert status == "succeeded", (status, error_message, result_json)
            result = json.loads(result_json)
            assert result["via"] == "candidate", result
            # Which account's connection served the read is service-side:
            # the requesting user must not learn the rotated account id.
            assert "candidate_user_id" not in result, result
            # The account lane may have cached the release first; what matters
            # is that this refresh really read the repository through the
            # follower's token.
            assert result["fetched_count"] == 1, result
            assert result["not_modified"] is False, result
            alpha_requests = [r["authorization"] for r in requests_for(ALPHA_FULL_NAME)]
            # Every attempt tries the payload user's stale token first and only
            # then the follower's token, so the sequence is made of such pairs
            # regardless of how the single-repository and account lanes interleave.
            assert alpha_requests, alpha_requests
            assert len(alpha_requests) % 2 == 0, alpha_requests
            assert alpha_requests[0::2] == [f"Bearer {PAYLOAD_TOKEN}"] * (
                len(alpha_requests) // 2
            ), alpha_requests
            assert alpha_requests[1::2] == [f"Bearer {FOLLOWER_TOKEN}"] * (
                len(alpha_requests) // 2
            ), alpha_requests
            assert work_item(ALPHA_REPO)[0] == "succeeded", work_item(ALPHA_REPO)
            assert usage_status(ALPHA_REPO).split("|") == ["ready", ""], usage_status(ALPHA_REPO)
            assert sql(
                "SELECT tag_name FROM public.repo_releases WHERE release_id = 9001", database_url
            ) == "v1.0.0"

            # --- (b) only a public usage row remains ------------------------
            status, error_message, result_json = wait_for("task-beta")
            assert status == "succeeded", (status, error_message, result_json)
            result = json.loads(result_json)
            assert result["via"] == "public", result
            assert "candidate_user_id" not in result, result
            beta_requests = requests_for(BETA_FULL_NAME)
            assert beta_requests, "the public repository must still be read once"
            assert all(r["authorization"] is None for r in beta_requests), beta_requests
            assert work_item(BETA_REPO)[0] == "succeeded", work_item(BETA_REPO)
            assert usage_status(BETA_REPO).split("|") == ["ready", ""], usage_status(BETA_REPO)
            assert sql(
                "SELECT tag_name FROM public.repo_releases WHERE release_id = 9002", database_url
            ) == "v2.0.0"

            # --- (c) private repository without candidates or public row ----
            status, error_message, result_json = wait_for("task-gamma")
            assert status == "failed", (status, error_message, result_json)
            assert error_message.startswith("repo_inaccessible:"), error_message
            gamma_item = work_item(GAMMA_REPO)
            assert gamma_item[0] == "failed", gamma_item
            assert gamma_item[1].startswith("repo_inaccessible:"), gamma_item
            gamma_sync_state = sql(
                "SELECT COALESCE(last_error_text, ''), COALESCE(backoff_until, '')"
                f" FROM public.repo_release_sync_state WHERE repo_id = {GAMMA_REPO}",
                database_url,
            )
            assert gamma_sync_state.split("|")[0].startswith("repo_inaccessible:"), gamma_sync_state
            assert gamma_sync_state.split("|")[1] == "", gamma_sync_state
            assert sql(
                "SELECT COUNT(*) FROM public.public_repo_release_usage"
                f" WHERE repo_id = {GAMMA_REPO}",
                database_url,
            ) == "0"
            assert sql(
                "SELECT COUNT(*) FROM public.repo_releases WHERE repo_id = " + str(GAMMA_REPO),
                database_url,
            ) == "0"
            # The token was rejected and no public attempt is allowed: there is
            # no usage row, so the original classification stands.
            gamma_requests = [r["authorization"] for r in requests_for(GAMMA_FULL_NAME)]
            assert gamma_requests, gamma_requests
            assert gamma_requests == [f"Bearer {PAYLOAD_TOKEN}"] * len(gamma_requests), (
                gamma_requests
            )

            # --- (d) public usage row flips to inaccessible -----------------
            status, error_message, _ = wait_for("task-delta")
            assert status == "failed", (status, error_message)
            assert error_message.startswith("repo_inaccessible:"), error_message
            delta_usage = usage_status(DELTA_REPO).split("|", 1)
            assert delta_usage[0] == "inaccessible", delta_usage
            assert delta_usage[1].startswith("repo_inaccessible:"), delta_usage
            delta_requests = [r["authorization"] for r in requests_for(DELTA_FULL_NAME)]
            # Each attempt is the rejected token followed by an anonymous probe.
            assert delta_requests, delta_requests
            assert len(delta_requests) % 2 == 0, delta_requests
            assert delta_requests[0::2] == [f"Bearer {PAYLOAD_TOKEN}"] * (
                len(delta_requests) // 2
            ), delta_requests
            assert delta_requests[1::2] == [None] * (len(delta_requests) // 2), delta_requests

            # --- account lane survives an unreadable repository -------------
            status, error_message, result_json = wait_for("task-account")
            assert status == "succeeded", (status, error_message, result_json)
            account = json.loads(result_json)
            # The viewer holds no connection: alpha is read through the rotated
            # follower token and gamma is reported without failing the task.
            assert account["repositories"] == 1, account
            assert account["releases"] == 1, account
            failed = {entry["repo_full_name"]: entry for entry in account["failed_repositories"]}
            assert set(failed) == {GAMMA_FULL_NAME}, account
            assert failed[GAMMA_FULL_NAME]["reason_code"] == "repo_inaccessible", account

            # --- a throttled token still reaches the public fallback ---------
            status, error_message, result_json = wait_for("task-epsilon")
            assert status == "succeeded", (status, error_message, result_json)
            epsilon = json.loads(result_json)
            assert epsilon["via"] == "public", epsilon
            assert "candidate_user_id" not in epsilon, epsilon
            epsilon_auth = [r["authorization"] for r in requests_for(EPSILON_FULL_NAME)]
            assert epsilon_auth[:2] == [f"Bearer {PAYLOAD_TOKEN}", None], epsilon_auth
            assert usage_status(EPSILON_REPO).split("|")[0] == "ready", usage_status(EPSILON_REPO)

            # --- account lane is bounded, not serial ------------------------
            status, error_message, result_json = wait_for("task-pace")
            assert status == "succeeded", (status, error_message, result_json)
            paced = json.loads(result_json)
            assert paced["repositories"] == len(PACE_REPOS), paced
            assert paced["failed_repositories"] == [], paced
            assert STUB["max_in_flight"] <= 4, STUB["max_in_flight"]
            assert STUB["max_in_flight"] >= 2, STUB["max_in_flight"]
            assert sql(
                "SELECT COUNT(*) FROM public.repo_releases"
                " WHERE repo_id BETWEEN 5000 AND 5007",
                database_url,
            ) == str(len(PACE_REPOS))

            watchers = sql(
                "SELECT task_id, status FROM public.repo_release_watchers"
                " WHERE task_id IN ('task-alpha', 'task-beta', 'task-gamma', 'task-delta')"
                " ORDER BY task_id",
                database_url,
            )
            assert watchers.splitlines() == [
                "task-alpha|succeeded",
                "task-beta|succeeded",
                "task-delta|failed",
                "task-gamma|failed",
            ], watchers

            print(
                json.dumps(
                    {
                        "candidate_rotation": "pass",
                        "public_fallback": "pass",
                        "failure_classification": "pass",
                        "usage_status_transitions": "pass",
                        "account_lane_resilience": "pass",
                        "account_lane_concurrency": "pass",
                        "throttled_token_public_fallback": "pass",
                    }
                )
            )
        finally:
            if process is not None and process.poll() is None:
                process.send_signal(2)
                process.wait(timeout=15)
            if log is not None:
                log.close()
            if tmp is not None:
                tmp.cleanup()
    finally:
        if process is not None and process.poll() is None:
            process.send_signal(2)
            process.wait(timeout=15)
        stub.shutdown()
        stub.server_close()
        sql("DROP DATABASE " + name + " WITH (FORCE)")


if __name__ == "__main__":
    main()
