# HTTP APIs

## `GET /api/admin/jobs/llm/status`

### Response delta

```json
{
  "max_concurrency": 5,
  "ai_model_context_limit": 32768,
  "effective_model_input_limit": 32768,
  "effective_model_input_limit_source": "admin_override"
}
```

- `ai_model_context_limit`: `number | null`，管理员保存的手动覆盖值；`null` 表示跟随模型目录。
- `effective_model_input_limit`: 当前实际用于预算计算的输入上限。
- `effective_model_input_limit_source`: `admin_override | synced_catalog | builtin_catalog | unknown_fallback`。

## `PATCH /api/admin/jobs/llm/runtime-config`

### Request delta

```json
{
  "max_concurrency": 5,
  "ai_model_context_limit": null
}
```

- `max_concurrency` 仍要求正整数。
- `ai_model_context_limit` 允许 `null` 或正整数。
- 非法值返回 `400 bad_request`。

## `GET /api/admin/jobs/translations/status`

### Response note

- 继续返回 `ai_model_context_limit`、`effective_model_input_limit`、`effective_model_input_limit_source`，作为翻译调度观察面板的只读观测字段。
- 该接口不再承担管理员保存输入预算的职责。
