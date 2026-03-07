PRAGMA foreign_keys = ON;

ALTER TABLE job_tasks ADD COLUMN log_file_path TEXT;

CREATE TABLE IF NOT EXISTS scheduled_task_dispatch_state (
  schedule_name TEXT PRIMARY KEY,
  last_dispatch_key TEXT,
  last_task_id TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_subscription_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  event_type TEXT NOT NULL,
  severity TEXT NOT NULL,
  recoverable INTEGER NOT NULL DEFAULT 0,
  attempt INTEGER NOT NULL DEFAULT 0,
  user_id INTEGER,
  repo_id INTEGER,
  repo_full_name TEXT,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sync_subscription_events_task_id_id
  ON sync_subscription_events(task_id, id);
CREATE INDEX IF NOT EXISTS idx_sync_subscription_events_user_id_created_at
  ON sync_subscription_events(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_sync_subscription_events_repo_id_created_at
  ON sync_subscription_events(repo_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_sync_subscription_events_repo_full_name_created_at
  ON sync_subscription_events(repo_full_name, created_at DESC);
