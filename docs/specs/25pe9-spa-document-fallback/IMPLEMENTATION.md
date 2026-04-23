# 实现状态（SPA document fallback 与全局 404 页面收口）

## 当前状态

- Lifecycle: active
- Implementation: 部分完成（3/4）
- Created: 2026-04-20
- Last: 2026-04-20
- Summary: 部分完成（3/4）；local implementation + regression coverage complete; PR path proceeds without persisted screenshot assets
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结“仅 HTML 文档导航走 SPA fallback，静态资源 / API / auth miss 保持真实 404”的契约。
- [x] M2: 源站静态 fallback 改为通用 SPA document fallback，不再让 `/settings` 文档请求带 `404`。
- [x] M3: 根路由补齐全局 404 页面、Storybook 与测试覆盖。
- [ ] M4: 完成视觉证据、PR 收敛与 merge+cleanup。
