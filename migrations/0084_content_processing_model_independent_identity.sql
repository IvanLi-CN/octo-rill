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
  ),
  UNIQUE (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  )
);

CREATE UNIQUE INDEX idx_content_work_items_identity_fk
  ON content_work_items (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  );

CREATE TABLE content_work_identity_members (
  work_item_id TEXT PRIMARY KEY,
  identity_id TEXT NOT NULL,
  canonical_resource_type TEXT NOT NULL,
  canonical_resource_id TEXT NOT NULL,
  pipeline TEXT NOT NULL,
  variant TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  UNIQUE (identity_id, work_item_id),
  FOREIGN KEY (
    work_item_id, canonical_resource_type, canonical_resource_id, pipeline,
    variant, target_lang, source_hash, protocol_version
  ) REFERENCES content_work_items (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  ) ON DELETE RESTRICT,
  FOREIGN KEY (
    identity_id, canonical_resource_type, canonical_resource_id, pipeline,
    variant, target_lang, source_hash, protocol_version
  ) REFERENCES content_work_identities (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  ) ON DELETE RESTRICT
);

CREATE TABLE content_current_result_projections (
  identity_id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  active_work_item_id TEXT,
  source_projection_id TEXT,
  payload_json TEXT NOT NULL,
  published_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (identity_id) REFERENCES content_work_identities(id) ON DELETE RESTRICT,
  FOREIGN KEY (identity_id, work_item_id)
    REFERENCES content_work_identity_members(identity_id, work_item_id) ON DELETE RESTRICT,
  FOREIGN KEY (active_work_item_id) REFERENCES content_work_items(id) ON DELETE RESTRICT,
  FOREIGN KEY (source_projection_id) REFERENCES content_result_projections(id) ON DELETE RESTRICT
);

CREATE INDEX idx_content_current_result_projections_work_item
  ON content_current_result_projections(work_item_id);

CREATE TRIGGER trg_content_current_result_projections_active_scope_insert
BEFORE INSERT ON content_current_result_projections
WHEN NEW.active_work_item_id IS NOT NULL
 AND NOT EXISTS (
   SELECT 1
   FROM content_work_identities AS identity
   JOIN content_work_items AS work ON work.id = NEW.active_work_item_id
   WHERE identity.id = NEW.identity_id
     AND work.canonical_resource_type = identity.canonical_resource_type
     AND work.canonical_resource_id = identity.canonical_resource_id
     AND work.pipeline = identity.pipeline
     AND work.variant = identity.variant
     AND work.target_lang = identity.target_lang
     AND work.protocol_version = identity.protocol_version
 )
BEGIN
  SELECT RAISE(ABORT, 'active work item identity scope mismatch');
END;

CREATE TRIGGER trg_content_current_result_projections_active_scope_update
BEFORE UPDATE OF identity_id, active_work_item_id ON content_current_result_projections
WHEN NEW.active_work_item_id IS NOT NULL
 AND NOT EXISTS (
   SELECT 1
   FROM content_work_identities AS identity
   JOIN content_work_items AS work ON work.id = NEW.active_work_item_id
   WHERE identity.id = NEW.identity_id
     AND work.canonical_resource_type = identity.canonical_resource_type
     AND work.canonical_resource_id = identity.canonical_resource_id
     AND work.pipeline = identity.pipeline
     AND work.variant = identity.variant
     AND work.target_lang = identity.target_lang
     AND work.protocol_version = identity.protocol_version
 )
BEGIN
  SELECT RAISE(ABORT, 'active work item identity scope mismatch');
END;

CREATE TRIGGER trg_content_current_result_projections_source_scope_insert
BEFORE INSERT ON content_current_result_projections
WHEN NEW.source_projection_id IS NOT NULL
 AND NOT EXISTS (
   SELECT 1
   FROM content_work_identities AS identity
   JOIN content_result_projections AS source
     ON source.id = NEW.source_projection_id
   WHERE identity.id = NEW.identity_id
     AND source.canonical_resource_type = identity.canonical_resource_type
     AND source.canonical_resource_id = identity.canonical_resource_id
     AND source.pipeline = identity.pipeline
     AND source.variant = identity.variant
     AND source.target_lang = identity.target_lang
     AND source.source_hash = identity.source_hash
     AND source.protocol_version = identity.protocol_version
 )
BEGIN
  SELECT RAISE(ABORT, 'source projection identity mismatch');
END;

CREATE TRIGGER trg_content_current_result_projections_source_scope_update
BEFORE UPDATE OF identity_id, source_projection_id ON content_current_result_projections
WHEN NEW.source_projection_id IS NOT NULL
 AND NOT EXISTS (
   SELECT 1
   FROM content_work_identities AS identity
   JOIN content_result_projections AS source
     ON source.id = NEW.source_projection_id
   WHERE identity.id = NEW.identity_id
     AND source.canonical_resource_type = identity.canonical_resource_type
     AND source.canonical_resource_id = identity.canonical_resource_id
     AND source.pipeline = identity.pipeline
     AND source.variant = identity.variant
     AND source.target_lang = identity.target_lang
     AND source.source_hash = identity.source_hash
     AND source.protocol_version = identity.protocol_version
 )
BEGIN
  SELECT RAISE(ABORT, 'source projection identity mismatch');
END;

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
