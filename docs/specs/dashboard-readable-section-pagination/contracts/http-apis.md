# Dashboard 可读区块 HTTP API 契约

## GET /api/dashboard/feed

仅用于非 scoped Dashboard 根页 `全部` tab。服务端从当前认证用户解析阅读范围；该接口不接受 `types`、`scope`、`items` 或 `org` 参数。

查询参数：

- `cursor`：可选、不透明的区块 continuation token。客户端只能回传先前响应给出的 token。

响应：

```json
{
  "sections": [
    {
      "id": "section_token_current",
      "kind": "raw",
      "display_date": "2026-09-01",
      "window_start": "2026-08-31T16:00:00Z",
      "window_end": "2026-09-01T16:00:00Z",
      "activity_count": 18,
      "items": ["FeedItem"],
      "items_next_cursor": "section_items_cursor"
    },
    {
      "id": "section_token_brief",
      "kind": "brief",
      "display_date": "2026-08-31",
      "window_start": "2026-08-30T16:00:00Z",
      "window_end": "2026-08-31T16:00:00Z",
      "activity_count": 24,
      "brief": {
        "id": "brief_01",
        "date": "2026-08-31",
        "window_start": "2026-08-30T16:00:00Z",
        "window_end": "2026-08-31T16:00:00Z",
        "effective_time_zone": "Asia/Shanghai",
        "effective_local_boundary": "00:00",
        "release_count": 17,
        "covers_repo_stars": true,
        "covers_followers": false,
        "content_markdown": "## 项目更新\\n\\n...",
        "created_at": "2026-09-01T00:02:00Z",
        "updated_at": "2026-09-01T00:02:00Z"
      },
      "supplemental_items": ["FeedItem"],
      "supplemental_next_cursor": null,
      "items_next_cursor": null
    }
  ],
  "next_cursor": "dashboard_sections_cursor"
}
```

规则：

- 成功响应最多包含三个完整区块；`next_cursor` 非空时，`sections` 至少包含一个此前未交付的区块。
- `brief.content_markdown` 是完整日报正文。该接口不得以 `preview_markdown` 或空正文代替它，也不返回 `brief.release_ids`。
- `supplemental_items` 只包含日报没有覆盖的活动，并随完整日报区块直接返回；`supplemental_next_cursor` 为兼容字段，当前实现保留为 `null`。
- `items` 是原始活动区块的初始明细；没有日报的历史区块与当前原始区块均使用 `kind: "raw"`。`items` 最多三十条，后续由 `items_next_cursor` 取得。
- `id`、`next_cursor`、`items_next_cursor` 与 `supplemental_next_cursor` 均为不透明 token，服务端必须绑定认证用户和区块边界验证。
- 普通同步、日报生成和日报内容刷新不得使有效区块 cursor 返回 stale 错误。日界设置变更或用户主动刷新由客户端从无 cursor 的首屏请求重新开始。

错误：

- `400 invalid_cursor`：cursor 格式不合法或不属于当前用户/区块。
- 其他认证和服务端错误遵循现有 API 错误合同。

## GET /api/dashboard/feed/sections/{section_id}/items

返回用户显式选择“列表”的区块完整原始活动集合。`section_id` 必须来自 `GET /api/dashboard/feed` 的 `sections[].id`。

查询参数：

- `cursor`：可选、不透明的区块明细 cursor。

响应：

```json
{
  "items": ["FeedItem"],
  "next_cursor": "section_items_cursor"
}
```

规则：

- 每页最多三十条，排序和 `FeedItem` 字段沿用 `/api/feed` 的原始活动语义。
- 响应包含该区块的完整活动集合，包括日报已覆盖的发布、补充动态和其他同窗口活动；不得越过区块边界。
- 该请求与主区块 cursor 独立。它的加载、失败和重试不得阻塞主阅读流。

错误：

- `404 section_not_found`：token 已不再对应当前用户可访问的区块。
- `400 invalid_cursor`：明细 cursor 不属于该区块或格式不合法。

## Compatibility

- `GET /api/feed`、`GET /api/briefs` 和 `GET /api/briefs/{brief_id}` 的请求和响应合同保持不变。
- Dashboard 根页 `全部` tab 迁移后不以 `/api/feed` 或全量 `/api/briefs` 作为生成日报区块的输入。
