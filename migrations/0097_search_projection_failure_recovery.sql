-- Keep permanently failing realtime projections observable without blocking the
-- active outbox or search readiness forever.
CREATE TABLE IF NOT EXISTS search_projection_outbox_failed (
  projection_key TEXT PRIMARY KEY,
  operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
  resource_type TEXT NOT NULL CHECK (resource_type IN ('announcement', 'content', 'notification')),
  resource_id TEXT NOT NULL,
  previous_resource_id TEXT,
  canonical_resource_type TEXT,
  user_id TEXT,
  attempt_count INTEGER NOT NULL CHECK (attempt_count >= 0),
  last_error TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  failed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_search_projection_outbox_failed_resource
  ON search_projection_outbox_failed(resource_type, canonical_resource_type, resource_id);

-- A new source event with the same logical key is a forward recovery signal.
DROP TRIGGER IF EXISTS search_projection_outbox_reactivate_failed;
CREATE TRIGGER search_projection_outbox_reactivate_failed
AFTER INSERT ON search_projection_outbox
BEGIN
  DELETE FROM search_projection_outbox_failed
  WHERE projection_key = NEW.projection_key;
END;

-- A content projection can move out of the searchable pipeline or change its
-- canonical identity. Refresh the old key as well as the new key so stale
-- translated/smart text is cleared or recomputed.
DROP TRIGGER IF EXISTS search_content_projections_outbox_au;
DROP TRIGGER IF EXISTS search_content_projections_outbox_au_old;
DROP TRIGGER IF EXISTS search_content_projections_outbox_au_new;

CREATE TRIGGER search_content_projections_outbox_au_old
AFTER UPDATE ON content_result_projections
WHEN OLD.pipeline IN ('translation', 'polishing')
 AND OLD.target_lang = 'zh-CN'
 AND (
   NEW.pipeline NOT IN ('translation', 'polishing')
   OR NEW.target_lang <> 'zh-CN'
   OR NEW.canonical_resource_type IS NOT OLD.canonical_resource_type
   OR NEW.canonical_resource_id IS NOT OLD.canonical_resource_id
 )
BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, canonical_resource_type,
    available_at, created_at, updated_at
  ) VALUES (
    'content:' || OLD.canonical_resource_type || ':' || OLD.canonical_resource_id,
    'upsert', 'content', OLD.canonical_resource_id, OLD.canonical_resource_type,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    canonical_resource_type = excluded.canonical_resource_type,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;

CREATE TRIGGER search_content_projections_outbox_au_new
AFTER UPDATE ON content_result_projections
WHEN NEW.pipeline IN ('translation', 'polishing')
 AND NEW.target_lang = 'zh-CN'
BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, canonical_resource_type,
    available_at, created_at, updated_at
  ) VALUES (
    'content:' || NEW.canonical_resource_type || ':' || NEW.canonical_resource_id,
    'upsert', 'content', NEW.canonical_resource_id, NEW.canonical_resource_type,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    canonical_resource_type = excluded.canonical_resource_type,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;

-- If a content document was absent when projection retries exhausted, its
-- later backfill insertion reactivates the retained failed item.
DROP TRIGGER IF EXISTS search_projection_outbox_reactivate_content_document;
CREATE TRIGGER search_projection_outbox_reactivate_content_document
AFTER INSERT ON search_documents
WHEN EXISTS (
  SELECT 1
  FROM search_projection_outbox_failed f
  WHERE f.resource_type = 'content'
    AND f.canonical_resource_type = NEW.resource_type
    AND f.resource_id = NEW.resource_id
)
BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, canonical_resource_type,
    user_id, attempt_count, available_at, lease_owner, lease_expires_at,
    last_error, created_at, updated_at
  )
  SELECT projection_key, operation, resource_type, resource_id, canonical_resource_type,
         user_id, 0, CURRENT_TIMESTAMP, NULL, NULL, NULL, created_at, CURRENT_TIMESTAMP
  FROM search_projection_outbox_failed
  WHERE resource_type = 'content'
    AND canonical_resource_type = NEW.resource_type
    AND resource_id = NEW.resource_id
  ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    canonical_resource_type = excluded.canonical_resource_type,
    user_id = excluded.user_id,
    attempt_count = excluded.attempt_count,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
