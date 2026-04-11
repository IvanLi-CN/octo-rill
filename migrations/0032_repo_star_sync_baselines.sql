CREATE TABLE IF NOT EXISTS repo_star_sync_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

INSERT INTO repo_star_sync_baselines (id, user_id, initialized_at, updated_at)
SELECT
  user_id || '-repo-star-sync',
  user_id,
  MIN(initialized_at),
  MAX(updated_at)
FROM owned_repo_star_baselines
GROUP BY user_id
ON CONFLICT(user_id) DO UPDATE
SET updated_at = excluded.updated_at;
