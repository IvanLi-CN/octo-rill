-- Keep provider delivery facts separate from the consuming content contract.
ALTER TABLE llm_calls
  ADD COLUMN finish_reason TEXT;

ALTER TABLE llm_calls
  ADD COLUMN provider_request_id TEXT;

ALTER TABLE llm_calls
  ADD COLUMN provider_http_status INTEGER;

ALTER TABLE translation_work_items
  ADD COLUMN error_code TEXT;

ALTER TABLE translation_work_items
  ADD COLUMN processing_stage TEXT;

ALTER TABLE translation_work_items
  ADD COLUMN provider_status TEXT
  CHECK (
    provider_status IS NULL
    OR provider_status IN ('not_started', 'succeeded', 'failed')
  );

ALTER TABLE translation_work_items
  ADD COLUMN output_contract_status TEXT
  CHECK (
    output_contract_status IS NULL
    OR output_contract_status IN ('not_run', 'passed', 'failed', 'recovered')
  );

ALTER TABLE translation_work_items
  ADD COLUMN retry_disposition TEXT NOT NULL DEFAULT 'not_needed'
  CHECK (
    retry_disposition IN ('not_needed', 'scheduled', 'manual_only', 'in_attempt_recovered')
  );

ALTER TABLE translation_attempt_events
  ADD COLUMN processing_stage TEXT;

ALTER TABLE translation_attempt_events
  ADD COLUMN provider_status TEXT
  CHECK (
    provider_status IS NULL
    OR provider_status IN ('not_started', 'succeeded', 'failed')
  );

ALTER TABLE translation_attempt_events
  ADD COLUMN output_contract_status TEXT
  CHECK (
    output_contract_status IS NULL
    OR output_contract_status IN ('not_run', 'passed', 'failed', 'recovered')
  );

ALTER TABLE translation_attempt_events
  ADD COLUMN retry_disposition TEXT NOT NULL DEFAULT 'not_needed'
  CHECK (
    retry_disposition IN ('not_needed', 'scheduled', 'manual_only', 'in_attempt_recovered')
  );

CREATE TABLE translation_attempt_llm_calls (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  work_item_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no >= 1),
  llm_call_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  relation_role TEXT NOT NULL
    CHECK (relation_role IN ('primary', 'length_recovery', 'schema_repair', 'fallback', 'legacy_unverified')),
  evidence_availability TEXT NOT NULL DEFAULT 'available'
    CHECK (evidence_availability IN ('available', 'expired', 'not_captured')),
  created_at TEXT NOT NULL,
  UNIQUE(work_item_id, attempt_no, llm_call_id, stage, ordinal)
);

CREATE INDEX idx_translation_attempt_llm_calls_attempt
  ON translation_attempt_llm_calls(work_item_id, attempt_no, ordinal, id);

CREATE INDEX idx_translation_attempt_llm_calls_call
  ON translation_attempt_llm_calls(llm_call_id, created_at, id);

CREATE TABLE llm_diagnostic_access_audit (
  id TEXT PRIMARY KEY,
  call_id TEXT NOT NULL,
  actor_user_id TEXT,
  action TEXT NOT NULL
    CHECK (action IN ('reveal', 'copy')),
  created_at TEXT NOT NULL
);

CREATE INDEX idx_llm_diagnostic_access_audit_call_created
  ON llm_diagnostic_access_audit(call_id, created_at DESC, id DESC);

CREATE INDEX idx_llm_diagnostic_access_audit_created
  ON llm_diagnostic_access_audit(created_at);

CREATE INDEX idx_translation_work_items_retry_disposition
  ON translation_work_items(retry_disposition, next_retry_at ASC, updated_at ASC)
  WHERE retry_disposition != 'not_needed';
