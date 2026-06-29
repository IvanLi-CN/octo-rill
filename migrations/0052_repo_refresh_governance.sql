ALTER TABLE admin_runtime_settings
  ADD COLUMN repo_refresh_system_budget_per_window INTEGER NOT NULL DEFAULT 1000
  CHECK (repo_refresh_system_budget_per_window BETWEEN 1 AND 20000);

ALTER TABLE starred_repos
  ADD COLUMN repo_stargazer_count INTEGER;

ALTER TABLE starred_repos
  ADD COLUMN repo_stargazer_count_updated_at TEXT;

ALTER TABLE owned_repo_star_baselines
  ADD COLUMN repo_stargazer_count INTEGER;

ALTER TABLE owned_repo_star_baselines
  ADD COLUMN repo_stargazer_count_updated_at TEXT;

CREATE TABLE IF NOT EXISTS repo_refresh_governance_snapshots (
  repo_id INTEGER PRIMARY KEY,
  repo_full_name TEXT NOT NULL,
  is_private INTEGER NOT NULL DEFAULT 0,
  watcher_user_count INTEGER NOT NULL,
  watcher_repo_total_sum INTEGER NOT NULL,
  cached_stargazer_count INTEGER,
  cached_stargazer_count_updated_at TEXT,
  priority_rank INTEGER NOT NULL,
  target_window INTEGER NOT NULL,
  target_interval_minutes INTEGER NOT NULL,
  urgency_score REAL NOT NULL DEFAULT 1,
  urgency_bucket TEXT NOT NULL DEFAULT 'active',
  system_last_selected_at TEXT,
  system_last_success_at TEXT,
  actual_last_success_at TEXT,
  actual_last_success_source TEXT,
  active_cycle_id TEXT,
  active_cycle_window_index INTEGER,
  active_cycle_completed INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_repo_refresh_governance_snapshots_priority
  ON repo_refresh_governance_snapshots(priority_rank ASC, repo_id ASC);

CREATE INDEX IF NOT EXISTS idx_repo_refresh_governance_snapshots_cycle
  ON repo_refresh_governance_snapshots(active_cycle_id, active_cycle_completed, active_cycle_window_index, priority_rank);

CREATE TABLE IF NOT EXISTS repo_refresh_governance_cycles (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL CHECK (status IN ('active', 'completed')),
  window_budget INTEGER NOT NULL,
  frozen_repo_count INTEGER NOT NULL,
  completed_repo_count INTEGER NOT NULL DEFAULT 0,
  window_index_started_at INTEGER NOT NULL,
  window_index_completed_at INTEGER,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_repo_refresh_governance_cycles_status
  ON repo_refresh_governance_cycles(status, started_at DESC);

CREATE TABLE IF NOT EXISTS repo_refresh_governance_cycle_members (
  cycle_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  completed_at TEXT,
  removed_from_pool INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (cycle_id, repo_id),
  FOREIGN KEY(cycle_id) REFERENCES repo_refresh_governance_cycles(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_repo_refresh_governance_cycle_members_status
  ON repo_refresh_governance_cycle_members(cycle_id, completed_at, removed_from_pool, repo_id);
