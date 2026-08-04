# 演进记录（Dashboard SPA 导航避免回退启动骨架）

## 生命周期

- Lifecycle: active
- Created: 2026-04-17
- Last: 2026-07-08

## 变更记录

- 2026-04-17: 新建 follow-up spec，冻结“SPA 内 tab 切换不得回退到 Dashboard startup skeleton”的验收口径。
- 2026-04-17: 实现完成；Dashboard shell hydration guard、Storybook pending story、Playwright 回归与视觉证据路径已补齐。
- 2026-04-17: 主人确认本轮不需要截图资产，最终以本地浏览器手测替代截图落盘。
- 2026-04-24: path-backed tab surface 上线后，补齐跨 route remount 的会话态保留，继续保证 `/stars` 等 tab 切换只显示局部 skeleton、不会回退全局 startup skeleton，也不会重复触发 sidebar / reaction-token 启动链路。
- 2026-07-08: Dashboard server-state cache 改用 React Query；已访问 route 的 Back/Forward 命中直接恢复 feed 内容，briefs/notifications/reaction-token status 进入 1 小时 Dashboard query 持久化白名单，并保持 Service Worker 私有 API bypass 边界不变。

## 变更记录（Change log）

- 2026-04-17: 新建 follow-up spec，冻结“SPA 内 tab 切换不得回退到 Dashboard startup skeleton”的验收口径。
- 2026-04-17: 实现完成；Dashboard shell hydration guard、Storybook pending story、Playwright 回归与视觉证据路径已补齐。
- 2026-04-17: 主人确认本轮不需要截图资产，最终以本地浏览器手测替代截图落盘。
- 2026-04-22: 同步 Dashboard 主 tab current truth；SPA tab 切换的 canonical URL 已切到 pathname-driven `/stars` 等路径。
- 2026-07-08: 引入 React Query 作为 Dashboard server-state 缓存层；浏览器 Back/Forward 命中已访问 route 时直接恢复缓存内容，并以 1 小时短期本地持久化支持 PWA 恢复。
