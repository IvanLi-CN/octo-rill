# 演进记录（Dashboard 顶部 tab 路径化与 GitHub 风格 release deep link）

## 生命周期

- Lifecycle: active
- Created: 2026-04-22
- Last: 2026-04-24

## 变更记录

- 2026-04-22：创建 follow-up spec，冻结 Dashboard 顶部主 tab path-backed、GitHub 风格 release deep link、legacy ingress 兼容与 repo/tag API contract。
- 2026-04-22：完成前端 routeState / lazy routes、后端 repo/tag detail lookup、brief link parser、回归验证与地址栏浏览器 proof；owner-facing 视觉证据通过聊天快照回传，不新增仓库截图资产。
- 2026-04-24：在 branch freshness 收敛前补齐 companion docs，并把 README catalog 升级到 companion-docs 目录结构下的 current truth。
- 2026-04-24：根据 merge-proof review 补强 repo/tag lookup SQL 约束与 brief fallback 批量 resolve；同时修复 pathname-backed tab remount 期间误回退 startup skeleton 的回归。
