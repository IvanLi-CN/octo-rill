# 实现状态（OctoRill Web Demo 与悬浮 Inspector Contract）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-07-08
- Last: 2026-07-09
- Summary: fast-track / `/demo/` runtime + floating inspector + Pages assembly 已落地，最后补齐了 deep-link query 保真、MSW 漏拦截、inspector live snapshot 同步、native anchor `/demo` share-state 保真、demo boot failure 安全兜底，以及桌面态 toast 避让 + 视口内高度钳制 + tall viewport 自适应长高，并进一步补上桌面 safe area 与 PAT 固定假 mask，确保 simulated write 可点击且 mock surface 不回显 secret 前缀
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `README.md`
- `web/README.md`
- `docs-site/docs/web-demo.mdx`
- `docs-site/docs/index.mdx`
- `docs/specs/README.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新 spec、scene registry、demo bootstrap 与 `/demo` build target 落地。
- [x] M2: 六个页面级 surface 的 mock-only handlers / SSE / simulated writes 补齐。
- [x] M3: Pages 组装、404 recovery shim 与公开文档入口同步。
- [x] M4: Storybook inspector stories / docs（含 `ShortDesktopSurface`）、lint/build/e2e、visual evidence 与 review-loop 收口。
