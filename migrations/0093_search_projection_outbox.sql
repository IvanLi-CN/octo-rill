-- Defer searchable projection work until after the source write commits.
-- A single logical document key is enough to coalesce repeated source updates.
CREATE TABLE IF NOT EXISTS search_projection_outbox (
  projection_key TEXT PRIMARY KEY,
  operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
  resource_type TEXT NOT NULL CHECK (resource_type IN ('announcement', 'content', 'notification')),
  resource_id TEXT NOT NULL,
  canonical_resource_type TEXT,
  user_id TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  available_at TEXT NOT NULL,
  lease_owner TEXT,
  lease_expires_at TEXT,
  last_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_search_projection_outbox_ready
  ON search_projection_outbox(available_at, created_at, projection_key)
  WHERE lease_expires_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_search_projection_outbox_lease
  ON search_projection_outbox(lease_expires_at, available_at, created_at);

DROP TRIGGER IF EXISTS search_notifications_ai;
DROP TRIGGER IF EXISTS search_notifications_au;
DROP TRIGGER IF EXISTS search_notifications_ad;
DROP TRIGGER IF EXISTS search_announcements_ai;
DROP TRIGGER IF EXISTS search_announcements_au;
DROP TRIGGER IF EXISTS search_announcements_ad;
DROP TRIGGER IF EXISTS search_content_projections_ai;
DROP TRIGGER IF EXISTS search_content_projections_au;
DROP TRIGGER IF EXISTS search_content_projections_ad;

CREATE TRIGGER search_notifications_outbox_ai AFTER INSERT ON notifications BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'notification:' || NEW.user_id || ':' || NEW.thread_id,
    'upsert', 'notification', NEW.thread_id, NEW.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;

CREATE TRIGGER search_notifications_outbox_au AFTER UPDATE ON notifications BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'notification:' || NEW.user_id || ':' || NEW.thread_id,
    'upsert', 'notification', NEW.thread_id, NEW.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;

CREATE TRIGGER search_notifications_outbox_ad AFTER DELETE ON notifications BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'notification:' || OLD.user_id || ':' || OLD.thread_id,
    'delete', 'notification', OLD.thread_id, OLD.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;

CREATE TRIGGER search_announcements_outbox_ai AFTER INSERT ON social_activity_events
WHEN NEW.kind = 'announcement' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'announcement:' || NEW.id,
    'upsert', 'announcement', NEW.id, NEW.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;

CREATE TRIGGER search_announcements_outbox_au AFTER UPDATE ON social_activity_events
WHEN NEW.kind = 'announcement' OR OLD.kind = 'announcement' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'announcement:' || COALESCE(NEW.id, OLD.id),
    CASE WHEN NEW.kind = 'announcement' THEN 'upsert' ELSE 'delete' END,
    'announcement', COALESCE(NEW.id, OLD.id), COALESCE(NEW.user_id, OLD.user_id),
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;

CREATE TRIGGER search_announcements_outbox_ad AFTER DELETE ON social_activity_events
WHEN OLD.kind = 'announcement' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'announcement:' || OLD.id,
    'delete', 'announcement', OLD.id, OLD.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;

CREATE TRIGGER search_content_projections_outbox_ai AFTER INSERT ON content_result_projections
WHEN NEW.pipeline IN ('translation', 'polishing') AND NEW.target_lang = 'zh-CN' BEGIN
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

CREATE TRIGGER search_content_projections_outbox_au AFTER UPDATE ON content_result_projections
WHEN NEW.pipeline IN ('translation', 'polishing') AND NEW.target_lang = 'zh-CN' BEGIN
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

CREATE TRIGGER search_content_projections_ad AFTER DELETE ON content_result_projections
WHEN OLD.pipeline IN ('translation', 'polishing') AND OLD.target_lang = 'zh-CN' BEGIN
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
