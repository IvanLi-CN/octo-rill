-- Prepare the model-independent identity upgrade without changing live rows or
-- the current model-specific scheduler behavior.

ALTER TABLE content_attempt_events
  ADD COLUMN configuration_snapshot_json TEXT;
ALTER TABLE content_attempt_events
  ADD COLUMN route_snapshot_json TEXT;
ALTER TABLE content_attempt_events
  ADD COLUMN configuration_fingerprint TEXT;

CREATE TABLE content_work_identities (
  id TEXT PRIMARY KEY,
  canonical_resource_type TEXT NOT NULL
    CHECK (canonical_resource_type IN ('release', 'announcement', 'notification')),
  canonical_resource_id TEXT NOT NULL,
  pipeline TEXT NOT NULL CHECK (pipeline IN ('translation', 'polishing')),
  variant TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (
    canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  )
);

CREATE TABLE content_work_identity_members (
  work_item_id TEXT PRIMARY KEY,
  identity_id TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE RESTRICT,
  FOREIGN KEY (identity_id) REFERENCES content_work_identities(id) ON DELETE RESTRICT
);

CREATE INDEX idx_content_work_identity_members_identity
  ON content_work_identity_members(identity_id, work_item_id);

CREATE TABLE content_current_result_projections (
  identity_id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  active_work_item_id TEXT,
  source_projection_id TEXT,
  payload_json TEXT NOT NULL,
  published_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (identity_id) REFERENCES content_work_identities(id) ON DELETE RESTRICT,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE RESTRICT,
  FOREIGN KEY (active_work_item_id) REFERENCES content_work_items(id) ON DELETE RESTRICT,
  FOREIGN KEY (source_projection_id) REFERENCES content_result_projections(id) ON DELETE RESTRICT
);

CREATE INDEX idx_content_current_result_projections_work_item
  ON content_current_result_projections(work_item_id);

CREATE TABLE content_identity_upgrade_control (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  generation INTEGER NOT NULL CHECK (generation >= 1),
  status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'paused', 'completed', 'failed')),
  phase TEXT NOT NULL CHECK (
    phase IN ('work_identity_backfill', 'projection_backfill', 'blocked_config_recovery', 'complete')
  ),
  cursor TEXT,
  last_error_code TEXT,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL
);

INSERT INTO content_identity_upgrade_control (
  id, generation, status, phase, cursor, updated_at
) VALUES (
  1, 1, 'pending', 'work_identity_backfill', NULL, CURRENT_TIMESTAMP
);
