-- Collection timestamps are only populated for releases discovered after this
-- migration. Existing rows deliberately remain NULL: their original discovery
-- moment was not stored and must not be reconstructed from an update timestamp.
ALTER TABLE repo_releases
  ADD COLUMN detected_at TEXT;

CREATE INDEX idx_repo_releases_detected_at
  ON repo_releases(detected_at DESC, release_id DESC);

-- Daily brief polish calls need a first-class parent to be auditable from the
-- generated brief record. Older calls remain unlinked by design.
ALTER TABLE llm_calls
  ADD COLUMN parent_brief_id TEXT;

CREATE INDEX idx_llm_calls_parent_brief_id
  ON llm_calls(parent_brief_id, created_at ASC);
