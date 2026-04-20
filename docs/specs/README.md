# 规格（Spec）总览

本目录用于管理工作项的**规格与追踪**：记录范围、验收标准、任务清单与状态，作为交付依据；实现与验证应以对应 `SPEC.md` 为准。

> Legacy compatibility: historical repos may still contain `docs/plan/**/PLAN.md`. New entries must be created under `docs/specs/**/SPEC.md`.

## 快速新增一个规格

1. 生成一个新的规格 `ID`（推荐 5 个字符的 nanoId 风格，降低并行建规格时的冲突概率）。
2. 新建目录：`docs/specs/<id>-<title>/`（`<title>` 用简短 slug，建议 kebab-case）。
3. 在该目录下创建 `SPEC.md`（模板见下方“SPEC.md 写法（简要）”）。
4. 在下方 Index 表新增一行，并把 `Status` 设为 `待设计` 或 `待实现`（取决于是否已冻结验收标准），并填入 `Last`（通常为当天）。

## 目录与命名规则

- 每个规格一个目录：`docs/specs/<id>-<title>/`
- `<id>`：推荐 5 个字符的 nanoId 风格，一经分配不要变更。
  - 推荐字符集（小写 + 避免易混淆字符）：`23456789abcdefghjkmnpqrstuvwxyz`
  - 正则：`[23456789abcdefghjkmnpqrstuvwxyz]{5}`
  - 兼容：若仓库历史已使用四位数字 `0001`-`9999`，允许继续共存。
- `<title>`：短标题 slug（建议 kebab-case，避免空格与特殊字符）；目录名尽量稳定。
- 人类可读标题写在 Index 的 `Title` 列；标题变更优先改 `Title`，不强制改目录名。

## 状态（Status）说明

仅允许使用以下状态值：

- `待设计`：范围/约束/验收标准尚未冻结，仍在补齐信息与决策。
- `待实现`：规格已冻结，可开工；实现与测试验证应以该规格为准。
- `跳过`：计划已冻结或部分完成，但**当前明确不应自动开工**（例如需要特定时机/外部条件/等待依赖）；自动挑选“下一个规格”时应跳过它。需要实现时再把状态改回 `待实现`（或由主人显式点名实现该规格）。
- `部分完成（x/y）`：实现进行中；`y` 为该规格里定义的“实现里程碑”数，`x` 为已完成“实现里程碑”数（见该规格 `SPEC.md` 的 Milestones；不要把计划阶段产出算进里程碑）。
- `已完成`：该规格已完成（实现已落地或将随某个 PR 落地）；如需关联 PR 号，写在 Index 的 `Notes`（例如 `PR #123`）。
- `作废`：不再推进（取消/价值不足/外部条件变化）。
- `重新设计（#<id>）`：该规格被另一个规格取代；`#<id>` 指向新的规格编号。

## `Last` 字段约定（推进时间）

- `Last` 表示该规格**上一次“推进进度/口径”**的日期，用于快速发现长期未推进的规格。
- 仅在以下情况更新 `Last`（不要因为改措辞/排版就更新）：
  - `Status` 变化（例如 `待设计` -> `待实现`，或 `部分完成（x/y）` -> `已完成`）
  - `Notes` 中写入/更新 PR 号（例如 `PR #123`）
  - `SPEC.md` 的里程碑勾选变化
  - 范围/验收标准冻结或发生实质变更

## SPEC.md 写法（简要）

每个规格的 `SPEC.md` 至少应包含：

- 背景/问题陈述（为什么要做）
- 目标 / 非目标（做什么、不做什么）
- 范围（in/out）
- 需求列表（MUST/SHOULD/COULD）
- 功能与行为规格（Functional/Behavior Spec：核心流程/关键边界/错误反馈）
- 验收标准（Given/When/Then + 边界/异常）
- 实现前置条件（Definition of Ready / Preconditions；未满足则保持 `待设计`）
- 非功能性验收/质量门槛（测试策略、质量检查、Storybook/视觉回归等按仓库已有约定）
- 文档更新（需要同步更新的项目设计文档/架构说明/README/ADR）
- 实现里程碑（Milestones，用于驱动 `部分完成（x/y）`；只写实现交付物，不要包含计划阶段产出）
- 风险与开放问题（需要决策的点）
- 假设（需主人确认）

## Index（固定表格）

| ID | Title | Status | Spec | Last | Notes |
| --- | --- | --- | --- | --- | --- |
| fvh8d | Release 失败 Telegram 告警接入 | 已完成 | `fvh8d-release-failure-telegram-alerts/SPEC.md` | 2026-04-11 | fast-track / shared github-workflows notifier rollout / smoke test target |
| apras | 翻译请求单条记录制重建 | 已完成 | `apras-translation-request-single-record/SPEC.md` | 2026-03-09 | PR #35; checks green; review-loop clear; single-request contract live |
| nbz5z | Translation worker board follow-up | 已完成 | `nbz5z-translation-worker-board/SPEC.md` | 2026-03-27 | PR #32, PR #38; 3 general + 1 user_dedicated worker board + runtime lease recovery |
| gh3tz | 可配置 LLM 并行调度（取消固定限流） | 已完成 | `gh3tz-llm-max-concurrency/SPEC.md` | 2026-03-10 | PR #34; 24h summary-only UI; retry backoff requeues as queued with floor; main-list-only status grouping; non-blocking observable overrides |
| 67n8t | 全库主键 NanoID 化与公开标识收口 | 已完成 | `67n8t-nanoid-primary-keys/SPEC.md` | 2026-03-08 | PR #29; checks green; review-loop clear; destructive SQLite rebuild |
| 35r55 | 统一翻译调度器与独立管理界面改造 | 已完成 | `35r55-translation-scheduler/SPEC.md` | 2026-03-27 | PR #38; unified request scheduler + stale runtime recovery completed |
| gvxnw | 仓库级 Worktree Bootstrap | 已完成 | `gvxnw-worktree-bootstrap/SPEC.md` | 2026-03-06 | local implementation completed; repo-local hook installer + worktree smoke CI |
| s8qkn | 全局 Repo Release 复用与访问触发增量同步 | 已完成 | `s8qkn-subscription-sync/SPEC.md` | 2026-04-12 | PR #41; shared repo release cache + access refresh + scheduler social/inbox sync |
| dynup | shadcn/ui 全量整改与组件收敛 | 已完成 | `dynup-shadcn-ui-full-remediation/SPEC.md` | 2026-03-08 | PR #25; dashboard/browser-timezone timestamp follow-up synced; Playwright merge-gate evidence aligned |
| x4k7m | 发布有效版本显示修复（API + Footer + Release 注入闭环） | 已完成 | `x4k7m-effective-version-surfacing/SPEC.md` | 2026-04-15 | PR #20；新增 `/api/version`，health/version 同源，footer 回退 health；release web-builder fallback + build gate docker smoke + historical backfill overlay |
| 9vb46 | 管理员任务中心刷新闪烁修复 | 已完成 | `9vb46-admin-jobs-refresh-flicker-fix/SPEC.md` | 2026-03-07 | PR #23、PR #26；已补齐首载后筛选复载不闪烁，且后台刷新期间旧行交互已禁用 |
| vj7sr | 管理端任务详情可观测性增强（Release 批量翻译 + 日报） | 已完成 | `vj7sr-admin-job-detail-observability/SPEC.md` | 2026-03-08 | task drawer route outlet + in-drawer LLM detail navigation; recent-events browser-timezone sync + merge-gate evidence aligned |
| 3s4jc | Landing 登录页移除开发提示 | 已完成 | `3s4jc-landing-login-remove-dev-tip/SPEC.md` | 2026-02-26 | PR #15; review-loop added bootError e2e |
| g4456 | LLM 批处理效率改造 | 已完成 | `g4456-llm-batch-efficiency/SPEC.md` | 2026-03-30 | local implementation completed; batching/dedupe contract landed; runtime input-limit source superseded by #y2yf8 |
| 3k9fd | Release Feed 正文卡片与同步后后台翻译 | 重新设计（#y2yf8） | `3k9fd-release-feed-body-translation/SPEC.md` | 2026-04-13 | historical body-limit contract superseded by #y2yf8 LLM-input-budget chunk translation |
| gd6zm | 管理员任务中心（二期）+ 用户管理字段补齐 | 已完成 | `gd6zm-admin-job-center-phase2/SPEC.md` | 2026-03-27 | PR #28, PR #38; admin jobs runtime recovery + stale running cleanup aligned |
| n6zd8 | 管理员面板一期（首登管理员 + 用户管理） | 已完成 | `n6zd8-admin-panel-user-management/SPEC.md` | 2026-02-25 | local implementation completed |
| regzy | OctoRill 文档站点与 Storybook 文档快车道实施 | 已完成 | `regzy-octo-rill-docs-site/SPEC.md` | 2026-03-09 | PR #31; docs-site + pages workflow + storybook docs; checks green; review-loop clear |
| erscd | 管理员任务中心：LLM 调度观测与调用排障 | 已完成 | `erscd-admin-llm-scheduler-observability/SPEC.md` | 2026-02-27 | local implementation completed |
| 96dp9 | Dashboard 同步入口收敛与顺序固定 | 已完成 | `96dp9-dashboard-sync-unification/SPEC.md` | 2026-03-27 | PR #39; single sync entry shipped with one retained visual evidence asset |
| g8p8z | 管理员任务中心 Tab 路由化 | 已完成 | `g8p8z-admin-jobs-tab-routing/SPEC.md` | 2026-03-27 | local implementation completed; pathname-driven primary tabs + translation `view` deep links + task drawer `from` restore |
| epn56 | 管理员任务中心运行时 worker 数量设置 | 重新设计（#y2yf8） | `epn56-admin-jobs-runtime-worker-settings/SPEC.md` | 2026-04-13 | historical worker-only contract superseded by #y2yf8 translation runtime settings |
| r8m4k | Dashboard 日报阅读流与详情弹窗修正 | 已完成 | `r8m4k-dashboard-brief-detail-auto-height/SPEC.md` | 2026-04-03 | PR #45; brief card grows with content and release detail now opens in a modal dialog |
| qvewp | Release 成功后回写 PR 版本评论 | 已完成 | `qvewp-release-pr-version-comment/SPEC.md` | 2026-04-04 | PR #46, PR #47; release run #43 comments PRs and rerun updates in place |
| xaycu | Dashboard 按日报边界分组与历史日报折叠 | 已完成 | `xaycu-dashboard-day-grouping/SPEC.md` | 2026-04-16 | top-level tab copy aligned to `发布`; spec wording refreshed for current dashboard labels |
| tvujt | 品牌刷新：生成图接管 favicon / Web / Docs | 已完成 | `tvujt-brand-generated-icon-refresh/SPEC.md` | 2026-04-06 | local implementation completed; PR pending screenshot push approval |
| 7f2b9 | Release Feed 三 Tabs 与智能版本变化卡片 | 已完成 | `7f2b9-release-feed-smart-tabs/SPEC.md` | 2026-04-08 | PR #53; page-level lane selector, segmented selector polish, and visual evidence refreshed |
| h4yvc | 服务端版本更新轮询轻提示 | 已完成 | `h4yvc-version-update-polling-notice/SPEC.md` | 2026-04-10 | PR #57; version polling notice + visual evidence + regression coverage |
| crzva | Release 视图仓库图标补齐 | 已完成 | `crzva-release-repo-visuals/SPEC.md` | 2026-04-10 | local implementation completed; review-loop clear; release repo visuals shipped with aligned repo identity polish |
| vgqp9 | Dashboard 社交活动记录扩展（含头像） | 已完成 | `vgqp9-dashboard-social-activity/SPEC.md` | 2026-04-16 | dashboard social activity labels aligned to `发布 / 加星 / 关注 / 收件箱`; spec wording refreshed for current top-level tabs |
| 76bxs | Dashboard 页头品牌优先重设计 | 已完成 | `76bxs-dashboard-header-brand-layout/SPEC.md` | 2026-04-10 | PR #61; brand-first header, avatar popover, and refreshed visual evidence assets |
| g63t8 | Release 视图固定 owner/org avatar | 已完成 | `g63t8-release-avatar-only-visuals/SPEC.md` | 2026-04-10 | local implementation completed; avatar-only repo identity follow-up for PR #58 behavior |
| zcp33 | Release reaction 扁平化重绘与圆形按钮收敛 | 已完成 | `zcp33-release-reaction-bubble-polish/SPEC.md` | 2026-04-10 | PR #59; Fluent Flat SVG assets + circular reaction trigger + external badge + Storybook evidence landed |
| u6b32 | Web 暗色模式接通 | 已完成 | `u6b32-web-dark-mode/SPEC.md` | 2026-04-11 | PR #63; header-based theme toggle, Storybook evidence, and review-loop clear |
| at76w | 修复 Release 自动发版断链并补齐漏发版本 | 待实现 | `at76w-release-reliability-backfill/SPEC.md` | 2026-04-11 | release trigger cut over to push@main + backfill queue planned |
| 2nsc2 | Landing 登录页重做：降噪文案、重构布局、修复移动端 CTA | 已完成 | `2nsc2-landing-login-refresh/SPEC.md` | 2026-04-12 | PR #72; fast-track / storybook canvas evidence / mobile CTA above the fold |
| y2yf8 | Release 翻译输入预算与运行时设置收口 | 已完成 | `y2yf8-release-translation-input-budget-runtime/SPEC.md` | 2026-04-13 | local implementation completed; runtime `ai_model_context_limit`; release_detail chunk translation unified; visual evidence landed |
| jfkcf | Release reaction 反馈图标轻量收敛 | 已完成 | `jfkcf-release-reaction-compact-size/SPEC.md` | 2026-04-12 | local implementation completed; compact reaction trigger + Storybook canvas evidence |
| p82d7 | Dashboard / Admin 移动端壳层与顶栏收敛优化 | 部分完成（4/4） | `p82d7-dashboard-admin-mobile-shell-polish/SPEC.md` | 2026-04-14 | PR #77; visual evidence refreshed and mobile shell gesture polish landed |
| y9qpf | TanStack Router 接管前端路由并消除登录页闪现 | 已完成 | `y9qpf-tanstack-router-auth-boot-no-login-flicker/SPEC.md` | 2026-04-15 | PR #80; fast-track; TanStack Router SPA routing + three-layer startup model + build-time version monitor landed |
| 2bhas | Dashboard 社交卡片移动端横向紧凑重设计 | 已完成 | `2bhas-dashboard-social-mobile-compact-layout/SPEC.md` | 2026-04-18 | local follow-up fixes target entity-group trailing whitespace; refreshed Storybook edge-case evidence pending owner approval before push |
| gzyja | 全局字标路径化与几何对齐修复 | 部分完成（2/3） | `gzyja-wordmark-path-geometry-fix/SPEC.md` | 2026-04-15 | fast-track follow-up to #tvujt; local implementation + visual evidence landed; PR pending screenshot push approval |
| 6x959 | Dashboard 启动骨架页头与 Tabs 占位收敛 | 已完成 | `6x959-dashboard-startup-skeleton-header-tabs-alignment/SPEC.md` | 2026-04-16 | fast-track follow-up to #y9qpf; warm skeleton shell alignment + visual evidence |
| 67g9w | Dashboard SPA 导航避免回退启动骨架 | 已完成 | `67g9w-spa-nav-startup-skeleton-guard/SPEC.md` | 2026-04-17 | fast-track; shell hydration gate + local feed skeleton + visual evidence |
| w9by9 | Dashboard 移动端日分组标题防重叠修复 | 部分完成（3/4） | `w9by9-dashboard-mobile-day-divider-overlap/SPEC.md` | 2026-04-16 | fast-track; local implementation + visual evidence + review clear; PR pending screenshot push approval |
| qvfxq | Release 日报内容格式 V2 与历史快照修复 | 已完成 | `qvfxq-release-daily-brief-v2/SPEC.md` | 2026-04-19 | fast-track / canonical brief markdown validator + refresh drift repair landed |
| m2k8d | 管理后台仪表盘与 rollup 统计 | 已完成 | `m2k8d-admin-dashboard-rollups/SPEC.md` | 2026-04-18 | local implementation completed; Recharts dashboard + daily rollup analytics |
| n4x7e | Dashboard 历史日报折叠补齐与社交空态裁剪 | 已完成 | `n4x7e-dashboard-brief-social-folding/SPEC.md` | 2026-04-19 | PR #96; fast-track follow-up to #xaycu / #qvfxq merged with historical social folding, optional social brief sections, and visual evidence |
| cm2je | 移除项目内 UI UX Pro Max skill | 已完成 | `cm2je-remove-ui-ux-pro-max-skill/SPEC.md` | 2026-04-18 | local implementation completed; removed project-local skill assets and obsolete install spec |
| em5uh | 公开文档体系重写与职责收口 | 已完成 | `em5uh-public-docs-rewrite/SPEC.md` | 2026-04-19 | PR #95; fast-track / public docs rewrite / PR ready |
| y9ngx | LinuxDO 绑定与用户设置页改造 | 已完成 | `y9ngx-linuxdo-user-settings/SPEC.md` | 2026-04-20 | PR #94; fast-track / unified settings page + LinuxDO binding + inline PAT fallback + PAT autofill guard |
| gms6p | 移除公开文档中的 Storybook 导览页 | 已完成 | `gms6p-remove-storybook-guide-page/SPEC.md` | 2026-04-19 | fast-track follow-up to #em5uh; docs-site now links directly to Storybook without guide page |
| pnzd2 | Dashboard 启动期请求风暴热修复 | 已完成 | `pnzd2-dashboard-startup-request-storm/SPEC.md` | 2026-04-20 | fast-track / dashboard bootstrap request storm hotfix |
| w5gaz | “我的发布”开关与自有仓库 Release 可见性扩展 | 部分完成（3/4） | `w5gaz-owned-release-opt-in/SPEC.md` | 2026-04-20 | PR #101 open; local implementation + validation + owner-facing evidence landed |
| 7yr2m | Dashboard 移动端 release 卡片操作收敛 | 部分完成（3/4） | `7yr2m-dashboard-mobile-release-card-action-polish/SPEC.md` | 2026-04-20 | local implementation + storybook/mobile e2e + visual evidence landed; push/PR pending owner screenshot approval |
| 25pe9 | SPA document fallback 与全局 404 页面收口 | 部分完成（3/4） | `25pe9-spa-document-fallback/SPEC.md` | 2026-04-20 | local implementation + regression coverage complete; PR path proceeds without persisted screenshot assets |
| bydfx | Web 前端懒路由与按需拆包 | 待实现 | `bydfx-web-lazy-route-code-splitting/SPEC.md` | 2026-04-20 | fast-track / lazy routes + branch-level split |
