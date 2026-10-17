-- Bound supersession reconciliation to complete resource identities and active
-- statuses without scanning unrelated work items for every candidate.
CREATE INDEX IF NOT EXISTS idx_content_work_items_resource_scope_status
  ON content_work_items(
    canonical_resource_type,
    canonical_resource_id,
    pipeline,
    variant,
    target_lang,
    protocol_version,
    status,
    created_at DESC,
    id DESC
  );
