-- Bound announcement lane fanout and preserve a cursor between short writer transactions.
ALTER TABLE search_projection_outbox ADD COLUMN fanout_cursor TEXT;

-- Realtime projection workers own announcement lane fanout in bounded chunks.
-- The legacy insert trigger scans every matching translation before the worker
-- can apply its cursor, defeating the transaction bound on the first chunk.
DROP TRIGGER IF EXISTS search_documents_ai;

-- Make the scheduler's active star-connection coalescing lookup point-addressable.
CREATE INDEX IF NOT EXISTS idx_job_tasks_star_connection_active
  ON job_tasks(
    task_type,
    status,
    json_extract(payload_json, '$.github_connection_id')
  );

-- A source update starts a fresh projection budget even when an older active row
-- is coalesced in place. The worker's cursor-advance update changes fanout_cursor,
-- so it does not trip this source-event reset.
DROP TRIGGER IF EXISTS search_projection_outbox_reset_source_state;
CREATE TRIGGER search_projection_outbox_reset_source_state
AFTER UPDATE ON search_projection_outbox
WHEN NEW.lease_owner IS NULL
 AND NEW.last_error IS NULL
 AND NEW.fanout_cursor IS OLD.fanout_cursor
 AND (NEW.attempt_count > 0 OR NEW.fanout_cursor IS NOT NULL)
BEGIN
  UPDATE search_projection_outbox
  SET attempt_count = 0,
      fanout_cursor = NULL,
      updated_at = CURRENT_TIMESTAMP
  WHERE projection_key = NEW.projection_key;
END;
