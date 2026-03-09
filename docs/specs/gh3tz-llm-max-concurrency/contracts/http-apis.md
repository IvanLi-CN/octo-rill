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
