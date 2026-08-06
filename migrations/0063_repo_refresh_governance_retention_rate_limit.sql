CREATE TABLE IF NOT EXISTS repo_refresh_governance_retention_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  window_started_at TEXT NOT NULL,
  deleted_rows INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);
