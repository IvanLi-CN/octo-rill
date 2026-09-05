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
      "model": "configured-candidate",
      "requested_by": 1,
      "parent_task_id": null,
      "parent_task_type": null,
      "max_tokens": 900,
      "attempt_count": 1,
      "scheduler_wait_ms": 123,
      "first_token_wait_ms": 180,
      "duration_ms": 780,
      "input_tokens": 860,
      "output_tokens": 212,
      "cached_input_tokens": 420,
      "total_tokens": 1072,
      "created_at": "2026-02-27T12:00:00Z",
      "started_at": "2026-02-27T12:00:00Z",
      "finished_at": "2026-02-27T12:00:01Z",
      "updated_at": "2026-02-27T12:00:01Z",
      "failure_class": null,
      "final_model": "configured-candidate",
      "fallback_count": 0,
      "retry_scheduled_at": null,
      "recovery_attempt_count": 0
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
  "model": "configured-candidate",
  "requested_by": 1,
  "parent_task_id": "d5bf4a8b-8fc3-4d6d-af68-8b34db732457",
  "parent_task_type": "translate.release",
  "max_tokens": 900,
  "attempt_count": 4,
  "scheduler_wait_ms": 3102,
  "first_token_wait_ms": 640,
  "duration_ms": 5098,
  "input_tokens": 1420,
  "output_tokens": null,
  "cached_input_tokens": 700,
  "total_tokens": 1420,
  "input_messages_json": "[{\"role\":\"system\",\"content\":\"...\"},{\"role\":\"user\",\"content\":\"...\"}]",
  "output_messages_json": null,
  "prompt_text": "full prompt ...",
  "response_text": null,
  "error_text": "上游请求被限流",
  "finish_reason": null,
  "provider_request_id": "provider-request-placeholder",
  "provider_http_status": 429,
  "failure_class": "rate_limited",
  "final_model": "configured-candidate",
  "fallback_count": 1,
  "retry_scheduled_at": "2026-02-27T12:05:00Z",
  "recovery_attempt_count": 0,
  "attempt_history": [
    {
      "event_type": "llm.attempt_failed",
      "model": "configured-candidate",
      "failure_class": "rate_limited",
      "attempt": 1,
      "retry_after_ms": 1000,
      "created_at": "2026-02-27T12:00:02Z"
    }
  ],
  "created_at": "2026-02-27T12:00:00Z",
  "started_at": "2026-02-27T12:00:01Z",
  "finished_at": "2026-02-27T12:00:06Z",
  "updated_at": "2026-02-27T12:00:06Z"
}
```

详情响应还包含安全 `failure_class`、最终路由、回退次数、逐次尝试历史、恢复时间和可用的 provider delivery metadata。内容处理下钻额外传递该调用在尝试中的阶段、关系与输出契约结果；诊断载荷过期时，该下钻返回安全的 `expired` 证据状态而非完整 payload。示例中的路由标识和内容均为合成占位值，不代表运行时配置或线上记录。

详情还返回 `processing_stage`、`provider_status`、`output_contract_status`、`retry_disposition`、`relation_role` 与 `evidence_availability`。Provider 成功不等于输出契约成功；例如 `provider_status=succeeded` 且 `output_contract_status=failed` 表示已收到响应但 JSON/schema 校验失败。

## New: `POST /api/admin/jobs/llm/calls/{call_id}/diagnostic-access`

管理员在展开或复制诊断响应时提交 `{ "action": "reveal" | "copy" }`。接口只写入 metadata-only 审计记录；调用不存在或其七天诊断载荷已过期时返回 `404`，不会恢复或回放 provider 调用。

## Extended: `GET /api/admin/jobs/events` (SSE)

Besides existing `job.event`, this stream now emits `llm.call` in real time.

### Event: `llm.call`

```json
{
  "event_id": 21001,
  "call_id": "3a0f5147-4cb1-4f0e-a2c4-67cb4d70d2f5",
  "status": "running",
  "source": "api.translate_releases_batch",
  "requested_by": 1,
  "parent_task_id": null,
  "event_type": "llm.running",
  "created_at": "2026-02-27T12:00:00Z"
}
```

## Error codes

- `forbidden_admin_only` (`403`): caller is not admin.
- `not_found` (`404`): call id does not exist.
- `bad_request` (`400`): invalid filter values.
