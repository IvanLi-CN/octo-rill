# 演进记录（OctoRill Web Demo 与悬浮 Inspector Contract）

## 生命周期

- Lifecycle: active
- Last: 2026-07-09

## 变更记录

- 2026-07-08: 新建 `k8d4m` 规格，冻结 `/demo/` 子应用、`demo=` share contract、floating inspector 与 Pages deep-link recovery 的首版交付口径。
- 2026-07-09: 补齐 `/api/version` 与 `/api/feed/reactions/refresh` 的 demo MSW 拦截，禁用 demo 模式下的正式 PWA service worker 注册，修复 dashboard canonical URL 对 `demo/d_*` 的保留，以及 inspector 对 simulated writes / publication share state 的实时同步。
- 2026-07-09: 修复桌面态 inspector 在 publish toast 出现时的顶部遮挡与底部视口溢出风险，改为按 header / toast / viewport inset 动态钳制位置与最大高度，并补上对应的 Playwright 回归测试。
- 2026-07-09: 将桌面态 inspector 从固定 640px 高改为按内容自然长高、仅在短视口内回退到内部滚动；补上 tall desktop 视口回归测试，并刷新 owner-facing `ui_demo` 视觉证据。
