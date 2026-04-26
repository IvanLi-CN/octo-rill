# Feed 热路径与 Reaction 异步刷新历史（#z8vjj）

## 关键演进

- 首屏 feed 曾在返回前同步拉取 GitHub GraphQL reaction viewer/counts，导致第三方网络进入 Dashboard 主列首屏热路径。
- 本规范将 feed 热路径与 reaction 最新性拆分：feed 优先本地缓存快速返回，reaction 在首屏后异步补齐。

## 决策记录

- 保持 `/api/feed` 响应 shape 兼容，避免大规模前端迁移。
- 接受 viewer 初始短暂默认 false，以换取首屏不被外部 API 阻塞；异步 refresh 静默补齐，用户操作由 toggle API 的 live 校验兜底。
- Reaction refresh 失败不打扰用户，因为它只影响 reaction 最新 viewer/counts，不影响 feed 主信息展示。
