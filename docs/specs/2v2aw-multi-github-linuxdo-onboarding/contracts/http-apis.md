# HTTP API

## `GET /api/me`

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: session

### Response 200

```json
{
  "user": {
    "id": "usr_123",
    "github_user_id": 10001,
    "login": "octo-main",
    "name": "Octo Main",
    "avatar_url": "https://linux.do/user_avatar/linux.do/octo-main/96/1_2.png",
    "email": "octo@example.com",
    "is_admin": false
  },
  "access_sync": {
    "task_id": null,
    "task_type": null,
    "event_path": null,
    "reason": "none"
  },
  "dashboard": {
    "daily_boundary_local": "08:00",
    "daily_boundary_time_zone": "Asia/Shanghai",
    "daily_boundary_utc_offset_minutes": 480
  }
}
```

### Notes

- `github_user_id/login/name/email` 取第一条 GitHub connection（`linked_at ASC, id ASC`）。
- `avatar_url` 优先取 LinuxDO 头像；若未绑定 LinuxDO，则回退第一条 GitHub 头像。
- 不再暴露任何“主 GitHub 账号”语义。

## `GET /auth/github/login`

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: none

### 行为（Behavior）

- 作为匿名 GitHub 登录入口。
- 写入 session 中的 GitHub OAuth state，并把 mode 标记为 `login`。
- 成功时 302 跳转 GitHub OAuth authorize URL，请求 `read:user`、`user:email`、`notifications`、`public_repo` scope。

### 兼容性与迁移（Compatibility / migration）

- 路径保持不变；旧入口仍可用。
- 新语义允许匿名登录，也允许在 LinuxDO pending onboarding 场景下继续使用。

## `GET /auth/github/connect`

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: session

### 行为（Behavior）

- 仅允许已登录用户访问。
- 写入 GitHub OAuth state，并把 mode 标记为 `connect`。
- 成功时 302 跳转 GitHub OAuth authorize URL。

### 错误（Errors）

- `401 unauthorized`: 当前无有效 OctoRill session。

## `GET /auth/github/callback`

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: oauth callback + optional session

### Request（Query）

| field | type | required | notes |
| --- | --- | --- | --- |
| `code` | string | yes | GitHub authorization code |
| `state` | string | yes | 必须匹配当前 session 里的 GitHub OAuth state |

### 行为（Behavior）

- 统一处理三种模式：
  - `login`：匿名 GitHub 登录 / 首次建账号；
  - `connect`：已登录账号追加绑定 GitHub；
  - `linuxdo pending onboarding`：匿名 LinuxDO 首登后补绑 GitHub。
- 若 GitHub 已绑定某个 OctoRill 账号：
  - `login` 模式直接登录所属账号；
  - `connect` 模式返回已占用冲突；
  - LinuxDO pending onboarding 模式若账号不一致则返回冲突。
- 若 GitHub 未绑定任何账号：
  - `login` 模式创建或确认账号，并新增一条 GitHub connection；
  - `connect` 模式追加为当前账号的新 connection；
  - LinuxDO pending onboarding 模式在绑定 GitHub 后继续写入 LinuxDO 绑定并登录该账号。

### Redirects

| redirect | meaning |
| --- | --- |
| `/` | 匿名 GitHub 登录成功 |
| `/settings?section=github-accounts&github=connected` | 绑定 GitHub 成功 |
| `/settings?section=github-accounts&github=already_bound` | 追加绑定的 GitHub 已被其他账号占用 |
| `/settings?section=github-accounts&github=connected&linuxdo=connected` | LinuxDO pending onboarding + GitHub 补绑成功 |
| `/bind/github?linuxdo=github_already_bound` | LinuxDO pending onboarding 时 GitHub 已归属其他账号 |
| `/bind/github?linuxdo=linuxdo_account_conflict` | LinuxDO pending onboarding 时当前账号已绑定其他 LinuxDO |

## `GET /auth/linuxdo/login`

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: none

### 行为（Behavior）

- 允许匿名访问。
- 若 LinuxDO OAuth 未配置，返回 `503 linuxdo_oauth_not_configured`。
- 成功时写入 LinuxDO OAuth state，并 302 跳转 LinuxDO Connect authorize URL。

## `GET /auth/linuxdo/callback`

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: oauth callback + optional session

### Request（Query）

| field | type | required | notes |
| --- | --- | --- | --- |
| `code` | string | yes | LinuxDO authorization code |
| `state` | string | yes | 必须匹配当前 session 里的 LinuxDO OAuth state |

### 行为（Behavior）

- 若 LinuxDO 已绑定某个 OctoRill 账号：直接把该账号写入 session 并回跳 `/`。
- 若当前已有登录 session：把 LinuxDO 绑定到当前账号，成功后回跳 `/settings?section=linuxdo&linuxdo=connected`。
- 若当前匿名且 LinuxDO 尚未绑定：把 LinuxDO 快照写入 session 的 `pending_linuxdo`，并跳转 `/bind/github`。

### 错误回跳（Error redirects）

| code | logged-in redirect | anonymous redirect |
| --- | --- | --- |
| `not_configured` | `/settings?section=linuxdo&linuxdo=not_configured` | `/bind/github?linuxdo=not_configured` |
| `state_mismatch` | `/settings?section=linuxdo&linuxdo=state_mismatch` | `/bind/github?linuxdo=state_mismatch` |
| `exchange_failed` | `/settings?section=linuxdo&linuxdo=exchange_failed` | `/bind/github?linuxdo=exchange_failed` |
| `fetch_user_failed` | `/settings?section=linuxdo&linuxdo=fetch_user_failed` | `/bind/github?linuxdo=fetch_user_failed` |
| `linuxdo_already_bound` | `/settings?section=linuxdo&linuxdo=linuxdo_already_bound` | `/bind/github?linuxdo=linuxdo_already_bound` |
| `linuxdo_account_conflict` | `/settings?section=linuxdo&linuxdo=linuxdo_account_conflict` | `/bind/github?linuxdo=linuxdo_account_conflict` |

## `GET /api/auth/bind-context`

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: session optional

### Response 200

```json
{
  "linuxdo_available": true,
  "pending_linuxdo": {
    "linuxdo_user_id": 9527,
    "username": "linuxdo-first-login",
    "name": "LinuxDo First Login",
    "avatar_url": "https://linux.do/user_avatar/linux.do/linuxdo-first-login/96/1_2.png",
    "trust_level": 2,
    "active": true,
    "silenced": false
  }
}
```

### Notes

- `pending_linuxdo` 在匿名 LinuxDO 首登、待补绑 GitHub 时存在；否则返回 `null`。
- `linuxdo_available` 只反映服务器是否配置 LinuxDO OAuth，不代表当前账号是否已绑定。

## `GET /api/me/github-connections`

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: session

### Response 200

```json
{
  "items": [
    {
      "id": "ghc_123",
      "github_user_id": 10001,
      "login": "octo-main",
      "name": "Octo Main",
      "avatar_url": "https://avatars.githubusercontent.com/u/10001?v=4",
      "email": "octo@example.com",
      "scopes": "notifications,public_repo,read:user,user:email",
      "linked_at": "2026-04-21T09:00:00Z",
      "updated_at": "2026-04-21T09:00:00Z"
    }
  ]
}
```

### Notes

- 返回顺序固定为：`linked_at ASC, id ASC`。
- 列表不暴露“主账号 / 附加账号”语义。

## `DELETE /api/me/github-connections/{connection_id}`

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: session

### 行为（Behavior）

- 删除当前账号的一条 GitHub connection。
- 若删除的是 PAT owner，对应 PAT 也一并删除。
- 删除成功后，`GET /api/me` 与聚合同步会自动基于剩余 connection 顺序重新解析展示摘要。

### 错误（Errors）

- `404 github_connection_not_found`: connection 不属于当前账号。
- `409 last_github_connection_guard`: 当前账号只剩最后一条 GitHub connection，禁止删除。

## `/bind/github`

- 范围（Scope）: internal
- 变更（Change）: New
- 鉴权（Auth）: none

### Search params

| field | type | required | notes |
| --- | --- | --- | --- |
| `linuxdo` | string | no | LinuxDO onboarding 状态码，用于前端提示 |

### 行为（Behavior）

- 前端通过 `GET /api/auth/bind-context` 读取 pending LinuxDO 快照。
- 当 `pending_linuxdo` 存在时，页面展示 LinuxDO 快照与“绑定 GitHub 并继续” CTA（指向 `/auth/github/login`）。
- 当 `pending_linuxdo` 不存在时，页面提示用户重新从 LinuxDO 登录入口发起。

## `GET /api/reaction-token/status`

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: session

### Response 200

```json
{
  "configured": true,
  "masked_token": "ghp_********1234",
  "check": {
    "state": "valid",
    "message": "token is valid",
    "checked_at": "2026-04-21T10:00:00Z"
  },
  "owner": {
    "github_connection_id": "ghc_123",
    "github_user_id": 10001,
    "login": "octo-main"
  }
}
```

### Notes

- `owner` 为新增字段；未配置 PAT 时返回 `null`。

## `POST /api/reaction-token/check`

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: session

### Request body

```json
{
  "token": "ghp_example"
}
```

### Response 200

```json
{
  "state": "valid",
  "message": "token is valid for @octo-main",
  "owner": {
    "github_connection_id": "ghc_123",
    "github_user_id": 10001,
    "login": "octo-main"
  }
}
```

### Notes

- 服务端调用 GitHub `/user` 校验 PAT 所属用户。
- 只有当 owner 属于当前账号任一 GitHub connection 时才返回 `valid`。

## `PUT /api/reaction-token`

- 范围（Scope）: external
- 变更（Change）: Modify
- 鉴权（Auth）: session

### Request body

```json
{
  "token": "ghp_example"
}
```

### 行为（Behavior）

- 仍保持“一账号一个 PAT”。
- 持久化前必须先完成 owner 校验。
- 持久化时同时写入 `owner_github_connection_id`、`owner_github_user_id`、`owner_login`。

### Response 200

- 返回形状与 `GET /api/reaction-token/status` 相同。
