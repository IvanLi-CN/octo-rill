-- Durable, metadata-only history for scheduler attempts. Unlike llm_calls,
-- this stream intentionally does not retain source text or model responses.
ALTER TABLE translation_work_items
  ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0
  CHECK (attempt_count >= 0);

ALTER TABLE translation_work_items
  ADD COLUMN next_attempt_trigger TEXT NOT NULL DEFAULT 'initial'
  CHECK (
    next_attempt_trigger IN (
      'initial',
      'manual_retry',
      'automatic_recovery',
      'system_requeue'
    )
  );

CREATE TABLE translation_attempt_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  work_item_id TEXT NOT NULL,
  request_id TEXT,
  batch_id TEXT,
  scope_user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no >= 1),
  trigger TEXT NOT NULL
    CHECK (
      trigger IN (
        'initial',
        'manual_retry',
        'automatic_recovery',
        'system_requeue'
      )
    ),
  event_type TEXT NOT NULL
    CHECK (
      event_type IN (
        'attempt_queued',
        'attempt_started',
        'attempt_completed',
        'retry_scheduled'
      )
    ),
  result_status TEXT
    CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  error_code TEXT,
  error_summary TEXT,
  failure_class TEXT
    CHECK (
      failure_class IS NULL
      OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
    ),
  retry_eligible INTEGER NOT NULL DEFAULT 0 CHECK (retry_eligible IN (0, 1)),
  next_retry_at TEXT,
  llm_call_ids_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL
);

CREATE INDEX idx_translation_attempt_events_entity_created
  ON translation_attempt_events(entity_id, kind, variant, created_at ASC, id ASC);

CREATE INDEX idx_translation_attempt_events_work_item_created
  ON translation_attempt_events(work_item_id, created_at ASC, id ASC);

CREATE INDEX idx_translation_attempt_events_batch_id
  ON translation_attempt_events(batch_id, id ASC);
