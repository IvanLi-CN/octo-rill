# HTTP API contracts

## New: `GET /api/admin/users/{user_id}/profile`

### Response

```json
{
  "user_id": 1,
  "daily_brief_utc_time": "08:00",
  "last_active_at": "2026-02-25T12:00:00Z"
}
```

## New: Admin jobs APIs

- `GET /api/admin/jobs/overview`
- `GET /api/admin/jobs/realtime`
- `GET /api/admin/jobs/realtime/{task_id}`
- `POST /api/admin/jobs/realtime/{task_id}/retry`
- `POST /api/admin/jobs/realtime/{task_id}/cancel`
- `GET /api/admin/jobs/scheduled`
- `PATCH /api/admin/jobs/scheduled/{hour_utc}`
- `GET /api/admin/jobs/translations/status`
- `PATCH /api/admin/jobs/translations/runtime-config`
- `GET /api/admin/jobs/translations/requests`
- `GET /api/admin/jobs/translations/requests/{request_id}`
- `GET /api/admin/jobs/translations/batches`
- `GET /api/admin/jobs/translations/batches/{batch_id}`

### `GET /api/admin/jobs/overview` response

```json
{
  "queued": 0,
  "running": 1,
  "failed_24h": 2,
  "succeeded_24h": 8,
  "enabled_scheduled_slots": 24,
  "total_scheduled_slots": 24
}
```

### `GET /api/admin/jobs/realtime` response

```json
{
  "items": [
    {
      "id": "6bc40f0f-27f3-4c67-bf9e-c62a0fbe84c8",
      "task_type": "brief.daily_slot",
      "status": "failed",
      "source": "scheduler",
      "requested_by": null,
      "parent_task_id": null,
      "cancel_requested": false,
      "error_message": "...",
      "created_at": "2026-02-25T00:00:00Z",
      "started_at": "2026-02-25T00:00:01Z",
      "finished_at": "2026-02-25T00:00:05Z",
      "updated_at": "2026-02-25T00:00:05Z"
    }
  ],
  "page": 1,
  "page_size": 20,
  "total": 1
}
```

### `GET /api/admin/jobs/realtime/{task_id}` response

```json
{
  "task": {
    "id": "...",
    "task_type": "translate.release.batch",
    "status": "running",
    "source": "api.translate_release_batch",
    "requested_by": 1,
    "parent_task_id": null,
    "cancel_requested": false,
    "error_message": null,
    "payload_json": "{\"user_id\":1,\"release_ids\":[290836643]}",
    "result_json": "{\"total\":1,\"ready\":1,\"missing\":0,\"disabled\":0,\"error\":0}",
    "created_at": "2026-02-25T00:00:00Z",
    "started_at": "2026-02-25T00:00:01Z",
    "finished_at": "2026-02-25T00:00:05Z",
    "updated_at": "2026-02-25T00:00:05Z"
  },
  "events": [
    {
      "id": 1,
      "event_type": "task.created",
      "payload_json": "{\"task_id\":\"...\"}",
      "created_at": "2026-02-25T00:00:00Z"
    }
  ]
}
```

`payload_json` 与 `result_json` 由前端任务详情页用于按 `task_type` 渲染专属业务信息，不再依赖单一通用文案。

### `GET /api/admin/jobs/translations/status` response

```json
{
  "scheduler_enabled": true,
  "llm_enabled": true,
  "scan_interval_ms": 5000,
  "batch_token_threshold": 12000,
  "ai_model_context_limit": 64000,
  "effective_model_input_limit": 56000,
  "effective_model_input_limit_source": "model_context_limit",
  "general_worker_concurrency": 2,
  "dedicated_worker_concurrency": 1,
  "worker_concurrency": 3,
  "target_general_worker_concurrency": 2,
  "target_dedicated_worker_concurrency": 1,
  "target_worker_concurrency": 3,
  "idle_workers": 1,
  "busy_workers": 2,
  "workers": [],
  "queued_requests": 4,
  "queued_work_items": 2,
  "running_batches": 1,
  "requests_24h": 120,
  "completed_batches_24h": 18,
  "clean_completed_batches_24h": 12,
  "completed_with_issues_batches_24h": 6,
  "failed_batches_24h": 1,
  "error_work_items_24h": 9,
  "missing_work_items_24h": 3,
  "avg_wait_ms_24h": 742,
  "last_batch_finished_at": "2026-04-24T10:38:21Z"
}
```

Notes:

- `completed_batches_24h` continues to reflect raw `translation_batches.status='completed'`.
- `clean_completed_batches_24h` only counts completed batches whose items resolve to `ready` / `disabled` with no `error` / `missing` / pending work.
- `completed_with_issues_batches_24h` counts completed batches whose item-level business outcome is not fully clean.

### `GET /api/admin/jobs/translations/batches` response

```json
{
  "items": [
    {
      "id": "batch_01",
      "status": "completed",
      "trigger_reason": "release_auto_polish",
      "worker_slot": 1,
      "request_count": 4,
      "item_count": 4,
      "estimated_input_tokens": 2300,
      "created_at": "2026-04-24T10:00:00Z",
      "started_at": "2026-04-24T10:00:01Z",
      "finished_at": "2026-04-24T10:00:05Z",
      "updated_at": "2026-04-24T10:00:05Z",
      "result_summary": {
        "ready": 2,
        "error": 1,
        "missing": 1,
        "disabled": 0,
        "queued": 0,
        "running": 0
      },
      "business_outcome": {
        "code": "partial",
        "label": "部分成功",
        "message": "2 个结果可用，2 个结果仍缺失或失败。"
      }
    }
  ],
  "page": 1,
  "page_size": 20,
  "total": 1
}
```

### `GET /api/admin/jobs/translations/batches/{batch_id}` response

```json
{
  "batch": {
    "id": "batch_01",
    "status": "completed",
    "trigger_reason": "release_auto_polish",
    "worker_slot": 1,
    "request_count": 4,
    "item_count": 4,
    "estimated_input_tokens": 2300,
    "created_at": "2026-04-24T10:00:00Z",
    "started_at": "2026-04-24T10:00:01Z",
    "finished_at": "2026-04-24T10:00:05Z",
    "updated_at": "2026-04-24T10:00:05Z",
    "result_summary": {
      "ready": 2,
      "error": 1,
      "missing": 1,
      "disabled": 0,
      "queued": 0,
      "running": 0
    },
    "business_outcome": {
      "code": "partial",
      "label": "部分成功",
      "message": "2 个结果可用，2 个结果仍缺失或失败。"
    }
  },
  "items": [
    {
      "request_id": "req_01",
      "entity_id": "313130120",
      "status": "error",
      "result_status": "error",
      "translated_title": null,
      "error": "429 Chat upstream returned 429",
      "error_code": "rate_limited",
      "error_summary": "上游限流",
      "error_detail": "Chat upstream returned 429"
    }
  ],
  "llm_calls": [
    {
      "id": "call_01",
      "status": "failed",
      "source": "job.sync.subscriptions.auto_smart",
      "model": "grok-4.20-fast",
      "scheduler_wait_ms": 184,
      "duration_ms": 921,
      "created_at": "2026-04-24T10:00:04Z"
    }
  ]
}
```

Notes:

- Batch list/detail preserve the raw batch `status` for scheduler truth, but `business_outcome` is the preferred operator-facing interpretation.
- `result_summary` is computed from item-level terminal statuses and may expose `queued` / `running` even when the batch row already left `running`, which is useful for diagnosing partial completion.

### `PATCH /api/admin/jobs/scheduled/{hour_utc}` request

```json
{
  "enabled": true
}
```

## Modified: trigger-style APIs support `return_mode`

Applies to:

- `POST /api/sync/starred`
- `POST /api/sync/releases`
- `POST /api/sync/notifications`
- `POST /api/briefs/generate`
- `POST /api/translate/release`
- `POST /api/translate/release/detail`
- `POST /api/translate/notification`

### Query param

- `return_mode=sync|task_id|sse` (default: `sync`)

### `return_mode=task_id` response

```json
{
  "mode": "task_id",
  "task_id": "6bc40f0f-27f3-4c67-bf9e-c62a0fbe84c8",
  "task_type": "sync.releases",
  "status": "queued"
}
```

### `return_mode=sse` response

SSE stream events include `task_id` in payload:

- `task.created`
- `task.running`
- `task.progress`
- `task.completed`

## Error codes

- `forbidden_admin_only` (`403`): caller is not admin.
- `not_found` (`404`): task/user/slot not found.
- `bad_request` (`400`): invalid input (e.g. bad `return_mode`/`hour_utc`).
