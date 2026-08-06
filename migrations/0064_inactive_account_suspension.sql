ALTER TABLE users
  ADD COLUMN paused_at TEXT;

CREATE INDEX IF NOT EXISTS idx_users_pause_maintenance
  ON users (is_disabled, paused_at, last_active_at, created_at);
