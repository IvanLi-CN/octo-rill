# HTTP API

## Translate Releases Batch（POST /api/translate/releases/batch）

- 范围（Scope）: internal
- 变更（Change）: New
- 鉴权（Auth）: session

### 请求（Request）

- Body:
  - `release_ids: string[]`（最多 60 个，元素需为整数字符串）

### 响应（Response）

- Success:
  - `{ items: TranslateBatchItem[] }`
  - `TranslateBatchItem`:
    - `id: string`
    - `lang: string`
    - `status: "ready" | "disabled" | "missing" | "error"`
    - `title: string | null`
    - `summary: string | null`
    - `error: string | null`

### 兼容性与迁移（Compatibility / migration）

- 旧接口 `/api/translate/release` 保留，内部转调 batch（单元素）。

## Translate Release Detail Batch（POST /api/translate/release/detail/batch）

- 范围（Scope）: internal
- 变更（Change）: New
- 鉴权（Auth）: session

### 请求（Request）

- Body:
  - `release_ids: string[]`（最多 20 个）

### 响应（Response）

- Success: `{ items: TranslateBatchItem[] }`

### 兼容性与迁移（Compatibility / migration）

- 旧接口 `/api/translate/release/detail` 保留，内部复用同一翻译核心。

## Translate Notifications Batch（POST /api/translate/notifications/batch）

- 范围（Scope）: internal
- 变更（Change）: New
- 鉴权（Auth）: session

### 请求（Request）

- Body:
  - `thread_ids: string[]`（最多 60 个）

### 响应（Response）

- Success: `{ items: TranslateBatchItem[] }`

### 兼容性与迁移（Compatibility / migration）

- 旧接口 `/api/translate/notification` 保留，内部转调 batch（单元素）。
