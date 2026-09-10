-- Global content-processing storage. Legacy translation tables remain read-only
-- historical facts and are intentionally not referenced by these tables.

CREATE TABLE content_processing_control (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  mode TEXT NOT NULL CHECK (mode IN ('legacy', 'rollback_freeze', 'global')),
  switch_token TEXT,
  updated_at TEXT NOT NULL
);

INSERT INTO content_processing_control (id, mode, switch_token, updated_at)
VALUES (1, 'legacy', NULL, CURRENT_TIMESTAMP);

CREATE TABLE content_work_items (
  id TEXT PRIMARY KEY,
  canonical_resource_type TEXT NOT NULL
    CHECK (canonical_resource_type IN ('release', 'announcement', 'notification')),
  canonical_resource_id TEXT NOT NULL,
  pipeline TEXT NOT NULL CHECK (pipeline IN ('translation', 'polishing')),
  variant TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  source_snapshot_json TEXT NOT NULL,
  configuration_fingerprint TEXT NOT NULL,
  status TEXT NOT NULL CHECK (
    status IN (
      'queued', 'running', 'ready', 'failed', 'not_applicable',
      'deferred_provider', 'blocked_config', 'cancelled', 'superseded'
    )
  ),
  priority INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),
  cache_hit INTEGER NOT NULL DEFAULT 0 CHECK (cache_hit IN (0, 1)),
  token_estimate INTEGER NOT NULL DEFAULT 0 CHECK (token_estimate >= 0),
  batch_id TEXT,
  lease_owner TEXT,
  lease_expires_at TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  next_retry_at TEXT,
  retry_expires_at TEXT,
  retry_after_at TEXT,
  failure_class TEXT,
  supersedes_work_item_id TEXT,
  cancelled_at TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  UNIQUE (
    canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version, model_profile
  ),
  FOREIGN KEY (supersedes_work_item_id) REFERENCES content_work_items(id)
);

CREATE INDEX idx_content_work_items_status_priority
  ON content_work_items(status, priority DESC, created_at ASC);
CREATE INDEX idx_content_work_items_retry
  ON content_work_items(status, next_retry_at ASC);
CREATE INDEX idx_content_work_items_resource
  ON content_work_items(canonical_resource_type, canonical_resource_id, pipeline);

CREATE TABLE content_batches (
  id TEXT PRIMARY KEY,
  partition_key TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  trigger_reason TEXT NOT NULL,
  worker_id TEXT,
  worker_kind TEXT NOT NULL DEFAULT 'general'
    CHECK (worker_kind = 'general'),
  request_count INTEGER NOT NULL DEFAULT 0 CHECK (request_count >= 0),
  item_count INTEGER NOT NULL DEFAULT 0 CHECK (item_count >= 0),
  estimated_input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (estimated_input_tokens >= 0),
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed')),
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL
);

CREATE INDEX idx_content_batches_status_created
  ON content_batches(status, created_at ASC);

CREATE TABLE content_batch_items (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  item_index INTEGER NOT NULL CHECK (item_index >= 0),
  request_count INTEGER NOT NULL DEFAULT 0 CHECK (request_count >= 0),
  token_estimate INTEGER NOT NULL DEFAULT 0 CHECK (token_estimate >= 0),
  result_status TEXT,
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (batch_id, work_item_id),
  FOREIGN KEY (batch_id) REFERENCES content_batches(id) ON DELETE CASCADE,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_content_batch_items_batch_order
  ON content_batch_items(batch_id, item_index ASC);

CREATE TABLE content_result_projections (
  id TEXT PRIMARY KEY,
  canonical_resource_type TEXT NOT NULL
    CHECK (canonical_resource_type IN ('release', 'announcement', 'notification')),
  canonical_resource_id TEXT NOT NULL,
  pipeline TEXT NOT NULL CHECK (pipeline IN ('translation', 'polishing')),
  variant TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  active_work_item_id TEXT,
  payload_json TEXT NOT NULL,
  published_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (
    canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, protocol_version, model_profile, source_hash
  ),
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id),
  FOREIGN KEY (active_work_item_id) REFERENCES content_work_items(id)
);

CREATE INDEX idx_content_result_projections_resource
  ON content_result_projections(canonical_resource_type, canonical_resource_id, pipeline);

CREATE TABLE content_request_links (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE,
  work_item_id TEXT NOT NULL,
  requester_type TEXT NOT NULL CHECK (requester_type IN ('user', 'system')),
  requester_id TEXT,
  authorization_snapshot_json TEXT NOT NULL,
  producer_ref TEXT NOT NULL,
  request_source TEXT NOT NULL,
  delivery_mode TEXT NOT NULL CHECK (delivery_mode IN ('async', 'wait', 'stream')),
  response_status TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id)
);

CREATE INDEX idx_content_request_links_work_item
  ON content_request_links(work_item_id, created_at ASC);

CREATE TABLE content_attempt_events (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no >= 1),
  trigger TEXT NOT NULL
    CHECK (trigger IN ('initial', 'manual_retry', 'automatic_recovery', 'system_requeue')),
  event_type TEXT NOT NULL
    CHECK (event_type IN ('attempt_queued', 'attempt_started', 'attempt_completed', 'retry_scheduled')),
  result_status TEXT,
  error_code TEXT,
  error_summary TEXT,
  failure_class TEXT,
  retry_disposition TEXT,
  retry_eligible INTEGER NOT NULL DEFAULT 0 CHECK (retry_eligible IN (0, 1)),
  next_retry_at TEXT,
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  token_count INTEGER CHECK (token_count IS NULL OR token_count >= 0),
  cost_microunits INTEGER CHECK (cost_microunits IS NULL OR cost_microunits >= 0),
  created_at TEXT NOT NULL,
  UNIQUE (work_item_id, attempt_no, event_type),
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_content_attempt_events_work_item
  ON content_attempt_events(work_item_id, created_at ASC, id ASC);

CREATE TABLE content_attempt_llm_calls (
  id TEXT PRIMARY KEY,
  attempt_event_id TEXT NOT NULL,
  provider_call_id TEXT NOT NULL,
  model TEXT NOT NULL,
  status TEXT NOT NULL,
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  input_tokens INTEGER CHECK (input_tokens IS NULL OR input_tokens >= 0),
  output_tokens INTEGER CHECK (output_tokens IS NULL OR output_tokens >= 0),
  cost_microunits INTEGER CHECK (cost_microunits IS NULL OR cost_microunits >= 0),
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  UNIQUE (attempt_event_id, provider_call_id),
  FOREIGN KEY (attempt_event_id) REFERENCES content_attempt_events(id) ON DELETE CASCADE
);

CREATE INDEX idx_content_attempt_llm_calls_event
  ON content_attempt_llm_calls(attempt_event_id, created_at ASC);

CREATE TABLE content_legacy_observations (
  id TEXT PRIMARY KEY,
  legacy_table TEXT NOT NULL,
  legacy_primary_key TEXT NOT NULL,
  canonical_resource_type TEXT,
  canonical_resource_id TEXT,
  pipeline TEXT CHECK (pipeline IS NULL OR pipeline IN ('translation', 'polishing')),
  classification TEXT NOT NULL CHECK (classification IN ('legacy_cached', 'legacy_conflict')),
  observation_basis_json TEXT NOT NULL,
  observed_at TEXT NOT NULL,
  UNIQUE (legacy_table, legacy_primary_key)
);

CREATE INDEX idx_content_legacy_observations_resource
  ON content_legacy_observations(canonical_resource_type, canonical_resource_id, pipeline);
