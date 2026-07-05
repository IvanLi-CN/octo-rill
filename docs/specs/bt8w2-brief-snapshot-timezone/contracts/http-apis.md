# HTTP API 契约

## GET /api/me

Dashboard 启动字段改为读取当前用户已保存的日报偏好：

```json
{
  "dashboard": {
    "daily_boundary_local": "08:00",
    "daily_boundary_time_zone": "Asia/Shanghai",
    "daily_boundary_utc_offset_minutes": 480
  }
}
```

说明：

- `daily_boundary_local`：用户当前保存的本地日报边界
- `daily_boundary_time_zone`：用户当前保存的 IANA 时区
- `daily_boundary_utc_offset_minutes`：以当前时刻计算出的 UTC offset，仅作前端 fallback

## GET /api/me/profile

```json
{
  "user_id": "abc123",
  "daily_brief_local_time": "08:00",
  "daily_brief_time_zone": "Asia/Shanghai",
  "last_active_at": "2026-04-13T03:12:00Z"
}
```

## PATCH /api/me/profile

请求：

```json
{
  "daily_brief_local_time": "09:00",
  "daily_brief_time_zone": "America/New_York"
}
```

行为：

- 时间必须是整点 `HH:00`
- 时区必须是合法 IANA 名称，且其 UTC offset 必须全年保持整点
- 成功后返回同 `GET /api/me/profile`

## GET /api/admin/users/{user_id}/profile

返回结构与 `GET /api/me/profile` 一致，但目标用户由 path param 指定。

## PATCH /api/admin/users/{user_id}/profile

请求 / 响应结构与 `PATCH /api/me/profile` 一致。

## GET /api/briefs

默认返回 Dashboard sidebar 摘要列表，不包含完整 `content_markdown`：

```json
[
  {
    "id": "brief_01",
    "date": "2026-04-12",
    "window_start": "2026-04-11T00:00:00Z",
    "window_end": "2026-04-12T00:00:00Z",
    "effective_time_zone": "Asia/Shanghai",
    "effective_local_boundary": "08:00",
    "release_count": 2,
    "release_ids": ["123", "124"],
    "preview_markdown": "## 摘要\n\n...",
    "covers_repo_stars": true,
    "covers_followers": true,
    "created_at": "2026-04-12T00:00:03Z",
    "updated_at": "2026-04-12T00:00:03Z"
  }
]
```

说明：

- `date` 保留为展示标签，不再承担业务主键语义
- `release_ids` 顺序与 brief 正文中的 release 顺序一致
- `preview_markdown` 是用于首屏列表/选中项预览的短 Markdown，不能替代完整正文
- `covers_repo_stars` / `covers_followers` 是从完整正文派生的轻量去重元数据，用于避免前端把截断 preview 当完整正文解析
- 默认列表响应不得包含 `content_markdown`
- `updated_at` 是前端详情缓存的版本戳；同一 `id` 的 brief 被原位刷新时必须变化

## GET /api/briefs/{brief_id}

返回单条完整 snapshot，用于 Dashboard 选中后懒加载正文：

```json
{
  "id": "brief_01",
  "date": "2026-04-12",
  "window_start": "2026-04-11T00:00:00Z",
  "window_end": "2026-04-12T00:00:00Z",
  "effective_time_zone": "Asia/Shanghai",
  "effective_local_boundary": "08:00",
  "release_count": 2,
  "release_ids": ["123", "124"],
  "preview_markdown": "## 摘要\n\n...",
  "covers_repo_stars": true,
  "covers_followers": true,
  "content_markdown": "...",
  "created_at": "2026-04-12T00:00:03Z",
  "updated_at": "2026-04-12T00:00:03Z"
}
```

## POST /api/briefs/generate

同步返回 snapshot 元数据：

```json
{
  "id": "brief_01",
  "date": "2026-04-12",
  "window_start": "2026-04-11T00:00:00Z",
  "window_end": "2026-04-12T00:00:00Z",
  "effective_time_zone": "Asia/Shanghai",
  "effective_local_boundary": "08:00",
  "release_count": 2,
  "release_ids": ["123", "124"],
  "content_markdown": "..."
}
```
