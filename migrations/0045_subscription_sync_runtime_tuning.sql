ALTER TABLE admin_runtime_settings
ADD COLUMN repo_release_worker_concurrency INTEGER NOT NULL DEFAULT 5
CHECK (repo_release_worker_concurrency BETWEEN 1 AND 32);

CREATE TABLE IF NOT EXISTS repo_release_sync_state (
  repo_id INTEGER PRIMARY KEY,
  etag TEXT NULL,
  last_modified TEXT NULL,
  last_success_at TEXT NULL,
  last_attempt_at TEXT NULL,
  last_not_modified_at TEXT NULL,
  last_error_text TEXT NULL,
  backoff_until TEXT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_repo_release_sync_state_backoff
  ON repo_release_sync_state(backoff_until);
