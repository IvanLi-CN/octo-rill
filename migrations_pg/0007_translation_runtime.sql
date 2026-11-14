-- PostgreSQL translation request and batching state.
CREATE TABLE IF NOT EXISTS translation_work_items (
    id text PRIMARY KEY,
    dedupe_key text NOT NULL UNIQUE,
    scope_user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL,
    variant text NOT NULL,
    entity_id text NOT NULL,
    target_lang text NOT NULL,
    protocol_version text NOT NULL,
    model_profile text NOT NULL,
    source_hash text NOT NULL,
    source_blocks_json text NOT NULL,
    target_slots_json text NOT NULL,
    token_estimate integer NOT NULL CHECK (token_estimate >= 0),
    deadline_at timestamptz NOT NULL,
    status text NOT NULL CHECK (status IN ('queued', 'batched', 'running', 'completed', 'failed')),
    batch_id text,
    result_status text CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
    title_zh text,
    summary_md text,
    body_md text,
    error_text text,
    cache_hit boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL,
    started_at timestamptz,
    finished_at timestamptz,
    updated_at timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_translation_work_items_status_partition ON translation_work_items(status, target_lang, protocol_version, model_profile, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_translation_work_items_deadline ON translation_work_items(status, deadline_at ASC);

CREATE TABLE IF NOT EXISTS translation_requests (
    id text PRIMARY KEY,
    mode text NOT NULL CHECK (mode IN ('async', 'wait', 'stream')),
    source text NOT NULL,
    request_origin text NOT NULL CHECK (request_origin IN ('user', 'system')),
    requested_by text REFERENCES users(id) ON DELETE SET NULL,
    scope_user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    producer_ref text NOT NULL,
    kind text NOT NULL,
    variant text NOT NULL,
    entity_id text NOT NULL,
    target_lang text NOT NULL,
    max_wait_ms integer NOT NULL CHECK (max_wait_ms >= 0),
    source_hash text NOT NULL,
    source_blocks_json text NOT NULL,
    target_slots_json text NOT NULL,
    work_item_id text REFERENCES translation_work_items(id) ON DELETE SET NULL,
    status text NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed')),
    result_status text CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
    title_zh text,
    summary_md text,
    body_md text,
    error_text text,
    created_at timestamptz NOT NULL,
    started_at timestamptz,
    finished_at timestamptz,
    updated_at timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_translation_requests_status_updated_at ON translation_requests(status, updated_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_translation_requests_scope_created_at ON translation_requests(scope_user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS translation_batches (
    id text PRIMARY KEY,
    partition_key text NOT NULL,
    protocol_version text NOT NULL,
    model_profile text NOT NULL,
    target_lang text NOT NULL,
    trigger_reason text NOT NULL,
    worker_slot integer NOT NULL,
    request_count integer NOT NULL CHECK (request_count >= 0),
    item_count integer NOT NULL CHECK (item_count >= 0),
    estimated_input_tokens integer NOT NULL CHECK (estimated_input_tokens >= 0),
    status text NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed')),
    error_text text,
    created_at timestamptz NOT NULL,
    started_at timestamptz,
    finished_at timestamptz,
    updated_at timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_translation_batches_status_created_at ON translation_batches(status, created_at DESC);

CREATE TABLE IF NOT EXISTS translation_batch_items (
    id text PRIMARY KEY,
    batch_id text NOT NULL REFERENCES translation_batches(id) ON DELETE CASCADE,
    work_item_id text NOT NULL REFERENCES translation_work_items(id) ON DELETE CASCADE,
    item_index integer NOT NULL CHECK (item_index >= 0),
    kind text NOT NULL,
    variant text NOT NULL,
    entity_id text NOT NULL,
    producer_count integer NOT NULL DEFAULT 0 CHECK (producer_count >= 0),
    token_estimate integer NOT NULL CHECK (token_estimate >= 0),
    result_status text,
    error_text text,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE (batch_id, work_item_id)
);

CREATE INDEX IF NOT EXISTS idx_translation_batch_items_batch_id ON translation_batch_items(batch_id, item_index ASC);
