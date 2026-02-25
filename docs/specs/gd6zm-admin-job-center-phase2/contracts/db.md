# DB contract

## `users` (modified)

Added columns:

- `daily_brief_utc_time TEXT NOT NULL DEFAULT '00:00'`
- `last_active_at TEXT NULL`

Notes:

- `daily_brief_utc_time` stores UTC clock time (`HH:MM`) without date.
- `last_active_at` stores RFC3339 UTC timestamp.

## `job_tasks` (new)

Task queue and execution status.

Key columns:

- `id TEXT PRIMARY KEY`
- `task_type TEXT NOT NULL`
- `status TEXT NOT NULL` (`queued|running|succeeded|failed|canceled`)
- `source TEXT NOT NULL`
- `requested_by INTEGER NULL`
- `parent_task_id TEXT NULL`
- `payload_json TEXT NOT NULL`
- `result_json TEXT NULL`
- `error_message TEXT NULL`
- `cancel_requested INTEGER NOT NULL DEFAULT 0`
- `created_at / started_at / finished_at / updated_at`

Indexes:

- `idx_job_tasks_status_created_at`
- `idx_job_tasks_task_type_created_at`

## `job_task_events` (new)

Task event stream persisted for audit/SSE replay.

Key columns:

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `task_id TEXT NOT NULL`
- `event_type TEXT NOT NULL`
- `payload_json TEXT NOT NULL`
- `created_at TEXT NOT NULL`

Index:

- `idx_job_task_events_task_id_id`

## `daily_brief_hour_slots` (new)

Fixed 24-slot scheduler configuration table.

Key columns:

- `hour_utc INTEGER PRIMARY KEY` (0..23)
- `enabled INTEGER NOT NULL DEFAULT 1`
- `last_dispatch_at TEXT NULL`
- `updated_at TEXT NOT NULL`

Initialization:

- Migration inserts 24 rows for `hour_utc=0..23` via recursive CTE.
