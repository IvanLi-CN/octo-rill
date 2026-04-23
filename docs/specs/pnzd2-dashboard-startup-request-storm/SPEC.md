# Dashboard 启动期请求风暴热修复（#pnzd2）

## 背景 / 问题陈述

- 生产环境 Dashboard 在首屏进入后持续重放 `GET /api/briefs` 与 `GET /api/reaction-token/status`，Network 面板会快速堆积大量 pending fetch。
- 线上实测同一会话内，`/api/briefs` 与 `/api/reaction-token/status` 均能在短时间内被重复触发百次以上，而 `/api/version` 仅维持既有单独轮询，不是本轮根因。
- 根因收敛为：Dashboard 首屏 bootstrap effect 依赖 `loadReactionToken`，而 `useReactionTokenEditor()` 又因为消费了 render 期漂移的回调，导致 `loadReactionToken` 身份变化；effect 被重新执行后，又会再次触发 sidebar 与 reaction-token 状态请求，形成自激循环。
- 当重复请求持续堆积时，边缘层会开始返回 `ERR_HTTP2_SERVER_REFUSED_STREAM` / `554`，页面出现 `Failed to fetch`，属于真实线上故障而不是单纯的开发态重复渲染。

## 目标 / 非目标

### Goals

- 切断 Dashboard 首屏 mount 期间对 `/api/briefs` 与 `/api/reaction-token/status` 的自触发循环。
- 保持 `useReactionTokenEditor()` 的外部接口不变，但让 `loadReactionToken` / `savePat` 不再因为 caller re-render 而漂移身份。
- 将 Dashboard 的 sidebar + PAT 启动 bootstrap 明确为 mount-only 语义，只允许显式 refresh 或任务流刷新重新进入。
- 补齐 Playwright 回归，固定“首屏只打一轮 bootstrap 请求 + tab 切换不额外重打”的长期约束。

### Non-goals

- 不修改 Rust 后端接口、数据库、限流策略或边缘层配置。
- 不改 `/api/version` 的轮询合约，也不顺手重做 Settings / PAT 产品信息架构。
- 不做与本次行为热修复无关的视觉布局调整或 Storybook 资产刷新。

## 范围（Scope）

### In scope

- `/Users/ivan/.codex/worktrees/87aa/octo-rill/web/src/pages/Dashboard.tsx`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/web/src/settings/reactionTokenEditor.ts`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/web/e2e/dashboard-social-activity.spec.ts`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/docs/specs/pnzd2-dashboard-startup-request-storm/SPEC.md`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/docs/specs/README.md`

### Out of scope

- `/Users/ivan/.codex/worktrees/87aa/octo-rill/src/**`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/web/src/version/versionMonitor.tsx`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/web/src/pages/Settings.tsx` 的 UI 结构与 section 信息架构

## 需求（Requirements）

### MUST

- Dashboard 首屏稳定后，`/api/briefs` 与 `/api/reaction-token/status` 只允许各出现一轮 bootstrap 请求，不得继续自增。
- `refreshSidebar` 与 `loadReactionToken` 不能因为普通 render 或 feed tab 切换重复触发。
- `useReactionTokenEditor()` 在存在 `onStatusLoaded` / `onPatSaved` 回调时，仍需保持 `loadReactionToken` / `savePat` 对外身份稳定。
- `全部 -> 发布 / 加星 / 关注` 的普通 tab 切换不得新增 `briefs` / `reaction-token/status` 请求计数。
- PAT 读取、校验、保存、快捷 fallback 与 Settings 页读取语义必须保持不变。

### SHOULD

- Dashboard 页面层仍然应该用 mount-only guard 明确保护启动 bootstrap，避免未来其它依赖漂移再次把副作用绑回 render 链。
- Playwright 回归应在 idle settle 窗口内断言请求计数，避免只验证 DOM 而遗漏静默请求风暴。

### COULD

- 无。

## 功能与行为规格（Functional / Behavior Spec）

### Core flows

- 用户首次打开 Dashboard 时，页面仍会各自请求 feed、briefs 与 reaction token status，但 `briefs` 和 `reaction token status` 只运行一次。
- 首屏 bootstrap 完成后，Dashboard 普通 re-render、顶部 tab 切换、局部 loading skeleton 与 reaction 状态展示都不得重新触发 mount-only bootstrap。
- 用户显式点击同步、任务流完成后触发 `refreshAll`、或主动保存 PAT 时，相关数据仍可按既有路径刷新。
- Settings 页面继续复用同一个 `useReactionTokenEditor()`，但不会因为回调身份漂移造成重复 auto-load。

### Edge cases / errors

- 若首屏 bootstrap 的 `briefs` 或 `reaction token status` 请求失败，Dashboard 仍按现有 `bootError` 路径展示错误，不自动重试成风暴。
- 如果 Dashboard future render 中再次传入新的内联回调，hook 内部也必须保证只消费最新回调，不改变稳定函数身份。
- `/api/version` 继续由既有版本监视逻辑控制，本轮不视为异常请求。

## 验收标准（Acceptance Criteria）

- Given 用户首次进入已登录 Dashboard
  When 页面完成首屏稳定
  Then `/api/feed`、`/api/briefs`、`/api/reaction-token/status` 各只出现一轮启动请求。

- Given Dashboard 已完成首屏 bootstrap
  When 用户点击 `加星`
  Then 页面壳层继续保留，且 `/api/briefs` 与 `/api/reaction-token/status` 的调用次数都不增加。

- Given `useReactionTokenEditor()` 收到新的 `onStatusLoaded` / `onPatSaved`
  When caller 发生普通 re-render
  Then `loadReactionToken` 与 `savePat` 的外部身份保持稳定，hook 只消费最新回调实现。

- Given 修复已部署到生产环境
  When 在真实 Dashboard 页面观察 Network / Console
  Then 不再出现成批 pending `briefs/status` 请求，也不再持续刷 `ERR_HTTP2_SERVER_REFUSED_STREAM` / `554`。

## 实现前置条件（Definition of Ready / Preconditions）

- Dashboard 已有独立的 `refreshSidebar`、`refreshAll`、`loadReactionToken` 启动路径。
- `useReactionTokenEditor()` 已被 Dashboard 与 Settings 共同使用，允许通过内部稳定 ref 调整回调消费方式。
- Playwright `dashboard-social-activity.spec.ts` 已具备拦截 `briefs` 与 `reaction-token/status` 计数的基础桩。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cd /Users/ivan/.codex/worktrees/87aa/octo-rill/web && bun run build`
- `cd /Users/ivan/.codex/worktrees/87aa/octo-rill/web && bun run e2e -- dashboard-social-activity.spec.ts app-auth-boot.spec.ts`
- `cd /Users/ivan/.codex/worktrees/87aa/octo-rill && codex -m gpt-5.4 -c model_reasoning_effort=\"medium\" -a never review --base main`

### UI / Storybook (if applicable)

- N/A：本轮属于行为 / 网络层热修复，无新增 owner-facing 视觉差异。

## Visual Evidence

- 不要求新增截图资产；本轮以 Playwright 计数回归、生产 Network/Console smoke 与源码链路校验作为交付证据。

## 方案概述（Approach, high-level）

- 在 `useReactionTokenEditor()` 内把 `onStatusLoaded` / `onPatSaved` 放入 ref 并以 effect 同步最新值，再让 `loadReactionToken` / `savePat` 读取 ref，而不是把 callback 直接放进 `useCallback` 依赖。
- 在 Dashboard 页面层为首屏 `refreshSidebar + loadReactionToken` 增加 mount-only guard，并把 reaction status 回调改为稳定 callback，防止未来再把首屏 bootstrap 绑回 render 链。
- 复用现有 Playwright dashboard 社交流程，在 tab 切换测试里加入 `briefs` / `reaction-token/status` 的启动计数断言，保证问题可长期回归。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若未来再把新的 mount-only 数据拉取直接耦合到 render 漂移依赖，仍可能出现新的请求风暴，需要继续按“稳定 callback + mount guard”模式治理。
- 开放问题：生产 smoke 依赖部署生效与可用登录态；若部署未完成，需等待最新前端构建上线后再复核。
- 假设：显式 refresh、task stream refresh 与 PAT 保存后的状态更新仍由既有显式触发链路负责，不依赖首次 mount effect 再次执行。

## 参考（References）

- `/Users/ivan/.codex/worktrees/87aa/octo-rill/docs/specs/67g9w-spa-nav-startup-skeleton-guard/SPEC.md`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/docs/specs/y9ngx-linuxdo-user-settings/SPEC.md`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/web/src/pages/Dashboard.tsx`
- `/Users/ivan/.codex/worktrees/87aa/octo-rill/web/src/settings/reactionTokenEditor.ts`
