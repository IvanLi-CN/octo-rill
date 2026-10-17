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

-- Keep candidate pagination in created_at/id order without sorting the full
-- set of work items that can never be superseded.
CREATE INDEX IF NOT EXISTS idx_content_work_items_supersession_cursor
  ON content_work_items(created_at ASC, id ASC)
  WHERE status IN ('queued', 'failed', 'deferred_provider', 'blocked_config', 'ready');

-- Bound active-history pages without sorting unrelated work items.
CREATE INDEX IF NOT EXISTS idx_content_work_items_supersession_history
  ON content_work_items(
    canonical_resource_type,
    canonical_resource_id,
    pipeline,
    variant,
    target_lang,
    protocol_version,
    created_at ASC,
    id ASC
  )
  WHERE status NOT IN ('cancelled', 'superseded');
