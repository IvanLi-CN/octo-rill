# 管理员任务中心刷新闪烁修复（#9vb46）

## 状态

- Status: 已完成
- Created: 2026-03-06
- Last: 2026-03-06

## 背景 / 问题陈述

当前 `/admin/jobs` 在手动刷新和 SSE 自动刷新时，会把已渲染的内容区退回到阻塞式 loading 文案：

- `实时异步任务` 区会被替换成“正在加载任务...”。
- `定时任务` 区会被替换成“正在加载运行记录...”。
- `LLM 调度` 区会被替换成“正在加载调用记录...”。

同时，组件挂载阶段既调用 `loadAll()`，又分别调用分区加载 effect，导致首屏存在重复请求，放大了 loading 态切换与闪烁体感。

## 目标 / 非目标

### Goals

- 将 realtime / scheduled / llm 三块的加载状态拆分为“首载阻塞”和“后台刷新”。
- 首次进入 `/admin/jobs` 仍允许阻塞式加载；首载完成后，手动刷新与 SSE 自动刷新必须保留现有内容。
- 为三块内容增加轻量刷新提示，避免整块内容消失。
- 消除组件自身重复首载请求，保留一套清晰的初始化加载来源。
- 补齐 e2e 回归，覆盖 realtime / scheduled / llm 三块在手动刷新、SSE 刷新与重叠请求下的不闪烁场景。

### Non-goals

- 不修改 `/api/admin/jobs/**` 接口、SSE 事件格式或后端调度逻辑。
- 不做按事件粒度的本地增量 patch。
- 不调整 `/admin` 用户管理页面行为。

## 范围（Scope）

### In scope

- `web/src/admin/JobManagement.tsx` 的加载状态模型与刷新编排。
- `/admin/jobs` 三个列表区域的刷新提示文案与渲染分支。
- `web/e2e/admin-jobs.spec.ts` 的延迟 mock 与不闪烁回归断言。
- `docs/specs/README.md` 与本规格状态同步。

### Out of scope

- 管理端其它页面的轮询/刷新体验。
- 新增缓存层或数据预取机制。

## 接口契约（Interfaces & Contracts）

None。

## 验收标准（Acceptance Criteria）

- Given 管理员首次打开 `/admin/jobs`
  When 页面首屏数据尚未返回
  Then 允许显示阻塞式 loading 提示。

- Given `实时异步任务` 已经渲染完成
  When 管理员点击“刷新”或 SSE 触发全量刷新
  Then 当前任务卡片持续可见，仅显示轻量刷新提示，不再退化为只剩“正在加载任务...”。

- Given `定时任务` 已经渲染完成
  When 管理员点击“刷新”或 SSE 触发全量刷新
  Then 当前运行记录持续可见，仅显示轻量刷新提示，不再退化为只剩“正在加载运行记录...”。

- Given `LLM 调度` 已经渲染完成
  When 管理员点击“刷新”或 SSE 触发相关刷新
  Then 当前调用记录与调度状态持续可见，仅显示轻量刷新提示，不再退化为只剩“正在加载调用记录...”。

- Given 页面初次挂载
  When 前端执行初始化请求
  Then 不再同时触发 `loadAll()` 与分区级重复首载请求。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- E2E tests: 扩展 `web/e2e/admin-jobs.spec.ts`，覆盖 realtime / scheduled / llm 刷新期间保留已渲染内容、首次未加载完成时仍保持阻塞 loader，以及重叠请求失败路径不会冒出过期错误横幅。
- Browser verification: 本地浏览器复核 `/admin/jobs` 的手动刷新与 SSE 自动刷新体验。

### Quality checks

- `bun run e2e -- admin-jobs.spec.ts`
- `bun run build`

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增本规格索引并在实现完成后同步状态。
- `docs/specs/9vb46-admin-jobs-refresh-flicker-fix/SPEC.md`: 实现完成后补充变更记录与里程碑状态。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: `JobManagement` 拆分三块列表的首载/后台刷新状态，保留已渲染内容。
- [x] M2: 初始化与刷新编排收敛，消除组件自身重复首载请求。
- [x] M3: e2e 与本地浏览器验证补齐，确认手动刷新与 SSE 不再闪烁。

## 方案概述（Approach, high-level）

- 为 overview / realtime / scheduled / llm status / llm calls 建立显式加载阶段：仅手动刷新与 SSE 走后台刷新，筛选/分页切换继续使用阻塞式列表加载，避免新筛选文案与旧列表并存。
- 为 overview / realtime / scheduled / llm status / llm calls 增加“仅最新请求可提交结果”的保护，避免 SSE / 手动刷新 / 筛选分页切换并发时被旧响应回写。
- 挂载阶段只保留必要的初始化 effect，后续分页、筛选、手动刷新与 SSE 刷新统一复用分区加载函数。
- 当列表已有数据时，刷新期间显示内联“刷新中/更新中”提示，而不是替换整个内容区。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：背景刷新期间保留旧列表，会带来短时间的“旧结果仍可见”窗口；仅限手动刷新与 SSE 使用，并通过请求序号保护避免旧响应覆盖新结果。
- 风险：SSE 与手动刷新重叠时仍可能产生并发请求，需要与现有节流/排队逻辑一起保持最新请求优先。
- 假设：管理员接受“后台刷新保留旧数据”的交互优先级高于“立刻清空显示新筛选的空态”。

## 变更记录（Change log）

- 2026-03-06: 新建规格，冻结 `/admin/jobs` 刷新闪烁修复范围、非目标与验收标准。
- 2026-03-06: 完成 `JobManagement` 刷新态重构与请求编排收敛，补充手动刷新 / SSE 刷新不闪烁回归测试。
- 2026-03-06: 根据 review-loop 补上 overview / llm status / 三组列表的最新请求保护，收紧“后台刷新”只用于手动刷新与 SSE，并补齐 scheduled / 首载阻塞 / 重叠刷新 / 过期错误回归测试。

## 参考（References）

- `web/src/admin/JobManagement.tsx`
- `web/e2e/admin-jobs.spec.ts`
