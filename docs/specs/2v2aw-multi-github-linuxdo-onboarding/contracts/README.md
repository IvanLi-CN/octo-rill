# 接口契约（Contracts）

本规格把账号体系拆成“内部账号 + 外部连接 + PAT owner + onboarding flow”四个层次，因此契约只保留本轮实际需要的两类文档：

- `http-apis.md`：OAuth 入口、补绑页绑定上下文、GitHub 连接管理与 PAT owner 返回形状。
- `db.md`：`github_connections`、`reaction_pat_tokens.owner_*`、legacy backfill 与 notifications cursor namespace 语义。

编写约定：

- `../SPEC.md` 的 Inventory 是唯一接口清单来源。
- HTTP 契约负责 owner-facing 路由 / API 行为；数据库契约负责 schema、backfill、兼容层与 rollout。
- 本轮没有新增 RPC、CLI、events 或文件格式契约，因此不创建对应文件。
