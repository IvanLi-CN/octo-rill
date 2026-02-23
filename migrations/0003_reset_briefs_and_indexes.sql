-- Reset historical briefs and add index for daily brief window scans.

PRAGMA foreign_keys = ON;

DELETE FROM briefs;

CREATE INDEX IF NOT EXISTS idx_releases_user_published_at
  ON releases(user_id, published_at);
