CREATE TABLE IF NOT EXISTS admin_dashboard_daily_rollups (
  rollup_date TEXT NOT NULL,
  time_zone TEXT NOT NULL,
  task_type TEXT NOT NULL,
  total_users INTEGER NOT NULL,
  active_users INTEGER NOT NULL,
  queued_count INTEGER NOT NULL DEFAULT 0,
  running_count INTEGER NOT NULL DEFAULT 0,
  succeeded_count INTEGER NOT NULL DEFAULT 0,
  failed_count INTEGER NOT NULL DEFAULT 0,
  canceled_count INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (rollup_date, time_zone, task_type)
);

CREATE INDEX IF NOT EXISTS idx_admin_dashboard_rollups_tz_date
  ON admin_dashboard_daily_rollups(time_zone, rollup_date DESC);
