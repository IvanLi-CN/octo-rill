ALTER TABLE admin_runtime_settings
  ADD COLUMN sync_auto_fetch_effective_at TEXT;

ALTER TABLE repo_refresh_governance_cycles
  ADD COLUMN window_minutes INTEGER NOT NULL DEFAULT 10
  CHECK (window_minutes BETWEEN 1 AND 120);

ALTER TABLE repo_refresh_governance_cycles
  ADD COLUMN last_selection_window_index INTEGER;

UPDATE repo_refresh_governance_cycles
SET window_minutes = 10,
    last_selection_window_index = window_index_started_at
WHERE last_selection_window_index IS NULL;
