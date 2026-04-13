# DB / Runtime Contracts

## `admin_runtime_settings`

- 新增列：`ai_model_context_limit INTEGER NULL CHECK (ai_model_context_limit IS NULL OR ai_model_context_limit > 0)`
- 首次 seed：写入 `NULL`
- 后续真相源：只认该表持久化值，不再读取 `AI_MODEL_CONTEXT_LIMIT` 环境变量

## Release feed translation canonicalization

- feed `release_summary/feed_body` 请求在服务端 canonicalize 为 `release_detail/feed_body`
- source hash 基于完整 release 正文，而不是卡片展示截断正文
- sync 后 `translate.release.batch` 统一复用 `release_detail` chunk translation cache
