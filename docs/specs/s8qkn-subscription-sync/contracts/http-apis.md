# HTTP / Job contract

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
  "releases_written": 1840,
  "critical_events": 6
}
```

Semantics:

- `skipped=true` means the run record exists but the body did not execute because a previous run was still active.
- `skip_reason` currently uses `previous_run_active`.

## `GET /api/admin/jobs/realtime`

Query changes:

- Add `task_group=scheduled|realtime|all`.
- `scheduled` includes `brief.daily_slot` + `sync.subscriptions`.
- `realtime` excludes those scheduled task types.

## `GET /api/admin/jobs/realtime/{task_id}`

Response changes for `task_type=sync.subscriptions`:

- `diagnostics.sync_subscriptions`
  - `trigger`
  - `schedule_key`
  - `skipped`
  - `skip_reason`
  - `log_available`
  - `log_download_path`
  - `star` summary
  - `release` summary
  - `recent_events[]`

## `GET /api/admin/jobs/realtime/{task_id}/log`

Behavior:

- Admin only.
- Returns the task run log as downloadable `application/x-ndjson`.
- `404 not_found` when the task does not exist or has no log file.
