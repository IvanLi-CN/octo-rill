# 实现状态（Dashboard 顶部 tab 路径化与 GitHub 风格 release deep link）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-22
- Last: 2026-04-24
- Summary: 已交付；path-backed tabs / repo-tag release locator / browser URL proof / post-review hardening landed
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/2x7av-dashboard-tab-path-release-deep-link/SPEC.md`
- `docs/specs/2x7av-dashboard-tab-path-release-deep-link/IMPLEMENTATION.md`
- `docs/specs/2x7av-dashboard-tab-path-release-deep-link/HISTORY.md`
- `docs/specs/qvfxq-release-daily-brief-v2/SPEC.md`
- `docs/specs/67g9w-spa-nav-startup-skeleton-guard/SPEC.md`
- `docs/product.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/2x7av-dashboard-tab-path-release-deep-link/assets/`
- Owner-facing 视觉证据通过聊天快照回传；仓库内不新增浏览器 proof 资产。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 dashboard path-backed tabs、release deep link、legacy ingress 兼容 contract。
- [x] M2: 完成前端 routeState / route files / Dashboard detail restore 的 path-backed 重构。
- [x] M3: 完成 repo/tag detail API、brief link parser / reconciler 与回归测试。
- [x] M4: 完成 targeted browser URL 证明、review-loop 修复、PR 收敛与 merge-ready 前置文档同步。

## 当前实现补充

- repo/tag release detail lookup 已在 SQL 层同时约束 `tag_name + repo release URL prefix`，避免 common tag 先按 tag 扫全库、再在 Rust 侧逐行过滤。
- brief markdown fallback 的 release ref 解析已改为“跨 brief 批量 resolve 一次，再按 brief 回填”，不再对每条 fallback brief 单独发起一轮 locator 解析查询。
- brief markdown fallback 的 batched resolve 现已按 locator 数分批执行，避免超过 SQLite bind limit；同时按 brief 回填时再次去重，保持 `release_ids` / `release_count` 的既有唯一性语义。
- canonical path-backed tab 切换继续遵守 `67g9w` 的壳层保留约束：Dashboard 在同一会话内已完成首屏 hydration 后，再切 `/stars` 等 path-backed tab 只显示局部 feed skeleton，不再回退全局 startup skeleton。
