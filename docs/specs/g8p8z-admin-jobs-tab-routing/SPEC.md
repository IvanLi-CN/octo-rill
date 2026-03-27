# 管理员任务中心 Tab 路由化（#g8p8z）

## 状态

- Status: 已完成
- Created: 2026-03-27
- Last: 2026-03-27

## 背景 / 问题陈述

当前 `/admin/jobs` 已经把任务详情抽屉路由化为 `/admin/jobs/tasks/:taskId` 与 `/admin/jobs/tasks/:taskId/llm/:callId`，但页面内两层核心切换仍停留在组件局部 state：

- 一级 tabs `实时异步任务 / 定时任务 / LLM调度 / 翻译调度` 只切 UI，不改 URL。
- 翻译调度内二级 tabs `需求队列 / 任务记录` 只切 UI，不改 URL。
- 刷新、深链与浏览器前进后退无法稳定恢复当前视图。
- 从非默认 tab 打开任务详情后，关闭抽屉会退回 `/admin/jobs`，丢失来源上下文。

需要把这两层切换提升为路由状态，并保持现有任务详情抽屉链路兼容。

## 目标 / 非目标

### Goals

- 将 `/admin/jobs` 一级 tabs 提升为 `pathname` 路由。
- 将翻译调度二级 tabs 提升为 `query` 路由。
- 保持任务详情抽屉 path 路由兼容，并补齐来源 tab 恢复能力。
- 支持深链、刷新恢复以及浏览器前进/后退。
- 补齐 Storybook 与 Playwright，使 route state 成为稳定可验证入口。

### Non-goals

- 不把筛选器、分页参数或全局 LLM 详情 sheet 一并做成 URL 状态。
- 不把翻译请求 / 批次 / worker 抽屉做成独立路由。
- 不修改 `/api/admin/jobs/**` 接口、SSE 事件格式或后端调度逻辑。

## 路由契约

- `/admin/jobs` => `实时异步任务`
- `/admin/jobs/scheduled` => `定时任务`
- `/admin/jobs/llm` => `LLM 调度`
- `/admin/jobs/translations?view=queue` => `翻译调度 / 需求队列`
- `/admin/jobs/translations?view=history` => `翻译调度 / 任务记录`
- `/admin/jobs/tasks/:taskId` => 任务详情抽屉
- `/admin/jobs/tasks/:taskId/llm/:callId` => 任务抽屉内 LLM 详情
- 任务详情路径支持可选 query `from=<realtime|scheduled|llm|translations>`，用于关闭抽屉或从任务内 LLM 详情返回时恢复来源一级 tab。
- `translations` 路径缺失或非法 `view` 时，前端必须规范化到 `?view=queue`。
- 不新增 `/admin/jobs/realtime` 别名；`/admin/jobs` 继续作为默认实时任务入口。

## 验收标准

- Given 管理员直接访问 `/admin/jobs/scheduled`
  When 页面完成首载
  Then `定时任务` tab 被选中，且无需二次点击即可看到对应内容。

- Given 管理员在 `/admin/jobs` 点击 `LLM调度`
  When tab 切换完成
  Then URL 变为 `/admin/jobs/llm`，并可通过浏览器后退回到 `/admin/jobs`。

- Given 管理员在 `/admin/jobs/translations?view=queue`
  When 点击 `任务记录`
  Then URL 更新为 `/admin/jobs/translations?view=history`，顶部工作者板持续可见。

- Given 管理员从 `定时任务` 打开任务详情
  When 抽屉路由建立
  Then URL 带有 `?from=scheduled`，关闭抽屉后返回 `/admin/jobs/scheduled`。

- Given 管理员从 `/admin/jobs/tasks/:taskId/llm/:callId?from=llm` 点击 `返回任务详情`
  When 返回完成
  Then 页面回到 `/admin/jobs/tasks/:taskId?from=llm`，并可继续关闭回 `/admin/jobs/llm`。

- Given 管理员直接访问旧深链 `/admin/jobs/tasks/:taskId`
  When 打开与关闭抽屉
  Then 页面仍可正常工作，并回退到 `/admin/jobs`。

## 非功能性验收 / 质量门槛

### Testing

- Storybook：至少覆盖 `Scheduled` 深链、`Translations Queue`、`Translations History` 三种 route state。
- Playwright：覆盖一级 tab 路由切换、翻译二级 tab 路由切换、深链首载、`from` 恢复与浏览器 back/forward。
- Web build：确保路由 helper、stories 与测试入口都能通过构建。

### Quality checks

- [x] `cd web && bun run build`
- [x] `cd web && bun run storybook:build`
- [x] `cd web && bun run e2e -- admin-jobs.spec.ts`

## Visual Evidence

- 验证来源：本地 Storybook route-state 与真实后台预览页。
- 仓库内不保留截图资产；本次交付以构建、e2e 与人工走查结果为准。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 admin jobs route contract，并在前端引入统一解析/序列化 helper。
- [x] M2: 一级 tabs 与翻译二级 tabs 切换改为 URL 驱动，支持深链与 popstate。
- [x] M3: 任务详情抽屉补齐 `from` 上下文恢复。
- [x] M4: Storybook、Playwright、视觉证据与本地验证收口。
