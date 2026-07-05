# 数据库（DB）

## `user_api_keys`

- 范围（Scope）: internal
- 变更（Change）: New
- 影响表（Affected tables）: `user_api_keys`

### Schema delta（结构变更）

- DDL / migration snippet:
  - `id TEXT PRIMARY KEY`
  - `user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE`
  - `name TEXT NOT NULL`
  - `key_hash TEXT NOT NULL UNIQUE`
  - `key_prefix TEXT NOT NULL`
  - `masked_key TEXT NOT NULL`
  - `created_at TEXT NOT NULL`
  - `last_used_at TEXT`
  - `revoked_at TEXT`
- Constraints / indexes:
  - `idx_user_api_keys_user_created_at` on `(user_id, created_at, id)`
  - `idx_user_api_keys_hash_active` on `key_hash` where `revoked_at IS NULL`

### Migration notes（迁移说明）

- 向后兼容窗口（Backward compatibility window）: additive migration，不影响现有用户与 session。
- 发布/上线步骤（Rollout steps）: 先应用 migration，再发布后端与前端。
- 回滚策略（Rollback strategy）: 回滚应用代码后，该表可保留不用；不要求自动删除用户已创建 Key。
- 回填/数据迁移（Backfill / data migration）: None。
