# Feed 热路径与 Reaction 异步刷新实现状态（#z8vjj）

## 当前实现

- `GET /api/feed` 已从首屏热路径移除 GitHub GraphQL reaction 拉取，只读取本地 SQLite 并返回缓存 reaction counts。
- `POST /api/feed/reactions/refresh` 提供 batch reaction refresh，负责在 PAT 可用时拉取 GitHub GraphQL、持久化 counts，并返回 viewer/counts 给前端合并。
- Dashboard 在 reaction PAT 可用后异步刷新当前 feed release reactions；失败静默降级为缓存状态。
- 前端使用短 TTL、in-flight refresh toggle guard 与 optimistic pending guard，避免刷新结果造成请求风暴、刷新中误操作或覆盖用户正在提交的 reaction 状态；refresh 失败后不会永久阻断 toggle。
- 后端对 feed hot path 与 reaction refresh 均输出耗时日志。

## 验证

- 通过：`cargo test`
- 通过：`cd web && bun run lint`
- 通过：`cargo test list_feed_serves_cached_reactions_without_live_viewer_lookup`
- 通过：`cargo test refresh_feed_reactions_without_pat_is_non_blocking_empty_result`
- 通过：`cargo test list_feed_returns_mixed_items_and_supports_social_filters`
- 通过：`cd web && bun run build`

## 剩余缺口

- 无已知产品缺口；真实生产 TTFB 仍需结合部署环境日志观察。
