# 实现状态（OctoRill Web Demo 与悬浮 Inspector Contract）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-07-08
- Last: 2026-07-09
- Summary: fast-track / `/demo/` runtime + floating inspector + Pages assembly 已落地，最后补齐了 deep-link query 保真、MSW 漏拦截、inspector live snapshot 同步、native anchor `/demo` share-state 保真、demo boot failure 安全兜底，以及桌面态 toast 避让 + 视口内高度钳制 + tall viewport 自适应长高，并进一步收口为双态桌面合同：常规宽度保持真正的悬浮覆盖层，超宽视口切到根层双栏 left-docked frame，同时保留按 scene 选择默认停靠边位与 PAT 固定假 mask，确保 simulated write 可点击且 mock surface 不回显 secret 前缀；review-loop 追加修正了超宽根层 frame 的盒模型，使 docked 模式的总宽度不会因 `padding-left` 叠加而顶出视口，并把 inspector 布局存储升级为真正的 per-scene map，确保从 Dashboard 切到 Settings / Public Release 时会重新应用左侧默认停靠位，同时多个 scene 的自定义位置也能并存持久化
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
- [x] M4: Storybook inspector stories / docs（含 `ShortDesktopSurface` / `CompactDesktopSurface`）、lint/build/e2e、visual evidence 与 review-loop 收口。
