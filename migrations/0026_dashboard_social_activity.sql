CREATE TABLE IF NOT EXISTS owned_repo_star_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_owned_repo_star_baselines_user
  ON owned_repo_star_baselines(user_id, repo_id);

CREATE TABLE IF NOT EXISTS repo_star_current_members (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  actor_github_user_id INTEGER NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  starred_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, repo_id, actor_github_user_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_repo_star_current_members_user_repo
  ON repo_star_current_members(user_id, repo_id, actor_github_user_id);

CREATE TABLE IF NOT EXISTS follower_sync_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS follower_current_members (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  actor_github_user_id INTEGER NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, actor_github_user_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_follower_current_members_user
  ON follower_current_members(user_id, actor_github_user_id);

CREATE TABLE IF NOT EXISTS social_activity_events (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  repo_id INTEGER,
  repo_full_name TEXT,
  actor_github_user_id INTEGER NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  occurred_at TEXT NOT NULL,
  detected_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, kind, repo_id, actor_github_user_id, occurred_at),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_social_activity_events_user_sort
  ON social_activity_events(user_id, occurred_at DESC, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_social_activity_events_user_kind_sort
  ON social_activity_events(user_id, kind, occurred_at DESC, created_at DESC, id DESC);
