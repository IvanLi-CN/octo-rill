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
