# SPA 内导航全局骨架回退修复（#67g9w）

## 背景 / 问题陈述

- Dashboard 在首次 hydration 完成后，用户通过 SPA 直接切换 `全部 / 发布 / 加星 / 关注` 这类 feed-backed tabs 时，URL 已经完成前端导航，但主页面仍会因为新的 feed 请求短暂回退到 `DashboardStartupSkeleton`。
- 这种“整页骨架回退”会把 header、tabs、notice 与 footer 一并卸载，看起来像整页重启，而不是同一工作台内部的局部加载。
- 现有 warm skeleton 语义只应该用于首屏启动阶段；一旦 AppShell 已经稳定挂载，后续 tab 切换应降级为内容区局部 loading，而不是再次显示全局启动骨架。

## 目标 / 非目标

### Goals

- 将 Dashboard 的“首次 shell hydration”与“后续 feed tab 切换”明确拆分成两层 loading 语义。
- 保证 `DashboardStartupSkeleton` 只在首屏启动阶段出现，后续 SPA 导航始终保留现有 AppShell。
- 为局部 feed loading 补稳定的 Storybook 入口、视觉证据与 Playwright 回归。
- 顺手审计 admin 相关 startup skeleton guard，确认本轮问题没有在同类 SPA 导航里复现。

### Non-goals

- 不改冷启动 `AppBoot` 或硬刷新的 auth bootstrap 语义。
- 不改 Rust 后端、API、数据库、分页契约或路由 contract。
- 不顺手重构无复现证据的 Admin Users / Admin Jobs loading 结构。

## 范围（Scope）

### In scope

- `web/src/pages/Dashboard.tsx`
- `web/src/feed/FeedList.tsx`
- `web/src/feed/FeedGroupedList.tsx`
- `web/src/stories/Dashboard.stories.tsx`
- `web/e2e/dashboard-social-activity.spec.ts`
- `docs/specs/67g9w-spa-nav-startup-skeleton-guard/SPEC.md`
- `docs/specs/README.md`

### Out of scope

- `src/**`
- `web/src/pages/AppBoot.tsx`
- `web/src/routes/__root.tsx`
- `web/src/routes/admin/**` 的认证启动逻辑

## 需求（Requirements）

### MUST

- 首次 hydrated 之后，Dashboard 任意 feed-backed tab 切换都不得再次显示 `DashboardStartupSkeleton`。
- `全部 -> 加星` 的延迟请求期间，AppShell、header、tabs、notice、footer 必须持续可见。
- tab 切换期间只允许 feed 主列出现局部 loading skeleton；旧数据需要先清空，避免串 tab 残留。
- `refreshSidebar` 与 `loadReactionTokenStatus` 不得因为普通 feed tab 切换重复触发；它们只在页面初次进入或显式刷新时运行。
- Storybook 必须提供稳定的“post-boot tab pending without global skeleton”入口。
- Playwright 必须回归已复现路径，并断言 `data-dashboard-boot-header` / `data-app-boot` 在 SPA 切换期间都不会再出现。

### SHOULD

- 局部 loading skeleton 应提供稳定 selector，便于 Storybook / Playwright 断言。
- warm-start 命中时，sidebar 刷新应默认走 background 模式，避免重新挡住 shell。

### COULD

- 无。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 用户首次进入 Dashboard 且 feed / sidebar 仍未就绪时，页面继续允许显示 `DashboardStartupSkeleton`，直到 shell hydration 完成。
- 一旦 shell hydration 完成，用户切换 `全部 / 发布 / 加星 / 关注` 任意 feed-backed tab 时，路由与顶部壳层保持原位，只让 feed panel 切到局部 skeleton。
- 当新的 feed 数据返回后，局部 skeleton 消失并切换到目标 tab 的真实数据集，不保留上一 tab 的残留条目。
- Storybook `Evidence / Post-Boot Stars Tab Loading` 直接固定在“首屏已就绪、stars 数据待回包”的状态，用于稳定截图与人工审阅。

### Edge cases / errors

- 若 tab 切换发生在 warm-start 之后，即使后台 sidebar 刷新仍在进行中，也不得重新回到启动骨架。
- `发布` 与 `关注` 共用相同 feed-request 切换路径，本轮修复必须覆盖这一共性路径，而不是只修 `加星`。
- admin route startup skeleton 仅在 auth bootstrap 阶段由 route guard 控制；本轮审计未发现与 Dashboard 相同的“SPA tab 切换误触发”路径。

## 验收标准（Acceptance Criteria）

- Given Dashboard 已完成首次 hydration
  When 用户点击 `加星`
  Then URL 更新为 `/?tab=stars`，且 `data-dashboard-secondary-controls` 仍保持可见。

- Given `/api/feed?types=stars` 仍在 pending
  When 页面处于 tab 切换中的 loading 态
  Then 仅出现 `data-feed-loading-skeleton="true"`，且 `data-dashboard-boot-header` 与 `data-app-boot` 都保持为 0。

- Given stars 数据返回
  When 页面完成本次切换
  Then 新数据集（如 `octocat-new` / Storybook 中的 `torvalds`）可见，且局部 skeleton 消失。

- Given 用户切换 `全部 -> 加星`
  When 检查 sidebar / reaction bootstrap 请求
  Then `notifications`、`briefs`、`reaction-token/status` 的调用次数不因 tab 切换额外增加。

- Given 审查 Admin Users / Admin Jobs 的启动骨架 guard
  When 对照当前实现路径
  Then 仅在 auth bootstrap 或空表首载时显示骨架，本轮无需额外代码改动。

## 实现前置条件（Definition of Ready / Preconditions）

- Dashboard 的 feed tab 已经通过 `types=releases|stars|followers` 在前端切换数据集。
- Storybook 与 Playwright 已可用，可作为本轮 UI 证据与回归入口。
- 本轮不触碰 `AppBoot` 与路由层 auth bootstrap 合约。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cd /Users/ivan/.codex/worktrees/1edd/octo-rill/web && bun run lint`
- `cd /Users/ivan/.codex/worktrees/1edd/octo-rill/web && bun run build`
- `cd /Users/ivan/.codex/worktrees/1edd/octo-rill/web && bun run storybook:build`
- `cd /Users/ivan/.codex/worktrees/1edd/octo-rill/web && bun run e2e -- dashboard-social-activity.spec.ts app-auth-boot.spec.ts`
- `cd /Users/ivan/.codex/worktrees/1edd/octo-rill && codex -m gpt-5.4 -c model_reasoning_effort="medium" -a never review --base main`（latest PR head merge-proof，无新的可操作回归）

### UI / Storybook (if applicable)

- Stories to add/update: `web/src/stories/Dashboard.stories.tsx`
- Docs pages / state galleries to add/update: `Pages/Dashboard` autodocs
- `play` / interaction coverage to add/update: `PostBootStarsTabSwitchKeepsShell`
- Visual evidence source: `storybook_canvas`
- Owner-facing screenshot persistence: 不要求；主人通过本地浏览器手测验收

## Visual Evidence

- 主人已明确选择“不需要截图”；本轮以稳定 Storybook pending story 作为回归入口，并通过本地浏览器手测完成验收。
- 主人已在真实应用页 `http://127.0.0.1:55174/` 手测 `全部 -> 加星` 的 SPA 切换链路，确认壳层持续保留且未再回退全局 startup skeleton。
- 验收关注点：Dashboard 已完成首屏 hydration 后，切到 `加星` 时只让主列进入局部 skeleton；页头、tabs、notice 与 footer 继续保留，不再回退到全局 startup skeleton。

## 方案概述（Approach, high-level）

- 在 Dashboard 内引入 `shellHydrated` 门槛，让 `DashboardStartupSkeleton` 仅在首屏 feed + sidebar 都未完成前可见。
- 将 `loadInitialFeed` 与 sidebar / reaction-token bootstrap 拆成独立 effect，避免 feed type 变化重新触发整套“启动期”副作用。
- 在 `FeedList` / `FeedGroupedList` 内为局部 loading skeleton 提供统一 selector 与可访问性标记，方便交互断言与稳定截图。
- 通过 Storybook pending story 与 Playwright 延迟接口回归，把“URL 已切换但壳层不卸载”固定成长期约束。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：如果未来再把其它 mount-only bootstrap side effects 绑回 feed type 依赖，仍可能重新引入整页骨架回退。
- 开放问题：无。
- 假设：Dashboard 的 feed-backed tabs 继续共用当前 `useFeed` 初载语义，本轮仅在页面层切开启动骨架边界。

## 参考（References）

- `docs/specs/vgqp9-dashboard-social-activity/SPEC.md`
- `docs/specs/6x959-dashboard-startup-skeleton-header-tabs-alignment/SPEC.md`
- `docs/specs/y9qpf-tanstack-router-auth-boot-no-login-flicker/SPEC.md`
- `web/src/pages/Dashboard.tsx`
- `web/src/stories/Dashboard.stories.tsx`
