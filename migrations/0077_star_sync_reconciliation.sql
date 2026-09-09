ALTER TABLE admin_runtime_settings
  ADD COLUMN star_sync_delta_interval_minutes INTEGER NOT NULL DEFAULT 30
  CHECK (star_sync_delta_interval_minutes BETWEEN 1 AND 120);

ALTER TABLE admin_runtime_settings
  ADD COLUMN star_sync_full_sweep_interval_minutes INTEGER NOT NULL DEFAULT 1440
  CHECK (star_sync_full_sweep_interval_minutes BETWEEN 60 AND 10080);

CREATE TABLE IF NOT EXISTS starred_repo_connection_memberships (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_connection_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  full_name TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  html_url TEXT NOT NULL,
  stargazed_at TEXT,
  is_private INTEGER NOT NULL DEFAULT 0,
  owner_avatar_url TEXT,
  open_graph_image_url TEXT,
  uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0,
  repo_stargazer_count INTEGER,
  last_seen_epoch_id TEXT,
  last_delta_seen_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, github_connection_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY(github_connection_id) REFERENCES github_connections(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_starred_repo_connection_memberships_epoch_prune
  ON starred_repo_connection_memberships(
    user_id,
    github_connection_id,
    last_seen_epoch_id,
    last_delta_seen_at
  );

CREATE INDEX IF NOT EXISTS idx_starred_repo_connection_memberships_user_repo
  ON starred_repo_connection_memberships(user_id, repo_id, stargazed_at DESC);

CREATE TABLE IF NOT EXISTS star_sync_epochs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_connection_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  started_at TEXT NOT NULL,
  next_cursor TEXT,
  total_count_at_start INTEGER,
  processed_pages INTEGER NOT NULL DEFAULT 0,
  processed_items INTEGER NOT NULL DEFAULT 0,
  next_slice_not_before TEXT,
  lease_owner_id TEXT,
  lease_expires_at TEXT,
  completed_at TEXT,
  failure_reason TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY(github_connection_id) REFERENCES github_connections(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_star_sync_epochs_one_active_connection
  ON star_sync_epochs(github_connection_id)
  WHERE status IN ('queued', 'running');

CREATE INDEX IF NOT EXISTS idx_star_sync_epochs_connection_completed
  ON star_sync_epochs(github_connection_id, status, completed_at DESC);

CREATE INDEX IF NOT EXISTS idx_star_sync_epochs_slice_due
  ON star_sync_epochs(status, next_slice_not_before, lease_expires_at);

-- A legacy user-level snapshot does not identify which linked connection saw each repo.
-- Seed every current connection with the legacy record; completed connection epochs remove
-- those conservative duplicates only after their own complete scan succeeds.
INSERT INTO starred_repo_connection_memberships (
  id,
  user_id,
  github_connection_id,
  repo_id,
  full_name,
  owner_login,
  name,
  description,
  html_url,
  stargazed_at,
  is_private,
  owner_avatar_url,
  open_graph_image_url,
  uses_custom_open_graph_image,
  repo_stargazer_count,
  last_seen_epoch_id,
  last_delta_seen_at,
  created_at,
  updated_at
)
SELECT
  lower(hex(randomblob(16))),
  sr.user_id,
  gc.id,
  sr.repo_id,
  sr.full_name,
  sr.owner_login,
  sr.name,
  sr.description,
  sr.html_url,
  sr.stargazed_at,
  sr.is_private,
  sr.owner_avatar_url,
  sr.open_graph_image_url,
  sr.uses_custom_open_graph_image,
  sr.repo_stargazer_count,
  NULL,
  NULL,
  sr.updated_at,
  sr.updated_at
FROM starred_repos sr
JOIN github_connections gc ON gc.user_id = sr.user_id
ON CONFLICT(user_id, github_connection_id, repo_id) DO NOTHING;
