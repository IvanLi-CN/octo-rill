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
- 2026-07-09: 根据 owner 反馈撤回正文 safe area 让位逻辑，改回真正的桌面悬浮覆盖层；Dashboard 重新允许正文延伸到 inspector 下方，Settings / Public Release scene 默认停靠左侧以避免压住主操作，同时保留 demo 保存 PAT 的固定假 mask，避免把用户输入的 secret 前缀写回 mock UI。
- 2026-07-09: 继续根据 owner 反馈细化桌面合同：当浏览器宽度足够容纳 Web App 最宽版心时，demo 根层会切成双栏 frame，左侧固定 inspector、右侧保留现有 app layout；普通桌面宽度仍保持真正的悬浮覆盖层，并新增超宽 left-docked 几何回归与 `ui_demo` 视觉证据。
- 2026-07-09: review-loop 发现超宽双栏 frame 仍以 `content-box` 计算总宽度，导致 `maxWidth + padding-left` 在接近断点时可能顶出视口；已改为 `border-box` 约束根层宽度，并把 ultra-wide 回归用例改为校验 frame 右边界不超过 viewport，确保 docked inspector 与 app frame 对齐。
- 2026-07-09: 第二轮 review-loop 继续收紧 scene 默认停靠位：inspector 布局存储现在带上 `sceneId`，切 scene 或整页跳转到 `settings-my-releases` / `public-release-ready` 时会重新读取该 scene 的默认/持久布局，不再让旧的右侧布局覆盖左侧默认停靠位；同时补上 Dashboard -> Settings 的回归测试。
