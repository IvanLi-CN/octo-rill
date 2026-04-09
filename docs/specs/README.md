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
| apras | 翻译请求单条记录制重建 | 已完成 | `apras-translation-request-single-record/SPEC.md` | 2026-03-09 | PR #35; checks green; review-loop clear; single-request contract live |
| nbz5z | Translation worker board follow-up | 已完成 | `nbz5z-translation-worker-board/SPEC.md` | 2026-03-27 | PR #32, PR #38; 3 general + 1 user_dedicated worker board + runtime lease recovery |
| gh3tz | 可配置 LLM 并行调度（取消固定限流） | 已完成 | `gh3tz-llm-max-concurrency/SPEC.md` | 2026-03-10 | PR #34; 24h summary-only UI; retry backoff requeues as queued with floor; main-list-only status grouping; non-blocking observable overrides |
| 67n8t | 全库主键 NanoID 化与公开标识收口 | 已完成 | `67n8t-nanoid-primary-keys/SPEC.md` | 2026-03-08 | PR #29; checks green; review-loop clear; destructive SQLite rebuild |
| 35r55 | 统一翻译调度器与独立管理界面改造 | 已完成 | `35r55-translation-scheduler/SPEC.md` | 2026-03-27 | PR #38; unified request scheduler + stale runtime recovery completed |
| gvxnw | 仓库级 Worktree Bootstrap | 已完成 | `gvxnw-worktree-bootstrap/SPEC.md` | 2026-03-06 | local implementation completed; repo-local hook installer + worktree smoke CI |
| s8qkn | 全局 Repo Release 复用与访问触发增量同步 | 已完成 | `s8qkn-subscription-sync/SPEC.md` | 2026-03-28 | PR #41; shared repo release cache + access refresh + staged dashboard sync |
| dynup | shadcn/ui 全量整改与组件收敛 | 已完成 | `dynup-shadcn-ui-full-remediation/SPEC.md` | 2026-03-08 | PR #25; dashboard/browser-timezone timestamp follow-up synced; Playwright merge-gate evidence aligned |
| x4k7m | 发布有效版本显示修复（API + Footer + Release 注入闭环） | 已完成 | `x4k7m-effective-version-surfacing/SPEC.md` | 2026-03-03 | PR #20；新增 `/api/version`，health/version 同源，footer 回退 health |
| 9vb46 | 管理员任务中心刷新闪烁修复 | 已完成 | `9vb46-admin-jobs-refresh-flicker-fix/SPEC.md` | 2026-03-07 | PR #23、PR #26；已补齐首载后筛选复载不闪烁，且后台刷新期间旧行交互已禁用 |
| vj7sr | 管理端任务详情可观测性增强（Release 批量翻译 + 日报） | 已完成 | `vj7sr-admin-job-detail-observability/SPEC.md` | 2026-03-08 | task drawer route outlet + in-drawer LLM detail navigation; recent-events browser-timezone sync + merge-gate evidence aligned |
| ejdn8 | 项目内安装 UI UX Pro Max skill 并放开 skills 跟踪 | 已完成 | `ejdn8-uipro-skill-install/SPEC.md` | 2026-02-26 | local installation + gitignore rule update |
| 3s4jc | Landing 登录页移除开发提示 | 已完成 | `3s4jc-landing-login-remove-dev-tip/SPEC.md` | 2026-02-26 | PR #15; review-loop added bootError e2e |
| g4456 | LLM 批处理效率改造 | 已完成 | `g4456-llm-batch-efficiency/SPEC.md` | 2026-03-30 | local implementation completed; release feed visible-window resolve flow, backend dedupe, and Playwright coverage aligned |
| 3k9fd | Release Feed 正文卡片与同步后后台翻译 | 已完成 | `3k9fd-release-feed-body-translation/SPEC.md` | 2026-04-03 | PR #44; body-based release feed cards + sync-triggered background translation |
| gd6zm | 管理员任务中心（二期）+ 用户管理字段补齐 | 已完成 | `gd6zm-admin-job-center-phase2/SPEC.md` | 2026-03-27 | PR #28, PR #38; admin jobs runtime recovery + stale running cleanup aligned |
| n6zd8 | 管理员面板一期（首登管理员 + 用户管理） | 已完成 | `n6zd8-admin-panel-user-management/SPEC.md` | 2026-02-25 | local implementation completed |
| regzy | OctoRill 文档站点与 Storybook 文档快车道实施 | 已完成 | `regzy-octo-rill-docs-site/SPEC.md` | 2026-03-09 | PR #31; docs-site + pages workflow + storybook docs; checks green; review-loop clear |
| erscd | 管理员任务中心：LLM 调度观测与调用排障 | 已完成 | `erscd-admin-llm-scheduler-observability/SPEC.md` | 2026-02-27 | local implementation completed |
| 96dp9 | Dashboard 同步入口收敛与顺序固定 | 已完成 | `96dp9-dashboard-sync-unification/SPEC.md` | 2026-03-27 | PR #39; single sync entry shipped with one retained visual evidence asset |
| g8p8z | 管理员任务中心 Tab 路由化 | 已完成 | `g8p8z-admin-jobs-tab-routing/SPEC.md` | 2026-03-27 | local implementation completed; pathname-driven primary tabs + translation `view` deep links + task drawer `from` restore |
| epn56 | 管理员任务中心运行时 worker 数量设置 | 已完成 | `epn56-admin-jobs-runtime-worker-settings/SPEC.md` | 2026-03-28 | PR #42; checks green; review-loop clear; runtime config dialogs + persisted hot updates |
| r8m4k | Dashboard 日报阅读流与详情弹窗修正 | 已完成 | `r8m4k-dashboard-brief-detail-auto-height/SPEC.md` | 2026-04-03 | PR #45; brief card grows with content and release detail now opens in a modal dialog |
| qvewp | Release 成功后回写 PR 版本评论 | 已完成 | `qvewp-release-pr-version-comment/SPEC.md` | 2026-04-04 | PR #46, PR #47; release run #43 comments PRs and rerun updates in place |
| xaycu | Dashboard 按日报边界分组与历史日报折叠 | 已完成 | `xaycu-dashboard-day-grouping/SPEC.md` | 2026-04-04 | local implementation completed; PR pending |
| tvujt | 品牌刷新：生成图接管 favicon / Web / Docs | 已完成 | `tvujt-brand-generated-icon-refresh/SPEC.md` | 2026-04-06 | local implementation completed; PR pending screenshot push approval |
| 7f2b9 | Release Feed 三 Tabs 与智能版本变化卡片 | 已完成 | `7f2b9-release-feed-smart-tabs/SPEC.md` | 2026-04-08 | PR #53; page-level lane selector, segmented selector polish, and visual evidence refreshed |
| h4yvc | 服务端版本更新轮询轻提示 | 已完成 | `h4yvc-version-update-polling-notice/SPEC.md` | 2026-04-10 | PR #57; version polling notice + visual evidence + regression coverage |
| crzva | Release 视图仓库图标补齐 | 已完成 | `crzva-release-repo-visuals/SPEC.md` | 2026-04-10 | local implementation completed; review-loop clear; release repo visuals shipped with aligned repo identity polish |
| zcp33 | Release reaction 扁平化重绘与圆形按钮收敛 | 部分完成（3/4） | `zcp33-release-reaction-bubble-polish/SPEC.md` | 2026-04-10 | Fluent Flat SVG assets + circular reaction trigger + external badge shipped locally; awaiting screenshot push approval |
