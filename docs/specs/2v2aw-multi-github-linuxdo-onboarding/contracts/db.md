# 数据库（DB）

## 模型对照（Original / Current / Legacy）

| 主题 | 原方案 | 当前运行时真相源 | 当前保留的 legacy |
| --- | --- | --- | --- |
| 账号身份 | `users` 直接承载唯一 GitHub 身份 | `users` 只表示 OctoRill 内部账号 | `users.github_*` 仍在 schema 中，但不再决定账号归属或展示优先级 |
| GitHub 连接 | 无 | `github_connections` | 无 |
| GitHub token | `user_tokens` 一账号一条 | `github_connections.access_token_*` | `user_tokens` 仅作为 backfill 输入保留 |
| LinuxDO 连接 | 无统一模型 | `linuxdo_connections` | 无 |
| PAT owner | 无 owner 维度 | `reaction_pat_tokens.owner_*` | 旧 owner 为空的记录允许短暂存在，直到 backfill 或下次更新 |

> Legacy boundary: 本轮完成后，运行时逻辑只应读取 `github_connections` / `linuxdo_connections` / `reaction_pat_tokens.owner_*`；`users.github_*` 与 `user_tokens` 只保留为历史兼容壳，后续版本可用独立 cleanup migration 删除。

## `github_connections`

- 范围（Scope）: internal
- 变更（Change）: New
- 影响表（Affected tables）: `github_connections`, `users`, `reaction_pat_tokens`

### Schema delta（结构变更）

- 新表：
  - `id TEXT PRIMARY KEY`
  - `user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE`
  - `github_user_id INTEGER NOT NULL UNIQUE`
  - `login TEXT NOT NULL`
  - `name TEXT NULL`
  - `avatar_url TEXT NULL`
  - `email TEXT NULL`
  - `access_token_ciphertext BLOB NOT NULL`
  - `access_token_nonce BLOB NOT NULL`
  - `scopes TEXT NOT NULL`
  - `linked_at TEXT NOT NULL`
  - `updated_at TEXT NOT NULL`
- 索引：
  - `idx_github_connections_user_login (user_id, login)`

### Migration notes（迁移说明）

- 向后兼容窗口（Backward compatibility window）:
  - 旧 `users.github_*` 与 `user_tokens` 仍保留在 schema 中，作为历史数据来源与兼容存储存在。
  - 本规格完成后，运行时不再依赖它们来判断账号归属、OAuth token 或 GitHub 连接顺序。
- 发布/上线步骤（Rollout steps）:
  1. 执行 `0039_multi_github_connections.sql` 创建 `github_connections` 与 PAT owner 字段。
  2. 服务启动后执行 `backfill_github_connections(pool)`。
  3. 新登录 / 追加绑定 / 同步 / PAT 校验统一以 `github_connections` 为真实写源与读源。
- 回滚策略（Rollback strategy）:
  - 若只需停用多连接逻辑，可回退到读取第一条 GitHub connection；不建议删除 backfilled 数据。
  - 不建议在回滚时物理删除 `github_connections` 或 PAT owner 字段，以避免新增连接数据丢失。
- 回填/数据迁移（Backfill / data migration）:
  - 从现有 `users + user_tokens` 读取历史 GitHub 身份，为每个用户补至少 1 条 connection。
  - `linked_at/updated_at` 复用历史用户 / token 时间戳。
  - 对已有 `reaction_pat_tokens`，优先回填到该账号的第一条 GitHub connection（`linked_at ASC, id ASC`）。
- 后续清理（Future cleanup）:
  - 等所有运行时查询都稳定迁移到 `github_connections` 后，可以在后续版本删除 `users.github_*` 与 `user_tokens` 的物理依赖。
  - cleanup migration 应与一次完整数据校验一起发布，避免误删仍被历史脚本使用的字段。

## `users` 历史 GitHub 字段

- 范围（Scope）: internal
- 变更（Change）: Semantics narrowed
- 影响表（Affected tables）: `users`

### Runtime contract（运行时约束）

- `users` 继续作为内部账号主表。
- `users.github_user_id/login/name/avatar_url/email` 不再是多 GitHub 绑定的真实来源；它们不能再决定：
  - 哪个 GitHub 归属当前账号；
  - 当前账号使用哪个 OAuth token；
  - 哪个 GitHub 在产品层被视为“主账号”。
- `/api/me` 等运行时账号摘要改为动态读取：
  - GitHub 基础信息来自第一条 GitHub connection；
  - 头像遵循 `linuxdo_connections.avatar_url` 优先，否则回退第一条 GitHub connection 的头像。
- 清理提示（Cleanup note）:
  - 这些字段保留仅为 legacy schema 兼容；后续版本可在确认无运行时依赖后整体移除。

## `reaction_pat_tokens.owner_*`

- 范围（Scope）: internal
- 变更（Change）: Modify
- 影响表（Affected tables）: `reaction_pat_tokens`

### Schema delta（结构变更）

- 新增列：
  - `owner_github_connection_id TEXT NULL`
  - `owner_github_user_id INTEGER NULL`
  - `owner_login TEXT NULL`

### Migration notes（迁移说明）

- 向后兼容窗口（Backward compatibility window）:
  - 旧 token 记录在 backfill 前 owner 为空；接口层需要容忍 `owner = null`。
- 发布/上线步骤（Rollout steps）:
  1. migration 增列；
  2. startup backfill 补齐 owner；
  3. 新增/更新 PAT 时强制写 owner 三元组。
- 回滚策略（Rollback strategy）:
  - 保留新增列即可；旧代码可忽略这些列，不影响读取既有 token ciphertext。
- 回填/数据迁移（Backfill / data migration）:
  - `owner_github_connection_id` 取当前用户第一条 GitHub connection。
  - `owner_github_user_id` / `owner_login` 取该 connection 的 GitHub 摘要。

## `user_tokens`

- 范围（Scope）: internal
- 变更（Change）: Legacy-only
- 影响表（Affected tables）: `user_tokens`

### Runtime contract（运行时约束）

- `user_tokens` 只作为历史单 GitHub 账号的迁移输入。
- 本规格完成后，运行时不再向 `user_tokens` 镜像写入 GitHub OAuth token，也不再以其作为同步或登录的读取源。
- 清理提示（Cleanup note）:
  - 后续版本可以直接删除 `user_tokens` 表，并清理相关迁移/回填代码。

## `sync_state` notifications cursor namespacing

- 范围（Scope）: internal
- 变更（Change）: Modify
- 影响表（Affected tables）: `sync_state`

### Schema delta（结构变更）

- 无 schema 变更。
- 语义变更：notifications 相关 key 从单账号共享 cursor 改成按 connection 命名空间存储：
  - `notifications_since:<connection_id>`
  - `notifications_open_url_repair_v2:<connection_id>`

### Migration notes（迁移说明）

- 向后兼容窗口（Backward compatibility window）:
  - 旧单连接用户仍可继续工作；当账号只有一个 connection 时，新的 namespaced key 与旧行为等价。
- 发布/上线步骤（Rollout steps）:
  1. 新代码按 connection 维度读取和写入 namespaced key；
  2. 每个 connection 独立推进 cursor，再把结果聚合到账户级 inbox；
  3. 不清理旧单 key，允许历史部署平滑过渡。
- 回滚策略（Rollback strategy）:
  - 回退到单连接逻辑时，可继续使用第一条 connection 对应的 namespaced key 或旧单 key；无需额外数据迁移。
- 回填/数据迁移（Backfill / data migration）:
  - 不做预回填；首次运行时按 connection 维度从当前时间点建立独立 cursor。
