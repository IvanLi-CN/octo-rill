# 数据库契约

## users

新增字段：

- `daily_brief_local_time TEXT NULL`
  - 语义：用户保存的本地日报整点，格式固定 `HH:00`
- `daily_brief_time_zone TEXT NULL`
  - 语义：用户保存的 IANA 时区，例如 `Asia/Shanghai`

兼容字段：

- `daily_brief_utc_time TEXT NOT NULL`
  - 仅作为旧数据迁移种子值；新逻辑不再把它作为长期真相源。

## briefs

表语义升级为 brief snapshot，字段：

- `id TEXT PRIMARY KEY`
- `user_id TEXT NOT NULL`
- `date TEXT NOT NULL`
  - 仅作展示标签，代表 snapshot 的 `display_date`
- `window_start_utc TEXT NULL`
- `window_end_utc TEXT NULL`
- `effective_time_zone TEXT NULL`
- `effective_local_boundary TEXT NULL`
- `generation_source TEXT NOT NULL DEFAULT 'legacy'`
  - `legacy | manual | scheduled | history_recompute`
- `content_markdown TEXT NOT NULL`
- `created_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

索引 / 约束：

- `UNIQUE(user_id, window_start_utc, window_end_utc)`（仅在两个窗口字段均非空时生效）
- `idx_briefs_user_window_end_created_at`
- `idx_briefs_user_date_created_at`

## brief_release_memberships

显式记录 brief 与 release 的关联：

- `brief_id TEXT NOT NULL`
- `release_id INTEGER NOT NULL`
- `release_ts_utc TEXT NOT NULL`
- `ordinal INTEGER NOT NULL`
- `created_at TEXT NOT NULL`

约束：

- `PRIMARY KEY (brief_id, release_id)`
- `FOREIGN KEY (brief_id) REFERENCES briefs(id) ON DELETE CASCADE`
- `FOREIGN KEY (release_id) REFERENCES repo_releases(id) ON DELETE CASCADE`

索引：

- `idx_brief_release_memberships_release_id`
- `idx_brief_release_memberships_brief_ordinal`
