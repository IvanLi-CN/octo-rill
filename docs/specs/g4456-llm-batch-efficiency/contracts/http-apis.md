# HTTP API

## Resolve Translation Results（POST /api/translate/results）

- 范围（Scope）: internal
- 变更（Change）: New
- 鉴权（Auth）: session

### 请求（Request）

- Body:
  - `items: TranslationRequestItemInput[]`（最多 60 个）
  - `retry_on_error?: boolean`（默认 `false`；仅显式重试时传 `true`）
  - `TranslationRequestItemInput`:
    - `producer_ref: string`
    - `kind: "release_summary" | "release_detail" | "notification"`
    - `variant: string`
    - `entity_id: string`
    - `target_lang: "zh-CN"`
    - `max_wait_ms: number`
    - `source_blocks: { slot: "title" | "excerpt" | "body_markdown" | "metadata"; text: string }[]`
    - `target_slots: ("title_zh" | "summary_md" | "body_md")[]`

### 响应（Response）

- Success:
  - `{ items: TranslationResultItem[] }`
  - `TranslationResultItem`:
    - `producer_ref: string`
    - `entity_id: string`
    - `kind: string`
    - `variant: string`
    - `status: "queued" | "running" | "ready" | "disabled" | "missing" | "error"`
    - `title_zh: string | null`
    - `summary_md: string | null`
    - `body_md: string | null`
    - `error: string | null`
    - `work_item_id: string | null`
    - `batch_id: string | null`

### 语义（Semantics）

- 这是一个“结果聚合 + ensure”接口，而不是纯缓存读取接口。
- 接口先读取 `ai_translations` 结果表；只有 `status=ready` 的记录会被其他读取接口当作可直接展示的译文。
- 若结果表中当前 source hash 已是 `ready/disabled/missing/error`，接口直接返回对应终态。
- 若结果表中当前 source hash 已是 `queued/running`，接口会继续核对活跃 work item；若 work item 仍在途则直接返回 queued/running。
- 若结果表未命中当前 source hash，或结果表处于 pending 但已找不到活跃 work item，接口会在后端创建或复用 work item，并把结果表推进到新的 pending 状态后再返回当前状态。
- 若条目上一次结果为 error，默认继续返回 error；只有 `retry_on_error=true` 时才允许在原 request/work item 上重置并重新入队，并优先复用最近一次失败的 `translation_requests` 快照供 request-based 读取接口继续追踪。

## Submit Translation Request（POST /api/translate/requests）

- 范围（Scope）: internal
- 变更（Change）: Existing
- 鉴权（Auth）: session

### 请求（Request）

- Body:
  - `mode: "async" | "wait" | "stream"`
  - `item?: TranslationRequestItemInput`
  - `items?: TranslationRequestItemInput[]`

### 响应（Response）

- `mode=async, item`:
  - `{ request_id, status, result }`
- `mode=async, items`:
  - `{ requests: TranslationBatchSubmitItemResponse[] }`
- `mode=wait`:
  - `{ request_id, status, result }`
- `mode=stream`:
  - NDJSON stream

### 使用约束（Usage）

- Release feed 自动翻译不再直接调用该接口。
- Release detail 侧栏与其他保留路径仍可继续使用该接口。

## Get Translation Request（GET /api/translate/requests/{request_id}）

- 范围（Scope）: internal
- 变更（Change）: Existing
- 鉴权（Auth）: session

### 响应（Response）

- `{ request_id, status, result }`

### 使用约束（Usage）

- 主要供 release detail 等保留的 request-based 交互使用。
