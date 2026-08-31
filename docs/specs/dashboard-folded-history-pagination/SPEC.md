# Dashboard 折叠历史的可见进展分页

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，主题局部演进见 `./HISTORY.md`，持久决策的完整取舍见关联 ADR。

## 背景 / 问题陈述

- Dashboard `全部` tab 按自然日分组，历史日期默认以日报折叠；同一历史日期后续追加的活动可能已经被现有日报覆盖，因此成功合并数据也不会产生新的可见卡片或页面高度。
- 当前无限滚动哨兵以一次可见周期为单位触发。成功加载一页、但渲染投影没有变化时，哨兵持续可见且锁未释放，后续 cursor 无法继续请求，用户会误以为已经到底。
- 直接在每次追加后重置哨兵锁会在折叠历史上无界地自动拉取所有后续页，既增加网络和渲染成本，也没有给用户可理解的阅读进展。

## 目标 / 非目标

### Goals

- 保持有可见进展时的自动无限滚动。
- 当一页成功追加但被既有历史日报完全折叠时，明确提示仍有数据，并让用户主动继续加载。
- 保持既有日报优先阅读与“列表”恢复原始混排的语义。

### Non-goals

- 不变更 `/api/feed` 的 cursor、排序、批量大小或响应字段。
- 不改变日报分组算法、历史日报内容，或默认把日报切换为列表。
- 不自动连锁拉取任意数量的不可见历史页。

## 范围（Scope）

### In scope

- `web/src/feed/FeedGroupedList.tsx` 的哨兵、可见投影判定与续载状态。
- `web/src/feed/useFeed.ts` 的分页结果协作边界，如实现判定需要返回追加结果。
- Dashboard 分页的 Playwright 回归测试及 `FeedGroupedList` 的交互/视觉覆盖。
- `CONTEXT.md` 与本 topic spec。

### Out of scope

- Rust API、数据库和 PWA Service Worker。
- `发布`、`加星`、`关注` 独立 tab 的列表语义。
- 历史日报的生成、详情加载和“列表 / 日报”切换内容。

## Related ADRs

- None

## 需求（Requirements）

### MUST

- `全部` tab 的自动分页在成功合并新页后，必须根据**渲染后的可见投影**判定是否取得进展；不得以 `items.length`、cursor 变化或单纯 `scrollHeight` 作为唯一判据。
- 若一页成功合并、仍有下一 cursor，且没有新增可见日组、日报面板或原始活动卡片，自动分页必须暂停，并在当前列表底部显示 `继续加载历史动态` 命令。
- 续载命令一次只请求一个后续页。该页出现可见进展后，清除暂停状态并恢复正常自动分页；该页再次完全折叠时，保留命令以供再次显式续载。
- 自动分页暂停期间，IntersectionObserver 不得继续自动发起下一页请求；按钮、请求中的禁用状态和已有追加失败重试必须继续走同一去重/并发保护。
- 用户切换 tab、scope 或刷新首屏时必须清除旧的暂停状态。用户切到某历史组的“列表”并产生可见进展时，也必须解除该状态。
- 末页、追加失败和重复项合并继续沿用现有提示、错误和 cursor 语义；不得因为本功能重复请求同一 cursor 或吞掉错误。

### SHOULD

- 将“可见投影”收敛为可测试的纯派生结果，覆盖日组、日报/列表视图和实际渲染的原始活动键。
- 续载控件应有稳定的测试选择器、可访问名称和加载/失败状态，且在窄屏不会压缩或遮挡末页提示。

### COULD

- 在续载控件附近显示已被日报归档的历史动态数量；该数量必须基于当前已合并数据，不能暗示服务器总量。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 用户滚动到 `全部` tab 的底部时，哨兵按既有方式自动请求下一页。
- 如果新页在渲染后新增了可见日组、日报面板或原始活动卡片，自动滚动保持可用。
- 如果新页只向已经显示日报的同一历史日组补充了被该日报覆盖的活动，列表保持现有视觉内容，底部改为显示 `继续加载历史动态`。
- 用户点击该命令后只加载下一页；如果下一页首次带来可见内容，命令消失并恢复自动分页。用户点击历史日组的“列表”仍可查看已合并的所有原始记录。

### Edge cases / errors

- 续载请求失败时，复用追加失败气泡与重试入口；重试成功后重新按照可见投影判定自动或显式续载。
- 续载后到达末页时，不显示续载命令，仍显示既有末页状态。
- 首屏、路由或 scope 变化替换列表时，不得残留上一列表的续载命令。
- 只有重复记录而没有可见投影变化的响应不得造成请求循环；cursor 仍由后端响应决定。

## 接口契约（Interfaces & Contracts）

None。此主题不改变 HTTP、数据库、事件或公开组件接口；内部 hook 与组件间的结果类型可以为实现可测试性而调整。

## 验收标准（Acceptance Criteria）

- Given 首屏已经显示某历史日组的日报
  When 自动分页追加同一天且被该日报完全覆盖的一页，并且响应仍有 `next_cursor`
  Then 该页会合并一次，列表显示 `继续加载历史动态`，且不会自动请求第三页。

- Given 列表处于显式续载状态
  When 用户点击 `继续加载历史动态`，下一页新增一个可见日组或原始卡片
  Then 新内容可见、暂停状态清除，后续自动分页恢复。

- Given 显式续载的一页再次被同一日报完全折叠
  When 请求成功且仍有下一 cursor
  Then 控件保持可用，且只在用户下一次点击后才请求下一页。

- Given 显式续载状态中的历史日组
  When 用户点击该日组的“列表”
  Then 此前已合并的原始活动全部可见，且分页不会因旧的暂停状态永久停住。

- Given 追加请求失败、重试成功或到达末页
  When 用户继续操作
  Then 错误/末页语义与现有分页一致，不会重复消费 cursor。

## 验收清单（Acceptance checklist）

- [x] 核心路径的长期行为已被明确描述。
- [x] 关键边界/错误场景已被覆盖。
- [x] 涉及的接口/契约已明确为 `None`。
- [x] 相关验收条件已经可以用于实现与 review 对齐。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- E2E：在 Dashboard mock 中构造“第二页被既有日报完全折叠、第三页仍存在”的 cursor 序列，断言自动请求止于第二页、显式续载取得第三页。
- E2E：覆盖续载失败重试、末页和“列表”切换解除暂停状态。
- Component / interaction：验证续载控件的可访问名称、禁用状态和窄屏布局。

### UI / Storybook

- 更新 `Feed/FeedGroupedList` 的可见进展分页场景，并在实现后提供当前主题的受控视觉证据。

### Quality checks

- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bunx playwright test e2e/dashboard-brief-detail.spec.ts`

## Visual Evidence

## Related PRs

- None

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：只比较数据条数或页面总高度会把“已合并但被折叠”误判为可见进展，或因 root margin 产生错误的连续请求；实现必须比较语义化渲染投影。
- 风险：把暂停状态绑定到旧的 tab/scope 会使新列表不可续载；状态重置是必要边界。
- 开放问题：无。
- 假设：已有日报覆盖的历史活动仍可通过该组的“列表”操作完整查看。

## 参考（References）

- `web/src/feed/FeedGroupedList.tsx`
- `web/src/feed/useFeed.ts`
- `web/e2e/dashboard-brief-detail.spec.ts`
- `docs/specs/dashboard-day-grouping/SPEC.md`
- `docs/specs/dashboard-brief-social-folding/SPEC.md`
