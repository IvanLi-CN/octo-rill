# 实现状态（Dashboard SPA 导航避免回退启动骨架）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-17
- Last: 2026-04-24
- Summary: 已交付；shell hydration gate + local feed skeleton + path-backed remount guard
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/67g9w-spa-nav-startup-skeleton-guard/SPEC.md`
- `docs/specs/README.md`

## 计划资产（Plan assets）

- Directory: 不要求新增截图资产
- Visual evidence source: Storybook pending story + 本地浏览器手测

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 拆分 Dashboard 首屏 hydration 与后续 feed tab 切换的 loading 边界。
- [x] M2: 为局部 feed loading 补稳定 selector 与 Storybook pending story。
- [x] M3: 补齐 Playwright 回归，并审计 admin startup skeleton guard。
- [x] M4: 回填视觉证据并同步 specs index。

## 当前实现补充

- `shellHydrated` 不再只依赖当前组件实例；同一用户在当前会话里已完成一次 Dashboard hydration 后，再沿着 `/` → `/stars` 这类 pathname-backed tab surface remount，也会继续保留壳层。
- Dashboard 会话态现在会记住 sidebar / notifications / reaction-token bootstrap 与最近一次 briefs / inbox 快照，避免 path-backed route remount 把这类“只该启动一次”的副作用重新打回启动期。
