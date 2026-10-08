-- Keep release projection metadata lookups indexable by repository id and make
-- translation lane selection a bounded lookup instead of a correlated scan.
CREATE INDEX IF NOT EXISTS idx_user_repo_associations_repo_id
  ON user_repo_associations(repo_id, user_id);

CREATE INDEX IF NOT EXISTS idx_ai_translations_search_entity
  ON ai_translations(lang, entity_id, status, updated_at DESC, id DESC, user_id);

CREATE INDEX IF NOT EXISTS idx_ai_translations_search_entity_lower
  ON ai_translations(lang, lower(entity_id), status, updated_at DESC, id DESC, user_id);

DROP TRIGGER IF EXISTS search_documents_ai;

CREATE TRIGGER search_documents_ai AFTER INSERT ON search_documents
WHEN NEW.resource_type IN ('release', 'announcement', 'notification') BEGIN
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
      AND (
        (NEW.resource_type = 'release' AND lower(t.entity_type) LIKE 'release%' AND t.entity_id = NEW.resource_id)
        OR (NEW.resource_type = 'announcement' AND lower(t.entity_type) LIKE 'announcement%' AND (lower(t.entity_id) = lower(NEW.resource_id) OR t.entity_id = NEW.resource_id))
        OR (NEW.resource_type = 'notification' AND lower(t.entity_type) LIKE 'notification%' AND t.entity_id = NEW.resource_id)
      )
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
      AND (
        (NEW.resource_type = 'release' AND lower(t.entity_type) LIKE 'release%' AND t.entity_id = NEW.resource_id)
        OR (NEW.resource_type = 'announcement' AND lower(t.entity_type) LIKE 'announcement%' AND (lower(t.entity_id) = lower(NEW.resource_id) OR t.entity_id = NEW.resource_id))
        OR (NEW.resource_type = 'notification' AND lower(t.entity_type) LIKE 'notification%' AND t.entity_id = NEW.resource_id)
      )
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
