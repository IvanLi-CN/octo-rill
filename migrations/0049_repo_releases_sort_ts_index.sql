CREATE INDEX IF NOT EXISTS idx_repo_releases_repo_sort_ts
  ON repo_releases(
    repo_id,
    COALESCE(published_at, created_at, updated_at),
    release_id
  );
