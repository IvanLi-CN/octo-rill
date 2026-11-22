#!/usr/bin/env python3
"""Seed an isolated PG18 database and verify the anonymous GitHub read budget.

Unauthenticated GitHub reads are counted per egress IP (60 per hour), so the
service keeps one process-wide budget for them. The stub GitHub API counts the
requests it receives, which makes the budget observable from the outside. The
budget lives in memory, so the suite uses two service processes to observe it:

* first process — a multi-page repository read is charged per page: three pages
  leave exactly 47 of the 50 local slots, and 47 single-repository reads then
  consume the rest, so the next anonymous read must be deferred without sending
  anything. The deferral is not a failure: `public_repo_release_usage` keeps its
  status and `repo_release_sync_state` keeps its backoff empty, while the work
  item and watcher record `deferred`;
* second process — `X-RateLimit-Remaining` recalibrates the local count: a
  response reporting `3` after one spent slot leaves exactly three readable
  repositories even though the local count was 49;
* below the budget, both lanes behave exactly as before (reads stored, usage
  rows marked `ready`).
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
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pg_seed_common import psql
from urllib.parse import parse_qs, urlsplit, urlunsplit

MULTI_USER = "seed-anon-multi"
BULK_USER = "seed-anon-bulk"
PROBE_USER = "seed-anon-probe"
CAL_USER = "seed-anon-cal"
TAIL_USER = "seed-anon-tail"

MULTI_REPO = 7001
MULTI_FULL_NAME = "anon/multipage"
MULTI_RELEASE_COUNT = 250
MULTI_PAGES = 3

BULK_REPOS = [(7100 + index, f"anon/bulk{index:02d}") for index in range(47)]
PROBE_REPO = 7200
PROBE_FULL_NAME = "anon/probe"
CAL_REPOS = [(7210 + index, f"anon/cal{index:02d}") for index in range(4)]
TAIL_REPOS = [(7300 + index, f"anon/tail{index:02d}") for index in range(3)]
TAIL_SINGLE_REPO = 7400
TAIL_SINGLE_FULL_NAME = "anon/tail-single"

ETAG = '"stub-releases-v1"'
# Slots the local budget starts with; keep in sync with ANONYMOUS_BUDGET_LIMIT.
BUDGET_LIMIT = 50
PINNED_AT = "2026-11-20T00:00:00Z"

STUB = {
    "releases": {},
    "requests": [],
    "rate_limit": {"remaining": None, "reset": None},
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
    """Serve `/repos/{owner}/{repo}/releases`, counting anonymous requests."""

    def do_GET(self):
        parsed = urlsplit(self.path)
        prefix = "/repos/"
        suffix = "/releases"
        if not parsed.path.startswith(prefix) or not parsed.path.endswith(suffix):
            self.respond(404, {"message": "not found"})
            return
        full_name = parsed.path[len(prefix) : -len(suffix)]
        query = parse_qs(parsed.query)
        page = int(query.get("page", ["1"])[0])
        per_page = int(query.get("per_page", ["30"])[0])
        authorization = self.headers.get("Authorization")
        with STUB["lock"]:
            STUB["requests"].append(
                {
                    "repo": full_name,
                    "page": page,
                    "authorization": authorization,
                }
            )
        releases = STUB["releases"].get(full_name)
        if releases is None:
            self.respond(404, {"message": "Not Found"})
            return
        start = (page - 1) * per_page
        extra = {}
        if authorization is None:
            # Only anonymous responses carry the egress-IP allowance.
            rate_limit = STUB["rate_limit"]
            if rate_limit["remaining"] is not None:
                extra["X-RateLimit-Remaining"] = str(rate_limit["remaining"])
            if rate_limit["reset"] is not None:
                extra["X-RateLimit-Reset"] = str(rate_limit["reset"])
        self.respond(200, releases[start : start + per_page], extra)

    def respond(self, status, body, extra_headers=None):
        payload = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        if status == 200:
            self.send_header("ETag", ETAG)
        for name, value in (extra_headers or {}).items():
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--admin-url", required=True)
    parser.add_argument("--binary-dir", type=pathlib.Path, default=pathlib.Path("target/debug"))
    parser.add_argument("--port", type=int, default=3040)
    parser.add_argument("--stub-port", type=int, default=0)
    args = parser.parse_args()

    name = "octo_rill_release_anon_budget_seed_" + str(os.getpid())
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

        def user(user_id, github_user_id, login):
            return (
                "INSERT INTO public.users(id, github_user_id, login, created_at, updated_at,"
                " last_active_at, is_disabled, paused_at)"
                f" VALUES ('{user_id}', {github_user_id}, '{login}', '2026-01-01T00:00:00Z',"
                " '2026-01-01T00:00:00Z', '2026-11-01T00:00:00Z', 0, NULL);"
            )

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

        def usage(repo_id, full_name, status="pending"):
            owner, repo = full_name.split("/", 1)
            return (
                "INSERT INTO public.public_repo_release_usage(id, repo_id, owner_login, repo_name,"
                " full_name, full_name_lower, first_registered_at, last_requested_at,"
                " last_sync_status, access_kind, published_by_user_id, created_at, updated_at)"
                f" VALUES ('usage-{repo_id}', {repo_id}, '{owner}', '{repo}', '{full_name}',"
                f" '{full_name}', '2026-01-01T00:00:00Z', '2026-11-20T00:00:00Z', '{status}',"
                f" 'github_public', NULL, '2026-01-01T00:00:00Z', '{PINNED_AT}');"
            )

        def task(task_id, task_type, payload):
            body = json.dumps(payload).replace("'", "''")
            return (
                "INSERT INTO public.job_tasks(id, task_type, status, source, requested_by,"
                " payload_json, cancel_requested, created_at, updated_at)"
                f" VALUES ('{task_id}', '{task_type}', 'queued', 'validation', '{payload['user_id']}',"
                f" '{body}', 0, now(), now());"
            )

        statements = [
            user(MULTI_USER, 701, "anon-multi"),
            user(BULK_USER, 702, "anon-bulk"),
            user(PROBE_USER, 703, "anon-probe"),
            user(CAL_USER, 704, "anon-cal"),
            user(TAIL_USER, 705, "anon-tail"),
            # Every account holds no GitHub connection: each repository in this
            # suite can only be read anonymously, so the budget alone decides
            # whether a request goes out.
            association(MULTI_USER, MULTI_REPO, MULTI_FULL_NAME),
            usage(MULTI_REPO, MULTI_FULL_NAME),
            association(PROBE_USER, PROBE_REPO, PROBE_FULL_NAME),
            usage(PROBE_REPO, PROBE_FULL_NAME),
            association(MULTI_USER, TAIL_SINGLE_REPO, TAIL_SINGLE_FULL_NAME),
            usage(TAIL_SINGLE_REPO, TAIL_SINGLE_FULL_NAME),
            # Concurrency 4 keeps the account lane realistic without letting the
            # budget boundary depend on request timing.
            "INSERT INTO public.admin_runtime_settings (id, llm_max_concurrency,"
            " translation_general_worker_concurrency, translation_dedicated_worker_concurrency,"
            " created_at, updated_at, repo_release_worker_concurrency)"
            " VALUES (1, 2, 2, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 4)"
            " ON CONFLICT (id) DO UPDATE SET repo_release_worker_concurrency = 4;",
        ]
        for repo_id, full_name in BULK_REPOS:
            statements.append(association(BULK_USER, repo_id, full_name))
            statements.append(usage(repo_id, full_name))
        for repo_id, full_name in CAL_REPOS:
            statements.append(association(CAL_USER, repo_id, full_name))
            statements.append(usage(repo_id, full_name))
        for repo_id, full_name in TAIL_REPOS:
            statements.append(association(TAIL_USER, repo_id, full_name))
            statements.append(usage(repo_id, full_name))
        sql("".join(statements), database_url)

        STUB["releases"][MULTI_FULL_NAME] = [
            github_release(MULTI_FULL_NAME, f"v{index}.0.0", 10_000 + index, "2026-11-18T00:00:00Z")
            for index in range(MULTI_RELEASE_COUNT)
        ]
        for repo_id, full_name in BULK_REPOS:
            STUB["releases"][full_name] = [
                github_release(full_name, "v1.0.0", repo_id * 10, "2026-11-18T00:00:00Z")
            ]
        STUB["releases"][PROBE_FULL_NAME] = [
            github_release(PROBE_FULL_NAME, "v1.0.0", PROBE_REPO * 10, "2026-11-18T00:00:00Z")
        ]
        for repo_id, full_name in CAL_REPOS:
            STUB["releases"][full_name] = [
                github_release(full_name, "v1.0.0", repo_id * 10, "2026-11-18T00:00:00Z")
            ]
        for repo_id, full_name in TAIL_REPOS:
            STUB["releases"][full_name] = [
                github_release(full_name, "v1.0.0", repo_id * 10, "2026-11-18T00:00:00Z")
            ]
        STUB["releases"][TAIL_SINGLE_FULL_NAME] = [
            github_release(
                TAIL_SINGLE_FULL_NAME, "v1.0.0", TAIL_SINGLE_REPO * 10, "2026-11-18T00:00:00Z"
            )
        ]

        server_env = dict(env)
        server_env.pop("OCTORILL_PG_BOOTSTRAP_ONLY")
        server_env.update(
            {
                "OCTORILL_PG_WORKER_ENABLED": "1",
                "OCTORILL_GITHUB_API_BASE": stub_base,
            }
        )
        tmp = tempfile.TemporaryDirectory(prefix="octo-rill-anon-budget-")
        log_path = pathlib.Path(tmp.name) / "server.log"

        def start_server(label):
            nonlocal process, log
            log = open(log_path, "a+")
            log.write(f"===== server {label} =====\n")
            log.flush()
            process = subprocess.Popen(
                [binary], env=server_env, stdout=log, stderr=subprocess.STDOUT
            )
            base = f"http://127.0.0.1:{args.port}"
            for _ in range(100):
                try:
                    urllib.request.urlopen(base + "/health", timeout=1).close()
                    return
                except Exception:
                    if process.poll() is not None:
                        log.flush()
                        log.seek(0)
                        raise RuntimeError(log.read()[-4000:])
                    time.sleep(0.1)
            raise RuntimeError("validation server did not become ready")

        def stop_server():
            nonlocal process, log
            if process is not None and process.poll() is None:
                process.send_signal(2)
                process.wait(timeout=15)
            process = None
            if log is not None:
                log.close()
                log = None

        def task_state(task_id):
            row = sql(
                "SELECT status, COALESCE(result_json, '')"
                f" FROM public.job_tasks WHERE id = '{task_id}'",
                database_url,
            )
            return row.split("|", 1) if row else None

        def wait_for(task_id, timeout=120):
            deadline = time.time() + timeout
            state = task_state(task_id)
            while time.time() < deadline:
                state = task_state(task_id)
                if state and state[0] in ("succeeded", "failed", "cancelled"):
                    return state
                time.sleep(0.2)
            raise RuntimeError(f"task {task_id} did not settle: {state}")

        def run_task(task_id, task_type, payload):
            sql(task(task_id, task_type, payload), database_url)
            return wait_for(task_id)

        def usage_row(repo_id):
            return sql(
                "SELECT COALESCE(last_sync_status, ''), COALESCE(last_sync_error, ''), updated_at"
                f" FROM public.public_repo_release_usage WHERE repo_id = {repo_id}",
                database_url,
            ).split("|")

        def sync_state(repo_id):
            return sql(
                "SELECT COALESCE(last_error_text, ''), COALESCE(backoff_until, '')"
                f" FROM public.repo_release_sync_state WHERE repo_id = {repo_id}",
                database_url,
            )

        def work_item(repo_id):
            row = sql(
                "SELECT status, COALESCE(error_text, ''), last_pages_fetched"
                f" FROM public.repo_release_work_items WHERE repo_id = {repo_id}",
                database_url,
            )
            return row.split("|") if row else None

        def anonymous_requests(full_name=None):
            with STUB["lock"]:
                requests = list(STUB["requests"])
            if full_name is not None:
                requests = [r for r in requests if r["repo"] == full_name]
            return [r for r in requests if r["authorization"] is None]

        # ===== first process: per-page charging and an exhausted budget ======
        start_server("budget")
        try:
            # --- (a) one multi-page repository is charged per page ------------
            status, result_json = run_task(
                "task-multipage",
                "sync.repo_release",
                {
                    "user_id": MULTI_USER,
                    "repo_id": MULTI_REPO,
                    "repo_full_name": MULTI_FULL_NAME,
                    "reason": "anon_budget_probe",
                },
            )
            assert status == "succeeded", (status, result_json)
            multipage = json.loads(result_json)
            assert multipage["via"] == "public", multipage
            assert multipage["pages_fetched"] == MULTI_PAGES, multipage
            assert multipage["stopped_reason"] == "short_page", multipage
            assert multipage["fetched_count"] == MULTI_RELEASE_COUNT, multipage
            assert not multipage.get("deferred_budget"), multipage
            assert len(anonymous_requests(MULTI_FULL_NAME)) == MULTI_PAGES, anonymous_requests(
                MULTI_FULL_NAME
            )
            assert sql(
                "SELECT COUNT(*) FROM public.repo_releases WHERE repo_id = " + str(MULTI_REPO),
                database_url,
            ) == str(MULTI_RELEASE_COUNT)
            assert usage_row(MULTI_REPO)[:2] == ["ready", ""], usage_row(MULTI_REPO)
            multi_item = work_item(MULTI_REPO)
            assert multi_item[0] == "succeeded", multi_item
            assert multi_item[2] == str(MULTI_PAGES), multi_item

            # --- (b) the remaining slots are spent one page at a time ---------
            # 50 slots - 3 pages = 47, so all 47 single-page repositories fit
            # and the anonymous allowance is exactly gone afterwards. Had the
            # multi-page read been charged once, two slots would remain and the
            # deferred read below would have succeeded instead.
            status, result_json = run_task("task-bulk", "sync.releases", {"user_id": BULK_USER})
            assert status == "succeeded", (status, result_json)
            bulk = json.loads(result_json)
            expected_bulk_reads = BUDGET_LIMIT - MULTI_PAGES
            assert bulk["repositories"] == expected_bulk_reads, bulk
            assert bulk["releases"] == expected_bulk_reads, bulk
            assert bulk["failed_repositories"] == [], bulk
            assert bulk["deferred_repositories"] == [], bulk
            bulk_anonymous = sum(len(anonymous_requests(full_name)) for _, full_name in BULK_REPOS)
            assert bulk_anonymous == expected_bulk_reads, bulk_anonymous
            assert len(anonymous_requests()) == BUDGET_LIMIT, len(anonymous_requests())

            # --- (c) a spent budget defers without a request or a failure -----
            status, result_json = run_task(
                "task-tail",
                "sync.releases",
                {"user_id": TAIL_USER},
            )
            assert status == "succeeded", (status, result_json)
            tail = json.loads(result_json)
            assert tail["repositories"] == 0, tail
            assert tail["releases"] == 0, tail
            assert tail["failed_repositories"] == [], tail
            assert len(tail["deferred_repositories"]) == len(TAIL_REPOS), tail
            for entry in tail["deferred_repositories"]:
                assert entry["reason_code"] == "deferred_budget", entry
                assert entry["anonymous_budget_remaining"] == 0, entry
            for repo_id, full_name in TAIL_REPOS:
                assert anonymous_requests(full_name) == [], full_name
                assert usage_row(repo_id)[:2] == ["pending", ""], usage_row(repo_id)
                assert usage_row(repo_id)[2] == PINNED_AT, usage_row(repo_id)

            status, result_json = run_task(
                "task-tail-single",
                "sync.repo_release",
                {
                    "user_id": MULTI_USER,
                    "repo_id": TAIL_SINGLE_REPO,
                    "repo_full_name": TAIL_SINGLE_FULL_NAME,
                    "reason": "anon_budget_probe",
                },
            )
            assert status == "succeeded", (status, result_json)
            single = json.loads(result_json)
            assert single["deferred_budget"] is True, single
            assert single["anonymous_budget_remaining"] == 0, single
            assert anonymous_requests(TAIL_SINGLE_FULL_NAME) == [], TAIL_SINGLE_FULL_NAME
            # Deferral is not a failure: the usage row and the sync state keep
            # their last values, and only the work item names the deferral.
            assert usage_row(TAIL_SINGLE_REPO)[:2] == ["pending", ""], usage_row(TAIL_SINGLE_REPO)
            assert usage_row(TAIL_SINGLE_REPO)[2] == PINNED_AT, usage_row(TAIL_SINGLE_REPO)
            assert sync_state(TAIL_SINGLE_REPO) == "", sync_state(TAIL_SINGLE_REPO)
            deferred_item = work_item(TAIL_SINGLE_REPO)
            assert deferred_item[0] == "deferred", deferred_item
            assert deferred_item[1].startswith("deferred_budget:"), deferred_item
            watcher = sql(
                "SELECT status, COALESCE(error_text, '') FROM public.repo_release_watchers"
                " WHERE task_id = 'task-tail-single'",
                database_url,
            ).split("|")
            assert watcher[0] == "deferred", watcher
            assert watcher[1].startswith("deferred_budget:"), watcher
            assert sql(
                "SELECT COUNT(*) FROM public.repo_releases WHERE repo_id = " + str(TAIL_SINGLE_REPO),
                database_url,
            ) == "0"
            assert len(anonymous_requests()) == BUDGET_LIMIT, len(anonymous_requests())
        finally:
            stop_server()

        # ===== second process: response headers recalibrate the budget ======
        STUB["rate_limit"] = {
            "remaining": BUDGET_LIMIT - 47,
            "reset": int(time.time()) + 3_600,
        }
        start_server("calibration")
        try:
            status, result_json = run_task("task-probe", "sync.releases", {"user_id": PROBE_USER})
            assert status == "succeeded", (status, result_json)
            probe = json.loads(result_json)
            assert probe["repositories"] == 1, probe
            assert probe["deferred_repositories"] == [], probe
            assert len(anonymous_requests(PROBE_FULL_NAME)) == 1, anonymous_requests(
                PROBE_FULL_NAME
            )
            assert usage_row(PROBE_REPO)[:2] == ["ready", ""], usage_row(PROBE_REPO)

            # The local count held 49 slots; the response said 3, so exactly
            # three of the four calibrated repositories may still be read.
            status, result_json = run_task("task-cal", "sync.releases", {"user_id": CAL_USER})
            assert status == "succeeded", (status, result_json)
            calibrated = json.loads(result_json)
            assert calibrated["repositories"] == BUDGET_LIMIT - 47, calibrated
            assert calibrated["failed_repositories"] == [], calibrated
            assert len(calibrated["deferred_repositories"]) == len(CAL_REPOS) - (
                BUDGET_LIMIT - 47
            ), calibrated
            for entry in calibrated["deferred_repositories"]:
                assert entry["reason_code"] == "deferred_budget", entry
            calibrated_reads = sum(
                len(anonymous_requests(full_name)) for _, full_name in CAL_REPOS
            )
            assert calibrated_reads == BUDGET_LIMIT - 47, calibrated_reads

            with STUB["lock"]:
                all_requests = list(STUB["requests"])
            assert all(request["authorization"] is None for request in all_requests), all_requests[:5]

            log_text = log_path.read_text()
            assert "anonymous GitHub read deferred: budget exhausted" in log_text
            assert '"deferred_reason":"anonymous_budget"' in log_text

            print(
                json.dumps(
                    {
                        "per_page_charging": "pass",
                        "budget_calibration": "pass",
                        "budget_deferral": "pass",
                        "usage_status_untouched": "pass",
                        "zero_budget_sends_nothing": "pass",
                        "deferral_logged": "pass",
                    }
                )
            )
        finally:
            stop_server()
    finally:
        if process is not None and process.poll() is None:
            process.send_signal(2)
            process.wait(timeout=15)
        if log is not None:
            log.close()
        if tmp is not None:
            tmp.cleanup()
        stub.shutdown()
        stub.server_close()
        sql("DROP DATABASE " + name + " WITH (FORCE)")


if __name__ == "__main__":
    main()
