# HTTP API

## API Key 列表（GET /api/me/api-keys）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: session only

### 请求（Request）

- Headers: browser session cookie
- Query: None
- Body: None

### 响应（Response）

- Success: `{ items: ApiKeySummary[] }`
- `ApiKeySummary`: `{ id, name, api_key, masked_key, created_at, last_used_at }`
- `api_key` 由服务端解密后返回；该接口仍为 session only。

### 错误（Errors）

- `401/unauthorized`: 未登录或仅提供 API Key。
- `403/account_disabled`: 当前账号被禁用。

## API Key 创建（POST /api/me/api-keys）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: session only

### 请求（Request）

- Headers: browser session cookie, `content-type: application/json`
- Body: `{ name?: string }`

### 响应（Response）

- Success: `{ item: ApiKeySummary, api_key: string }`
- `item.api_key` 与顶层 `api_key` 都包含完整 Key，便于当前创建成功态与列表态复用。

### 错误（Errors）

- `400/bad_request`: 名称过长或请求体无效。
- `401/unauthorized`: 未登录或仅提供 API Key。
- `403/account_disabled`: 当前账号被禁用。

## API Key 撤销（DELETE /api/me/api-keys/{api_key_id}）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: session only

### 请求（Request）

- Headers: browser session cookie
- Path: `api_key_id` 为本地 NanoID 风格 id
- Body: None

### 响应（Response）

- Success: `{ items: ApiKeySummary[] }`

### 错误（Errors）

- `400/bad_request`: `api_key_id` 格式非法。
- `401/unauthorized`: 未登录或仅提供 API Key。
- `404/not_found`: Key 不存在或不属于当前用户。

## Bearer API Key 认证（Authorization: Bearer <api_key>）

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: api key

### 允许范围

- Release/feed/notification/brief 读取接口。
- sync、translation、brief generation、reaction refresh/toggle。
- 与上述任务相关的 task/translation stream。

### 禁止范围

- `/api/me*`
- `/api/reaction-token*`
- `/api/auth*`
- `/api/admin*`
- `/auth*`

### 错误（Errors）

- `401/unauthorized`: Key 缺失、格式错误、hash 不匹配、已撤销，或调用禁止范围。
- `403/account_disabled`: Key 归属用户已禁用。

### 兼容性与迁移（Compatibility / migration）

- 未携带 `Authorization: Bearer` 的 Web 请求继续使用 session。
- 现有 `/api/...` response shape 保持不变。

## 设置页 API Key section（/settings?section=api-keys）

- 范围（Scope）: internal
- 变更（Change）: Modify
- 鉴权（Auth）: session

### 行为

- 新增 `api-keys` settings section。
- 展示创建表单、完整 Key 成功态、列表、空态和撤销入口。
- 列表显示完整 Key，并提供复制入口。
- 撤销入口必须先展示二次确认对话框，确认后才调用 DELETE。
