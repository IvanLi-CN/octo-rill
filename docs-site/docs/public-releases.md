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

常用 query 参数：

| 参数 | 可选值 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `content` | `original`、`translated`、`polished`、`all` | `all` | 控制返回原文、中文翻译、润色内容或全部内容。 |
| `lang` | `zh-CN` | `zh-CN` | 当前只支持中文翻译。 |
| `limit` | `1` 到 `30` | `6` | 仅列表接口支持。 |
| `cursor` | 上次响应的 `next_cursor` | 空 | 仅列表接口支持，用于分页。 |
| `source` | `page` | 空 | OctoRill 页面访问会使用；普通 API 调用可省略。 |

列表响应在数据可用时返回 `200 OK`：

```json
{
  "status": "ready",
  "repo_full_name": "IvanLi-CN/octo-rill",
  "next_cursor": null,
  "items": []
}
```

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
- `translated`
- `smart`

`translated` 是中文翻译结果；`smart` 是面向阅读的润色结果。它们可能为空，表示对应内容尚未生成或不可用。

## 接入前检查

- 目标仓库必须是 GitHub 公开仓库。
- OctoRill 实例必须能访问 GitHub API。
- 如果希望长期展示最新内容，后端同步任务必须保持运行。
- 部署地址应该与 `OCTORILL_PUBLIC_BASE_URL` 对齐，避免公开页面生成错误的站内链接。

OctoRill 自身的 v2.29.0 Release 是公开 Release 功能的参考入口：[v2.29.0](https://github.com/IvanLi-CN/octo-rill/releases/tag/v2.29.0)。
