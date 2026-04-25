# Feed 热路径与 Reaction 异步刷新（#z8vjj）

## 背景 / 问题陈述

- Dashboard 首屏主列依赖 `GET /api/feed` 返回 feed 数据后才能结束 skeleton。
- `GET /api/feed` 曾在本地 SQLite 查询后同步调用 GitHub GraphQL 拉取 release reaction viewer/counts，并把结果写回数据库。
- 该外部网络调用位于首屏热路径，TTFB 会受 GitHub 网络、限流、TLS 与第三方抖动影响，无法稳定满足接口在 100ms 内开始返回的体验目标。
- Inbox 与其它侧栏数据可先显示而主列仍停留在 skeleton，说明瓶颈集中在 feed 主列表链路。

## 目标 / 非目标

### Goals

- `GET /api/feed` 必须只依赖本地缓存读取并快速返回，不得同步等待 GitHub GraphQL。
- Release reaction 的最新 viewer/counts 通过独立异步路径补齐，不能阻塞 feed 首屏渲染。
- Dashboard 在 reaction 异步刷新失败、PAT 缺失、PAT 无权限或 GitHub 限流时，仍显示已缓存 feed，不产生全页错误。
- 顶部同步、access refresh 与 SSE 完成后的 `refreshAll` 继续刷新 feed/sidebar，保证最新 release/social/inbox 信息及时回流。
- 后端必须保留 feed 与 reaction 刷新耗时观测，便于验证热路径是否仍被外部依赖污染。

### Non-goals

- 不重写 feed SQL 数据模型或分页 cursor 合约。
- 不改变 GitHub OAuth、reaction PAT 配置、PAT fallback 对话框或 reaction toggle mutation 语义。
- 不改变 Dashboard 可见布局、reaction 图标视觉或 Storybook 视觉证据。

## 范围（Scope）

### In scope

- `src/api.rs` 的 feed 热路径、reaction batch refresh API 与计时日志。
- `src/server.rs` 的 API 路由注册。
- `web/src/pages/Dashboard.tsx` 的异步 reaction 补齐与非阻塞降级。
- `web/src/feed/types.ts` 的 reaction refresh 响应类型。

### Out of scope

- 同步任务模型、GitHub 授权模型、release/social 持久化模型。
- 任何 owner-facing 视觉布局变更。

## 需求（Requirements）

### MUST

- `GET /api/feed` 不得调用 `fetch_live_release_reactions`、`mutate_release_reaction` 或任何 GitHub HTTP/GraphQL 请求。
- `GET /api/feed` 对 release item 继续返回现有 `reactions` shape；counts 来自本地缓存，viewer 初始可为默认 false。
- Dashboard 仅在确认 reaction PAT 可用且 feed 中存在 ready release reaction 时，异步请求 reaction refresh。
- Reaction refresh 结果按 `release_id` 合并回现有 feed item；若该 item 正在 optimistic toggle 或 flush 中，不得覆盖本地 pending 状态。
- PAT 可用但某条 release reaction 尚未完成 live viewer 补齐前，客户端不得执行 toggle，避免默认 viewer=false 导致用户意图与 GitHub 实际状态相反。
- Reaction refresh 失败必须静默降级为缓存状态，不阻断 feed、sidebar、toast 或全页渲染。
- Reaction refresh 成功后应把最新 counts 持久化回本地缓存，供后续 feed 热路径使用。
- 后端必须记录 feed DB/total 耗时，以及 reaction refresh 的 DB/GitHub/persist/total 耗时。

### SHOULD

- 前端应对同一组 release ids 做短 TTL 去重，避免 reaction refresh 因合并结果触发重复请求。
- 无 PAT 或 release 缺少 node id 时，reaction refresh 应返回空结果而不是制造首屏错误。

## 接口契约（Interfaces & Contracts）

### `GET /api/feed`

- 响应 shape 保持兼容。
- release reaction counts 代表本地缓存快照。
- release reaction viewer 不再保证随 feed 同步实时获取；客户端可通过异步 refresh 补齐。

### `POST /api/feed/reactions/refresh`

Request:

```json
{
  "release_ids": ["120", "121"]
}
```

Response:

```json
{
  "items": [
    {
      "release_id": "120",
      "reactions": {
        "counts": { "plus1": 1, "laugh": 0, "heart": 0, "hooray": 0, "rocket": 0, "eyes": 0 },
        "viewer": { "plus1": false, "laugh": false, "heart": false, "hooray": false, "rocket": false, "eyes": false },
        "status": "ready"
      }
    }
  ]
}
```

- 仅返回当前用户可见且 GitHub GraphQL 成功返回的 release reaction。
- PAT 缺失或无可刷新 node id 时返回 `items: []`。
- PAT 失效可沿用现有 `pat_invalid` 错误；客户端仍按非阻塞降级处理。

## 验收标准（Acceptance Criteria）

- Given 用户打开 Dashboard
  When `/api/feed?limit=30` 被请求
  Then 服务端只读取本地数据库并返回缓存 reaction，不同步等待 GitHub GraphQL。

- Given GitHub GraphQL 慢、失败或限流
  When Dashboard 首屏加载
  Then feed 主列表仍可结束 skeleton，reaction 最新状态稍后补齐或保持缓存。

- Given reaction PAT 可用且 feed 包含 release items
  When feed 首屏渲染完成
  Then 客户端异步请求 reaction refresh，并将返回的 viewer/counts 合并到对应 release。

- Given 用户正在点击 reaction 且本地存在 optimistic pending 状态
  When 异步 refresh 返回旧状态
  Then 不覆盖 pending reaction 状态。

- Given reaction PAT 可用但某条 release 尚未完成 live viewer 补齐
  When 用户点击 reaction
  Then 页面提示正在同步状态且不执行 toggle mutation，避免误反转真实 GitHub reaction。

- Given access sync 或顶部同步任务完成
  When SSE completion 触发 `refreshAll`
  Then feed/sidebar 继续刷新，最新 release/social/inbox 信息仍及时回流。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cargo test list_feed_serves_cached_reactions_without_live_viewer_lookup`
- `cargo test refresh_feed_reactions_without_pat_is_non_blocking_empty_result`
- `cargo test list_feed_returns_mixed_items_and_supports_social_filters`
- `cd web && bun run build`

### UI / Storybook

- N/A：本轮不改变 owner-facing 视觉布局。

## Visual Evidence

- 不要求新增截图资产；本轮以接口热路径源码约束、后端单测和前端构建作为交付证据。

## 方案概述（Approach, high-level）

- 将 `GET /api/feed` 收敛为纯 SQLite 热路径，只返回本地缓存 reaction counts 与默认 viewer。
- 新增 batch reaction refresh API，把 GitHub GraphQL 拉取和 counts 持久化移到首屏之后的异步路径。
- Dashboard 在 PAT 可用时按 release id 批量刷新 reaction，并用 TTL 去重与 optimistic pending guard 防止请求风暴和状态回退。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：viewer 初始默认 false 时，reaction 按钮可能短暂未高亮；异步 refresh 成功后会恢复真实状态。
- 风险：GitHub GraphQL 长时间失败时，counts 会停留在本地缓存；顶部同步与后续 refresh 仍可更新 release/social/inbox 主数据。
- 假设：100ms 目标针对本地热路径开始返回，不包含冷启动、认证 cookie 解密、浏览器 chunk 加载或第三方 GitHub 请求耗时。

## 参考（References）

- `docs/specs/vgqp9-dashboard-social-activity/SPEC.md`
- `docs/specs/pnzd2-dashboard-startup-request-storm/SPEC.md`
- `docs/specs/96dp9-dashboard-sync-unification/SPEC.md`
- `src/api.rs`
- `web/src/pages/Dashboard.tsx`
