# Dashboard 准实时列表更新实现状态（#r7q4d）

## Current Coverage

- 新增 `GET /api/dashboard/updates`，通过本地缓存列表签名摘要生成 opaque token 与 changed/new_count；响应继续暴露稳定 latest keys 供前端标记。
- Dashboard 前端新增轮询 hook，支持前台检查、后台降频、离线暂停、失败退避和 task SSE 完成后的 silent baseline check。
- Feed、日报、Inbox 增加新内容提示；揭示后对新卡片应用 session-only 圆点暗示。
- Storybook 覆盖 Feed 新批次、Feed 连续推送、日报新批次和 Inbox 新线程状态，并提供视觉证据 canvas。

## Verification

- `cargo fmt --check`
- `cargo test`
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `cd web && bunx playwright test e2e/dashboard-live-updates.spec.ts`
