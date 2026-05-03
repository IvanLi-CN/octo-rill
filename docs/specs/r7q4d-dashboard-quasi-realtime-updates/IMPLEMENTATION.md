# Dashboard 准实时列表更新实现状态（#r7q4d）

## Current Coverage

- 新增 `GET /api/dashboard/updates`，通过本地缓存列表签名摘要生成 opaque token 与 changed/new_count；响应继续暴露稳定 latest keys 供前端标记。
- Dashboard 前端新增轮询 hook，支持前台检查、后台降频、离线暂停、失败退避和 task SSE 完成后的 silent baseline check。
- Feed、日报、Inbox 增加新内容提示；Feed 命中更新后会后台刷新当前列表，把提示放在最新新批次与原阅读内容之间，揭示后对新卡片应用 session-only 圆点暗示。
- Feed 自动插入时只保持当前可见卡片的阅读锚点，不主动滚动到分割线；后续批次会替换为新的分割线语义，旧批次卡片继续保留 fresh 圆点。
- Storybook 覆盖 Feed 新批次、Feed 连续推送、日报新批次和 Inbox 新线程状态，并提供视觉证据 canvas；连续推送 Story 会保留当前阅读锚点，模拟低打扰阅读体验。

## Verification

- `cargo fmt --check`
- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `cd web && bunx playwright test e2e/dashboard-live-updates.spec.ts`
