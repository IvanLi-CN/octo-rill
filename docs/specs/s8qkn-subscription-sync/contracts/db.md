# DB contract

## `job_tasks` (modified)

Added column:

- `log_file_path TEXT NULL`

Notes:

- Used by task types that persist downloadable run logs.
- Current planned user: `sync.subscriptions`.

## `scheduled_task_dispatch_state` (new)

Scheduler dedupe state table for system-owned recurring jobs.

Key columns:

- `schedule_name TEXT PRIMARY KEY`
- `last_dispatch_key TEXT NULL`
- `last_task_id TEXT NULL`
- `updated_at TEXT NOT NULL`

Notes:

- `schedule_name='sync.subscriptions'` uses UTC half-hour keys like `2026-03-06T14:30`.

## `sync_subscription_events` (new)

Append-only key-event audit table for subscription sync runs.

Key columns:

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `task_id TEXT NOT NULL` (`FK -> job_tasks.id`, `ON DELETE CASCADE`)
- `stage TEXT NOT NULL` (`scheduler|star|release`)
- `event_type TEXT NOT NULL`
- `severity TEXT NOT NULL` (`info|warning|error`)
- `recoverable INTEGER NOT NULL DEFAULT 0`
- `attempt INTEGER NOT NULL DEFAULT 0`
- `user_id INTEGER NULL`
- `repo_id INTEGER NULL`
- `repo_full_name TEXT NULL`
- `payload_json TEXT NULL`
- `created_at TEXT NOT NULL`

Indexes:

- `idx_sync_subscription_events_task_id_id` on `(task_id, id)`
- `idx_sync_subscription_events_user_id_created_at` on `(user_id, created_at DESC)`
- `idx_sync_subscription_events_repo_id_created_at` on `(repo_id, created_at DESC)`
- `idx_sync_subscription_events_repo_full_name_created_at` on `(repo_full_name, created_at DESC)`

Notes:

- Only `task_id` keeps a foreign key.
- `user_id` / `repo_id` are intentionally plain indexed columns to keep high-volume writes and historical retention cheaper.
