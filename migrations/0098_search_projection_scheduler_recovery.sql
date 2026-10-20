-- Bound announcement lane fanout and preserve a cursor between short writer transactions.
ALTER TABLE search_projection_outbox ADD COLUMN fanout_cursor TEXT;

-- Realtime projection workers own announcement and notification lane fanout in
-- bounded chunks. The legacy insert trigger scans every matching translation
-- before the worker can apply its cursor, defeating the transaction bound on
-- the first chunk. Keep only the release compatibility path because the
-- release source trigger still materializes its document directly.
DROP TRIGGER IF EXISTS search_documents_ai;
CREATE TRIGGER search_documents_ai AFTER INSERT ON search_documents
WHEN NEW.resource_type = 'release' BEGIN
  INSERT INTO search_document_user_lanes (
    document_id, user_id, translated_text, updated_at
  )
  SELECT NEW.id, ranked.user_id, ranked.translated_text, ranked.updated_at
  FROM (
    SELECT
      t.user_id,
      trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, '')) AS translated_text,
      t.updated_at,
      ROW_NUMBER() OVER (
        PARTITION BY t.user_id
        ORDER BY t.updated_at DESC, t.id DESC
      ) AS lane_rank
    FROM ai_translations t
    WHERE t.lang = 'zh-CN'
      AND t.status IN ('ready', 'disabled', 'missing')
      AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
      AND lower(t.entity_type) NOT LIKE '%smart%'
      AND lower(t.entity_type) LIKE 'release%'
      AND t.entity_id = NEW.resource_id
  ) ranked
  WHERE ranked.lane_rank = 1
  ON CONFLICT(document_id, user_id) DO UPDATE SET
    translated_text = excluded.translated_text,
    updated_at = MAX(search_document_user_lanes.updated_at, excluded.updated_at);

  INSERT INTO search_document_user_lanes (
    document_id, user_id, smart_text, updated_at
  )
  SELECT NEW.id, ranked.user_id, ranked.smart_text, ranked.updated_at
  FROM (
    SELECT
      t.user_id,
      trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, '')) AS smart_text,
      t.updated_at,
      ROW_NUMBER() OVER (
        PARTITION BY t.user_id
        ORDER BY t.updated_at DESC, t.id DESC
      ) AS lane_rank
    FROM ai_translations t
    WHERE t.lang = 'zh-CN'
      AND t.status IN ('ready', 'disabled', 'missing')
      AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
      AND lower(t.entity_type) LIKE '%smart%'
      AND lower(t.entity_type) LIKE 'release%'
      AND t.entity_id = NEW.resource_id
  ) ranked
  WHERE ranked.lane_rank = 1
  ON CONFLICT(document_id, user_id) DO UPDATE SET
    smart_text = excluded.smart_text,
    updated_at = MAX(search_document_user_lanes.updated_at, excluded.updated_at);

  DELETE FROM search_document_user_lanes_fts WHERE doc_id = NEW.id;
  INSERT INTO search_document_user_lanes_fts
    (doc_id, user_id, translated_text, smart_text)
  SELECT document_id, user_id, COALESCE(translated_text, ''), COALESCE(smart_text, '')
  FROM search_document_user_lanes
  WHERE document_id = NEW.id;
END;

-- Make the scheduler's active star-connection coalescing lookup point-addressable.
CREATE INDEX IF NOT EXISTS idx_job_tasks_star_connection_active
  ON job_tasks(
    task_type,
    status,
    json_extract(payload_json, '$.github_connection_id')
  )
  WHERE json_valid(payload_json);

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
