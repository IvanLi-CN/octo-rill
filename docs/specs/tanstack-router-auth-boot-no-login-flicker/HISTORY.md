# 演进记录（TanStack Router 接管前端路由并消除登录页闪现）

## 生命周期

- Lifecycle: active
- Created: 2026-04-15
- Last: 2026-04-15

## 历史摘要

- 2026-04-15: 建立该主题规格并冻结基础范围。
- 2026-04-15: 已交付；PR #80; fast-track; TanStack Router SPA routing + three-layer startup model + build-time version monitor landed

## Change log

- 2026-04-15：完成 TanStack Router 接管、auth bootstrap 与原有 Dashboard / Admin / Admin Jobs deep link 兼容。
- 2026-04-22：同步 Dashboard 路由 current truth；顶部主 tab 改为 pathname-driven canonical surface，release detail canonical deep link 改为 `/<owner>/<repo>/releases/tag/<tag>?from=<tab>`，legacy query ingress 继续兼容。
