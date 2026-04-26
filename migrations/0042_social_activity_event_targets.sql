CREATE TABLE social_activity_events_new (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  repo_id INTEGER,
  repo_full_name TEXT,
  repo_owner_avatar_url TEXT,
  repo_open_graph_image_url TEXT,
  repo_uses_custom_open_graph_image INTEGER,
  title TEXT,
  body TEXT,
  html_url TEXT,
  github_event_id TEXT,
  actor_github_user_id INTEGER NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  occurred_at TEXT NOT NULL,
  detected_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

INSERT INTO social_activity_events_new (
  id,
  user_id,
  kind,
  repo_id,
  repo_full_name,
  repo_owner_avatar_url,
  repo_open_graph_image_url,
  repo_uses_custom_open_graph_image,
  title,
  body,
  html_url,
  github_event_id,
  actor_github_user_id,
  actor_login,
  actor_avatar_url,
  actor_html_url,
  occurred_at,
  detected_at,
  created_at,
  updated_at
)
SELECT
  id,
  user_id,
  kind,
  repo_id,
  repo_full_name,
  repo_owner_avatar_url,
  repo_open_graph_image_url,
  repo_uses_custom_open_graph_image,
  NULL,
  NULL,
  NULL,
  NULL,
  actor_github_user_id,
  actor_login,
  actor_avatar_url,
  actor_html_url,
  occurred_at,
  detected_at,
  created_at,
  updated_at
FROM social_activity_events;

DROP TABLE social_activity_events;

ALTER TABLE social_activity_events_new RENAME TO social_activity_events;

CREATE INDEX IF NOT EXISTS idx_social_activity_events_user_sort
  ON social_activity_events(user_id, occurred_at DESC, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_social_activity_events_user_kind_sort
  ON social_activity_events(user_id, kind, occurred_at DESC, created_at DESC, id DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_social_activity_events_dedupe
  ON social_activity_events(user_id, kind, IFNULL(repo_id, -1), actor_github_user_id, occurred_at)
  WHERE kind NOT IN ('announcement', 'repo_forked');

CREATE UNIQUE INDEX IF NOT EXISTS idx_social_activity_events_github_event_dedupe
  ON social_activity_events(user_id, kind, github_event_id)
  WHERE github_event_id IS NOT NULL;
