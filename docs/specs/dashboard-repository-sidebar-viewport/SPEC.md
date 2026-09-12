# Dashboard 仓库侧栏视口高度

## Context and Scope

- Context: Dashboard 的仓库侧栏需要在桌面阅读流中保持可扫描、可滚动，并避免短列表收缩成过小的卡片。
- In scope: 根 Dashboard「全部」页的关注仓库右栏、`/focus/following` 的关注/关联仓库列表、`/focus/mine` 的个人仓库列表，以及它们的桌面视口高度边界。
- Out of scope: 后端仓库数据、路由语义、移动端新增侧栏和左侧 Feed 项目卡。

## Terms and Interfaces

- 项目卡: 仓库列表中的单个仓库项目行；高度阈值以两张项目卡为基准。
- 可用侧栏高度: 初始布局以及尺寸、内容或字体变化时，固定 AppMetaFooter 顶部到侧栏顶部的距离，扣除 `16px` 页脚间距。
- Interface: `ScopedSummaryCard` 内部仓库列表、`AppMetaFooter` 的 DOM 标记和 Dashboard 桌面侧栏布局。

## Requirements

### REQ-DASHBOARD-REPOSITORY-SIDEBAR-001

- The system MUST render the root Dashboard「全部」tab's following-repository sidebar at `min-width: 1024px`, while retaining the existing Focus following and mine repository surfaces.
- Inputs: authenticated Dashboard data and the existing following/personal repository responses.
- Outputs: the root desktop reading flow exposes a following-repository panel without changing API or route semantics.

### REQ-DASHBOARD-REPOSITORY-SIDEBAR-002

- The system MUST cap the repository sidebar at the available viewport height defined by the fixed footer top minus `16px`.
- Inputs: panel top, footer top, visual viewport resizing, repository item dimensions, and list content height.
- Outputs: lists scroll internally when content exceeds the cap, and the panel never crosses the footer gap at layout time. Ordinary document scrolling MUST NOT trigger a height recalculation or cause a bottom-sticky effect; the panel remains in the reading flow.

### REQ-DASHBOARD-REPOSITORY-SIDEBAR-003

- The system MUST reserve a repository-list viewport at least as tall as two project cards for every desktop repository-list state, including long lists.
- Inputs: actual first project-card height when present, otherwise a `76px` per-card fallback; panel chrome height and the available sidebar height.
- Outputs: empty, loading, and one-item states keep a stable near-viewport panel that ends `16px` above the footer. Lists whose natural content reaches two cards grow naturally and only cap at the available height; long lists expose at least two complete cards and scroll only inside the list. When the available list viewport can contain no more than two project cards, the panel enters the viewport-fill state even when the data set is long. When panel chrome would consume that minimum, the panel enters its compact density without crossing the footer boundary.

### REQ-DASHBOARD-REPOSITORY-SIDEBAR-004

- The system MUST keep the viewport policy desktop-only and preserve current following/associated controls, follow actions, personal-repository navigation, and release-count labels.
- Inputs: responsive breakpoint, list selection state, mutation state, and route links.
- Outputs: mobile and narrow-tablet reading flow remains unchanged and desktop interactions remain available.

## Verification

### VER-DASHBOARD-REPOSITORY-SIDEBAR-001

- Method: Dashboard Playwright coverage at `1440x900`, `1024x768`, `1171x620`, and below `1024px`.
- covers: `REQ-DASHBOARD-REPOSITORY-SIDEBAR-001`, `REQ-DASHBOARD-REPOSITORY-SIDEBAR-004`
- Pass condition: root desktop, Focus following, and Focus mine render the intended panels; narrow layouts do not render the desktop sidebar or issue the root-only following request.

### VER-DASHBOARD-REPOSITORY-SIDEBAR-002

- Method: Storybook repository-sidebar states plus DOM geometry assertions.
- covers: `REQ-DASHBOARD-REPOSITORY-SIDEBAR-002`, `REQ-DASHBOARD-REPOSITORY-SIDEBAR-003`
- Pass condition: empty, loading, and one-item panels end `16px` above the footer; two-or-more-item panels retain natural height until the cap and never cross the footer boundary. Every list viewport is at least two rendered project-card heights (or `152px` when empty), and long-list overflow belongs to the list container. The compact `1171x620` state covers following, associated, and personal repository lists, including follow-state mutation and normal document scrolling.

## Related ADRs

None

## Visual Evidence

- Source: `ui_demo` for Dashboard page states and `storybook_docs` for the reusable repository panel.
- Requested viewports: `1440x900`, `1024x768`, and `1171x620` CSS px.
- Geometry receipts: short panels have a `16px` footer gap; two-or-more-item panels remain natural until their cap; empty-list fallback is `152px`; long following, associated, and personal lists have internal overflow (`scrollHeight > clientHeight`) while retaining at least two visible project-card heights.
- Assets:
  - `./assets/root-following-1440x900.png`
  - `./assets/root-following-1024x768.png`
  - `./assets/root-following-1171x620.png`
  - `./assets/personal-repositories-1440x900.png`
  - `./assets/short-following-1440x900.png`
  - `./assets/long-following-1440x900.png`
  - `./assets/story-compact-long-following-1171x620.png`
  - `./assets/story-personal-1440x900.png`

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
