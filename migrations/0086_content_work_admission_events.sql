-- Content-work admission and provider-admission audit facts. Existing work,
-- attempt, call and projection rows remain unchanged.

CREATE TABLE content_work_admission_events (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  event_type TEXT NOT NULL CHECK (
    event_type IN (
      'admission_accepted',
      'admission_noop',
      'admission_rejected_superseded',
      'source_superseded',
      'reconciliation_superseded'
    )
  ),
  replaced_by_work_item_id TEXT NOT NULL DEFAULT '',
  source_hash TEXT NOT NULL,
  source_revision_json TEXT NOT NULL DEFAULT '{}',
  producer_ref TEXT NOT NULL DEFAULT '',
  requester_id TEXT,
  reason_code TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  UNIQUE (work_item_id, event_type, replaced_by_work_item_id, source_hash)
);

CREATE INDEX idx_content_work_admission_events_work_item
  ON content_work_admission_events(work_item_id, created_at ASC, id ASC);

CREATE INDEX idx_content_work_admission_events_retention
  ON content_work_admission_events(created_at ASC, id ASC);

CREATE TABLE content_attempt_provider_admissions (
  id TEXT PRIMARY KEY,
  attempt_event_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no >= 1),
  call_ordinal INTEGER NOT NULL CHECK (call_ordinal >= 0),
  relation_role TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  source_revision_json TEXT NOT NULL DEFAULT '{}',
  admitted_at TEXT NOT NULL,
  UNIQUE (attempt_event_id, call_ordinal),
  FOREIGN KEY (attempt_event_id) REFERENCES content_attempt_events(id) ON DELETE CASCADE,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_content_attempt_provider_admissions_work_item
  ON content_attempt_provider_admissions(work_item_id, attempt_no, call_ordinal);

CREATE INDEX idx_content_attempt_provider_admissions_attempt
  ON content_attempt_provider_admissions(attempt_event_id, call_ordinal);
