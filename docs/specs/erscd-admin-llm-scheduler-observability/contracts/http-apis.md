# HTTP API contracts

## New: `GET /api/admin/jobs/llm/status`

### Response

```json
{
  "scheduler_enabled": true,
  "request_interval_ms": 1000,
  "waiting_calls": 0,
  "in_flight_calls": 1,
  "next_slot_in_ms": 620,
  "calls_24h": 120,
  "failed_24h": 8,
  "avg_wait_ms_24h": 341,
  "avg_duration_ms_24h": 2110,
  "last_success_at": "2026-02-27T12:00:00Z",
  "last_failure_at": "2026-02-27T11:59:00Z"
}
```

## New: `GET /api/admin/jobs/llm/calls`

### Query params

- `status=all|queued|running|succeeded|failed` (default `all`)
- `source` (optional)
- `requested_by` (optional integer)
- `started_from` (optional RFC3339 UTC)
- `started_to` (optional RFC3339 UTC)
- `page` (default `1`)
- `page_size` (default `20`, max `100`)

### Response

```json
{
  "items": [
    {
      "id": "3a0f5147-4cb1-4f0e-a2c4-67cb4d70d2f5",
      "status": "succeeded",
      "source": "api.translate_releases_batch",
      "model": "gpt-4o-mini",
      "requested_by": 1,
      "parent_task_id": null,
      "parent_task_type": null,
      "max_tokens": 900,
      "attempt_count": 1,
      "scheduler_wait_ms": 123,
      "duration_ms": 780,
      "created_at": "2026-02-27T12:00:00Z",
      "started_at": "2026-02-27T12:00:00Z",
      "finished_at": "2026-02-27T12:00:01Z",
      "updated_at": "2026-02-27T12:00:01Z"
    }
  ],
  "page": 1,
  "page_size": 20,
  "total": 1
}
```

## New: `GET /api/admin/jobs/llm/calls/{call_id}`

### Response

```json
{
  "id": "3a0f5147-4cb1-4f0e-a2c4-67cb4d70d2f5",
  "status": "failed",
  "source": "job.translate.release",
  "model": "gpt-4o-mini",
  "requested_by": 1,
  "parent_task_id": "d5bf4a8b-8fc3-4d6d-af68-8b34db732457",
  "parent_task_type": "translate.release",
  "max_tokens": 900,
  "attempt_count": 4,
  "scheduler_wait_ms": 3102,
  "duration_ms": 5098,
  "prompt_text": "full prompt ...",
  "response_text": null,
  "error_text": "AI returned 429: rate limited",
  "created_at": "2026-02-27T12:00:00Z",
  "started_at": "2026-02-27T12:00:01Z",
  "finished_at": "2026-02-27T12:00:06Z",
  "updated_at": "2026-02-27T12:00:06Z"
}
```

## Error codes

- `forbidden_admin_only` (`403`): caller is not admin.
- `not_found` (`404`): call id does not exist.
- `bad_request` (`400`): invalid filter values.
