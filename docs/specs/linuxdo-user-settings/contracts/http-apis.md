## `GET /api/me/linuxdo`

返回当前登录用户的 LinuxDO 绑定状态与服务可用性。

### Response 200

```json
{
  "available": true,
  "connection": {
    "linuxdo_user_id": 1189,
    "username": "reno",
    "name": "Reno",
    "avatar_url": "https://linux.do/user_avatar/linux.do/reno/96/4043_2.png",
    "trust_level": 3,
    "active": true,
    "silenced": false,
    "linked_at": "2026-04-18T11:20:00Z",
    "updated_at": "2026-04-18T11:20:00Z"
  }
}
```

当服务端未配置 LinuxDO Connect 时：

```json
{
  "available": false,
  "connection": null
}
```

## `DELETE /api/me/linuxdo`

解绑当前登录用户的 LinuxDO 账号。

### Response 200

返回结构同 `GET /api/me/linuxdo`，其中 `connection = null`。

## `GET /auth/linuxdo/login`

发起 LinuxDO Connect OAuth 授权。

### Behavior

- 需要当前会话已登录 OctoRill。
- 若 LinuxDO OAuth 未配置，返回 `503 linuxdo_oauth_not_configured`。
- 成功时 302 跳转到 `https://connect.linux.do/oauth2/authorize`。

## `GET /auth/linuxdo/callback`

处理 LinuxDO Connect OAuth 回调，并把 LinuxDO 账号绑定到当前 OctoRill 用户。

### Request query

| field | type | required | notes |
| --- | --- | --- | --- |
| `code` | string | yes | OAuth authorization code |
| `state` | string | yes | 必须匹配当前会话里记录的 LinuxDO OAuth state |

### Behavior

- 使用 `code` 换取 access token，然后调用 `https://connect.linux.do/api/user` 获取 LinuxDO 用户信息。
- 绑定成功后 302 回跳 `/settings?section=linuxdo&linuxdo=connected`。
- 若 state 校验失败、token 交换失败、用户信息获取失败、或 LinuxDO 账号已被其他用户绑定：
  - 清理临时 OAuth state
  - 302 回跳 `/settings?section=linuxdo&linuxdo=<error-code>`

### Error redirect codes

| code | meaning |
| --- | --- |
| `state_mismatch` | callback state 与会话不匹配 |
| `exchange_failed` | token 交换失败 |
| `fetch_user_failed` | 获取 LinuxDO 用户信息失败 |
| `already_bound` | LinuxDO 账号已绑定到其他 OctoRill 用户 |
| `not_configured` | 服务器未配置 LinuxDO Connect |

## `/settings`

前端用户设置页。

### Search params

| field | type | required | notes |
| --- | --- | --- | --- |
| `section` | string | no | `linuxdo` \| `github-pat` \| `daily-brief` |
| `linuxdo` | string | no | LinuxDO callback 回跳状态，仅前端提示使用 |

