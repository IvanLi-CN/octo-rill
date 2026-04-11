DELETE FROM social_activity_events
WHERE rowid NOT IN (
  SELECT MIN(rowid)
  FROM social_activity_events
  GROUP BY user_id, kind, IFNULL(repo_id, -1), actor_github_user_id, occurred_at
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_social_activity_events_dedupe
  ON social_activity_events(user_id, kind, IFNULL(repo_id, -1), actor_github_user_id, occurred_at);
