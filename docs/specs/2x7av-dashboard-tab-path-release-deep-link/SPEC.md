# Dashboard 顶部 tab 路径化与 GitHub 风格 release deep link（#2x7av）

## 状态

- Status: 已完成
- Created: 2026-04-22
- Last: 2026-04-22

## 背景 / 问题陈述

- 当前 Dashboard 顶部主 tab 仍主要依赖 `?tab=` 查询参数表达状态，切换 `全部 / 发布 / 加星 / 关注 / 日报 / 收件箱` 时，URL 缺少清晰的 pathname 语义。
- Release 详情的 owner-facing deep link 仍停留在 `?release=<release_id>` 语义，无法像 GitHub release 页面一样表达 `owner / repo / tag`。
- brief Markdown、站内链接解析与详情权限判定依旧偏向旧 query 入口，导致未来生成内容难以稳定过渡到 canonical path。
- 现有 TanStack Router 虽已接管 Dashboard，但 path-backed tab surface、legacy ingress canonicalization 与 repo/tag release detail contract 还没有形成统一口径。

## 目标 / 非目标

### Goals

- 将 Dashboard 顶部主 tab 的 canonical URL 收口为 path-backed surface：`/`、`/releases`、`/stars`、`/followers`、`/briefs`、`/inbox`。
- 新增 GitHub 风格 release detail deep link：`/<owner>/<repo>/releases/tag/<tag>?from=<tab>`，并让 `from` 只表达返回上下文，默认 `briefs`。
- 新增 `GET /api/repos/:owner/:repo/releases/tag/:tag/detail`，保留 `GET /api/releases/:release_id/detail` 兼容旧入口与内部调用。
- 让 brief internal link 生成器、Markdown parser / reconciler 与详情权限判定同时兼容新旧链接，并默认生成 canonical path。
- 补齐 Storybook、Playwright、router helper、后端解析测试与 targeted browser URL 证明，最终按 fast-track 推进到 merge+cleanup。

### Non-goals

- 不重构 Admin / Settings / Bind 的路由体系。
- 不把历史 feed 日组“列表 / 日报”视图切换继续路径化。
- 不批量回填已落库历史 brief Markdown，只通过 ingress 兼容与新生成内容迁移。
- 不新增数据库层 tag 唯一性 schema 约束。

## 范围（Scope）

### In scope

- `web/src/dashboard/routeState.ts`
- `web/src/pages/Dashboard.tsx`
- `web/src/routes/**`
- `web/src/components/Markdown.tsx`
- `web/src/sidebar/ReleaseDetailCard.tsx`
- `web/src/stories/Dashboard.stories.tsx`
- `src/api.rs`
- `src/ai.rs`
- `src/jobs.rs`
- `src/release_links.rs`
- `src/server.rs`
- `docs/product.md`
- `docs/specs/README.md`

### Out of scope

- Admin / Settings / Bind 路由 contract 改造
- Release 详情业务字段与数据库 schema 重构
- 历史 brief Markdown 数据修复任务
- 非 Dashboard 业务语义调整

## 路由与链接契约

### Dashboard 顶部 tab canonical path

- `全部` → `/`
- `发布` → `/releases`
- `加星` → `/stars`
- `关注` → `/followers`
- `日报` → `/briefs`
- `收件箱` → `/inbox`

### Release detail canonical path

- canonical path：`/<owner>/<repo>/releases/tag/<tag>?from=<tab>`
- `from` 只允许表达返回上下文：`all | releases | stars | followers | briefs | inbox`
- `from` 默认值：`briefs`
- 关闭 detail 时，前端必须返回 `from` 指定的 tab canonical path，而不是回退到 query tab

### Legacy ingress compatibility

- `/?tab=<tab>` 继续可打开，但首轮前端导航必须 `replace` 到对应 canonical path
- `/?release=<release_id>` 继续可打开；当 detail resolve 到 `owner/repo/tag` 后，URL 必须 `replace` 到 canonical release path
- `/?tab=briefs&release=<release_id>` 继续可打开，并在 detail resolve 后 `replace` 到 `/<owner>/<repo>/releases/tag/<tag>?from=briefs`
- 历史 brief 中的旧 query 链接继续有效，不要求批量重写存量数据

## 后端与解析契约

### Release detail API

- 新增：`GET /api/repos/:owner/:repo/releases/tag/:tag/detail`
- 返回：沿用现有 release detail payload，并额外携带 canonical `release_id` 以复用前端现有 detail surface
- 兼容：保留 `GET /api/releases/:release_id/detail`

### Repo/tag 解析规则

- repo/tag 命中多条 release 时，按 `published_at DESC, created_at DESC, release_id DESC` 选择最新记录
- `tag` 必须按 `encodeURIComponent` 参与 URL 生成与反解析，保证 `/` 等保留字符端到端可访问

### brief/internal release link parser

- 新生成的 brief/internal links 默认输出 canonical release path
- link parser / reconciler 必须同时接受：
  - `/<owner>/<repo>/releases/tag/<tag>?from=briefs`
  - `/?release=<release_id>`
  - `/?tab=briefs&release=<release_id>`
- 详情 membership / brief access 判定必须同时兼容 repo/tag locator 与 legacy release_id query

## 验收标准（Acceptance Criteria）

- Given 用户直接访问 `/`、`/releases`、`/stars`、`/followers`、`/briefs`、`/inbox`
  When 页面完成路由加载
  Then 对应 tab 渲染正确，且切 tab 时 browser pathname 会同步更新。

- Given 用户直接访问 `/<owner>/<repo>/releases/tag/<tag>?from=briefs`
  When 页面完成首载并打开 release detail
  Then Dashboard 进入 `日报` 上下文，关闭 detail 后回到 `/briefs`。

- Given 用户直接访问 `/<owner>/<repo>/releases/tag/<tag>?from=stars`
  When 用户关闭 release detail
  Then 页面回到 `/stars`，而不是回到 query-based legacy URL。

- Given 用户访问 `/?tab=stars` 或 `/?release=<release_id>`
  When 前端完成首轮 canonicalization
  Then 页面仍可打开正确内容，并通过 `replace` 规范化到 path-backed canonical URL。

- Given brief Markdown 中包含 canonical repo/tag 链接或 legacy release query 链接
  When 后端执行 release membership 解析
  Then 两种链接都能正确回填到 release_id / locator，并通过详情访问判定。

- Given tag 包含 `/` 等保留字符，且同 repo 下存在同名 tag 的多条 release
  When 用户访问 canonical release path
  Then 系统按编码后的 tag 正确命中，并按最新记录优先规则打开 detail。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cargo fmt`
- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `PLAYWRIGHT_WEB_PORT=<leased-port> bunx playwright test e2e/release-detail.spec.ts e2e/dashboard-access-sync.spec.ts e2e/route-code-splitting.spec.ts`

### UI / Storybook (if applicable)

- Stories to add/update: `web/src/stories/Dashboard.stories.tsx`
- Docs pages / state galleries to add/update: 复用 `Pages/Dashboard` 稳定 route-backed stories
- `play` / interaction coverage to add/update: tab 切换 pathname 同步、canonical release detail restore、legacy ingress replace
- Visual evidence source: targeted browser URL 证明 + Storybook 稳定故事

### Verification results

- `cargo fmt`
- `cargo test` → `441 passed`
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `PLAYWRIGHT_WEB_PORT=30031 bunx playwright test e2e/release-detail.spec.ts e2e/dashboard-access-sync.spec.ts e2e/route-code-splitting.spec.ts` → `45 passed`

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/2x7av-dashboard-tab-path-release-deep-link/SPEC.md`
- `docs/specs/qvfxq-release-daily-brief-v2/SPEC.md`
- `docs/specs/67g9w-spa-nav-startup-skeleton-guard/SPEC.md`
- `docs/specs/y9qpf-tanstack-router-auth-boot-no-login-flicker/SPEC.md`
- `docs/product.md`

## 计划资产（Plan assets）

- Directory: 不额外落库截图资产；owner-facing 视觉证据通过聊天快照回传
- Visual evidence source: local preview browser proof（`target_app_window`）

## Visual Evidence

- 已在当前 branch 的本地稳定预览 `http://127.0.0.1:55174/releases` 验证真实浏览器地址栏与 Dashboard active tab 同步：地址栏显示 `/releases`，顶部 `发布` tab 处于激活态。
- owner-facing 证据通过聊天图片快照回传，不把浏览器截图资产落入仓库或 PR 正文。
- `chrome-devtools` 在当前桌面环境持续命中 `Network.enable timed out`，因此本轮浏览器 proof 改为当前 Google Chrome app-window 定向截图；功能验证本身仍绑定到当前 worktree 的稳定预览端口 `55174`。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 dashboard path-backed tabs、release deep link、legacy ingress 兼容 contract。
- [x] M2: 完成前端 routeState / route files / Dashboard detail restore 的 path-backed 重构。
- [x] M3: 完成 repo/tag detail API、brief link parser / reconciler 与回归测试。
- [x] M4: 补齐浏览器 URL 证据、review-loop、PR merge 与 cleanup 收口。

## 方案概述（Approach, high-level）

- 以 `DashboardRouteState` 为单一前端路由真相源，将 `tab + optional release locator + return tab` 合并进 path-backed contract。
- 通过 repo/tag locator 补齐 owner-facing canonical deep link，同时保留 release_id API 作为兼容层与内部复用层。
- 让 brief 生成与解析双向同时理解 canonical path 与 legacy query，从“新写入 canonical、旧链接继续可读”完成迁移。
- 用 TanStack Router lazy routes、Storybook mock surface 与 targeted Playwright 回归把 URL contract 固化为长期约束。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若未来新增 Dashboard tab 但没有同步 path helper / `from` allowlist，可能引入 detail close restore 漏洞。
- 风险：repo/tag locator 解析若缺少稳定编码/解码约束，tag 含 `/` 时可能出现误命中。
- 开放问题：无。
- 假设：Dashboard 顶部主 tab 继续只覆盖 `全部 / 发布 / 加星 / 关注 / 日报 / 收件箱` 六类 surface，不在本轮新增新 tab。

## Change log

- 2026-04-22：创建 follow-up spec，冻结 Dashboard 顶部主 tab path-backed、GitHub 风格 release deep link、legacy ingress 兼容与 repo/tag API contract。
- 2026-04-22：完成前端 routeState / lazy routes、后端 repo/tag detail lookup、brief link parser、回归验证与地址栏浏览器 proof；owner-facing 视觉证据通过聊天快照回传，不新增仓库截图资产。

## 参考（References）

- `docs/specs/y9qpf-tanstack-router-auth-boot-no-login-flicker/SPEC.md`
- `docs/specs/bydfx-web-lazy-route-code-splitting/SPEC.md`
- `docs/specs/qvfxq-release-daily-brief-v2/SPEC.md`
- `docs/product.md`
- `web/src/dashboard/routeState.ts`
- `src/release_links.rs`
