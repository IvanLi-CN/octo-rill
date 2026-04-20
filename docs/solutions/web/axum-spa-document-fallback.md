# Axum SPA document fallback（静态资源保持真实 404）

- status: active
- category: web
- applies_to:
  - Axum 源站同时托管前端静态资源与 SPA app shell
  - 需要让浏览器直接访问前端路由返回 `index.html + 200`
  - 不能把缺失静态资源、`/api/**`、`/auth/**` miss 伪装成成功页面

## Summary

当 Axum 使用 `ServeDir(...).not_found_service(ServeFile::new(index.html))` 承接 SPA 时，缺失路径虽然能拿到 app shell，但状态码仍会保留 `404`。这会让合法前端路由在 DevTools、监控与代理层表现成“页面不存在”，同时也会让未知路径与真实静态资源缺失共享同一种回退语义。

更稳妥的模式是：

1. 先让 `ServeDir` 按真实静态资源路径处理请求；
2. 仅当请求满足“HTML 文档导航”条件时，才在 `404` 后回退到 `index.html`；
3. 对静态资源扩展名路径、`/api/**`、`/auth/**` 保持原生 `404`；
4. 再由前端路由器自己的 `notFoundComponent` 承接未知 app path。

## Recommended pattern

- 服务端 fallback 条件至少同时满足：
  - 方法是 `GET` 或 `HEAD`
  - `Accept` 含 `text/html` / `application/xhtml+xml`，或 `sec-fetch-dest=document`
  - 路径不属于 `/api/**`、`/auth/**`
  - 路径看起来不是静态资源（例如末段带扩展名）
- 对未知前端路径返回 `index.html + 200`
- 对缺失资源保留 `404`
- 前端根路由必须提供明确的全局 404 页面，不能依赖“没匹配时静默落首页”

## Why this matters

- 合法前端深链会拥有正确的 document status，避免误报 404
- 静态资源缺失、后端路由缺失依然可观测，不会被 app shell 吃掉
- 新增顶层前端路由时，不需要再去 Rust 层维护白名单

## Verification checklist

- `/settings`、`/admin/...` 等合法 SPA 路由：`200 text/html`
- 未知前端路径：源站返回 app shell，前端展示 not-found UI
- `/assets/...missing...`：真实 `404`
- `/api/...missing...`、`/auth/...missing...`：真实 `404`
