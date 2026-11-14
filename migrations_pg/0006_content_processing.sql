-- PostgreSQL content-processing primitives. Legacy SQLite tables are imported
-- separately; these tables are the authoritative PG runtime state.
CREATE TABLE IF NOT EXISTS content_processing_control (
    id integer PRIMARY KEY CHECK (id = 1),
    mode text NOT NULL CHECK (mode IN ('legacy', 'rollback_freeze', 'global')),
    switch_token text,
    updated_at timestamptz NOT NULL
);

INSERT INTO content_processing_control (id, mode, updated_at)
VALUES (1, 'legacy', CURRENT_TIMESTAMP)
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS content_work_items (
    id text PRIMARY KEY,
    canonical_resource_type text NOT NULL CHECK (canonical_resource_type IN ('release', 'announcement', 'notification')),
    canonical_resource_id text NOT NULL,
    pipeline text NOT NULL CHECK (pipeline IN ('translation', 'polishing')),
    variant text NOT NULL,
    target_lang text NOT NULL,
    source_hash text NOT NULL,
    protocol_version text NOT NULL,
    model_profile text NOT NULL,
    source_snapshot_json text NOT NULL,
    configuration_fingerprint text NOT NULL,
    status text NOT NULL CHECK (status IN ('queued', 'running', 'ready', 'failed', 'not_applicable', 'deferred_provider', 'blocked_config', 'cancelled', 'superseded')),
    priority integer NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),
    cache_hit boolean NOT NULL DEFAULT false,
    token_estimate integer NOT NULL DEFAULT 0 CHECK (token_estimate >= 0),
    batch_id text,
    lease_owner text,
    lease_expires_at timestamptz,
    attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_retry_at timestamptz,
    retry_expires_at timestamptz,
    retry_after_at timestamptz,
    failure_class text,
    supersedes_work_item_id text REFERENCES content_work_items(id),
    cancelled_at timestamptz,
    created_at timestamptz NOT NULL,
    started_at timestamptz,
    finished_at timestamptz,
    updated_at timestamptz NOT NULL,
    UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile)
);

CREATE INDEX IF NOT EXISTS idx_content_work_items_status_priority ON content_work_items(status, priority DESC, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_content_work_items_retry ON content_work_items(status, next_retry_at ASC);
CREATE INDEX IF NOT EXISTS idx_content_work_items_resource ON content_work_items(canonical_resource_type, canonical_resource_id, pipeline);

CREATE TABLE IF NOT EXISTS content_batches (
    id text PRIMARY KEY,
    partition_key text NOT NULL,
    target_lang text NOT NULL,
    protocol_version text NOT NULL,
    model_profile text NOT NULL,
    trigger_reason text NOT NULL,
    worker_id text,
    worker_kind text NOT NULL DEFAULT 'general' CHECK (worker_kind = 'general'),
    request_count integer NOT NULL DEFAULT 0 CHECK (request_count >= 0),
    item_count integer NOT NULL DEFAULT 0 CHECK (item_count >= 0),
    estimated_input_tokens integer NOT NULL DEFAULT 0 CHECK (estimated_input_tokens >= 0),
    status text NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed')),
    error_code text,
    error_summary text,
    created_at timestamptz NOT NULL,
    started_at timestamptz,
    finished_at timestamptz,
    updated_at timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_content_batches_status_created ON content_batches(status, created_at ASC);

CREATE TABLE IF NOT EXISTS content_batch_items (
    id text PRIMARY KEY,
    batch_id text NOT NULL REFERENCES content_batches(id) ON DELETE CASCADE,
    work_item_id text NOT NULL REFERENCES content_work_items(id) ON DELETE CASCADE,
    item_index integer NOT NULL CHECK (item_index >= 0),
    request_count integer NOT NULL DEFAULT 0 CHECK (request_count >= 0),
    token_estimate integer NOT NULL DEFAULT 0 CHECK (token_estimate >= 0),
    result_status text,
    error_code text,
    error_summary text,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE (batch_id, work_item_id)
);

CREATE INDEX IF NOT EXISTS idx_content_batch_items_batch_order ON content_batch_items(batch_id, item_index ASC);

CREATE TABLE IF NOT EXISTS content_result_projections (
    id text PRIMARY KEY,
    canonical_resource_type text NOT NULL CHECK (canonical_resource_type IN ('release', 'announcement', 'notification')),
    canonical_resource_id text NOT NULL,
    pipeline text NOT NULL CHECK (pipeline IN ('translation', 'polishing')),
    variant text NOT NULL,
    target_lang text NOT NULL,
    protocol_version text NOT NULL,
    model_profile text NOT NULL,
    source_hash text NOT NULL,
    work_item_id text NOT NULL REFERENCES content_work_items(id),
    active_work_item_id text REFERENCES content_work_items(id),
    payload_json text NOT NULL,
    published_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash)
);

CREATE INDEX IF NOT EXISTS idx_content_result_projections_resource ON content_result_projections(canonical_resource_type, canonical_resource_id, pipeline);
