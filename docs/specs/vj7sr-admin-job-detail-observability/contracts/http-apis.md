# HTTP API contracts

## Modified: `GET /api/admin/jobs/realtime/{task_id}`

### Response (new fields)

```json
{
  "task": {
    "id": "6b7ec0d3-8947-4a4b-a8c5-b570c10af5d5",
    "task_type": "translate.release.batch",
    "status": "succeeded",
    "source": "api.translate_releases_batch_stream",
    "requested_by": 1,
    "parent_task_id": null,
    "cancel_requested": false,
    "error_message": null,
    "payload_json": "{\"user_id\":1,\"release_ids\":[291058027,291042015]}",
    "result_json": "{\"total\":2,\"ready\":1,\"error\":1}",
    "created_at": "2026-02-27T09:19:01.203336358+00:00",
    "started_at": "2026-02-27T09:19:01.203336358+00:00",
    "finished_at": "2026-02-27T09:20:20.984474338+00:00",
    "updated_at": "2026-02-27T09:20:20.984474338+00:00"
  },
  "events": [
    {
      "id": 130,
      "event_type": "task.progress",
      "payload_json": "{\"stage\":\"release\",\"release_id\":\"291058027\",\"item_status\":\"error\",\"item_error\":\"translation failed\",\"task_id\":\"...\"}",
      "created_at": "2026-02-27T09:20:20.982807983+00:00"
    }
  ],
  "event_meta": {
    "returned": 200,
    "total": 376,
    "limit": 200,
    "truncated": true
  },
  "diagnostics": {
    "business_outcome": {
      "code": "failed",
      "label": "业务失败",
      "message": "任务运行完成，但翻译结果全部失败。"
    },
    "translate_release_batch": {
      "target_user_id": 1,
      "release_total": 2,
      "summary": {
        "total": 2,
        "ready": 1,
        "missing": 0,
        "disabled": 0,
        "error": 1
      },
      "progress": {
        "processed": 2,
        "last_stage": "release"
      },
      "items": [
        {
          "release_id": "291058027",
          "item_status": "error",
          "item_error": "translation failed",
          "last_event_at": "2026-02-27T09:20:20.982807983+00:00"
        }
      ]
    }
  }
}
```

### `event_meta`

- `returned`: 当前响应中 `events` 数量。
- `total`: 数据库内该任务事件总数。
- `limit`: 详情接口单次返回上限（本次固定 `200`）。
- `truncated`: 是否发生截断（`total > returned`）。

### `diagnostics`

`diagnostics` 是按任务类型填充的对象，未知任务类型时可为 `null`。

#### Common

```json
{
  "business_outcome": {
    "code": "ok | partial | failed | disabled | unknown",
    "label": "string",
    "message": "string"
  }
}
```

#### `translate_release_batch`

```json
{
  "target_user_id": 1,
  "release_total": 4,
  "summary": { "total": 4, "ready": 0, "missing": 0, "disabled": 0, "error": 4 },
  "progress": { "processed": 4, "last_stage": "release" },
  "items": [
    {
      "release_id": "290978079",
      "item_status": "error",
      "item_error": "translation failed",
      "last_event_at": "2026-02-27T09:20:20.982807983+00:00"
    }
  ]
}
```

#### `brief_daily_slot`

```json
{
  "hour_utc": 0,
  "summary": {
    "total_users": 1,
    "progressed_users": 1,
    "succeeded_users": 1,
    "failed_users": 0,
    "canceled": false
  },
  "users": [
    {
      "user_id": 1,
      "key_date": "2026-02-27",
      "state": "succeeded",
      "error": null,
      "last_event_at": "2026-02-27T00:06:11.000000000+00:00"
    }
  ]
}
```

#### `brief_generate`

```json
{
  "target_user_id": 1,
  "content_length": 3840,
  "key_date": "2026-02-27"
}
```

## Event payload updates

### `translate.release.batch`

`task.progress` with `stage=release` now includes:

- `release_id`
- `item_status`
- `item_error` (nullable, only when status is error)

### `brief.daily_slot`

New progress stages:

- `stage=user_succeeded`
  - `user_id`, `key_date`, `content_length`
- `stage=summary`
  - `total`, `succeeded`, `failed`, `canceled`

## Compatibility

- Existing fields remain unchanged.
- `event_meta` and `diagnostics` are additive; old frontend clients remain compatible.
