# 演进记录（OctoRill Web Demo 与悬浮 Inspector Contract）

## 生命周期

- Lifecycle: active
- Last: 2026-07-09

## 变更记录

- 2026-07-08: 新建 `k8d4m` 规格，冻结 `/demo/` 子应用、`demo=` share contract、floating inspector 与 Pages deep-link recovery 的首版交付口径。
- 2026-07-09: 补齐 `/api/version` 与 `/api/feed/reactions/refresh` 的 demo MSW 拦截，禁用 demo 模式下的正式 PWA service worker 注册，修复 dashboard canonical URL 对 `demo/d_*` 的保留，以及 inspector 对 simulated writes / publication share state 的实时同步。
- 2026-07-09: 修复桌面态 inspector 在 publish toast 出现时的顶部遮挡与底部视口溢出风险，改为按 header / toast / viewport inset 动态钳制位置与最大高度，并补上对应的 Playwright 回归测试。
- 2026-07-09: 将桌面态 inspector 从固定 640px 高改为按内容自然长高、仅在短视口内回退到内部滚动；补上 tall desktop 视口回归测试，并刷新 owner-facing `ui_demo` 视觉证据。
- 2026-07-09: 为 demo bootstrap 增加 service worker 启动超时/失败的安全兜底错误页，统一修复 native anchor 在 `/demo` 下的 share-state `href` 保真，并把短视口 toast 场景截图写回本 spec 的 `## Visual Evidence`。
- 2026-07-09: 针对 owner 反馈继续压紧桌面态 inspector 内容密度，确保 `1366x768` + toast 的短视口截图里 `Actions & Share`、share URL 与 `Advanced` 摘要都留在首屏内，同时补了 Storybook `CompactDesktopSurface` 入口与对应 Playwright 断言。
- 2026-07-09: 继续收紧桌面态浮窗验收：主内容现在会为右侧 inspector 自动让出 safe area，Settings 中的 simulated API key / PAT 控件不再被遮挡；同时 demo 保存 PAT 时固定回显假 mask，避免把用户输入的 secret 前缀写回 mock UI。
