---
title: API Key 与外部 API
description: 使用 API Key 调用 OctoRill 用户态业务接口。
---

# API Key 与外部 API

API Key 用于让外部程序调用 OctoRill 的用户态业务接口，例如读取 feed、Release、通知、日报，或触发同步、翻译与 reaction 刷新。它和网页登录态绑定到同一个用户数据范围，不提供管理员接口、登录接口或 API Key 管理接口本身。

如果只需要展示公开 GitHub 仓库 Releases，不需要登录用户数据，请使用 [公开 Release 接入](/public-releases)。

## 创建与使用

API Key 在已登录的用户设置页创建，格式以 `orill_ak_` 开头。当前管理响应会返回完整 `api_key` 与 `masked_key`，因此这些 session-only 管理接口的响应也必须按敏感数据处理。

所有外部请求都使用 Bearer 认证：

```bash
curl "$OCTORILL_ORIGIN/api/feed?scope=mine&limit=20" \
  -H "Authorization: Bearer orill_ak_xxx"
```

浏览器端程序只有在页面 origin 等于 OctoRill 配置的 `public_base_url` origin 时，才能通过 CORS 携带 `Authorization` header 访问；后端允许常见 HTTP method 与 `Content-Type`、`Authorization` header，但不会放开任意第三方 origin。其它外部程序应从服务端、CLI 或同源前端调用。

API Key 被撤销后会立即失效。用户账号被禁用后，即使 API Key 仍存在，也会返回 `403 account_disabled`。

## 管理接口边界

API Key 管理接口只能用网页登录态调用，不能用 API Key 调用：

| 接口 | 说明 |
| --- | --- |
| `GET /api/me/api-keys` | 列出当前用户的 API Key 摘要。 |
| `POST /api/me/api-keys` | 创建 API Key。body: `{ "name": "ci" }`。 |
| `DELETE /api/me/api-keys/{api_key_id}` | 撤销 API Key。 |

`ApiKeySummary` 字段：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | string | API Key id。 |
| `name` | string | 用户填写的名称；未填写时使用默认名称。 |
| `api_key` | string | 完整 key；只出现在 session-only 管理响应中。 |
| `masked_key` | string | 可展示的脱敏 key。 |
| `created_at` | string | 创建时间。 |
| `last_used_at` | string? | 最近一次成功使用时间。 |

## 可调用接口目录

以下接口都支持 `Authorization: Bearer orill_ak_...`：

| 分组 | 接口 |
| --- | --- |
| 读取 | `GET /api/starred`、`GET /api/releases`、`GET /api/releases/{release_id}/detail`、`GET /api/repos/{owner}/{repo}/releases/tag/{tag}/detail`、`GET /api/notifications`、`GET /api/briefs`、`GET /api/briefs/{brief_id}`、`GET /api/feed`、`GET /api/dashboard/updates` |
| Reaction | `POST /api/feed/reactions/refresh`、`POST /api/release/reactions/toggle` |
| 同步与日报生成 | `POST /api/sync/starred`、`POST /api/sync/releases`、`POST /api/sync/notifications`、`POST /api/sync/all`、`POST /api/briefs/generate`、`GET /api/tasks/{task_id}/events` |
| 翻译 | `POST /api/translate/requests`、`GET /api/translate/requests/{request_id}`、`GET /api/translate/requests/{request_id}/stream`、`POST /api/translate/results` |
| 旧版翻译快捷接口 | `POST /api/translate/release`、`POST /api/translate/releases/batch`、`POST /api/translate/releases/batch/stream`、`POST /api/translate/release/detail`、`POST /api/translate/release/detail/batch`、`POST /api/translate/notification`、`POST /api/translate/notifications/batch` |

这些接口不属于 API Key 可调用范围：`/api/me*`、`/api/reaction-token*`、`/api/auth*`、`/api/admin*`、`/auth*`。

## Focus 页面对应的 API

`/focus/*` 页面对应同一个 feed API，通过 query 参数表达 scope：

| 页面 | API |
| --- | --- |
| `/focus/repo/:owner/:repo` | `GET /api/feed?scope=repo&items=owner/repo` |
| `/focus/repos?items=owner/a,owner/b` | `GET /api/feed?scope=repos&items=owner/a,owner/b` |
| `/focus/org/:org` | `GET /api/feed?scope=org&org=org` |
| `/focus/mine` | `GET /api/feed?scope=mine` |
| 任一 `/releases` 子页 | 追加 `types=releases` |

示例：

```bash
curl "$OCTORILL_ORIGIN/api/feed?scope=repo&items=IvanLi-CN/octo-rill&types=releases&limit=30" \
  -H "Authorization: Bearer orill_ak_xxx"

curl "$OCTORILL_ORIGIN/api/feed?scope=repos&items=owner/a,owner/b&cursor=$CURSOR" \
  -H "Authorization: Bearer orill_ak_xxx"

curl "$OCTORILL_ORIGIN/api/feed?scope=org&org=IvanLi-CN" \
  -H "Authorization: Bearer orill_ak_xxx"

curl "$OCTORILL_ORIGIN/api/feed?scope=mine" \
  -H "Authorization: Bearer orill_ak_xxx"
```

`scope=repos` 最多接受 12 个仓库，重复项会按大小写无关规则去重。`limit` 默认 `30`，范围会被限制在 `1..100`；响应里的 `next_cursor` 可作为下一页 `cursor`。

`types` 可选 `all`、`releases`、`stars`、`followers`。scoped focus feed 不返回 follower-only 内容。

## Feed

`GET /api/feed`

Query：

| 参数 | 说明 |
| --- | --- |
| `cursor` | 上一页响应的 `next_cursor`。 |
| `limit` | 默认 `30`，限制到 `1..100`。 |
| `types` | `all`、`releases`、`stars`、`followers`。 |
| `scope` | 空、`repo`、`repos`、`org`、`mine`。 |
| `items` | `scope=repo` 时为 `owner/repo`；`scope=repos` 时为逗号分隔仓库列表。 |
| `org` | `scope=org` 时必填。 |

响应 `FeedResponse`：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `items` | `FeedItem[]` | 当前页条目。 |
| `next_cursor` | string? | 下一页游标；为空表示没有下一页。 |

`FeedItem` 是按 `kind` 区分的 union：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `kind` | string | `release`、`repo_star_received`、`follower_received`、`announcement`、`release_update`、`repo_forked`。 |
| `ts` | string | 排序时间。 |
| `id` | string | 条目 id。 |
| `repo_full_name` | string? | 相关仓库。 |
| `repo_visual` | `RepoVisual`? | 仓库头像与 Open Graph 图。 |
| `title` | string? | 标题。 |
| `body` | string? | 正文或摘要。 |
| `body_truncated` | boolean | `body` 是否被截断。 |
| `subtitle` | string? | 副标题。 |
| `reason` | string? | 通知或事件原因。 |
| `subject_type` | string? | 通知 subject 类型。 |
| `html_url` | string? | GitHub 页面。 |
| `unread` | integer? | 通知未读标记。 |
| `actor` | `FeedActor`? | 事件 actor。 |
| `translated` | `TranslatedItem`? | 中文翻译状态。 |
| `smart` | `SmartItem`? | 阅读润色状态。 |
| `reactions` | `ReleaseReactions`? | Release reaction 缓存。 |

## Release、通知、日报

读取接口：

| 接口 | 响应 |
| --- | --- |
| `GET /api/starred` | `StarredRepoItem[]`，最多 2000 条。 |
| `GET /api/releases` | `ReleaseItem[]`，最多 200 条。 |
| `GET /api/releases/{release_id}/detail` | `ReleaseDetailResponse`。 |
| `GET /api/repos/{owner}/{repo}/releases/tag/{tag}/detail` | `ReleaseDetailResponse`。 |
| `GET /api/notifications` | `NotificationItem[]`，最多 200 条。 |
| `GET /api/briefs` | `BriefSummaryItem[]`。 |
| `GET /api/briefs/{brief_id}` | `BriefDetailItem`。 |

主要字段：

| 类型 | 字段 |
| --- | --- |
| `StarredRepoItem` | `repo_id`、`full_name`、`description`、`html_url`、`stargazed_at`、`is_private` |
| `ReleaseItem` | `full_name`、`tag_name`、`name`、`published_at`、`html_url`、`is_prerelease`、`is_draft` |
| `ReleaseDetailResponse` | `release_id`、`repo_full_name`、`repo_visual`、`tag_name`、`previous_tag_name`、`name`、`body`、`html_url`、`published_at`、`is_prerelease`、`is_draft`、`translated`、`smart` |
| `NotificationItem` | `thread_id`、`repo_full_name`、`subject_title`、`subject_type`、`reason`、`updated_at`、`unread`、`html_url` |
| `BriefSummaryItem` | `id`、`date`、`window_start`、`window_end`、`effective_time_zone`、`effective_local_boundary`、`release_count`、`release_ids`、`preview_markdown`、`covers_repo_stars`、`covers_followers`、`created_at`、`updated_at` |
| `BriefDetailItem` | `BriefSummaryItem` 的全部字段，加 `content_markdown` |

## Dashboard updates

`GET /api/dashboard/updates`

用于外部程序轮询“列表是否变化”，不会返回完整列表内容。

Query：

| 参数 | 说明 |
| --- | --- |
| `token` | 上次响应的 `token`；首次请求省略。 |
| `include` | 逗号分隔：`feed`、`briefs`、`notifications` 或 `inbox`。省略表示全部。 |
| `feed_type` | `all`、`releases`、`stars`、`followers`。 |
| `scope`、`items`、`org` | 与 `GET /api/feed` 相同。 |

响应：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `token` | string | 下次轮询使用。 |
| `generated_at` | string | 生成时间。 |
| `lists` | object | 按 include 返回 `feed`、`briefs`、`notifications`。 |
| `lists.*.changed` | boolean | 与上次 token 相比是否有新增变化。 |
| `lists.*.new_count` | number | 新增 key 数量。 |
| `lists.*.latest_keys` | string[] | 变化 key。首次请求通常为空。 |

## Reaction

`POST /api/feed/reactions/refresh`

```json
{ "release_ids": ["123", "456"] }
```

`release_ids` 最多 100 个，必须是当前用户可见 Release。响应：

```json
{
  "items": [
    {
      "release_id": "123",
      "reactions": {
        "counts": { "plus1": 0, "laugh": 0, "heart": 0, "hooray": 0, "rocket": 0, "eyes": 0 },
        "viewer": { "plus1": false, "laugh": false, "heart": false, "hooray": false, "rocket": false, "eyes": false },
        "status": "ready"
      }
    }
  ]
}
```

`POST /api/release/reactions/toggle`

```json
{ "release_id": "123", "content": "heart" }
```

`content` 可选 `plus1`、`laugh`、`heart`、`hooray`、`rocket`、`eyes`。该接口需要用户配置有效 GitHub PAT；未配置时返回 `403 pat_required`。

## 同步、日报生成与任务流

这些接口支持 query 参数 `return_mode`：

| 值 | 说明 |
| --- | --- |
| `sync` | 默认值；请求内同步执行并返回结果。 |
| `task_id` | 入队后台任务，返回 `TaskAcceptedResponse`。 |
| `sse` | 入队后台任务，并直接返回 Server-Sent Events。 |

`TaskAcceptedResponse`：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `mode` | string | 固定为 `task_id`。 |
| `task_id` | string | 任务 id。 |
| `task_type` | string | 后台任务类型。 |
| `status` | string | 初始任务状态。 |

可调用接口：

| 接口 | sync 响应 |
| --- | --- |
| `POST /api/sync/starred` | `{ "repos": number }` |
| `POST /api/sync/releases` | `{ "repos": number, "releases": number }` |
| `POST /api/sync/notifications` | `{ "notifications": number, "since": string? }` |
| `POST /api/sync/all` | `{ "starred": ..., "releases": ..., "social": ..., "notifications": ..., "social_error"?: string }` |
| `POST /api/briefs/generate` | `BriefGenerateResponse` |

`POST /api/briefs/generate` body 可省略；如指定日期：

```json
{ "date": "2026-07-06" }
```

`BriefGenerateResponse` 字段：`id`、`date`、`window_start`、`window_end`、`effective_time_zone`、`effective_local_boundary`、`release_count`、`release_ids`、`content_markdown`。

任务 SSE 可通过 `GET /api/tasks/{task_id}/events` 读取。这个接口同样支持 API Key，适合配合 `return_mode=task_id` 使用。

## 翻译接口

推荐使用通用翻译请求接口。

`POST /api/translate/requests`

Body：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `mode` | string | `async`、`wait`、`stream`。 |
| `item` | `TranslationRequestItemInput`? | 单个请求。 |
| `items` | `TranslationRequestItemInput[]`? | 批量请求，仅支持 `mode=async`。 |

`item` 与 `items` 互斥。`kind` 只支持 `release_summary`、`release_smart`、`release_detail`、`notification`；`target_lang` 只支持 `zh-CN`。

`TranslationRequestItemInput`：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `producer_ref` | string | 调用方自定义引用。 |
| `kind` | string | 翻译类型。 |
| `variant` | string | 调用方变体标识。 |
| `entity_id` | string | Release id 或 notification thread id。 |
| `target_lang` | string | `zh-CN`。 |
| `max_wait_ms` | number | 等待模式的最大等待时间，会按服务端上下限 clamp。 |
| `source_blocks` | `{ slot, text }[]` | `slot` 可为 `title`、`excerpt`、`body_markdown`、`metadata`。 |
| `target_slots` | string[] | 可为 `title_zh`、`summary_md`、`body_md`。 |

单个请求响应 `TranslationRequestResponse`：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `request_id` | string | 翻译请求 id。 |
| `status` | string | `queued`、`running`、`completed`、`failed` 等。 |
| `result` | `PublicTranslationResultItem` | 当前结果或占位结果。 |

批量提交响应：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `requests` | object[] | 每项包含 `request_id`、`status`、`producer_ref`、`entity_id`、`kind`、`variant`。 |

结果查询：

| 接口 | 说明 |
| --- | --- |
| `GET /api/translate/requests/{request_id}` | 读取单个请求当前状态。 |
| `GET /api/translate/requests/{request_id}/stream` | SSE stream；事件字段包含 `event`、`request_id`、`status`、`batch_id`、`result`、`error`。 |
| `POST /api/translate/results` | body: `{ "items": [...], "retry_on_error": false }`；返回 `{ "items": PublicTranslationResultItem[] }`。 |

`PublicTranslationResultItem` 字段：`producer_ref`、`entity_id`、`kind`、`variant`、`status`、`title_zh`、`summary_md`、`body_md`、`error`、`work_item_id`、`batch_id`、`error_code`、`error_summary`、`error_detail`。

旧版快捷接口仍可调用：

| 接口 | Body | 响应 |
| --- | --- | --- |
| `POST /api/translate/release` | `{ "release_id": "123" }` | `TranslateResponse` 或任务响应，支持 `return_mode`。 |
| `POST /api/translate/release/detail` | `{ "release_id": "123" }` | `TranslateResponse` 或任务响应，支持 `return_mode`。 |
| `POST /api/translate/notification` | `{ "thread_id": "..." }` | `TranslateResponse` 或任务响应，支持 `return_mode`。 |
| `POST /api/translate/releases/batch` | `{ "release_ids": ["123"] }` | `TranslateBatchResponse`。 |
| `POST /api/translate/release/detail/batch` | `{ "release_ids": ["123"] }` | `TranslateBatchResponse`。 |
| `POST /api/translate/notifications/batch` | `{ "thread_ids": ["..."] }` | `TranslateBatchResponse`。 |
| `POST /api/translate/releases/batch/stream` | `{ "release_ids": ["123"] }` | NDJSON stream，事件为 `item`、`done`、`error`。 |

`TranslateResponse` 字段：`lang`、`status`、`title`、`summary`。`TranslateBatchResponse` 字段：`items`；每个 item 包含 `id`、`lang`、`status`、`title`、`summary`、`error`。

## 通用字段

`RepoVisual`：

| 字段 | 类型 |
| --- | --- |
| `owner_avatar_url` | string? |
| `open_graph_image_url` | string? |
| `uses_custom_open_graph_image` | boolean |

`TranslatedItem` 与 `SmartItem`：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `lang` | string | 当前为 `zh-CN`。 |
| `status` | string | `ready`、`missing`、`disabled`、`error`；`smart` 还可能是 `insufficient`。 |
| `title` | string? | 中文标题或润色标题。 |
| `summary` | string? | 中文摘要或润色摘要。 |
| `error_code` | string? | 错误分类。 |
| `error_summary` | string? | 面向用户的错误摘要。 |
| `error_detail` | string? | 详细错误。 |
| `auto_translate` | boolean? | 是否自动翻译产生。 |

`ReleaseReactions`：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `counts` | object | `plus1`、`laugh`、`heart`、`hooray`、`rocket`、`eyes` 数量。 |
| `viewer` | object | 当前用户是否已点对应 reaction。 |
| `status` | string | `ready` 或 `sync_required`。 |

## 错误码

错误响应通常包含 `code` 与 `message`。常见状态：

| HTTP | code | 说明 |
| --- | --- | --- |
| `400` | `bad_request` | 参数、body 或 mode 不合法。 |
| `401` | `unauthorized` | 未提供可用 session，且请求没有携带 `Authorization`。 |
| `401` | `invalid_api_key` | Bearer API Key 格式错误、已撤销或不存在。 |
| `403` | `account_disabled` | API Key 所属用户已禁用。 |
| `403` | `pat_required` | reaction toggle 需要 GitHub PAT。 |
| `404` | `not_found` | 资源不存在或不属于当前用户范围。 |
| `500` | `internal` | 服务端内部错误。 |
