# Database contract

## New table

`admin_dashboard_daily_rollups`

- `rollup_date TEXT NOT NULL`
- `time_zone TEXT NOT NULL`
- `task_type TEXT NOT NULL`
- `total_users INTEGER NOT NULL`
- `active_users INTEGER NOT NULL`
- `queued_count INTEGER NOT NULL`
- `running_count INTEGER NOT NULL`
- `succeeded_count INTEGER NOT NULL`
- `failed_count INTEGER NOT NULL`
- `canceled_count INTEGER NOT NULL`
- `updated_at TEXT NOT NULL`

Primary key:

- `(rollup_date, time_zone, task_type)`

Index:

- `idx_admin_dashboard_rollups_tz_date(time_zone, rollup_date DESC)`

## Upsert rules

- Rollups are written per local day, per time zone, per task type.
- Reads of `GET /api/admin/dashboard` must upsert the latest 7-day window before returning the response.
- `total_users` is counted from users created before the end of the target local day.
- `active_users` is counted from users whose `last_active_at` falls within the target local day.
- Task status counts are computed from `job_tasks.created_at` within the target local day.
