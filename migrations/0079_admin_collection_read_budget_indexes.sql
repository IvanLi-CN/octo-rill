-- Keep the administrator collection-record read path selective without
-- changing any source row or legacy processing fact.
CREATE INDEX IF NOT EXISTS idx_notifications_admin_canonical_source
  ON notifications(thread_id, updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_translation_work_items_admin_entity_kind_attempt
  ON translation_work_items(entity_id, kind, attempt_count DESC);
