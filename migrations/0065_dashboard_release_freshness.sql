ALTER TABLE admin_runtime_settings
  ADD COLUMN dashboard_release_freshness_profile TEXT NOT NULL DEFAULT 'balanced'
  CHECK (dashboard_release_freshness_profile IN ('latest', 'balanced', 'capacity'));

ALTER TABLE repo_release_watchers
  ADD COLUMN freshness_window_minutes INTEGER
  CHECK (freshness_window_minutes IS NULL OR freshness_window_minutes BETWEEN 1 AND 30);

ALTER TABLE repo_release_watchers
  ADD COLUMN freshness_decision TEXT
  CHECK (
    freshness_decision IS NULL
    OR freshness_decision IN ('fetch', 'reused_fresh', 'reused_running')
  );

ALTER TABLE repo_release_watchers
  ADD COLUMN freshness_assessment_json TEXT;

CREATE INDEX IF NOT EXISTS idx_repo_release_watchers_task_freshness
  ON repo_release_watchers(task_id, freshness_decision, updated_at ASC, id ASC);
