ALTER TABLE social_activity_events
  ADD COLUMN discussion_number INTEGER;

UPDATE social_activity_events
SET discussion_number = CAST(
  substr(
    html_url,
    instr(lower(html_url), '/discussions/') + length('/discussions/')
  ) AS INTEGER
)
WHERE kind = 'announcement'
  AND discussion_number IS NULL
  AND html_url IS NOT NULL
  AND instr(lower(html_url), '/discussions/') > 0;

CREATE INDEX IF NOT EXISTS idx_social_activity_events_user_repo_discussion
  ON social_activity_events(user_id, kind, repo_full_name, discussion_number, occurred_at DESC, id DESC);
