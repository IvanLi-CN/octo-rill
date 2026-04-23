# 演进记录（Dashboard 日报阅读流与详情弹窗修正）

## 生命周期

- Lifecycle: active
- Created: 2026-04-03
- Last: 2026-04-03

## 变更记录

- 2026-04-03: 创建规格，冻结“日报/详情卡片跟随内容高度展开，不再内部滚动”的实现口径。
- 2026-04-03: 去除 `Release 日报` 与 `Release 详情` Markdown 容器的 `max-h-96` / `overflow-auto` 限制，并补齐长内容 Storybook 场景。
- 2026-04-03: 通过 `bun run lint`、`bun run build`、`bun run storybook:build`，并用 Storybook 稳定场景补入视觉证据，状态更新为 `部分完成（2/3）`。
- 2026-04-03: 创建 PR #45，快车道收口到 PR-ready，规格状态更新为 `已完成`。
- 2026-04-03: 根据评审反馈，将 `Release 详情` 从 Dashboard 文档流卡片改为模态弹窗，并重新生成 Storybook 场景与视觉证据。
- 2026-04-03: 重新通过 `bun run lint`、`bun run build`、`bun run storybook:build`，并以 Storybook 长日报/长详情弹窗场景更新视觉证据。
