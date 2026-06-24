# HTTP API contracts

## `POST /api/translate/requests`

### Request

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

### Async batch request

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
    "status": "queued | running | ready | disabled | missing | error",
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
- `wait` 最多阻塞到 `item.max_wait_ms`；若预算内未进入终态，则返回该 request 当前的单结果快照，`result.status` 可保持 `queued | running`。
- release detail 等 request-based 交互不得在前端继续追加超出 `max_wait_ms` 合同的同步阻塞；拿到 pending 快照后应转为后台轮询或等待下次显式读取。
- release detail 批次若遇到 retryable upstream `429` / rate-limit / 临时 slow，后端会把 request/work item 复位到 `queued` 后再返回后续快照，不把本次失败沉成默认终态错误。

### Stream events

- `queued`
- `batched`
- `running`
- `completed`
- `failed`

所有事件均绑定单个 `request_id`；终态事件携带单个 `result` 与可选 `error`。

## `GET /api/translate/requests/{request_id}`

Returns request status, timing, and the single `result` attached to the given request.

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

All legacy translation endpoints remain compatibility shims. They still accept the historical request shapes and delegate to the same translation handlers during frontend/backend rollouts; new producers should migrate to `/api/translate/requests*`.
