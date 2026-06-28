# DB / Runtime Contracts

## `admin_runtime_settings`

- 新增列：`llm_models_json TEXT NOT NULL DEFAULT '[]'`
- 存储格式：JSON string array，顺序即路由优先级。
- 首次 seed / 旧实例 backfill：若该列为空数组，则使用当前 `AI_MODEL` 写入单元素列表。

## LLM runtime model health state

- 进程内按 normalize 后 model id 跟踪：
  - `consecutive_final_failures`
  - `cooldown_until`
- 成功一次即清零对应模型失败计数。
- 连续 3 次最终失败即进入 10 分钟冷却。
- 该状态不持久化；进程重启后重置。

## Translation `model_profile`

- `translation_work_items.model_profile` 与 `translation_batches.model_profile` 改为记录稳定的有序模型列表画像。
- 不再使用“本次实际命中的单个模型”作为 profile，避免 failover 造成缓存键分叉。
