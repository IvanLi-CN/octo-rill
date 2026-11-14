-- Metadata-only audit and recovery state for PostgreSQL content workers.
CREATE TABLE IF NOT EXISTS translation_attempt_events (
    id text PRIMARY KEY,
    work_item_id text NOT NULL REFERENCES translation_work_items(id) ON DELETE CASCADE,
    request_id text,
    batch_id text,
    scope_user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL,
    variant text NOT NULL,
    entity_id text NOT NULL,
    target_lang text NOT NULL,
    attempt_no integer NOT NULL CHECK (attempt_no >= 1),
    trigger text NOT NULL CHECK (trigger IN ('initial', 'manual_retry', 'automatic_recovery', 'system_requeue')),
    event_type text NOT NULL CHECK (event_type IN ('attempt_queued', 'attempt_started', 'attempt_completed', 'retry_scheduled')),
    result_status text CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
    error_code text,
    error_summary text,
    failure_class text CHECK (failure_class IS NULL OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')),
    retry_eligible boolean NOT NULL DEFAULT false,
    next_retry_at timestamptz,
    llm_call_ids_json text NOT NULL DEFAULT '[]',
    created_at timestamptz NOT NULL,
    UNIQUE (work_item_id, attempt_no, event_type)
);

CREATE INDEX IF NOT EXISTS idx_translation_attempt_events_work_item_created ON translation_attempt_events(work_item_id, created_at ASC, id ASC);

CREATE TABLE IF NOT EXISTS translation_attempt_llm_calls (
    id text PRIMARY KEY,
    work_item_id text NOT NULL REFERENCES translation_work_items(id) ON DELETE CASCADE,
    attempt_no integer NOT NULL CHECK (attempt_no >= 1),
    llm_call_id text NOT NULL,
    stage text NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    relation_role text NOT NULL CHECK (relation_role IN ('primary', 'length_recovery', 'schema_repair', 'fallback', 'legacy_unverified')),
    evidence_availability text NOT NULL DEFAULT 'available' CHECK (evidence_availability IN ('available', 'expired', 'not_captured')),
    created_at timestamptz NOT NULL,
    UNIQUE (work_item_id, attempt_no, llm_call_id, stage, ordinal)
);

CREATE INDEX IF NOT EXISTS idx_translation_attempt_llm_calls_attempt ON translation_attempt_llm_calls(work_item_id, attempt_no, ordinal, id);

CREATE TABLE IF NOT EXISTS llm_diagnostic_access_audit (
    id text PRIMARY KEY,
    call_id text NOT NULL,
    actor_user_id text REFERENCES users(id) ON DELETE SET NULL,
    action text NOT NULL CHECK (action IN ('reveal', 'copy')),
    created_at timestamptz NOT NULL
);

CREATE TABLE IF NOT EXISTS llm_recovery_flags (
    id integer PRIMARY KEY CHECK (id = 1),
    llm_recovery_enabled boolean NOT NULL DEFAULT false,
    llm_recovery_rollout_percent integer NOT NULL DEFAULT 0 CHECK (llm_recovery_rollout_percent BETWEEN 0 AND 100),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

INSERT INTO llm_recovery_flags (id, created_at, updated_at)
VALUES (1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS llm_model_health (
    model text PRIMARY KEY,
    relevant_failure_count integer NOT NULL DEFAULT 0 CHECK (relevant_failure_count >= 0),
    window_started_at timestamptz,
    cooldown_until timestamptz,
    last_failure_class text CHECK (last_failure_class IS NULL OR last_failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')),
    last_failure_at timestamptz,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_llm_model_health_cooldown_until ON llm_model_health(cooldown_until);
