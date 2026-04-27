ALTER TABLE admin_runtime_settings
  ADD COLUMN sync_auto_fetch_interval_minutes INTEGER NOT NULL DEFAULT 60
  CHECK (sync_auto_fetch_interval_minutes >= 1 AND sync_auto_fetch_interval_minutes <= 120);
