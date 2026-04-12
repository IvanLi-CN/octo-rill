# HTTP / Job contract

## `GET /api/me`

Response shape:

```json
{
  "user": {
    "id": "usr_xxx",
    "github_user_id": 30215105,
    "login": "IvanLi-CN",
    "name": "Ivan Li",
    "avatar_url": null,
    "email": null,
    "is_admin": true
  },
  "access_sync": {
    "task_id": "task_xxx",
    "task_type": "sync.access_refresh",
    "event_path": "/api/tasks/task_xxx/events",
    "reason": "first_visit"
  }
}
```

Notes:

- `access_sync.reason` uses `first_visit | inactive_over_1h | reused_inflight | none`.
- `task_id / task_type / event_path` are `null` when `reason=none`.
- `GET /api/me` decides whether to create or reuse `sync.access_refresh` by comparing the pre-touch `last_active_at` against a 1 hour inactivity window.

## `GET /api/tasks/{task_id}/events`

Behavior:

- Authenticated user only.
- Only the task owner (`job_tasks.requested_by`) can subscribe.
- Returns task events as SSE.

Event payload contract:

- `task.running`
- `task.progress`
  - `stage=star_refreshed`
  - `stage=release_attached`
  - `stage=release_summary`
  - `stage=social_summary`
  - `stage=notifications_summary`
- `task.completed`

## `POST /api/sync/all`

Behavior:

- Authenticated user only.
- `return_mode=task_id|sse` enqueues `sync.access_refresh`.
- `return_mode=sync` runs `sync.starred + sync.releases` inline.
- `sync.access_refresh` covers `Star + Release + social + Inbox`.
- `return_mode=sync` remains the legacy inline path; owner-facing Dashboard and scheduler flows use task-based sync so progress can be observed.

## `POST /api/sync/releases`

Behavior change:

- Still user-scoped.
- No longer performs per-user GitHub Release fan-out writes.
- Instead it attaches the current user's starred repos to the shared repo release queue and waits for shared outcomes.

Result shape:

```json
{
  "repos": 42,
  "releases": 133
}
```

Notes:

- `repos` is the number of visible starred repos attached to shared release demand.
- `releases` is the summed shared release count from the satisfied work items.

## `sync.access_refresh` task payload / result

Payload (`job_tasks.payload_json`):

```json
{
  "user_id": "usr_xxx"
}
```

Result (`job_tasks.result_json`):

```json
{
  "starred": {
    "repos": 42
  },
  "release": {
    "repos": 42,
    "releases": 133,
    "reused_running": 6,
    "reused_fresh": 19,
    "queued": 17,
    "failed": 0
  },
  "social": {
    "repo_stars": 48,
    "followers": 19,
    "events": 67
  },
  "social_error": null,
  "notifications": {
    "notifications": 192,
    "since": "2026-03-06T14:20:00Z"
  },
  "notifications_error": null
}
```

Notes:

- `social_error` and `notifications_error` are optional best-effort error strings.
- A transient social / Inbox failure does not fail `sync.access_refresh`; the task still completes with the successful Star / Release data it already collected.

## `sync.subscriptions` task payload / result

Payload (`job_tasks.payload_json`):

```json
{
  "trigger": "schedule",
  "schedule_key": "2026-03-06T14:30"
}
```

Result (`job_tasks.result_json`):

```json
{
  "skipped": false,
  "skip_reason": null,
  "star": {
    "total_users": 12,
    "succeeded_users": 11,
    "failed_users": 1,
    "total_repos": 340
  },
  "release": {
    "total_repos": 128,
    "succeeded_repos": 123,
    "failed_repos": 5,
    "candidate_failures": 7
  },
  "social": {
    "total_users": 11,
    "succeeded_users": 9,
    "failed_users": 2,
    "repo_stars": 48,
    "followers": 19,
    "events": 67
  },
  "notifications": {
    "total_users": 11,
    "succeeded_users": 10,
    "failed_users": 1,
    "notifications": 192
  },
  "releases_written": 1840,
  "critical_events": 6
}
```

Updated semantics:

- `release` summary now reflects linked shared repo release work items instead of inline repo fetch fan-out.
- `releases_written` now represents shared release rows observed from the satisfied repo work items.
- `social` summarizes post-release social activity refresh across all Star-succeeded users.
- `notifications` summarizes post-release Inbox refresh across all Star-succeeded users.
- `sync.subscriptions` still completes as a single task even when individual social / Inbox users fail; those failures are surfaced through `sync_subscription_events`, `recent_events`, and partial business outcome diagnostics.

## `GET /api/admin/jobs/realtime/{task_id}`

Response changes for `task_type=sync.subscriptions` remain under `diagnostics.sync_subscriptions`.

Notes:

- Existing admin diagnostics stay compatible while adding `social` and `notifications` sections.
- `sync.access_refresh` currently reuses generic task detail rendering.

## `GET /api/admin/jobs/realtime/{task_id}/log`

Behavior:

- Admin only.
- Returns the task run log as downloadable `application/x-ndjson`.
- `404 not_found` when the task does not exist or has no log file.
