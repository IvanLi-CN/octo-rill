# SPA document fallback 与全局 404 页面收口（#25pe9）

## 背景 / 问题陈述

- `/settings` 已经是合法前端路由，但生产环境直接访问时 document request 仍返回 `404 Not Found`，只是响应体碰巧是 `index.html`，导致 DevTools、监控与代理层事实自相矛盾。
- 当前后端使用 `ServeDir(...).not_found_service(ServeFile::new(index.html))` 承接静态资源与 SPA 页面，缺失任意路径都会回落到 app shell，并强制维持 `404`。
- 仅给 `/settings` 加一条显式白名单路由只能修单点，后续新增顶层前端路由仍会重复掉坑。

## 目标 / 非目标

### Goals

- 把 SPA 文档导航的源站契约收敛为：命中前端路由时返回 `index.html + 200`。
- 保留静态资源、`/api/**`、`/auth/**` 的真实 `404`，不把资源缺失伪装成成功页面。
- 为未知前端路径补一个清晰的全局 404 页面，让 SPA fallback 与“页面不存在”语义可以同时成立。
- 补齐服务端回归测试、前端 Storybook / Playwright 覆盖与 owner-facing 视觉证据。

### Non-goals

- 不修改 `/settings` 业务逻辑、OAuth/PAT 流程、数据库结构或公开 API schema。
- 不引入 SSR、后端模板渲染或 CDN 专属 rewrite。
- 不继续维护“每个前端路由都在 Rust 里打白名单”的模式。

## 范围（Scope）

### In scope

- `src/server.rs` 静态文件与 SPA fallback 逻辑。
- `web/src/routes/__root.tsx` 与新的全局 404 页面组件。
- `web/src/stories/**`、`web/e2e/settings.spec.ts` 对 settings 直达与未知路由 404 的覆盖。
- `docs/solutions/**` 中一条可复用的 Axum + SPA fallback 经验文档。

### Out of scope

- `/settings` 页面信息架构或 UI 业务文案重构。
- 管理后台、release feed、auth bootstrap 以外的无关页面行为改造。

## 需求（Requirements）

### MUST

- 浏览器直接访问 `/settings` 与 `/settings?section=github-pat` 时，document request 必须返回 `200 text/html`。
- 访问未知前端路径时，后端可以回退到 app shell，但前端必须稳定显示“页面不存在”而不是首页或空白页。
- 访问不存在的静态资源时，仍必须返回真实 `404`，不能回退到 `index.html`。
- `/api/**` 与 `/auth/**` miss 必须保持后端错误语义，不进入 SPA fallback。

### SHOULD

- 全局 404 页面应复用现有 Landing / Settings 的壳层视觉语言，并为已登录用户提供回到工作台、进入设置页的 CTA。
- Storybook 应提供稳定的 `Pages/Not Found` 与 `Pages/Settings` 入口，供后续视觉证据复用。

## 验收标准（Acceptance Criteria）

- Given 浏览器直接访问 `/settings`
  When 源站返回文档
  Then status 为 `200`，且页面正确渲染 Settings 内容。

- Given 浏览器直接访问 `/does-not-exist`
  When 应用壳层完成启动
  Then 页面显示全局 404 文案与稳定 CTA，而不是静默回到其他页面。

- Given 浏览器请求 `/assets/does-not-exist.js`
  When 目标资源不存在
  Then 源站返回真实 `404`，不会返回 app shell。

- Given 浏览器请求 `/api/missing` 或 `/auth/missing`
  When 路径不存在
  Then 请求保持后端 `404` 语义，不被 SPA fallback 劫持。

## 实现前置条件（Definition of Ready / Preconditions）

- 前端仍由 TanStack Router 驱动，根路由支持 `notFoundComponent`。
- 生产源站静态资源继续由 Axum + `tower-http` 提供。
- Storybook 已启用 autodocs，可直接为页面类 story 产出 docs surface。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust tests: SPA fallback predicate + `settings` / unknown route / missing asset / missing api auth status contract
- Web tests: unknown route renders global 404 page
- Existing settings route regression remains green

### UI / Storybook

- Stories to add/update: `Pages/Not Found`、现有 `Pages/Settings`
- Docs pages / state galleries to add/update: `Pages/Not Found` autodocs
- `play` / interaction coverage: 404 CTA 可见性、Settings 直达态回归

### Quality checks

- `cargo test`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `cd web && bun run e2e -- settings.spec.ts`

## Visual Evidence

- 本任务不在规格中持久化截图资产。
- 视觉验收依赖稳定的 `Pages/Settings`、`Pages/Not Found` Storybook canvas，以及 `web/e2e/settings.spec.ts` 对直达 `/settings` 与未知前端路径 404 的回归覆盖。

## 方案概述（Approach, high-level）

- 后端不再使用 `not_found_service(index.html)` 把所有缺失路径强行压成 `404 + app shell`；改为先按静态资源路径处理，再仅对 HTML 导航请求执行 `index.html + 200` 回退。
- 前端通过根路由 `notFoundComponent` 明确承接未知路由，让源站 SPA fallback 与应用内 404 语义分层成立。
- 回归测试同时锁定“合法前端路由 200 / 未知前端路由 shell / 静态资源与 API 真实 404”三类契约，防止以后再退回白名单式补丁。

## 风险 / 假设

- 风险：若后续新增真实静态资源路径但命名不符合现有扩展名约定，fallback 判定需要同步扩展。
- 假设：生产代理/CDN 不会在源站修复后再次改写 document status。
