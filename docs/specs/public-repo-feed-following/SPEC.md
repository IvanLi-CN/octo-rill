# 任意 Public Repo Feed 与关注仓库体系

## 背景 / 问题陈述

- `authenticated-scoped-focus-feed` 已经把 `/api/feed`、`/api/dashboard/updates` 和 `/focus/*` scoped 阅读面做成认证态只读工作台，但它的 scope 仍基于“当前用户可见仓库集合”，不支持任意 public repo 的 repo-scope 读取。
- `api-keys` 已经把 Bearer API Key 接进业务接口，`/api/feed` 当前确实属于用户态接口；本轮不新增匿名 public feed，而是在现有鉴权边界内扩展能力。
- `public-release-endpoints` 已经建立公开 Release 页面、`public_repo_release_usage` usage 登记与异步同步链路，但这是匿名公开页主题，不是用户自己的 feed / 关注仓库模型。
- 当前用户与仓库的关系分散在 `starred_repos`、`owned_repo_star_baselines`、`public_repo_release_usage` 等表里，没有统一的 canonical “用户关联仓库”真相源，因此无法稳定表达“关联但未关注”“首次来源”“显式取消关注后禁止被同步打回”等产品语义。

## Goals

- 保持 `/api/feed` 为用户态接口，继续要求 session 或 Bearer API Key。
- 新增 `user_repo_associations` canonical 模型，统一记录用户与仓库的关联来源、首次关联时间、最近见到时间与 follow 状态。
- 让 `/api/feed` 在 `scope=repo|repos` 下支持任意 GitHub public repo 的 release-first 读取：命中本地缓存时直接返回，未 warm 时异步预热且不阻塞请求。
- 新增 `HEAD /api/feed` repo-scope 预热语义；已 warm 返回 `204`，首次登记或新排队返回 `202`，无响应体。
- 新增 `scope=following` 到 `/api/feed` 与 `/api/dashboard/updates`，并把“关注仓库”定义为全局 release feed 与后台 refresh 的 canonical 仓库池。
- 新增关注仓库清单与 follow/unfollow 接口，并在 Dashboard 头像菜单、repo 聚焦页、关注仓库页补齐入口与操作闭环。

## Non-goals

- 不新增匿名 public feed、`/api/v1` 或任何放宽 `/api/feed` 鉴权边界的接口。
- 不为任意 public repo 新增通用 star/fork/announcement/follower 抓取；v1 只保证 release-first。
- 不重定义现有 `/focus/repos`；它继续表达“自定义仓库集合”。
- 不删除或替换 `starred_repos`、`owned_repo_star_baselines`、`public_repo_release_usage`；它们继续作为同步输入与 usage 元数据来源存在。
- 不在本轮实现复杂的 per-source 历史审计页、批量关注管理或推荐算法。

## 范围

### In scope

- `migrations/*`
- `src/api.rs`
- `src/server.rs`
- `src/sync.rs`
- `web/src/api.ts`
- `web/src/dashboard/routeState.ts`
- `web/src/dashboard/scopeSummary.ts`
- `web/src/dashboard/useDashboardLiveUpdates.ts`
- `web/src/feed/useFeed.ts`
- `web/src/pages/Dashboard.tsx`
- `web/src/pages/DashboardHeader.tsx`
- `web/src/routes/focus/**`
- `web/src/stories/**`
- `docs/specs/README.md`

### Out of scope

- Public anonymous release pages 本身的视觉或匿名 REST 契约
- GitHub 组织级关注策略
- 非 release 的任意 public repo 社交抓取
- Admin 治理页与推荐系统

## 数据模型契约

### `user_repo_associations`

- 唯一键：`(user_id, repo_full_name_lower)`。
- canonical 字段：
  - `repo_id`
  - `repo_full_name`
  - `repo_full_name_lower`
  - `owner_login`
  - `repo_name`
  - `html_url`
  - `description`
  - `is_private`
  - `owner_avatar_url`
  - `open_graph_image_url`
  - `uses_custom_open_graph_image`
  - `first_source`：`personal_owned | github_star | manual_feed`
  - `first_associated_at`
  - `last_seen_at`
  - `is_following`
  - `follow_state_source`：`system_default | user_explicit`
  - `has_personal_owned_source`
  - `has_github_star_source`
  - `has_manual_feed_source`
  - `created_at`
  - `updated_at`

### 来源与默认 follow 规则

- `manual_feed` 首次落库时：
  - 创建关联
  - `first_source=manual_feed`
  - `has_manual_feed_source=true`
  - 默认 `is_following=false`
  - `follow_state_source=system_default`
- `github_star` 首次或增量同步时：
  - 创建或补齐关联
  - `has_github_star_source=true`
  - 若当前 `follow_state_source=system_default`，则把 `is_following` 收敛到 `true`
- `personal_owned` 首次或增量同步时：
  - 创建或补齐关联
  - `has_personal_owned_source=true`
  - 若当前 `follow_state_source=system_default`，则把 `is_following` 收敛到 `true`
- 用户显式 `follow` / `unfollow` 后：
  - `follow_state_source` 变为 `user_explicit`
  - 后续 star / owned 同步不得覆盖该显式选择

### 关联回填

- migration 必须把历史 `starred_repos`、`owned_repo_star_baselines` 回填进 `user_repo_associations`。
- 历史同时命中 star 与 owned 的仓库：
  - `first_source` 取更早的首次时间；若时间相同，优先 `personal_owned` 再 `github_star`
  - 所有来源布尔位都要保留
- `public_repo_release_usage` 不是 canonical association 表，但 repo feed 注册会以 `manual_feed` 为来源写入 `user_repo_associations`。

## API 与后端行为契约

### 认证边界

- `GET /api/feed`、`HEAD /api/feed`、`GET /api/dashboard/updates`、`GET /api/repos/following`、`PUT /api/repos/{owner}/{repo}/following`、`DELETE /api/repos/{owner}/{repo}/following` 全部继续走 `require_business_user_id`。
- 无 session 且无有效 Bearer API Key 的情况下，`/api/feed` 与新增接口继续返回未授权。

### `GET /api/feed`

- 保持既有 query 契约并新增 `scope=following`。
- `scope=repo|repos`：
  - 若 repo 已在关联表中，直接按 release-first 读取本地缓存。
  - 若 repo 尚未关联且是合法 public repo path，则先登记 `manual_feed` association，并登记/更新 `public_repo_release_usage`。
  - 若本地已有 release 缓存，返回实际 release items。
  - 若本地未 warm，就走异步预热链路；请求本身不等待远端抓取完成。
- v1 对任意 public repo 只保证 release item 可读；social item 继续只展示当前本地已有且能归属的内容，不新增任意 public social 抓取。

### `HEAD /api/feed`

- 仅支持 `scope=repo|repos`。
- 不返回响应体。
- 对每个目标 repo 执行与 `GET /api/feed` 相同的 public repo 关联登记与 usage 预热判定。
- 响应语义：
  - 全部 repo 已 warm：`204 No Content`
  - 存在首次登记或新排队预热：`202 Accepted`
- `scope=following`、`scope=org`、`scope=mine`、无 scope 的 HEAD 请求返回 `400`。

### `GET /api/dashboard/updates`

- 新增 `scope=following`。
- baseline token / changed detection 必须纳入 `following` scope signature，避免与 `repo|repos|org|mine` 或全局工作台串更新基线。

### `GET /api/repos/following`

- 返回当前用户 `is_following=true` 的 canonical 仓库列表。
- 每项至少包含：
  - repo 标识与 visual
  - 来源摘要
  - `first_source`
  - `first_associated_at`
  - `last_seen_at`
  - `is_following`

### `PUT /api/repos/{owner}/{repo}/following`

- 对尚未关联的合法 public repo：
  - 先自动建立 `manual_feed` association
  - 再标记 `is_following=true`
  - `follow_state_source=user_explicit`
- 对已关联 repo：
  - 显式切换为 follow
  - 不清除已有来源布尔位

### `DELETE /api/repos/{owner}/{repo}/following`

- 对已关联 repo：
  - 显式切换为 `is_following=false`
  - `follow_state_source=user_explicit`
- 不删除关联记录，也不删除来源布尔位。

### `scope=following`

- `/api/feed?scope=following` 与 `/api/dashboard/updates?scope=following` 的 release 范围只包含当前 `is_following=true` 的仓库集合。
- `manual_feed` 但未 follow 的仓库不进入 following scope，也不进入全局 release refresh canonical 池。
- 后台 refresh 池需要从 following 仓库集合读取 release 目标；仅关联未关注仓库不自动进入该池。

## 前端与路由契约

### 新入口与路由

- 头像菜单在“个人仓库”和“设置”之间新增“关注仓库”入口。
- 新路由：
  - `/focus/following`
  - `/focus/following/releases`
- `DashboardScope` 新增 `following`，并参与：
  - path parsing / building
  - route restore
  - warm snapshot scope signature
  - live updates 轮询

### 页面语义

- `/focus/following` 是关注仓库聚焦页，不是自定义集合页。
- `/focus/repos` 继续表示自定义仓库集合。
- repo 聚焦页 summary 区新增 follow/unfollow 开关。
- 关注仓库页展示：
  - 左侧持续显示 `following` scope 的发布与相关动态信息流
  - 右侧摘要卡展示“关注仓库数 / 关联仓库数”
  - 点击统计卡切换下方仓库列表显示“关注仓库”或“关联仓库”
  - 列表展示来源摘要、首次关联时间、当前关注状态
  - 列表项支持取消关注 / 重新关注动作

### Public repo pending UX

- 当用户通过 repo focus 访问任意 public repo 且本地 release 缓存未就绪时，界面需要给出明确的“正在预热 / 可稍后重试”状态，而不是长时间空白或同步阻塞。
- pending 态必须与已有 feed 空态区分：它表示仓库已登记但数据尚未 ready。

## 验收标准

- Given 无 session 且无有效 Bearer API Key
  When 请求 `GET /api/feed`
  Then 继续返回未授权。

- Given 用户携带有效 API Key
  When 请求 `GET /api/feed?scope=repo&items=owner/repo`
  Then 请求以该 API Key 归属用户身份读取 feed。

- Given 用户请求任意 public repo 的 repo-scope feed
  When 本地已有 release 缓存
  Then `GET /api/feed?scope=repo&items=owner/repo` 返回 release items，且关联表中存在该 repo 的 `manual_feed` 关联记录。

- Given 用户请求任意 public repo 的 repo-scope feed
  When 本地尚未 warm
  Then 请求不会阻塞等待远端抓取；仓库会被登记为 `manual_feed` 关联并进入异步预热链路。

- Given 用户请求 `HEAD /api/feed?scope=repo&items=owner/repo`
  When 仓库已经 warm
  Then 返回 `204` 且无响应体。

- Given 用户首次请求 `HEAD /api/feed?scope=repo&items=owner/repo`
  When 需要新登记或新排队预热
  Then 返回 `202` 且无响应体。

- Given migration 已执行
  When 查看历史 starred repo 与 personal owned repo
  Then 它们都已生成 `user_repo_associations` 记录，且来源布尔位、`first_source` 与首次时间稳定。

- Given 用户显式 unfollow 某个 starred 或 owned 仓库
  When 后续再次同步 star / owned 数据
  Then 该仓库不会被自动改回关注状态。

- Given 用户访问 `GET /api/repos/following`
  When 当前存在已关注仓库
  Then 仅返回 `is_following=true` 的仓库，并带来源 / 时间 / visual 元数据。

- Given 用户访问 `/focus/following` 或 `/focus/following/releases`
  When 页面加载完成
  Then 页面可稳定渲染，头像菜单入口可见，且 follow 开关状态与 repo 聚焦页保持一致。

- Given 用户只是关联了某个 manual-feed public repo，但没有显式 follow
  When 读取全局 release feed 或 `scope=following`
  Then 该仓库不自动进入全局 release 范围，也不进入 following scope。

## 非功能性验收 / 质量门槛

### Testing

- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`

### Storybook / Visual

- 至少提供以下稳定入口：
  - 账号菜单包含“关注仓库”
  - following 页面默认态
  - following 页面“关注 / 关联”切换态
  - repo 聚焦页 follow / unfollow
  - public repo 预热 pending 态

## Visual Evidence

- 2026-07-08: 账号菜单新增“关注仓库”入口，位于“个人仓库”和“设置”之间
  - Storybook: `pages-dashboard--scoped-focus-mine-menu-entry-visible`
  - File: `docs/specs/public-repo-feed-following/visual-evidence/account-menu-following-entry.png`
- 2026-07-08: `/focus/following` 默认态展示“关注仓库数 / 关联仓库数”统计卡、默认关注仓库列表与左侧 following 信息流
  - Storybook: `pages-dashboard--scoped-focus-following-default`
  - File: `docs/specs/public-repo-feed-following/visual-evidence/following-page-default.png`
- 2026-07-08: `/focus/following` 点击“关联仓库”统计卡后，右侧列表切换为关联仓库并保留左侧 following 信息流
  - Storybook: `pages-dashboard--scoped-focus-following-default`
  - File: `docs/specs/public-repo-feed-following/visual-evidence/following-page-associated.png`
- 2026-07-08: `/focus/following` 仓库列表移除“已关注 / 仅关联”状态胶囊，改为仅保留仓库信息与眼睛 icon follow/unfollow 切换
  - Storybook: `pages-dashboard--scoped-focus-following-manual-demo`
  - File: `docs/specs/public-repo-feed-following/visual-evidence/following-eye-button.png`
