---
title: 公开 Release 接入
description: 面向希望用 OctoRill 展示特定 GitHub 仓库 Releases 的接入说明。
---

# 公开 Release 接入

OctoRill 可以在未登录状态下展示公开 GitHub 仓库的 Releases。接入方只需要把用户导向 OctoRill 部署上的公开 Release 页面；如果需要自行渲染列表或详情，也可以调用公开 REST API。

## 适用场景

- 在官网、文档站、公告页或产品内放一个“查看 Releases”入口。
- 让用户打开某个公开仓库的 Release 列表或指定 tag 详情。
- 在自己的页面中读取 OctoRill 的 Release 原文、中文翻译或润色内容后自行展示。

公开 Release 只支持 GitHub 公开仓库，不提供私有仓库、登录用户视图或非公开权限数据。

## 页面接入

把 `{OCTORILL_ORIGIN}` 替换成你的 OctoRill 部署地址：

```text
{OCTORILL_ORIGIN}/{owner}/{repo}/releases
{OCTORILL_ORIGIN}/{owner}/{repo}/releases/tag/{tag}
```

示例：

```text
https://example.com/IvanLi-CN/octo-rill/releases
https://example.com/IvanLi-CN/octo-rill/releases/tag/v2.29.0
```

### 高亮深链

列表页支持直接分享一个或多个 Release，或分享当前时间倒序列表中的连续范围。选择器必须显式使用 `tag:` 或 `id:` 前缀；离散模式通过重复的 `highlight` 参数传递，最多 20 个目标：

```text
{OCTORILL_ORIGIN}/{owner}/{repo}/releases?highlight=tag:<tag>&highlight=id:<release_id>
{OCTORILL_ORIGIN}/{owner}/{repo}/releases?highlight_start=tag:<tag>&highlight_end=id:<release_id>
```

两种模式互斥，范围端点可反向填写且包含首尾。可选的 `highlight_active` 使用同样的 typed selector，保存当前导航目标。页面会先请求服务端推荐的最多 30 条聚焦窗口，再自动滚动到 active 记录；不会从第一页开始逐页寻找目标。范围过大或离散目标之间存在间隔时，页面使用响应中的双向 cursor 与 gap 截止游标继续补齐；离散目标缺失时会显示已解析结果并提示遗漏。

`owner`、`repo` 和 `tag` 都应该按 URL path segment 编码。尤其是 tag 里包含 `/`、空格或其它特殊字符时，必须先编码：

```js
const href = `${origin}/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases/tag/${encodeURIComponent(tag)}`;
```

首次访问一个尚未缓存的仓库时，页面会自动登记该仓库并展示同步等待状态；后台同步完成后，页面会自动重试并显示 Release 内容。

## API 接入

列表接口：

```text
GET {OCTORILL_ORIGIN}/api/public/repos/{owner}/{repo}/releases
```

详情接口：

```text
GET {OCTORILL_ORIGIN}/api/public/repos/{owner}/{repo}/releases/tag/{tag}
```

可见记录的翻译或润色批量 hydration 接口：

```text
GET {OCTORILL_ORIGIN}/api/public/repos/{owner}/{repo}/releases/content
```

常用 query 参数：

| 参数 | 可选值 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `content` | `original`、`translated`、`polished`、`all` | `all` | 控制返回原文、中文翻译、润色内容或全部内容。 |
| `lang` | `zh-CN` | `zh-CN` | 当前只支持中文翻译。 |
| `limit` | `1` 到 `30` | `6` | 仅列表接口支持。 |
| `cursor` | 上次响应的 `next_cursor` | 空 | 仅列表接口支持，用于分页。 |
| `refresh` | `if_stale` | 空 | 仅限受信任的后端集成。首窗口携带有效 Bearer API key 时，若该仓库上次成功同步已超过 30 秒，会在返回缓存的同时附加单仓异步刷新；分页不可使用。 |
| `until_cursor` | gap 响应的另一侧 cursor | 空 | 与 `cursor` 一起使用，把内部 gap 加载限制在指定边界。 |
| `direction` | `older`、`newer` | `older` | 高亮范围的 cursor 方向；`newer` 使用 `previous_cursor`。 |
| `highlight` | 重复的 `tag:<tag>` 或 `id:<release_id>` | 空 | 离散高亮模式；解析到同一 Release 时去重，最多 20 个。 |
| `highlight_start` | typed selector | 空 | 连续范围模式的端点；必须与 `highlight_end` 一起使用。 |
| `highlight_end` | typed selector | 空 | 连续范围模式的端点；按时间倒序范围包含两端。 |
| `highlight_active` | typed selector | 空 | 当前导航目标；范围模式会尽量以它为中心推荐窗口。 |
| `source` | `page` | 空 | OctoRill 页面访问会使用；普通 API 调用可省略。 |

列表响应在数据可用时返回 `200 OK`：

```json
{
  "status": "ready",
  "repo_full_name": "IvanLi-CN/octo-rill",
  "next_cursor": null,
  "previous_cursor": null,
  "highlight": {
    "mode": "discrete",
    "status": "partial",
    "requested": ["tag:v2.31.0", "id:291058026"],
    "resolved": [
      {
        "selector": "tag:v2.31.0",
        "release_id": "291058027",
        "tag_name": "v2.31.0",
        "ordinal": 1
      }
    ],
    "unresolved": ["id:291058026"],
    "total": 1,
    "active_release_id": "291058027",
    "active_index": 1
  },
  "segments": [{ "first_release_id": "291058028", "last_release_id": "291058026" }],
  "gaps": [],
  "refresh": {
    "state": "queued",
    "last_success_at": "2026-08-20T01:02:03Z",
    "retry_after_seconds": 2
  },
  "items": [
    {
      "release_id": "291058028",
      "is_highlighted": false
    },
    {
      "release_id": "291058027",
      "is_highlighted": true
    },
    {
      "release_id": "291058026",
      "is_highlighted": false
    }
  ]
}
```

没有高亮参数时，`highlight`、`segments`、`gaps` 与 `previous_cursor` 会省略，列表仍保持首载 6 条和 `next_cursor` 的既有行为。范围响应的 `highlight.mode` 为 `range`，`total` 是精确范围数量；当范围超过首载窗口时，使用 `next_cursor` 请求更旧记录，使用 `previous_cursor` 和 `direction=newer` 请求更新记录。

### 受信任集成按需刷新

服务端集成可在首窗口使用 `refresh=if_stale`，并通过 `Authorization: Bearer orill_ak_...` 认证。它始终返回已有缓存；不会等待 GitHub，也不会把有内容的响应改为 `202`。缺失或无效的 key 返回 `401`，携带 `cursor` 或 `until_cursor` 的 opt-in 请求返回 `400`。

成功响应可选 `refresh` 字段使用 snake_case：

- `fresh`：最近成功同步不超过 30 秒；不提供 `retry_after_seconds`。
- `queued`：本次读取已附加等待中的单仓共享 work item；建议 2 秒后重读。
- `running`：已有 worker 正在同步这个仓库；建议 2 秒后重读。
- `backoff`：上游失败后的既有退避仍有效；`retry_after_seconds` 是剩余时间，最大 60 秒。

调用方只应在页面或会话仍打开时按该建议进行有限重读。匿名公开页面、普通列表读取以及后续分页不会触发这项 demand。

如果仓库已登记但同步尚未完成，接口返回 `202 Accepted`，并带有 `Retry-After` header：

```json
{
  "status": "pending_sync",
  "message": "Release data is being prepared. Retry after the suggested delay.",
  "reason": "repository_registered_metadata_pending",
  "retry_after_seconds": 60,
  "repo_full_name": "IvanLi-CN/octo-rill",
  "last_requested_at": "2026-05-06T16:56:44Z"
}
```

`reason` 可能是 `repository_registered_metadata_pending` 或 `repository_registered_release_sync_pending`。接入方通常只需要按 `status=pending_sync` 和重试时间处理，不应把具体 reason 当作稳定业务分支。

接入方应按 `retry_after_seconds` 或 `Retry-After` 延迟重试，不要在等待期内高频轮询。

## 内容字段

列表项会包含 Release 的基础信息：

- `release_id`
- `repo_full_name`
- `repo_visual`
- `tag_name`
- `previous_tag_name`
- `name`
- `body`
- `html_url`
- `published_at`
- `is_prerelease`
- `is_draft`
- `is_highlighted`
- `translated`
- `smart`

`translated` 是中文翻译结果；`smart` 是面向阅读的润色结果。公开页面默认展示润色，首屏同时携带原文作为缺失或失败时的立即回退；翻译在用户切换后按当前可见 Release ID 批量加载。它们可能为空，表示对应内容尚未生成或不可用。

## 接入前检查

- 目标仓库必须是 GitHub 公开仓库。
- OctoRill 实例必须能访问 GitHub API。
- 如果希望长期展示最新内容，后端同步任务必须保持运行。
- 部署地址应该与 `OCTORILL_PUBLIC_BASE_URL` 对齐，避免公开页面生成错误的站内链接。

OctoRill 自身的 v2.29.0 Release 是公开 Release 功能的参考入口：[v2.29.0](https://github.com/IvanLi-CN/octo-rill/releases/tag/v2.29.0)。
