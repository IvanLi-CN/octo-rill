CREATE TABLE IF NOT EXISTS public_repo_release_usage (
  id TEXT PRIMARY KEY,
  repo_id INTEGER,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  full_name TEXT NOT NULL,
  full_name_lower TEXT NOT NULL UNIQUE,
  first_registered_at TEXT NOT NULL,
  last_requested_at TEXT NOT NULL,
  last_list_requested_at TEXT,
  last_detail_requested_at TEXT,
  api_list_requests INTEGER NOT NULL DEFAULT 0,
  api_detail_requests INTEGER NOT NULL DEFAULT 0,
  page_list_requests INTEGER NOT NULL DEFAULT 0,
  page_detail_requests INTEGER NOT NULL DEFAULT 0,
  last_sync_status TEXT NOT NULL DEFAULT 'pending',
  last_sync_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (last_sync_status IN ('pending', 'ready', 'failed', 'inaccessible'))
);

CREATE INDEX IF NOT EXISTS idx_public_repo_release_usage_repo_id
  ON public_repo_release_usage(repo_id);

CREATE INDEX IF NOT EXISTS idx_public_repo_release_usage_last_requested_at
  ON public_repo_release_usage(last_requested_at DESC);
