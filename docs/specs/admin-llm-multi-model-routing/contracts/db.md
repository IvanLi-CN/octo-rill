# DB / Runtime Contracts

## `admin_runtime_settings`

- 新增列：`llm_models_json TEXT NOT NULL DEFAULT '[]'`
- 存储格式：JSON string array，顺序即路由优先级。
- 首次 seed / 旧实例 backfill：若该列为空数组，则使用当前 `AI_MODEL` 写入单元素列表。

## LLM route health state

- `llm_model_health` 按规范化路由标识持久化滚动 10 分钟失败窗口：
  - `relevant_failure_count`
  - `window_started_at`
  - `cooldown_until`
- 空内容、瞬态与限流失败才进入健康窗口；配置错误不触发自动恢复。
- 同一路由在窗口内累计两次相关失败即进入 10 分钟冷却；成功不会抹除窗口历史。
- 服务重启时从该表重新加载，数据库是健康状态真相源。

## LLM call recovery fields

- `llm_calls` 记录安全 `failure_class`、最终命中路由、回退次数、恢复安排时间与恢复尝试次数。
- `llm_call_events` 记录每次尝试和路由切换；原始上游错误不进入对外分类字段。

## Translation recovery fields

- `translation_work_items` 记录 `failure_class`、`retry_count`、`next_retry_at` 与 24 小时 `retry_expires_at`。
- 只对具备结构化失败分类的可恢复项自动恢复；历史未分类失败保持失败，仅允许人工重试。

## Translation `model_profile`

- `translation_work_items.model_profile` 与 `translation_batches.model_profile` 改为记录稳定的有序模型列表画像。
- 不再使用“本次实际命中的单个模型”作为 profile，避免 failover 造成缓存键分叉。
