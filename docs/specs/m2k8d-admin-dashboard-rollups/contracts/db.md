# Database contract

## Table

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
- `business_ok_count INTEGER NOT NULL DEFAULT 0`
- `business_partial_count INTEGER NOT NULL DEFAULT 0`
- `business_failed_count INTEGER NOT NULL DEFAULT 0`
- `business_disabled_count INTEGER NOT NULL DEFAULT 0`
- `updated_at TEXT NOT NULL`

Primary key:

- `(rollup_date, time_zone, task_type)`

Index:

- `idx_admin_dashboard_rollups_tz_date(time_zone, rollup_date DESC)`

## Upsert rules

- Rollups are written per local day, per system time zone, per task type.
- Scheduler refreshes the latest 30-day window and upserts every row idempotently.
- `total_users` counts users created before the end of the target local day.
- `active_users` counts users whose `last_active_at` falls within the target local day.
- Raw task status counts are computed from `job_tasks.created_at` within the target local day.
- Business outcome counts are derived from task payload/result/error for `translate.release.batch`, `summarize.release.smart.batch`, and `brief.daily.slot`, then persisted alongside raw counts.
- Time boundary comparisons use SQLite `julianday(...)` semantics so sub-second timestamps are preserved correctly.
