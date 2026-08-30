CREATE TABLE llm_recovery_flags (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  llm_recovery_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (llm_recovery_enabled IN (0, 1)),
  llm_recovery_rollout_percent INTEGER NOT NULL DEFAULT 0
    CHECK (llm_recovery_rollout_percent BETWEEN 0 AND 100),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

INSERT INTO llm_recovery_flags (
  id,
  llm_recovery_enabled,
  llm_recovery_rollout_percent,
  created_at,
  updated_at
)
VALUES (1, 0, 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE llm_model_health (
  model TEXT PRIMARY KEY,
  relevant_failure_count INTEGER NOT NULL DEFAULT 0
    CHECK (relevant_failure_count >= 0),
  window_started_at TEXT,
  cooldown_until TEXT,
  last_failure_class TEXT
    CHECK (
      last_failure_class IS NULL
      OR last_failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
    ),
  last_failure_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX idx_llm_model_health_cooldown_until
  ON llm_model_health(cooldown_until);

ALTER TABLE llm_calls
  ADD COLUMN failure_class TEXT
  CHECK (
    failure_class IS NULL
    OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
  );

ALTER TABLE llm_calls
  ADD COLUMN final_model TEXT;

ALTER TABLE llm_calls
  ADD COLUMN fallback_count INTEGER NOT NULL DEFAULT 0
  CHECK (fallback_count >= 0);

ALTER TABLE llm_calls
  ADD COLUMN retry_scheduled_at TEXT;

ALTER TABLE llm_calls
  ADD COLUMN recovery_attempt_count INTEGER NOT NULL DEFAULT 0
  CHECK (recovery_attempt_count >= 0);

CREATE INDEX idx_llm_calls_recovery_scan
  ON llm_calls(failure_class, retry_scheduled_at ASC, created_at ASC)
  WHERE failure_class IS NOT NULL AND retry_scheduled_at IS NOT NULL;

ALTER TABLE llm_call_events
  ADD COLUMN failure_class TEXT
  CHECK (
    failure_class IS NULL
    OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
  );

ALTER TABLE llm_call_events
  ADD COLUMN model TEXT;

ALTER TABLE llm_call_events
  ADD COLUMN attempt INTEGER
  CHECK (attempt IS NULL OR attempt >= 0);

ALTER TABLE llm_call_events
  ADD COLUMN retry_after_ms INTEGER
  CHECK (retry_after_ms IS NULL OR retry_after_ms >= 0);

ALTER TABLE llm_call_events
  ADD COLUMN from_model TEXT;

ALTER TABLE llm_call_events
  ADD COLUMN to_model TEXT;

ALTER TABLE llm_call_events
  ADD COLUMN fallback_count INTEGER
  CHECK (fallback_count IS NULL OR fallback_count >= 0);

ALTER TABLE translation_work_items
  ADD COLUMN failure_class TEXT
  CHECK (
    failure_class IS NULL
    OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
  );

ALTER TABLE translation_work_items
  ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0
  CHECK (retry_count >= 0);

ALTER TABLE translation_work_items
  ADD COLUMN next_retry_at TEXT;

ALTER TABLE translation_work_items
  ADD COLUMN retry_expires_at TEXT;

CREATE INDEX idx_translation_work_items_recovery_scan
  ON translation_work_items(failure_class, next_retry_at ASC, retry_expires_at ASC, id ASC)
  WHERE failure_class IS NOT NULL AND next_retry_at IS NOT NULL;
