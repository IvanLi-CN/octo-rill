ALTER TABLE users ADD COLUMN daily_brief_local_time TEXT;
ALTER TABLE users ADD COLUMN daily_brief_time_zone TEXT;

CREATE TABLE briefs_new (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  date TEXT NOT NULL,
  window_start_utc TEXT,
  window_end_utc TEXT,
  effective_time_zone TEXT,
  effective_local_boundary TEXT,
  generation_source TEXT NOT NULL DEFAULT 'legacy',
  content_markdown TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

INSERT INTO briefs_new (
  id,
  user_id,
  date,
  window_start_utc,
  window_end_utc,
  effective_time_zone,
  effective_local_boundary,
  content_markdown,
  created_at,
  updated_at,
  generation_source
)
SELECT
  briefs.id,
  briefs.user_id,
  briefs.date,
  NULL,
  NULL,
  NULL,
  NULL,
  briefs.content_markdown,
  briefs.created_at,
  briefs.created_at,
  'legacy'
FROM briefs
JOIN users ON users.id = briefs.user_id;

DROP TABLE briefs;
ALTER TABLE briefs_new RENAME TO briefs;

CREATE INDEX idx_briefs_user_date_created_at
  ON briefs(user_id, date DESC, created_at DESC);

CREATE INDEX idx_briefs_user_window_end_created_at
  ON briefs(user_id, window_end_utc DESC, created_at DESC);

CREATE UNIQUE INDEX idx_briefs_user_window_unique
  ON briefs(user_id, window_start_utc, window_end_utc)
  WHERE window_start_utc IS NOT NULL AND window_end_utc IS NOT NULL;

CREATE TABLE brief_release_memberships (
  brief_id TEXT NOT NULL,
  release_id INTEGER NOT NULL,
  release_ts_utc TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (brief_id, release_id),
  FOREIGN KEY(brief_id) REFERENCES briefs(id) ON DELETE CASCADE,
  FOREIGN KEY(release_id) REFERENCES repo_releases(release_id) ON DELETE CASCADE
);

CREATE INDEX idx_brief_release_memberships_release_id
  ON brief_release_memberships(release_id);

CREATE INDEX idx_brief_release_memberships_brief_ordinal
  ON brief_release_memberships(brief_id, ordinal ASC);
