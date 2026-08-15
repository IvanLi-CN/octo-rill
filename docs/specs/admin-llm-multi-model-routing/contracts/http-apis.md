# HTTP APIs

## `GET /api/admin/jobs/llm/activity`

管理员只读接口，无查询参数。固定返回当前 UTC 小时与前 49 个完整小时位置；每个桶采用 `[started_at, ended_at)`。

```json
{
  "bucket_minutes": 60,
  "bucket_count": 50,
  "window_started_at": "2026-08-13T09:00:00Z",
  "window_ended_at": "2026-08-15T11:00:00Z",
  "models": [
    { "model": "gpt-4o-mini", "priority": 1, "configured": true },
    { "model": "retired-model", "priority": null, "configured": false }
  ],
  "buckets": [
    {
      "started_at": "2026-08-13T09:00:00Z",
      "ended_at": "2026-08-13T10:00:00Z",
      "counts": [
        { "model": "gpt-4o-mini", "succeeded": 2, "failed": 1 },
        { "model": "retired-model", "succeeded": 0, "failed": 0 }
      ]
    }
  ]
}
```

- `bucket_minutes` 固定为 `60`，`bucket_count` 固定为 `50`。
- 当前配置模型按 `priority` 升序；历史模型按窗口内最近活动倒序、模型名升序追加，历史模型的 `priority` 为 `null`。
- 仅 `succeeded | failed` 计数，以终态时间落桶；运行时管理员 override 在聚合前与持久化记录对账。
- `buckets` 始终为 50 项，且每项 `counts` 按 `models` 顺序补齐零计数。
- 非管理员保持现有 `403 forbidden_admin_only` 错误语义。

## `GET /api/admin/jobs/llm/status`

### Response delta

```json
{
  "llm_models": ["gpt-4o-mini", "gpt-4.1-mini"],
  "selected_model_for_new_calls": "gpt-4.1-mini",
  "effective_model_input_limit": 1047576,
  "effective_model_input_limit_source": "builtin_catalog",
  "model_statuses": [
    {
      "model": "gpt-4o-mini",
      "priority": 1,
      "status": "cooldown",
      "consecutive_final_failures": 3,
      "cooldown_until": "2026-06-28T12:15:00Z",
      "effective_input_limit": 128000,
      "effective_input_limit_source": "builtin_catalog"
    },
    {
      "model": "gpt-4.1-mini",
      "priority": 2,
      "status": "ready",
      "consecutive_final_failures": 0,
      "cooldown_until": null,
      "effective_input_limit": 1047576,
      "effective_input_limit_source": "builtin_catalog"
    }
  ]
}
```

- `llm_models`: 管理员当前保存的有序模型列表。
- `selected_model_for_new_calls`: 如果此刻新来一个请求，运行时将优先选择的模型。
- `effective_model_input_limit` / `effective_model_input_limit_source`: `selected_model_for_new_calls` 对应的实际预算来源。
- `model_statuses[*].status`: `ready | cooldown`。

## `PATCH /api/admin/jobs/llm/runtime-config`

### Request delta

```json
{
  "max_concurrency": 5,
  "ai_model_context_limit": null,
  "llm_models": ["gpt-4o-mini", "gpt-4.1-mini"]
}
```

- `llm_models` 必须是至少 1 项的字符串数组。
- 每个元素 trim 后必须非空。
- normalize 后不得重复。
- 成功响应继续返回完整的 `GET /api/admin/jobs/llm/status` payload。
