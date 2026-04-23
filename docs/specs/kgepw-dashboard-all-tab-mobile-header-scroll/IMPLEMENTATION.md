# 实现状态（Dashboard「全部」tab 移动端页头意外上滑修复）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-20
- Last: 2026-04-20
- Summary: 已交付；viewport-height shell fix, Storybook evidence, mobile regression coverage, and owner approval all landed
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/kgepw-dashboard-all-tab-mobile-header-scroll/SPEC.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 follow-up spec 与 `全部` tab 壳层回归 contract。
- [x] M2: 修复 `AppShell` / 根节点移动端 viewport 高度链，并补 Storybook 入口。
- [x] M3: 跑通 lint / build / Storybook verify / Playwright，并生成 owner-facing 视觉证据。
- [x] M4: 在主人确认截图可提交后，推进 push / PR-ready 收口。
