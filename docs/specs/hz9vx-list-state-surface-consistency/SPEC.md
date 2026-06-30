# 列表状态一致性收敛（#hz9vx）

## 背景 / 问题陈述

当前后台与 Dashboard 的多处列表面仍各自维护 loading / empty / error 呈现：

- `/admin/public-releases` 首载只有文本 loading，空态与失败态也停留在裸文本/表格行。
- `/admin/users` 虽然已有首载 skeleton，但二次刷新仍是即时文本提示，失败继续挂在顶部红字。
- `/admin/repos`、`/admin/jobs`、Dashboard `InboxList` 仍存在 text-only loading、空态样式分裂、失败提示出口不一致。

这导致同类列表在“首次进入 / 二次刷新 / 空结果 / 失败”四类场景下观感不一致，也难以给 Storybook、E2E 与视觉证据提供统一 selector 与稳定状态入口。

## 目标 / 非目标

### Goals

- 冻结后台列表统一五态 contract：`initial-loading`、`ready`、`refreshing`、`empty`、`blocking-error`。
- 提供共享前端判定与渲染原语：状态 hook、列表壳层、刷新提示、空态、阻塞失败态、局部失败态。
- 首批接入 `/admin/public-releases`、`/admin/users`、`/admin/repos`、`/admin/jobs` 主列表段、Dashboard `InboxList`。
- 统一 selector：至少提供 `data-list-state`、`data-list-refreshing` 与对应 empty / inline-error / blocking-error 标记。
- 为 Storybook 与视觉验收补齐稳定五态入口，避免后续重新退回纯文本 loading。

### Non-goals

- 不改动 `PublicReleasePage` 的公开访问 contract、`pending_sync` 语义或公开页视觉结构。
- 不重写 Dashboard feed、release detail、smart/translated lane 的既有 loading / error contract。
- 不引入新的后端接口、状态码或数据模型。
- 不把 `/admin/jobs` 的详情抽屉、模态详情或非列表明细面板全部纳入统一改造。
- 不接受“刷新时重新清空旧内容”的回退实现。

## 范围（Scope）

### In scope

- `web/src/hooks/useListSurfaceState.ts`
- `web/src/components/feedback/listSurface.tsx`
- `web/src/admin/PublicReleaseRepoManagement.tsx`
- `web/src/admin/UserManagement.tsx`
- `web/src/admin/AdminRepoGovernance.tsx`
- `web/src/admin/JobManagement.tsx` 的列表主体状态壳层
- `web/src/inbox/InboxList.tsx`
- 相关 Storybook stories、play 断言与视觉证据

### Out of scope

- `web/src/pages/PublicReleasePage.tsx`
- Dashboard feed 主流、release detail、lane 内重试/失败语义
- 列表以外的详情抽屉与后台配置弹窗

## 接口契约（Interfaces & Contracts）

### Shared list state contract

- `initial-loading`
  首次进入且当前没有任何可展示数据时使用；必须显示接近真实排版的骨架，不允许只剩一行“正在加载...”。
- `ready`
  当前已有数据，且没有延迟后展示的 refresh 提示。
- `refreshing`
  首载完成后发生再次请求时使用；旧内容保留，超过短延迟阈值后才显示轻量刷新提示。
- `empty`
  当前请求成功完成，但结果为空；必须显示专用空态容器，不允许退化为空白表头或单行裸文本。
- `blocking-error`
  当前无任何可展示数据且请求失败；必须显示区域级失败面板和 retry。

### Warm refresh / refresh failure

- 非首次加载延迟阈值默认 `400ms`。
- 有旧数据时，请求失败必须保留旧数据，并在列表壳层内部显示 compact 失败提示与 retry。
- 只有“无数据 + 请求失败”才允许切换到阻塞失败面板。

### Stable selectors

- 列表壳层输出 `data-list-state=<state>`。
- 刷新提示期间同时输出 `data-list-refreshing="true"`。
- 空态、局部失败、阻塞失败分别提供稳定 DOM 标记供 Storybook / E2E / visual evidence 复用。

## 验收标准（Acceptance Criteria）

- Given 首次进入 `/admin/public-releases`、`/admin/users`、`/admin/repos`、`/admin/jobs` 主列表段、Dashboard `InboxList`
  When 本地没有任何已有数据
  Then 页面只允许出现骨架，不允许再以单独一行文本 loading 作为最终首载表现。

- Given 任一列表已完成首载
  When 发生筛选、分页、手动刷新、SSE 或局部 reload
  Then 旧内容必须继续可见，且请求超过 `400ms` 后才显示轻量刷新提示。

- Given 当前结果为空
  When 请求成功返回空数据
  Then 必须显示专用空态容器，而不是空表头、空白区域或单行裸文本。

- Given 当前没有任何已展示数据
  When 请求失败
  Then 必须显示区域级失败面板并提供 retry。

- Given 当前已有旧数据
  When 刷新请求失败
  Then 必须保留旧数据，并在列表壳层内显示 compact 失败提示与 retry。

- Given Storybook 进入受影响列表组件
  When 需要做状态验证与视觉验收
  Then 至少 `PublicReleaseRepoManagement` 与一个后台通用列表页必须提供稳定五态场景与可断言 selector。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Storybook：补齐 `initial-loading / refreshing / empty / blocking-error / ready` 稳定场景。
- Play / E2E：至少断言不会回退到 text-only loading，且 warm refresh 不会清空旧内容。
- Browser verification：基于稳定 Storybook 或可控 preview 产出 owner-facing 视觉证据。

### Quality checks

- `bun run build`
- `bun run storybook:build`
- 目标 Storybook play / E2E 断言通过

## 方案概述（Approach, high-level）

- 新增共享状态 hook，显式区分“首载未决”“首载已完成后的再次加载”“无数据失败”“空结果”。
- 新增共享渲染原语，统一列表壳层标记、轻量刷新提示、空态与失败态。
- 首批后台列表页面按“首载 skeleton + warm refresh + 空态 + 分层失败”统一接入。
- `/admin/jobs` 仅改造列表主体状态壳层，不触碰详情抽屉 contract。
- Storybook 以组件/后台列表入口直接暴露五态，提供稳定截图与断言基础。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：已有列表把“详情失败”“动作失败”“列表失败”共用一个错误状态时，接入统一壳层需要先拆边界，否则会误把详情失败渲染成列表失败。
- 风险：空列表在二次刷新时若没有“首载已完成”记忆，容易误退回首载骨架；实现必须显式区分。
- 假设：后台列表的一致性优先于每页维持独立文案细枝末节，但允许保留页面自身业务文案。

## 参考（References）

- `docs/specs/n6zd8-admin-panel-user-management/SPEC.md`
- `docs/specs/rap6f-repo-refresh-governance/SPEC.md`
- `docs/specs/9vb46-admin-jobs-refresh-flicker-fix/SPEC.md`
- `docs/specs/p8r3l-public-release-endpoints/SPEC.md`
- `docs/specs/wt8rb-frontend-error-presentation/SPEC.md`

## Visual Evidence

### Source

- Storybook canvas (`iframe.html`) only; evidence is captured from stable story states without the Storybook manager chrome.
- Evidence is bound to commit `1fbe5a781d39eb8a71c0a0b7e5578eeb7f69fb50`.

### Captured states

- `PublicReleaseRepoManagement`
  - [initial-loading](./assets/public-release-initial-loading.png)
  - [refreshing](./assets/public-release-refreshing.png)
  - [empty](./assets/public-release-empty.png)
  - [blocking-error](./assets/public-release-blocking-error.png)
- `AdminPanel` users list
  - [initial-loading](./assets/admin-users-initial-loading.png)
  - [refreshing](./assets/admin-users-refreshing.png)
  - [empty](./assets/admin-users-empty.png)
  - [blocking-error](./assets/admin-users-blocking-error.png)

### Notes

- Public release evidence uses whitespace trimming where the canvas left large uniform margins.
- Admin users evidence is captured at the list card boundary; the trimming script kept those images unchanged because no safe uniform border remained to remove.
