-- Support bounded twelve-hour source scans and the linked brief's latest
-- processing-state lookup without changing any retained record.
CREATE INDEX IF NOT EXISTS idx_repo_releases_admin_activity_source_time
  ON repo_releases(
    julianday(COALESCE(published_at, created_at, updated_at)) DESC,
    release_id DESC
  );

CREATE INDEX IF NOT EXISTS idx_social_activity_events_admin_activity_time
  ON social_activity_events(
    kind,
    julianday(occurred_at) DESC,
    lower(repo_full_name),
    discussion_number
  );

CREATE INDEX IF NOT EXISTS idx_social_activity_events_admin_activity_canonical
  ON social_activity_events(
    kind,
    lower(repo_full_name),
    discussion_number,
    occurred_at DESC
  );

CREATE INDEX IF NOT EXISTS idx_notifications_admin_activity_source_time
  ON notifications(
    julianday(updated_at) DESC,
    thread_id,
    updated_at DESC,
    id DESC
  );

CREATE INDEX IF NOT EXISTS idx_briefs_admin_activity_source_time
  ON briefs(julianday(created_at) DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_llm_calls_admin_brief_latest
  ON llm_calls(parent_brief_id, updated_at DESC, id DESC);
