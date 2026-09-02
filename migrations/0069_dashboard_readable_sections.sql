-- Keep the dashboard section projection on the existing source tables while making
-- the bounded date-window reads selective for users with large histories.
CREATE INDEX IF NOT EXISTS idx_repo_releases_dashboard_readable
  ON repo_releases(repo_id, published_at DESC, release_id DESC);

CREATE INDEX IF NOT EXISTS idx_social_activity_events_dashboard_readable
  ON social_activity_events(user_id, occurred_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_briefs_dashboard_readable
  ON briefs(user_id, date DESC, window_end_utc DESC, id DESC);
