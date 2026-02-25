PRAGMA foreign_keys = ON;

ALTER TABLE users ADD COLUMN daily_brief_utc_time TEXT NOT NULL DEFAULT '00:00';
ALTER TABLE users ADD COLUMN last_active_at TEXT;

CREATE TABLE IF NOT EXISTS job_tasks (
  id TEXT PRIMARY KEY,
  task_type TEXT NOT NULL,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  requested_by INTEGER,
  parent_task_id TEXT,
  payload_json TEXT NOT NULL,
  result_json TEXT,
  error_message TEXT,
  cancel_requested INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_job_tasks_status_created_at
  ON job_tasks(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_job_tasks_task_type_created_at
  ON job_tasks(task_type, created_at DESC);

CREATE TABLE IF NOT EXISTS job_task_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_job_task_events_task_id_id
  ON job_task_events(task_id, id);

CREATE TABLE IF NOT EXISTS daily_brief_hour_slots (
  hour_utc INTEGER PRIMARY KEY,
  enabled INTEGER NOT NULL DEFAULT 1,
  last_dispatch_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (hour_utc >= 0 AND hour_utc <= 23)
);

WITH RECURSIVE hours(n) AS (
  SELECT 0
  UNION ALL
  SELECT n + 1 FROM hours WHERE n < 23
)
INSERT OR IGNORE INTO daily_brief_hour_slots (hour_utc, enabled, last_dispatch_at, updated_at)
SELECT n, 1, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now') FROM hours;
