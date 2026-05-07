ALTER TABLE admin_runtime_settings
  ADD COLUMN retry_recent_failures_interval_minutes INTEGER NOT NULL DEFAULT 10
  CHECK (retry_recent_failures_interval_minutes >= 1 AND retry_recent_failures_interval_minutes <= 120);
