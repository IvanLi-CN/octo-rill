CREATE TABLE IF NOT EXISTS repo_releases (
  id TEXT PRIMARY KEY,
  repo_id INTEGER NOT NULL,
  release_id INTEGER NOT NULL UNIQUE,
  node_id TEXT,
  tag_name TEXT NOT NULL,
  name TEXT,
  body TEXT,
  html_url TEXT NOT NULL,
  published_at TEXT,
  created_at TEXT,
  is_prerelease INTEGER NOT NULL DEFAULT 0,
  is_draft INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  react_plus1 INTEGER NOT NULL DEFAULT 0,
  react_laugh INTEGER NOT NULL DEFAULT 0,
  react_heart INTEGER NOT NULL DEFAULT 0,
  react_hooray INTEGER NOT NULL DEFAULT 0,
  react_rocket INTEGER NOT NULL DEFAULT 0,
  react_eyes INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_repo_releases_repo_published_at
  ON repo_releases(repo_id, published_at DESC, created_at DESC, release_id DESC);

CREATE INDEX IF NOT EXISTS idx_repo_releases_updated_at
  ON repo_releases(updated_at DESC, release_id DESC);

CREATE TABLE IF NOT EXISTS repo_release_work_items (
  id TEXT PRIMARY KEY,
  repo_id INTEGER NOT NULL UNIQUE,
  repo_full_name TEXT NOT NULL,
  status TEXT NOT NULL,
  request_origin TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 0,
  has_new_repo_watchers INTEGER NOT NULL DEFAULT 0,
  deadline_at TEXT NOT NULL,
  last_release_count INTEGER NOT NULL DEFAULT 0,
  last_candidate_failures INTEGER NOT NULL DEFAULT 0,
  last_success_at TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  runtime_owner_id TEXT,
  lease_heartbeat_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_repo_release_work_items_queue
  ON repo_release_work_items(
    status,
    priority DESC,
    has_new_repo_watchers DESC,
    deadline_at ASC,
    created_at ASC
  );

CREATE INDEX IF NOT EXISTS idx_repo_release_work_items_runtime
  ON repo_release_work_items(status, runtime_owner_id, lease_heartbeat_at);

CREATE TABLE IF NOT EXISTS repo_release_watchers (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  user_id TEXT,
  origin TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 0,
  reason TEXT NOT NULL,
  is_new_repo INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'pending',
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(task_id, work_item_id),
  FOREIGN KEY(work_item_id) REFERENCES repo_release_work_items(id) ON DELETE CASCADE,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_repo_release_watchers_task_status
  ON repo_release_watchers(task_id, status, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_repo_release_watchers_work_item_status
  ON repo_release_watchers(work_item_id, status, created_at ASC);

WITH ranked_releases AS (
  SELECT
    repo_id,
    release_id,
    node_id,
    tag_name,
    name,
    body,
    html_url,
    published_at,
    created_at,
    is_prerelease,
    is_draft,
    updated_at,
    react_plus1,
    react_laugh,
    react_heart,
    react_hooray,
    react_rocket,
    react_eyes,
    ROW_NUMBER() OVER (
      PARTITION BY release_id
      ORDER BY updated_at DESC, rowid DESC
    ) AS release_rank
  FROM releases
)
INSERT INTO repo_releases (
  id,
  repo_id,
  release_id,
  node_id,
  tag_name,
  name,
  body,
  html_url,
  published_at,
  created_at,
  is_prerelease,
  is_draft,
  updated_at,
  react_plus1,
  react_laugh,
  react_heart,
  react_hooray,
  react_rocket,
  react_eyes
)
SELECT
  printf('repo-release-%020d', release_id),
  repo_id,
  release_id,
  node_id,
  tag_name,
  name,
  body,
  html_url,
  published_at,
  created_at,
  is_prerelease,
  is_draft,
  updated_at,
  react_plus1,
  react_laugh,
  react_heart,
  react_hooray,
  react_rocket,
  react_eyes
FROM ranked_releases
WHERE release_rank = 1
ON CONFLICT(release_id) DO NOTHING;
