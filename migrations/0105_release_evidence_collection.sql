-- Release evidence is collected by a background task.  Public and authorized
-- snapshots are separate durable identities so a private response can never
-- become a reusable public source.

CREATE TABLE IF NOT EXISTS release_evidence_snapshots (
  id TEXT PRIMARY KEY,
  request_key TEXT NOT NULL UNIQUE,
  release_id INTEGER NOT NULL,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  audience TEXT NOT NULL CHECK (audience IN ('public', 'authorized')),
  audience_user_id TEXT,
  source_revision TEXT NOT NULL,
  base_tag TEXT NOT NULL,
  head_tag TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'ready', 'failed')),
  snapshot_json TEXT,
  content_hash TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  next_retry_at TEXT,
  retry_expires_at TEXT,
  task_id TEXT,
  lease_owner TEXT,
  lease_expires_at TEXT,
  last_error_code TEXT,
  last_error_summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (audience = 'public' AND audience_user_id IS NULL)
    OR (audience = 'authorized' AND audience_user_id IS NOT NULL)
  )
);

CREATE INDEX IF NOT EXISTS idx_release_evidence_snapshots_release_audience
  ON release_evidence_snapshots(
    release_id,
    audience,
    audience_user_id,
    status,
    updated_at DESC
  );

CREATE INDEX IF NOT EXISTS idx_release_evidence_snapshots_retry
  ON release_evidence_snapshots(status, next_retry_at, updated_at);

CREATE TABLE IF NOT EXISTS release_evidence_index (
  id TEXT PRIMARY KEY,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  audience TEXT NOT NULL CHECK (audience IN ('public', 'authorized')),
  audience_user_id TEXT,
  resource_kind TEXT NOT NULL CHECK (
    resource_kind IN ('compare', 'commit', 'tree', 'blob', 'file')
  ),
  resource_key TEXT NOT NULL,
  side TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL CHECK (status IN ('ready', 'failed')),
  payload_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (audience = 'public' AND audience_user_id IS NULL)
    OR (audience = 'authorized' AND audience_user_id IS NOT NULL)
  )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_release_evidence_index_scope_key
  ON release_evidence_index(
    repo_id,
    audience,
    COALESCE(audience_user_id, ''),
    resource_kind,
    resource_key,
    side
  );

CREATE INDEX IF NOT EXISTS idx_release_evidence_index_lookup
  ON release_evidence_index(
    repo_id,
    audience,
    audience_user_id,
    resource_kind,
    updated_at DESC
  );

CREATE TABLE IF NOT EXISTS release_evidence_snapshot_items (
  snapshot_id TEXT NOT NULL,
  index_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  created_at TEXT NOT NULL,
  PRIMARY KEY (snapshot_id, index_id),
  FOREIGN KEY(snapshot_id) REFERENCES release_evidence_snapshots(id) ON DELETE CASCADE,
  FOREIGN KEY(index_id) REFERENCES release_evidence_index(id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_release_evidence_snapshot_items_order
  ON release_evidence_snapshot_items(snapshot_id, ordinal ASC, index_id ASC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_release_evidence_task_active_snapshot
  ON job_tasks(
    task_type,
    json_extract(payload_json, '$.snapshot_id')
  )
  WHERE task_type = 'release.evidence' AND status IN ('queued', 'running');

ALTER TABLE content_work_items
  ADD COLUMN evidence_snapshot_id TEXT;

ALTER TABLE content_work_items
  ADD COLUMN evidence_status TEXT NOT NULL DEFAULT 'not_required'
    CHECK (evidence_status IN ('not_required', 'pending', 'ready', 'failed'));

CREATE INDEX IF NOT EXISTS idx_content_work_items_evidence
  ON content_work_items(evidence_snapshot_id, evidence_status, status);
