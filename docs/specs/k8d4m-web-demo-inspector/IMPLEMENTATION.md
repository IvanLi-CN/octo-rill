# 实现状态（OctoRill Web Demo 与悬浮 Inspector Contract）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-07-08
- Last: 2026-07-09
- Summary: fast-track / `/demo/` runtime + floating inspector + Pages assembly 已落地，最后补齐了 deep-link query 保真、MSW 漏拦截、inspector live snapshot 同步、native anchor `/demo` share-state 保真、demo boot failure 安全兜底，以及桌面态 toast 避让 + 视口内高度钳制 + tall viewport 自适应长高，并进一步收口为双态桌面合同：常规宽度保持真正的悬浮覆盖层，超宽视口默认切到 root-level pinned left rail，inspector 固定贴住视口最左边、占满全高，右侧继续承载现有 Web App 最宽版心，但仍支持收起成 bubble 以恢复正常 layout 验收；同时保留按 scene 选择默认停靠边位与 PAT 固定假 mask，确保 simulated write 可点击且 mock surface 不回显 secret 前缀。最新一轮 owner feedback 还补上了 `WideDockedRail` Storybook 入口、wide collapse 交互与 wide/tall desktop Playwright 几何断言，并把 share deep link 收口为无横向滚动条的 readonly input，避免实现再次退化回“只是贴左的浮层”、锁死不可关闭的边栏，或需要先拖动滚动条才能复制链接的表面契约。
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
- [x] M4: Storybook inspector stories / docs（含 `ShortDesktopSurface` / `CompactDesktopSurface` / `WideDockedRail`）、lint/build/e2e、visual evidence 与 review-loop 收口。
