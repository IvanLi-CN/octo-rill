# 实现状态（公告翻译/润色与 Discussion 详情页对齐）

## 当前状态

- Lifecycle: active
- Implementation: 已实现
- Created: 2026-07-09
- Last: 2026-07-09
- Summary: announcement 已升级为 lane-capable 内容卡与登录态 discussion 详情页；后端 truth source、translation contract、Dashboard route/detail、Storybook 与 Playwright 回归均已落地并完成本地验证
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/announcement-discussion-reading/SPEC.md`
- `docs/specs/announcement-discussion-reading/IMPLEMENTATION.md`
- `docs/specs/announcement-discussion-reading/HISTORY.md`
- `docs/specs/dashboard-social-activity/IMPLEMENTATION.md`
- `docs/specs/release-feed-smart-tabs/IMPLEMENTATION.md`
- `docs/specs/dashboard-tab-path-release-deep-link/IMPLEMENTATION.md`
- `docs/product.md`

## 计划资产（Plan assets）

- Directory: owner-facing 视觉证据通过聊天快照回传，不新增仓库截图资产。
- Directory: `docs/specs/announcement-discussion-reading/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 announcement feed/detail/lane/deep-link contract，并创建 follow-up spec。
- [x] M2: 扩展后端 announcement truth source、feed row、detail API 与 translation kind allowlist。
- [x] M3: 扩展前端 feed model、page/card lane hooks、announcement content card 与 canonical discussion route。
- [x] M4: 完成 Storybook / Playwright / cargo test / visual evidence / merge-ready 收口。

## 当前实现补充

- `social_activity_events` 的 announcement 记录新增 `discussion_number`，同步链路直接持久化 GitHub Discussion number 与完整 body。
- `/api/feed` 现已把 announcement 提升为 lane-capable item，并通过 `announcement_summary` / `announcement_smart` 结果 join 暴露 `translated` / `smart`。
- 新增登录态 detail API：`GET /api/repos/:owner/:repo/discussions/:number/detail`。
- Dashboard 新增 `/<owner>/<repo>/discussions/<number>` canonical route，并在壳层内渲染独立的 `AnnouncementDetailPage`。
- 公告 title deep link 已改为透传当前 dashboard scope，避免从全局 `全部` feed 打开后错误回退到 repo focus route。

## 验证结果（Validation）

- `cargo test`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `cd web && bunx playwright test e2e/announcement-detail.spec.ts e2e/dashboard-scoped-focus.spec.ts`
