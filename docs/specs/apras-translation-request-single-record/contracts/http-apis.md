# HTTP API contracts

## `POST /api/translate/requests`

### Single request body

```json
{
  "mode": "async | wait | stream",
  "item": {
    "producer_ref": "feed.auto_translate:release:294043551",
    "kind": "release_summary | release_detail | notification",
    "variant": "feed_card | detail_card | inbox_summary",
    "entity_id": "294043551",
    "target_lang": "zh-CN",
    "max_wait_ms": 1200,
    "source_blocks": [
      { "slot": "title", "text": "v1.2.3" },
      { "slot": "excerpt", "text": "- Added..." }
    ],
    "target_slots": ["title_zh", "summary_md"]
  }
}
```

### Batch async request body

```json
{
  "mode": "async",
  "items": [
    {
      "producer_ref": "feed.auto_translate:release:294043551",
      "kind": "release_summary",
      "variant": "feed_card",
      "entity_id": "294043551",
      "target_lang": "zh-CN",
      "max_wait_ms": 1200,
      "source_blocks": [{ "slot": "title", "text": "v1.2.3" }],
      "target_slots": ["title_zh", "summary_md"]
    }
  ]
}
```

### Single response

```json
{
  "request_id": "req_xxx",
  "status": "queued | running | completed | failed",
  "result": {
    "producer_ref": "feed.auto_translate:release:294043551",
    "entity_id": "294043551",
    "kind": "release_summary",
    "variant": "feed_card",
    "status": "queued | ready | disabled | missing | error",
    "title_zh": "中文标题",
    "summary_md": "- 中文摘要",
    "body_md": null,
    "error": null,
    "work_item_id": "work_xxx",
    "batch_id": "batch_xxx"
  }
}
```

### Batch async response

```json
{
  "requests": [
    {
      "request_id": "req_xxx",
      "status": "queued",
      "producer_ref": "feed.auto_translate:release:294043551",
      "entity_id": "294043551",
      "kind": "release_summary",
      "variant": "feed_card"
    }
  ]
}
```

### Validation rules

- `item` 与 `items` 互斥。
- `wait` / `stream` 只接受 `item`。
- `async` 可接受 `item` 或 `items`。
- `wait` 最多阻塞到 `item.max_wait_ms`；若预算内未进入终态，则返回该 request 当前的单结果快照（`status` 可能仍为 `queued | running`）。
- 旧的 `{ "mode": "wait", "items": [...] }` 与 `{ "mode": "stream", "items": [...] }` 直接返回 `400 bad_request`。

## `GET /api/translate/requests/{request_id}`

返回单条 request 的状态与单个 `result`。

## `GET /api/translate/requests/{request_id}/stream`

### Stream events

- `queued`
- `batched`
- `running`
- `completed`
- `failed`

所有事件均绑定单个 `request_id`；终态事件携带单个 `result` 与可选 `error`。

## `/api/admin/jobs/translations/requests*`

- 请求列表不再返回 request 进度字段。
- 请求详情返回 `request` 与单个 `result`，不再返回 `items[]`。
