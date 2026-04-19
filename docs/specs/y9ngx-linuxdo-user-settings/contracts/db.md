## New table: `linuxdo_connections`

用于保存当前 OctoRill 用户绑定到的 LinuxDO 账号快照；仅存绑定态，不存 LinuxDO access token / refresh token / PAT。

### Columns

| column | type | notes |
| --- | --- | --- |
| `user_id` | TEXT PRIMARY KEY | FK -> `users.id` |
| `linuxdo_user_id` | INTEGER NOT NULL UNIQUE | LinuxDO 用户唯一标识 |
| `username` | TEXT NOT NULL | LinuxDO 用户名 |
| `name` | TEXT | LinuxDO 昵称 |
| `avatar_url` | TEXT | 归一化后的头像 URL |
| `trust_level` | INTEGER NOT NULL DEFAULT 0 | LinuxDO 信任等级 |
| `active` | INTEGER NOT NULL DEFAULT 1 | 账号活跃态快照 |
| `silenced` | INTEGER NOT NULL DEFAULT 0 | 禁言态快照 |
| `linked_at` | TEXT NOT NULL | 首次绑定时间 |
| `updated_at` | TEXT NOT NULL | 最近一次绑定快照刷新时间 |

### Constraints

- `FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE`
- `UNIQUE(linuxdo_user_id)`：同一个 LinuxDO 账号不能同时绑定多个 OctoRill 用户

### Notes

- 解绑时直接删除对应 `user_id` 行。
- 重新绑定同一用户时更新快照字段与 `updated_at`，保留最初 `linked_at`。
