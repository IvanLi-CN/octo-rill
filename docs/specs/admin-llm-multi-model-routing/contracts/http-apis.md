# HTTP APIs

## `GET /api/admin/jobs/llm/calls`

管理员调用记录接口保留既有 `status`、`source`、`requested_by`、`parent_task_id`、`started_from`、`started_to`、`sort`、`page` 与 `page_size` 参数，并新增：

- `model`：精确匹配模型名；空白值不构成筛选。
- `finished_from`：终态时间的包含下限，必须为 RFC3339。
- `finished_before`：终态时间的排他上限，必须为 RFC3339。

终态时间统一为 `COALESCE(finished_at, updated_at, created_at)`。因此 `finished_from` 与 `finished_before` 的范围为 `[finished_from, finished_before)`，与活动图桶的落桶口径完全一致。`started_to` 继续保持既有包含上限语义。

当运行时管理员 override 覆盖持久化记录的状态或时间时，服务端必须先使用覆盖后的有效记录应用模型、状态、开始时间和终态时间筛选，再统一排序与分页。

列表项还返回以下恢复字段：

- `failure_class`：`empty_content | transient | rate_limited | configuration` 或 `null`；只返回安全分类，不返回上游响应体。
- `final_model`：逻辑调用最终命中的模型，未结束时为 `null`。
- `fallback_count`：候选切换次数。
- `retry_scheduled_at`：同一候选的定时重试时间；即时切换候选时为 `null`。翻译 work item 的恢复时间由 `translation_work_items.next_retry_at` 表示。
- `recovery_attempt_count`：该调用已执行的恢复次数。

`GET /api/admin/jobs/llm/calls/{call_id}` 详情在上述字段之外返回 `attempt_history` 数组。数组只包含事件类型、模型、尝试序号、失败分类、限流等待、路由切换和时间戳等安全审计字段。

## `GET /api/admin/jobs/llm/activity`

管理员只读接口，无查询参数。固定返回当前 UTC 小时与前 49 个完整小时位置；每个桶采用 `[started_at, ended_at)`。

```json
{
  "bucket_minutes": 60,
  "bucket_count": 50,
  "window_started_at": "2026-08-13T09:00:00Z",
  "window_ended_at": "2026-08-15T11:00:00Z",
  "models": [
    { "model": "configured-candidate", "priority": null, "configured": true }
  ],
  "buckets": [
    {
      "started_at": "2026-08-13T09:00:00Z",
      "ended_at": "2026-08-13T10:00:00Z",
      "counts": [
        { "model": "configured-candidate", "succeeded": 2, "failed": 1 }
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
  "llm_models": ["configured-candidate"],
  "selected_model_for_new_calls": "configured-candidate",
  "effective_model_input_limit": 1047576,
  "effective_model_input_limit_source": "builtin_catalog",
  "model_statuses": [
    {
      "model": "configured-candidate",
      "priority": 1,
      "status": "cooldown",
      "consecutive_final_failures": 2,
      "cooldown_until": "2026-06-28T12:15:00Z",
      "effective_input_limit": 128000,
      "effective_input_limit_source": "builtin_catalog"
    },
    {
      "model": "configured-candidate",
      "priority": null,
      "status": "ready",
      "consecutive_final_failures": 0,
      "cooldown_until": null,
      "effective_input_limit": 128000,
      "effective_input_limit_source": "configured_catalog"
    }
  ]
}
```

- `llm_models`: 管理员当前保存的有序模型列表。
- `selected_model_for_new_calls`: 如果此刻新来一个请求，运行时将优先选择的模型。
- `effective_model_input_limit` / `effective_model_input_limit_source`: `selected_model_for_new_calls` 对应的实际预算来源。
- `model_statuses[*].status`: `ready | cooldown`。
- `model_statuses[*].relevant_failure_count`：当前滚动 10 分钟窗口内的空内容、瞬态和限流相关失败次数；配置错误不计入。
- `llm_recovery_enabled` / `llm_recovery_rollout_percent`：翻译与 `release_smart` 恢复开关及稳定分区百分比，默认 `false` / `0`。

## `PATCH /api/admin/jobs/llm/runtime-config`

### Request delta

```json
{
  "max_concurrency": 5,
  "ai_model_context_limit": null,
  "llm_models": ["configured-candidate"],
  "llm_recovery_enabled": false,
  "llm_recovery_rollout_percent": 0
}
```

- `llm_models` 必须是至少 1 项的字符串数组。
- 每个元素 trim 后必须非空。
- normalize 后不得重复。
- `llm_recovery_rollout_percent` 必须为 `0..=100` 的整数；未获得单独放量授权时保持 `0`。
- `llm_recovery_enabled` 默认关闭；关闭时失败仍以真实 `error` 结束，不自动重跑。
- 成功响应继续返回完整的 `GET /api/admin/jobs/llm/status` payload。
