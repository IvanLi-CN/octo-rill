-- Preserve the previous canonical announcement key while a source update is
-- waiting in the realtime projection outbox. This keeps cached translations
-- addressable when a discussion identity changes before projection runs.
ALTER TABLE search_projection_outbox
  ADD COLUMN previous_resource_id TEXT;

DROP TRIGGER IF EXISTS search_announcements_outbox_au;

CREATE TRIGGER search_announcements_outbox_au AFTER UPDATE ON social_activity_events
WHEN NEW.kind = 'announcement' OR OLD.kind = 'announcement' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, previous_resource_id,
    user_id, available_at, created_at, updated_at
  ) VALUES (
    'announcement:' || COALESCE(NEW.id, OLD.id),
    CASE WHEN NEW.kind = 'announcement' THEN 'upsert' ELSE 'delete' END,
    'announcement', COALESCE(NEW.id, OLD.id),
    CASE WHEN NEW.kind = 'announcement' AND OLD.kind = 'announcement' THEN
      CASE WHEN OLD.repo_full_name IS NOT NULL AND OLD.discussion_number IS NOT NULL
           THEN lower(OLD.repo_full_name) || '#' || OLD.discussion_number
           ELSE OLD.id
      END
    END,
    COALESCE(NEW.user_id, OLD.user_id),
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    previous_resource_id = CASE
      WHEN excluded.operation = 'upsert'
      THEN COALESCE(search_projection_outbox.previous_resource_id, excluded.previous_resource_id)
      ELSE search_projection_outbox.previous_resource_id
    END,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
