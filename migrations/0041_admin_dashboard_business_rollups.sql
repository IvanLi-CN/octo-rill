ALTER TABLE admin_dashboard_daily_rollups
  ADD COLUMN business_ok_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE admin_dashboard_daily_rollups
  ADD COLUMN business_partial_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE admin_dashboard_daily_rollups
  ADD COLUMN business_failed_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE admin_dashboard_daily_rollups
  ADD COLUMN business_disabled_count INTEGER NOT NULL DEFAULT 0;
