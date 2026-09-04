-- Admin collection records are listed by their source time, not discovery time.
-- These indexes support the cross-user administrator view without changing
-- historical detected_at values.
CREATE INDEX IF NOT EXISTS idx_repo_releases_admin_source_time
  ON repo_releases(
    COALESCE(published_at, created_at, updated_at) DESC,
    release_id DESC
  );

CREATE INDEX IF NOT EXISTS idx_social_activity_events_admin_announcement_time
  ON social_activity_events(
    kind,
    occurred_at DESC,
    repo_full_name,
    discussion_number
  );

CREATE INDEX IF NOT EXISTS idx_briefs_admin_created_at
  ON briefs(created_at DESC, id DESC);
