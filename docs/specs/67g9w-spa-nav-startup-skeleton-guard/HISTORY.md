# 演进记录（Dashboard SPA 导航避免回退启动骨架）

## 生命周期

- Lifecycle: active
- Created: 2026-04-17
- Last: 2026-04-17

## 变更记录

- 2026-04-17: 新建 follow-up spec，冻结“SPA 内 tab 切换不得回退到 Dashboard startup skeleton”的验收口径。
- 2026-04-17: 实现完成；Dashboard shell hydration guard、Storybook pending story、Playwright 回归与视觉证据路径已补齐。
- 2026-04-17: 主人确认本轮不需要截图资产，最终以本地浏览器手测替代截图落盘。
- 2026-04-24: path-backed tab surface 上线后，补齐跨 route remount 的会话态保留，继续保证 `/stars` 等 tab 切换只显示局部 skeleton、不会回退全局 startup skeleton，也不会重复触发 sidebar / reaction-token 启动链路。
