# HTTP API contracts

## Changed: `GET /api/admin/jobs/llm/status`

### Response

```json
{
  "scheduler_enabled": true,
  "max_concurrency": 3,
  "available_slots": 1,
  "waiting_calls": 2,
  "in_flight_calls": 2,
  "calls_24h": 120,
  "failed_24h": 8,
  "avg_wait_ms_24h": 341,
  "avg_duration_ms_24h": 2110,
  "last_success_at": "2026-03-09T12:00:00Z",
  "last_failure_at": "2026-03-09T11:59:00Z"
}
```

### Notes

- `max_concurrency` is the configured per-process permit ceiling from `AI_MAX_CONCURRENCY`.
- `available_slots` is the current number of free permits.
- `waiting_calls` counts requests waiting for a permit before an upstream attempt starts.
- `in_flight_calls` counts requests currently holding a permit and talking to the upstream model.
- `scheduler_wait_ms` on call logs keeps the accumulated permit-queue wait time across attempts.


## Changed: `GET /api/admin/jobs/llm/calls`

### Query

- `status`: `all | queued | running | succeeded | failed`
- `source`: optional source filter
- `requested_by`: optional user id filter
- `parent_task_id`: optional parent task filter
- `started_from` / `started_to`: optional RFC3339 timestamps
- `sort`: optional; `created_desc` by default, or `status_grouped` for the main LLM tab
- `page`, `page_size`: pagination

### Notes

- `sort=status_grouped` returns the main LLM list in `running -> queued -> terminal` groups, then `created_at DESC`.
- Omitting `sort` keeps the response reverse-chronological (`created_at DESC, id DESC`), which is what task-detail related-call views use.
- During retry / finalize transitions, the API may temporarily overlay the observable status in memory so readers do not see a released permit paired with a stale `running` row.
