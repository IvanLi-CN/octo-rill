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
- `runtime_owner_id TEXT NULL`
- `lease_heartbeat_at TEXT NULL`
- `created_at / started_at / finished_at / updated_at`

Indexes:

- `idx_job_tasks_status_created_at`
- `idx_job_tasks_task_type_created_at`
- `idx_job_tasks_running_runtime_lease` (partial, `status='running'`)

Runtime semantics:

- `runtime_owner_id + lease_heartbeat_at` represent the active process lease for `running` tasks.
- Service startup immediately reclaims `running` rows whose owner lease is missing or stale as `failed(runtime_lease_expired)`.
- Periodic sweep only reclaims rows with missing heartbeat or heartbeat older than 90s.
- Reclaimed tasks append a recovery event to `job_task_events`.

## `runtime_owners` (new)

Runtime-level lease registry used by startup recovery.

Key columns:

- `runtime_owner_id TEXT PRIMARY KEY`
- `lease_heartbeat_at TEXT NOT NULL`
- `created_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

Runtime semantics:

- each server process registers one `runtime_owners` row and heartbeats it every 10s
- startup recovery only reclaims foreign-owner work when that owner row is missing or older than 90s
- graceful shutdown removes the current runtime-owner row

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
