# HTTP APIs

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
