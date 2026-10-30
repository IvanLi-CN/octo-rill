CREATE TABLE IF NOT EXISTS release_reading_backfill_runs (
  id TEXT PRIMARY KEY,
  started_at TEXT NOT NULL,
  window_start_utc TEXT NOT NULL,
  window_end_utc TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'waiting', 'paused', 'succeeded', 'failed')),
  cursor_published_at TEXT,
  cursor_release_id INTEGER,
  total INTEGER NOT NULL DEFAULT 0,
  succeeded INTEGER NOT NULL DEFAULT 0,
  failed INTEGER NOT NULL DEFAULT 0,
  waiting INTEGER NOT NULL DEFAULT 0,
  skipped INTEGER NOT NULL DEFAULT 0,
  lease_owner TEXT,
  lease_expires_at TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_release_reading_backfill_runs_status
  ON release_reading_backfill_runs(status, created_at DESC);

CREATE TABLE IF NOT EXISTS release_reading_backfill_items (
  run_id TEXT NOT NULL,
  release_id INTEGER NOT NULL,
  authorized_user_id TEXT,
  published_at TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'waiting', 'succeeded', 'failed', 'skipped')),
  attempt_count INTEGER NOT NULL DEFAULT 0,
  error_text TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (run_id, release_id),
  FOREIGN KEY(run_id) REFERENCES release_reading_backfill_runs(id) ON DELETE CASCADE,
  FOREIGN KEY(release_id) REFERENCES repo_releases(release_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_release_reading_backfill_items_cursor
  ON release_reading_backfill_items(run_id, status, published_at ASC, release_id ASC);
