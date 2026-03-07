# HTTP API contracts

## `POST /api/translate/requests`

### Request

```json
{
  "mode": "async | wait | stream",
  "items": [
    {
      "producer_ref": "feed:release:294043551",
      "kind": "release_summary | release_detail | notification",
      "variant": "feed_card | detail_body | inbox_summary",
      "entity_id": "294043551",
      "target_lang": "zh-CN",
      "max_wait_ms": 1200,
      "source_blocks": [
        { "slot": "title", "text": "v1.2.3" },
        { "slot": "excerpt", "text": "- Added..." }
      ],
      "target_slots": ["title_zh", "summary_md"]
    }
  ]
}
```

### Async response

```json
{
  "request_id": "req_xxx",
  "status": "queued"
}
```

### Wait response

```json
{
  "request_id": "req_xxx",
  "status": "completed",
  "items": [
    {
      "producer_ref": "feed:release:294043551",
      "entity_id": "294043551",
      "kind": "release_summary",
      "status": "ready | disabled | missing | error",
      "title_zh": "中文标题",
      "summary_md": "- 中文摘要",
      "body_md": null,
      "error": null
    }
  ]
}
```

### Stream events

- `queued`
- `batched`
- `running`
- `completed`
- `failed`

`completed` / `failed` events carry the full request item result set.

## `GET /api/translate/requests/{request_id}`

Returns request status, timing, and only the items attached to the given request.

## `/api/admin/jobs/translations/*`

- `GET /api/admin/jobs/translations/status`
- `GET /api/admin/jobs/translations/requests`
- `GET /api/admin/jobs/translations/requests/{request_id}`
- `GET /api/admin/jobs/translations/batches`
- `GET /api/admin/jobs/translations/batches/{batch_id}`

Admin views expose scheduler runtime status, request aggregates, batch aggregates, trigger reason, token estimate, fan-out counts, and linked `llm_call` ids.


## Legacy endpoints

- `POST /api/translate/releases/batch`
- `POST /api/translate/releases/batch/stream`
- `POST /api/translate/release`
- `POST /api/translate/release/detail`
- `POST /api/translate/release/detail/batch`
- `POST /api/translate/notification`
- `POST /api/translate/notifications/batch`

All legacy translation endpoints now return `410 Gone` with `translation_scheduler_required`.
