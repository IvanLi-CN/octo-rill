# Dashboard 仓库侧栏视口高度

## Context and Scope

- Context: Dashboard 的仓库侧栏需要在桌面阅读流中保持可扫描、可滚动，并避免短列表收缩成过小的卡片。
- In scope: 根 Dashboard「全部」页的关注仓库右栏、`/focus/following` 的关注/关联仓库列表、`/focus/mine` 的个人仓库列表，以及它们的桌面视口高度边界。
- Out of scope: 后端仓库数据、路由语义、移动端新增侧栏和左侧 Feed 项目卡。

## Terms and Interfaces

- 项目卡: 仓库列表中的单个仓库项目行；高度阈值以两张项目卡为基准。
- 可用侧栏高度: 固定 AppMetaFooter 顶部到侧栏顶部的距离，扣除 `16px` 页脚间距。
- Interface: `ScopedSummaryCard` 内部仓库列表、`AppMetaFooter` 的 DOM 标记和 Dashboard 桌面侧栏布局。

## Requirements

### REQ-DASHBOARD-REPOSITORY-SIDEBAR-001

- The system MUST render the root Dashboard「全部」tab's following-repository sidebar at `min-width: 1024px`, while retaining the existing Focus following and mine repository surfaces.
- Inputs: authenticated Dashboard data and the existing following/personal repository responses.
- Outputs: the root desktop reading flow exposes a following-repository panel without changing API or route semantics.

### REQ-DASHBOARD-REPOSITORY-SIDEBAR-002

- The system MUST cap the repository sidebar at the available viewport height defined by the fixed footer top minus `16px`.
- Inputs: panel top, footer top, visual viewport changes, repository item dimensions, and list content height.
- Outputs: lists scroll internally when content exceeds the cap, and the panel never crosses the footer gap.

### REQ-DASHBOARD-REPOSITORY-SIDEBAR-003

- The system MUST fill the available panel height when the rendered repository list is shorter than two project cards.
- Inputs: actual first project-card height when present, otherwise a `76px` per-card fallback.
- Outputs: empty, loading, and one-item states keep a stable near-viewport panel instead of collapsing.

### REQ-DASHBOARD-REPOSITORY-SIDEBAR-004

- The system MUST keep the viewport policy desktop-only and preserve current following/associated controls, follow actions, personal-repository navigation, and release-count labels.
- Inputs: responsive breakpoint, list selection state, mutation state, and route links.
- Outputs: mobile and narrow-tablet reading flow remains unchanged and desktop interactions remain available.

## Verification

### VER-DASHBOARD-REPOSITORY-SIDEBAR-001

- Method: Dashboard Playwright coverage at `1440x900`, `1024x768`, and below `1024px`.
- covers: `REQ-DASHBOARD-REPOSITORY-SIDEBAR-001`, `REQ-DASHBOARD-REPOSITORY-SIDEBAR-004`
- Pass condition: root desktop, Focus following, and Focus mine render the intended panels; narrow layouts do not render the desktop sidebar or issue the root-only following request.

### VER-DASHBOARD-REPOSITORY-SIDEBAR-002

- Method: Storybook repository-sidebar states plus DOM geometry assertions.
- covers: `REQ-DASHBOARD-REPOSITORY-SIDEBAR-002`, `REQ-DASHBOARD-REPOSITORY-SIDEBAR-003`
- Pass condition: short panels end `16px` above the footer, long panels cap at that boundary, and overflow belongs to the list container.

## Related ADRs

None

## Visual Evidence

- Source: `ui_demo` for Dashboard page states and `storybook_docs` for the reusable repository panel.
- Requested viewports: `1440x900` and `1024x768` CSS px.
- Geometry receipts: root short list `16px` footer gap at both desktop viewports; Storybook short list `16px` gap; Storybook long list has internal overflow (`scrollHeight > clientHeight`) and a `16px` footer gap; personal repository list grows naturally below the cap.
- Assets:
  - `./assets/root-following-1440x900.png`
  - `./assets/root-following-1024x768.png`
  - `./assets/personal-repositories-1440x900.png`
  - `./assets/short-following-1440x900.png`
  - `./assets/long-following-1440x900.png`
  - `./assets/story-personal-1440x900.png`

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
