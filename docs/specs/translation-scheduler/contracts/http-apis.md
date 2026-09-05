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
- legacy batch responses include `failure_class` when the backend has a structured `empty_content | transient | rate_limited | configuration` classification. Historical records without a classification remain `null`; recovery logic never infers a class from `error` text.

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
- `GET /api/admin/jobs/translations/attempt-events?entity_id={entity_id}&kind={kind?}&variant={variant?}&page={page?}&page_size={page_size?}`
- `GET /api/admin/jobs/ai-records/{record_kind}?from={RFC3339?}&before={RFC3339?}&attempt_min={0..10?}&attempt_max={0..10?}&page={page?}&page_size={page_size?}`
- `GET /api/admin/jobs/ai-records/{record_kind}/{record_id}`

`attempt-events` requires an `entity_id`, is admin-only, and returns the matching event history in chronological order. `kind` and `variant` are optional narrowing filters. The response is paginated and has the following shape:

```json
{
  "items": [
    {
      "id": 42,
      "work_item_id": "work_xxx",
      "request_id": "req_xxx",
      "batch_id": "batch_xxx",
      "scope_user_id": "user_xxx",
      "kind": "release_detail",
      "variant": "feed_body",
      "entity_id": "294043551",
      "target_lang": "zh-CN",
      "attempt_no": 2,
      "trigger": "automatic_recovery",
      "event_type": "attempt_queued",
      "result_status": null,
      "error_code": null,
      "error_summary": null,
      "failure_class": null,
      "retry_eligible": false,
      "next_retry_at": null,
      "llm_call_ids": [],
      "created_at": "2026-04-15T03:25:00Z"
    }
  ],
  "page": 1,
  "page_size": 20,
  "total": 1
}
```

`trigger` is one of `initial | manual_retry | automatic_recovery | system_requeue`. `event_type` is one of `attempt_queued | attempt_started | attempt_completed | retry_scheduled`. Each terminal attempt also exposes its processing stage, stable error code, safe error summary, retry disposition, and stage-qualified LLM-call attribution links with `available | expired | not_captured` evidence availability. The endpoint exposes metadata and linked identifiers only; source text, prompts, raw model responses, and raw upstream error text remain outside this retention surface.

Admin views expose scheduler runtime status, request aggregates, batch aggregates, trigger reason, token estimate, fan-out counts, linked `llm_call` ids, and release-level retry audit history.

`ai-records` is admin-only. `record_kind` is `release`, `announcement`, or `brief`; list requests use an inclusive `from` and exclusive `before` RFC3339 range with page-number pagination. The time range is applied to the source time: Release uses `COALESCE(published_at, created_at, updated_at)`, announcements use the grouped `MAX(occurred_at)`, and daily briefs use `created_at`. The original `detected_at` remains an audit field and may be null for historical records; clients display that value as `未知` and must not use it to exclude a source record.

`attempt_min` and `attempt_max` are optional inclusive total-attempt bounds. `attempt_min` defaults to `0`; omitting `attempt_max` means no upper bound. Both values must be integers in `0..10`, and `attempt_max` must be greater than or equal to `attempt_min`, otherwise the server returns `400`. Release and announcement records use the maximum `attempt_count` across their applicable translation and polish work items; daily briefs use the maximum linked polish call attempt count. A record with no applicable work item or call has total attempts `0`. The attempt range is applied before total counting, ordering, and pagination.

A list row contains only the source record's type-specific identity and timestamps plus the translation and/or polish task summaries: status, retry count, started time, latest attempt, and completion time. The detail endpoint returns the same record summary and its ordered attempt history, including trigger, processing stage, provider and output-contract outcomes, stage-qualified model links, safe error classification/summary, retry eligibility, next retry time, and diagnostic evidence availability. It never returns source text, prompts, raw model output, or raw upstream errors.


## Legacy endpoints

- `POST /api/translate/releases/batch`
- `POST /api/translate/releases/batch/stream`
- `POST /api/translate/release`
- `POST /api/translate/release/detail`
- `POST /api/translate/release/detail/batch`
- `POST /api/translate/notification`
- `POST /api/translate/notifications/batch`

All legacy translation endpoints remain compatibility shims. They still accept the historical request shapes and delegate to the same translation handlers during frontend/backend rollouts; new producers should migrate to `/api/translate/requests*`.

Legacy batch item responses (`/api/translate/*/batch`) include the optional `failure_class` field. It is a structured, safe value and is kept separate from the human-readable `error` message.
