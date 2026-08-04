# 实现状态（GitHub PAT 1:1 参考界面）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-20
- Last: 2026-04-21
- Summary: 已交付；fast-track / DOM-based GitHub PAT reference + switchable settings story + evidence
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现概述

- `GitHubPatGuideCard` 使用 DOM 复刻 GitHub classic PAT 页面骨架，并按主题与断点切换 desktop/mobile × light/dark 四个变体。
- 设置页保留真实 PAT 编辑能力，只把参考界面作为嵌入式抄写区域；移动端嵌入版允许横向滚动并降低说明噪声。
- `Settings.stories.tsx` 新增可切换 section 的 story，并在 story 内拦截导航跳转、改为本地状态切换。
- 移动端设置页导航拆成独立 2×2 网格渲染，避免桌面按钮变体导致选中态与布局失真。
- 移动端 `GitHub PAT`、`日报设置`、`我的发布`、`LinuxDO 绑定` 的外层 section 卡片壳统一抹平，保留导航和内容之间的明确分隔。
