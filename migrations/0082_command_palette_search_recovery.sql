-- Local command-palette search projection and per-user anchored fixed-window quota.
-- The projection is deliberately denormalized so reads never need GitHub Search.

CREATE TABLE IF NOT EXISTS search_documents (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  resource_type TEXT NOT NULL CHECK (resource_type IN ('release', 'announcement', 'brief', 'notification', 'repository')),
  resource_id TEXT NOT NULL,
  repo_id INTEGER,
  repo_full_name TEXT,
  owner_login TEXT,
  title TEXT,
  body TEXT,
  translated_text TEXT,
  smart_text TEXT,
  source_time TEXT,
  unread INTEGER,
  target_path TEXT,
  target_url TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (user_id, resource_type, resource_id)
);

CREATE INDEX IF NOT EXISTS idx_search_documents_user_type_time
  ON search_documents(user_id, resource_type, source_time DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_search_documents_repo
  ON search_documents(repo_full_name, owner_login, resource_type);

-- Legacy ai_translations are user-scoped. Keep their lanes in a separate
-- overlay so a user's cached translation never updates another user's result.
CREATE TABLE IF NOT EXISTS search_document_user_lanes (
  document_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  translated_text TEXT,
  smart_text TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (document_id, user_id),
  FOREIGN KEY (document_id) REFERENCES search_documents(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_search_document_user_lanes_user
  ON search_document_user_lanes(user_id, document_id);

CREATE VIRTUAL TABLE IF NOT EXISTS search_document_user_lanes_fts USING fts5(
  doc_id UNINDEXED,
  user_id UNINDEXED,
  translated_text,
  smart_text,
  tokenize = 'trigram'
);

CREATE VIRTUAL TABLE IF NOT EXISTS search_documents_fts USING fts5(
  doc_id UNINDEXED,
  title,
  body,
  repo_full_name,
  translated_text,
  smart_text,
  tokenize = 'trigram'
);

CREATE TABLE IF NOT EXISTS search_rate_limits (
  user_id TEXT PRIMARY KEY,
  window_started_at INTEGER NOT NULL,
  request_count INTEGER NOT NULL CHECK (request_count >= 0),
  updated_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Resumable historical projection state. This migration intentionally performs no backfill.
CREATE TABLE IF NOT EXISTS search_projection_backfill_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  phase TEXT NOT NULL,
  cursor INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK (status IN ('pending', 'building', 'ready', 'paused_low_disk', 'failed')),
  last_error TEXT,
  updated_at TEXT NOT NULL
);
INSERT INTO search_projection_backfill_state (id, phase, cursor, status, updated_at)
VALUES (1, 'releases', 0, 'pending', CURRENT_TIMESTAMP)
ON CONFLICT(id) DO NOTHING;

-- A legacy translation may arrive before its canonical source row. Rebuild
-- both user-scoped lanes whenever a searchable source document is inserted so
-- late source writes and delete/reinsert recovery do not lose cached content.
CREATE TRIGGER IF NOT EXISTS search_documents_ai AFTER INSERT ON search_documents
WHEN NEW.resource_type IN ('release', 'announcement', 'notification') BEGIN
  INSERT INTO search_document_user_lanes (
    document_id, user_id, translated_text, updated_at
  )
  SELECT NEW.id, t.user_id,
    trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, '')),
    t.updated_at
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
    AND NOT EXISTS (
      SELECT 1
      FROM ai_translations newer
      WHERE newer.user_id = t.user_id
        AND newer.lang = t.lang
        AND newer.status IN ('ready', 'disabled', 'missing')
        AND (newer.title IS NOT NULL OR newer.summary IS NOT NULL)
        AND lower(newer.entity_type) NOT LIKE '%smart%'
        AND (
          (NEW.resource_type = 'release' AND lower(newer.entity_type) LIKE 'release%' AND newer.entity_id = NEW.resource_id)
          OR (NEW.resource_type = 'announcement' AND lower(newer.entity_type) LIKE 'announcement%' AND (lower(newer.entity_id) = lower(NEW.resource_id) OR newer.entity_id = NEW.resource_id))
          OR (NEW.resource_type = 'notification' AND lower(newer.entity_type) LIKE 'notification%' AND newer.entity_id = NEW.resource_id)
        )
        AND (newer.updated_at > t.updated_at OR (newer.updated_at = t.updated_at AND newer.id > t.id))
    )
  ON CONFLICT(document_id, user_id) DO UPDATE SET
    translated_text = excluded.translated_text,
    updated_at = MAX(search_document_user_lanes.updated_at, excluded.updated_at);

  INSERT INTO search_document_user_lanes (
    document_id, user_id, smart_text, updated_at
  )
  SELECT NEW.id, t.user_id,
    trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, '')),
    t.updated_at
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
    AND NOT EXISTS (
      SELECT 1
      FROM ai_translations newer
      WHERE newer.user_id = t.user_id
        AND newer.lang = t.lang
        AND newer.status IN ('ready', 'disabled', 'missing')
        AND (newer.title IS NOT NULL OR newer.summary IS NOT NULL)
        AND lower(newer.entity_type) LIKE '%smart%'
        AND (
          (NEW.resource_type = 'release' AND lower(newer.entity_type) LIKE 'release%' AND newer.entity_id = NEW.resource_id)
          OR (NEW.resource_type = 'announcement' AND lower(newer.entity_type) LIKE 'announcement%' AND (lower(newer.entity_id) = lower(NEW.resource_id) OR newer.entity_id = NEW.resource_id))
          OR (NEW.resource_type = 'notification' AND lower(newer.entity_type) LIKE 'notification%' AND newer.entity_id = NEW.resource_id)
        )
        AND (newer.updated_at > t.updated_at OR (newer.updated_at = t.updated_at AND newer.id > t.id))
    )
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

-- Keep the projection current at the source write boundaries. FTS rows are
-- replaced explicitly because the trigram table has no uniqueness constraint.
CREATE TRIGGER IF NOT EXISTS search_repo_releases_ai AFTER INSERT ON repo_releases BEGIN
  INSERT INTO search_documents (id, resource_type, resource_id, repo_id, repo_full_name, owner_login, title, body, source_time, target_path, target_url, created_at, updated_at)
  VALUES ('release:' || NEW.release_id, 'release', CAST(NEW.release_id AS TEXT), NEW.repo_id,
    COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)),
    COALESCE((SELECT CASE WHEN instr(rwi.repo_full_name, '/') > 0 THEN substr(rwi.repo_full_name, 1, instr(rwi.repo_full_name, '/') - 1) ELSE rwi.repo_full_name END FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.owner_login FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.owner_login FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.owner_login FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)),
    COALESCE(NEW.name, NEW.tag_name), NEW.body, COALESCE(NEW.published_at, NEW.created_at, NEW.updated_at),
    CASE WHEN COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)) IS NOT NULL
         THEN '/' || COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)) || '/releases/tag/' || NEW.tag_name END,
    NEW.html_url, NEW.updated_at, NEW.updated_at)
  ON CONFLICT(id) DO UPDATE SET repo_id=excluded.repo_id, repo_full_name=excluded.repo_full_name, owner_login=excluded.owner_login, title=excluded.title, body=excluded.body, source_time=excluded.source_time, target_path=excluded.target_path, target_url=excluded.target_url, updated_at=excluded.updated_at;
  DELETE FROM search_documents_fts WHERE doc_id = 'release:' || NEW.release_id;
  INSERT INTO search_documents_fts SELECT id, COALESCE(title,''), COALESCE(body,''), COALESCE(repo_full_name,''), COALESCE(translated_text,''), COALESCE(smart_text,'') FROM search_documents WHERE id = 'release:' || NEW.release_id;
END;

CREATE TRIGGER IF NOT EXISTS search_repo_releases_au AFTER UPDATE ON repo_releases BEGIN
  UPDATE search_documents SET repo_id=NEW.repo_id, repo_full_name=COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)), owner_login=COALESCE((SELECT CASE WHEN instr(rwi.repo_full_name, '/') > 0 THEN substr(rwi.repo_full_name, 1, instr(rwi.repo_full_name, '/') - 1) ELSE rwi.repo_full_name END FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.owner_login FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.owner_login FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.owner_login FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)), title=COALESCE(NEW.name,NEW.tag_name), body=NEW.body, source_time=COALESCE(NEW.published_at,NEW.created_at,NEW.updated_at), target_path=CASE WHEN COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)) IS NOT NULL THEN '/' || COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)) || '/releases/tag/' || NEW.tag_name END, target_url=NEW.html_url, updated_at=NEW.updated_at WHERE id='release:' || NEW.release_id;
  DELETE FROM search_documents_fts WHERE doc_id = 'release:' || NEW.release_id;
  INSERT INTO search_documents_fts SELECT id, COALESCE(title,''), COALESCE(body,''), COALESCE(repo_full_name,''), COALESCE(translated_text,''), COALESCE(smart_text,'') FROM search_documents WHERE id = 'release:' || NEW.release_id;
END;

CREATE TRIGGER IF NOT EXISTS search_repo_releases_ad AFTER DELETE ON repo_releases BEGIN
  DELETE FROM search_documents_fts WHERE doc_id = 'release:' || OLD.release_id;
  DELETE FROM search_documents WHERE id = 'release:' || OLD.release_id;
END;

CREATE TRIGGER IF NOT EXISTS search_repo_release_work_items_ai AFTER INSERT ON repo_release_work_items BEGIN
  UPDATE search_documents
  SET repo_full_name = NEW.repo_full_name,
      owner_login = CASE WHEN instr(NEW.repo_full_name, '/') > 0 THEN substr(NEW.repo_full_name, 1, instr(NEW.repo_full_name, '/') - 1) ELSE NEW.repo_full_name END,
      target_path = CASE WHEN NEW.repo_full_name IS NOT NULL THEN '/' || NEW.repo_full_name || '/releases/tag/' || (SELECT rr.tag_name FROM repo_releases rr WHERE rr.release_id = CAST(search_documents.resource_id AS INTEGER) LIMIT 1) END
  WHERE resource_type = 'release' AND repo_id = NEW.repo_id;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type = 'release' AND repo_id = NEW.repo_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type = 'release' AND repo_id = NEW.repo_id;
END;

CREATE TRIGGER IF NOT EXISTS search_repo_release_work_items_au AFTER UPDATE OF repo_id, repo_full_name ON repo_release_work_items BEGIN
  UPDATE search_documents
  SET repo_full_name = NEW.repo_full_name,
      owner_login = CASE WHEN instr(NEW.repo_full_name, '/') > 0 THEN substr(NEW.repo_full_name, 1, instr(NEW.repo_full_name, '/') - 1) ELSE NEW.repo_full_name END,
      target_path = CASE WHEN NEW.repo_full_name IS NOT NULL THEN '/' || NEW.repo_full_name || '/releases/tag/' || (SELECT rr.tag_name FROM repo_releases rr WHERE rr.release_id = CAST(search_documents.resource_id AS INTEGER) LIMIT 1) END
  WHERE resource_type = 'release' AND repo_id = NEW.repo_id;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type = 'release' AND repo_id = NEW.repo_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type = 'release' AND repo_id = NEW.repo_id;
END;

CREATE TRIGGER IF NOT EXISTS search_notifications_ai AFTER INSERT ON notifications BEGIN
  INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_full_name,owner_login,title,body,source_time,unread,target_url,created_at,updated_at)
  VALUES ('notification:'||NEW.user_id||':'||NEW.thread_id,NEW.user_id,'notification',NEW.thread_id,NEW.repo_full_name,
    CASE WHEN instr(COALESCE(NEW.repo_full_name,''),'/')>0 THEN substr(NEW.repo_full_name,1,instr(NEW.repo_full_name,'/')-1) ELSE NEW.repo_full_name END,
    NEW.subject_title,COALESCE(NEW.subject_type,'')||' '||COALESCE(NEW.reason,''),NEW.updated_at,NEW.unread,NEW.html_url,COALESCE(NEW.updated_at,CURRENT_TIMESTAMP),COALESCE(NEW.updated_at,CURRENT_TIMESTAMP))
  ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,unread=excluded.unread,target_url=excluded.target_url,updated_at=excluded.updated_at;
  INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at)
  SELECT 'notification:'||NEW.user_id||':'||NEW.thread_id, NEW.user_id,
    (SELECT trim(COALESCE(t.title,'')||' '||COALESCE(t.summary,'')) FROM ai_translations t
     WHERE t.user_id=NEW.user_id AND t.entity_id=NEW.thread_id
       AND t.lang='zh-CN'
       AND lower(t.entity_type) NOT LIKE '%smart%'
       AND t.status IN ('ready','disabled','missing')
     ORDER BY t.updated_at DESC LIMIT 1),
    (SELECT trim(COALESCE(t.title,'')||' '||COALESCE(t.summary,'')) FROM ai_translations t
     WHERE t.user_id=NEW.user_id AND t.entity_id=NEW.thread_id
       AND t.lang='zh-CN'
       AND lower(t.entity_type) LIKE '%smart%'
       AND t.status IN ('ready','disabled','missing')
     ORDER BY t.updated_at DESC LIMIT 1),
    COALESCE((SELECT MAX(t.updated_at) FROM ai_translations t
              WHERE t.user_id=NEW.user_id AND t.entity_id=NEW.thread_id
                AND t.lang='zh-CN'
                AND t.status IN ('ready','disabled','missing')), NEW.updated_at)
  WHERE EXISTS (SELECT 1 FROM ai_translations t
                WHERE t.user_id=NEW.user_id AND t.entity_id=NEW.thread_id
                  AND t.lang='zh-CN'
                  AND t.status IN ('ready','disabled','missing'))
  ON CONFLICT(document_id,user_id) DO UPDATE SET
    translated_text=COALESCE(excluded.translated_text,search_document_user_lanes.translated_text),
    smart_text=COALESCE(excluded.smart_text,search_document_user_lanes.smart_text),
    updated_at=MAX(search_document_user_lanes.updated_at,excluded.updated_at);
  DELETE FROM search_document_user_lanes_fts WHERE doc_id='notification:'||NEW.user_id||':'||NEW.thread_id AND user_id=NEW.user_id;
  INSERT INTO search_document_user_lanes_fts SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE document_id='notification:'||NEW.user_id||':'||NEW.thread_id AND user_id=NEW.user_id;
  DELETE FROM search_documents_fts WHERE doc_id='notification:'||NEW.user_id||':'||NEW.thread_id;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='notification:'||NEW.user_id||':'||NEW.thread_id;
END;

CREATE TRIGGER IF NOT EXISTS search_announcements_ai AFTER INSERT ON social_activity_events
WHEN NEW.kind = 'announcement' BEGIN
  INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at)
  VALUES ('announcement:'||NEW.id,NEW.user_id,'announcement',CASE WHEN NEW.repo_full_name IS NOT NULL AND NEW.discussion_number IS NOT NULL THEN lower(NEW.repo_full_name)||'#'||NEW.discussion_number ELSE NEW.id END,NEW.repo_id,NEW.repo_full_name,
    CASE WHEN instr(COALESCE(NEW.repo_full_name,''),'/')>0 THEN substr(NEW.repo_full_name,1,instr(NEW.repo_full_name,'/')-1) ELSE NEW.repo_full_name END,
    NEW.title,NEW.body,NEW.occurred_at,CASE WHEN NEW.repo_full_name IS NOT NULL AND NEW.discussion_number IS NOT NULL THEN '/'||NEW.repo_full_name||'/discussions/'||NEW.discussion_number END,NEW.html_url,NEW.created_at,NEW.updated_at)
  ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_url=excluded.target_url,updated_at=excluded.updated_at;
  DELETE FROM search_documents_fts WHERE doc_id='announcement:'||NEW.id;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='announcement:'||NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS search_announcements_au AFTER UPDATE ON social_activity_events
WHEN NEW.kind = 'announcement' BEGIN
  UPDATE search_documents SET resource_id=CASE WHEN NEW.repo_full_name IS NOT NULL AND NEW.discussion_number IS NOT NULL THEN lower(NEW.repo_full_name)||'#'||NEW.discussion_number ELSE NEW.id END,repo_id=NEW.repo_id,repo_full_name=NEW.repo_full_name,owner_login=CASE WHEN instr(COALESCE(NEW.repo_full_name,''),'/')>0 THEN substr(NEW.repo_full_name,1,instr(NEW.repo_full_name,'/')-1) ELSE NEW.repo_full_name END,title=NEW.title,body=NEW.body,source_time=NEW.occurred_at,target_path=CASE WHEN NEW.repo_full_name IS NOT NULL AND NEW.discussion_number IS NOT NULL THEN '/'||NEW.repo_full_name||'/discussions/'||NEW.discussion_number END,target_url=NEW.html_url,updated_at=NEW.updated_at WHERE id='announcement:'||NEW.id;
  DELETE FROM search_document_user_lanes_fts WHERE doc_id='announcement:'||NEW.id;
  DELETE FROM search_document_user_lanes WHERE document_id='announcement:'||NEW.id;
  INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at)
  SELECT 'announcement:'||NEW.id,NEW.user_id,
    (SELECT trim(COALESCE(t.title,'')||' '||COALESCE(t.summary,'')) FROM ai_translations t
     WHERE t.user_id=NEW.user_id AND t.lang='zh-CN' AND t.status IN ('ready','disabled','missing')
       AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
       AND lower(t.entity_type) LIKE 'announcement%' AND lower(t.entity_type) NOT LIKE '%smart%'
       AND lower(t.entity_id) IN (lower(NEW.id), lower(CASE WHEN NEW.repo_full_name IS NOT NULL AND NEW.discussion_number IS NOT NULL THEN NEW.repo_full_name||'#'||NEW.discussion_number ELSE NEW.id END), lower(CASE WHEN OLD.repo_full_name IS NOT NULL AND OLD.discussion_number IS NOT NULL THEN OLD.repo_full_name||'#'||OLD.discussion_number ELSE OLD.id END))
     ORDER BY t.updated_at DESC,t.id DESC LIMIT 1),
    (SELECT trim(COALESCE(t.title,'')||' '||COALESCE(t.summary,'')) FROM ai_translations t
     WHERE t.user_id=NEW.user_id AND t.lang='zh-CN' AND t.status IN ('ready','disabled','missing')
       AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
       AND lower(t.entity_type) LIKE 'announcement%' AND lower(t.entity_type) LIKE '%smart%'
       AND lower(t.entity_id) IN (lower(NEW.id), lower(CASE WHEN NEW.repo_full_name IS NOT NULL AND NEW.discussion_number IS NOT NULL THEN NEW.repo_full_name||'#'||NEW.discussion_number ELSE NEW.id END), lower(CASE WHEN OLD.repo_full_name IS NOT NULL AND OLD.discussion_number IS NOT NULL THEN OLD.repo_full_name||'#'||OLD.discussion_number ELSE OLD.id END))
     ORDER BY t.updated_at DESC,t.id DESC LIMIT 1),
    COALESCE((SELECT MAX(t.updated_at) FROM ai_translations t
              WHERE t.user_id=NEW.user_id AND t.lang='zh-CN' AND t.status IN ('ready','disabled','missing')
                AND lower(t.entity_type) LIKE 'announcement%'
                AND lower(t.entity_id) IN (lower(NEW.id), lower(CASE WHEN NEW.repo_full_name IS NOT NULL AND NEW.discussion_number IS NOT NULL THEN NEW.repo_full_name||'#'||NEW.discussion_number ELSE NEW.id END), lower(CASE WHEN OLD.repo_full_name IS NOT NULL AND OLD.discussion_number IS NOT NULL THEN OLD.repo_full_name||'#'||OLD.discussion_number ELSE OLD.id END))), NEW.updated_at)
  WHERE EXISTS (SELECT 1 FROM ai_translations t
                WHERE t.user_id=NEW.user_id AND t.lang='zh-CN' AND t.status IN ('ready','disabled','missing')
                  AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
                  AND lower(t.entity_type) LIKE 'announcement%'
                  AND lower(t.entity_id) IN (lower(NEW.id), lower(CASE WHEN NEW.repo_full_name IS NOT NULL AND NEW.discussion_number IS NOT NULL THEN NEW.repo_full_name||'#'||NEW.discussion_number ELSE NEW.id END), lower(CASE WHEN OLD.repo_full_name IS NOT NULL AND OLD.discussion_number IS NOT NULL THEN OLD.repo_full_name||'#'||OLD.discussion_number ELSE OLD.id END)));
  DELETE FROM search_documents_fts WHERE doc_id='announcement:'||NEW.id;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='announcement:'||NEW.id;
  INSERT INTO search_document_user_lanes_fts SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE document_id='announcement:'||NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS search_announcements_ad AFTER DELETE ON social_activity_events
WHEN OLD.kind = 'announcement' BEGIN
  DELETE FROM search_documents_fts WHERE doc_id='announcement:'||OLD.id;
  DELETE FROM search_documents WHERE id='announcement:'||OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS search_notifications_au AFTER UPDATE ON notifications BEGIN
  UPDATE search_documents SET resource_id=NEW.thread_id,repo_full_name=NEW.repo_full_name,owner_login=CASE WHEN instr(COALESCE(NEW.repo_full_name,''),'/')>0 THEN substr(NEW.repo_full_name,1,instr(NEW.repo_full_name,'/')-1) ELSE NEW.repo_full_name END,title=NEW.subject_title,body=COALESCE(NEW.subject_type,'')||' '||COALESCE(NEW.reason,''),source_time=NEW.updated_at,unread=NEW.unread,target_url=NEW.html_url,updated_at=COALESCE(NEW.updated_at,CURRENT_TIMESTAMP) WHERE id='notification:'||OLD.user_id||':'||OLD.thread_id;
  DELETE FROM search_documents_fts WHERE doc_id='notification:'||OLD.user_id||':'||OLD.thread_id;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='notification:'||NEW.user_id||':'||NEW.thread_id;
END;

CREATE TRIGGER IF NOT EXISTS search_notifications_ad AFTER DELETE ON notifications BEGIN
  DELETE FROM search_document_user_lanes_fts WHERE doc_id='notification:'||OLD.user_id||':'||OLD.thread_id AND user_id=OLD.user_id;
  DELETE FROM search_document_user_lanes WHERE document_id='notification:'||OLD.user_id||':'||OLD.thread_id AND user_id=OLD.user_id;
  DELETE FROM search_documents_fts WHERE doc_id='notification:'||OLD.user_id||':'||OLD.thread_id;
  DELETE FROM search_documents WHERE id='notification:'||OLD.user_id||':'||OLD.thread_id;
END;

CREATE TRIGGER IF NOT EXISTS search_briefs_ai AFTER INSERT ON briefs BEGIN
  INSERT INTO search_documents (id,user_id,resource_type,resource_id,title,body,source_time,target_path,created_at,updated_at)
  VALUES ('brief:'||NEW.id,NEW.user_id,'brief',NEW.id,'日报 '||NEW.date,NEW.content_markdown,COALESCE(NEW.window_end_utc,NEW.created_at),'/briefs?brief='||NEW.id,NEW.created_at,NEW.updated_at)
  ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,updated_at=excluded.updated_at;
  DELETE FROM search_documents_fts WHERE doc_id='brief:'||NEW.id;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='brief:'||NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS search_briefs_au AFTER UPDATE ON briefs BEGIN
  UPDATE search_documents SET title='日报 '||NEW.date,body=NEW.content_markdown,source_time=COALESCE(NEW.window_end_utc,NEW.updated_at),target_path='/briefs?brief='||NEW.id,updated_at=NEW.updated_at WHERE id='brief:'||NEW.id;
  DELETE FROM search_documents_fts WHERE doc_id='brief:'||NEW.id;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='brief:'||NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS search_briefs_ad AFTER DELETE ON briefs BEGIN
  DELETE FROM search_documents_fts WHERE doc_id='brief:'||OLD.id;
  DELETE FROM search_documents WHERE id='brief:'||OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS search_repo_associations_ai AFTER INSERT ON user_repo_associations BEGIN
  DELETE FROM search_documents_fts
  WHERE doc_id IN (
    SELECT id FROM search_documents
    WHERE user_id=NEW.user_id
      AND resource_type='repository'
      AND repo_id=NEW.repo_id
      AND id <> 'repository:'||NEW.user_id||':'||NEW.repo_full_name_lower
  );
  DELETE FROM search_documents
  WHERE user_id=NEW.user_id
    AND resource_type='repository'
    AND repo_id=NEW.repo_id
    AND id <> 'repository:'||NEW.user_id||':'||NEW.repo_full_name_lower;
  INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at)
  VALUES ('repository:'||NEW.user_id||':'||NEW.repo_full_name_lower,NEW.user_id,'repository',NEW.repo_full_name_lower,NEW.repo_id,NEW.repo_full_name,NEW.owner_login,NEW.repo_name,NEW.description,NEW.updated_at,'/focus/repo/'||NEW.owner_login||'/'||NEW.repo_name,NEW.html_url,NEW.created_at,NEW.updated_at)
  ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at;
  UPDATE search_documents
  SET repo_full_name=NEW.repo_full_name,
      owner_login=NEW.owner_login,
      target_path='/'||NEW.repo_full_name||'/releases/tag/'||(SELECT rr.tag_name FROM repo_releases rr WHERE rr.release_id=CAST(search_documents.resource_id AS INTEGER) LIMIT 1)
  WHERE resource_type='release' AND repo_id=NEW.repo_id;
  DELETE FROM search_documents_fts WHERE doc_id='repository:'||NEW.user_id||':'||NEW.repo_full_name_lower;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='repository:'||NEW.user_id||':'||NEW.repo_full_name_lower;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type='release' AND repo_id=NEW.repo_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type='release' AND repo_id=NEW.repo_id;
END;

CREATE TRIGGER IF NOT EXISTS search_repo_associations_au AFTER UPDATE ON user_repo_associations BEGIN
  DELETE FROM search_documents_fts
  WHERE doc_id IN (
    SELECT id FROM search_documents
    WHERE user_id=NEW.user_id
      AND resource_type='repository'
      AND repo_id=NEW.repo_id
      AND id <> 'repository:'||NEW.user_id||':'||NEW.repo_full_name_lower
  );
  DELETE FROM search_documents
  WHERE user_id=NEW.user_id
    AND resource_type='repository'
    AND repo_id=NEW.repo_id
    AND id <> 'repository:'||NEW.user_id||':'||NEW.repo_full_name_lower;
  DELETE FROM search_documents_fts
  WHERE doc_id='repository:'||OLD.user_id||':'||OLD.repo_full_name_lower
    AND lower(OLD.repo_full_name_lower) <> lower(NEW.repo_full_name_lower)
    AND NOT EXISTS (SELECT 1 FROM starred_repos sr
                    WHERE sr.user_id=OLD.user_id
                      AND lower(sr.full_name)=lower(OLD.repo_full_name_lower));
  DELETE FROM search_documents
  WHERE id='repository:'||OLD.user_id||':'||OLD.repo_full_name_lower
    AND lower(OLD.repo_full_name_lower) <> lower(NEW.repo_full_name_lower)
    AND NOT EXISTS (SELECT 1 FROM starred_repos sr
                    WHERE sr.user_id=OLD.user_id
                      AND lower(sr.full_name)=lower(OLD.repo_full_name_lower));
  INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at)
  VALUES ('repository:'||NEW.user_id||':'||NEW.repo_full_name_lower,NEW.user_id,'repository',NEW.repo_full_name_lower,NEW.repo_id,NEW.repo_full_name,NEW.owner_login,NEW.repo_name,NEW.description,NEW.updated_at,'/focus/repo/'||NEW.owner_login||'/'||NEW.repo_name,NEW.html_url,NEW.created_at,NEW.updated_at)
  ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at;
  UPDATE search_documents SET repo_id=NEW.repo_id,repo_full_name=NEW.repo_full_name,owner_login=NEW.owner_login,title=NEW.repo_name,body=NEW.description,source_time=NEW.updated_at,target_path='/focus/repo/'||NEW.owner_login||'/'||NEW.repo_name,target_url=NEW.html_url,updated_at=NEW.updated_at WHERE id='repository:'||NEW.user_id||':'||NEW.repo_full_name_lower;
  UPDATE search_documents
  SET repo_full_name=NEW.repo_full_name,
      owner_login=NEW.owner_login,
      target_path='/'||NEW.repo_full_name||'/releases/tag/'||(SELECT rr.tag_name FROM repo_releases rr WHERE rr.release_id=CAST(search_documents.resource_id AS INTEGER) LIMIT 1)
  WHERE resource_type='release' AND repo_id=NEW.repo_id;
  DELETE FROM search_documents_fts WHERE doc_id='repository:'||NEW.user_id||':'||NEW.repo_full_name_lower;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='repository:'||NEW.user_id||':'||NEW.repo_full_name_lower;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type='release' AND repo_id=NEW.repo_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type='release' AND repo_id=NEW.repo_id;
END;

CREATE TRIGGER IF NOT EXISTS search_repo_associations_ad AFTER DELETE ON user_repo_associations BEGIN
  DELETE FROM search_documents_fts WHERE doc_id='repository:'||OLD.user_id||':'||OLD.repo_full_name_lower
    AND NOT EXISTS (SELECT 1 FROM starred_repos sr WHERE sr.user_id=OLD.user_id AND lower(sr.full_name)=OLD.repo_full_name_lower);
  DELETE FROM search_documents WHERE id='repository:'||OLD.user_id||':'||OLD.repo_full_name_lower
    AND NOT EXISTS (SELECT 1 FROM starred_repos sr WHERE sr.user_id=OLD.user_id AND lower(sr.full_name)=OLD.repo_full_name_lower);
END;

-- A release can be cached before an owned-repository baseline is discovered.
-- Keep its repository metadata and canonical target aligned with the same
-- visibility sources used by the search permission predicate.
CREATE VIEW IF NOT EXISTS search_release_metadata AS
SELECT d.id,
       d.repo_id,
       COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=d.repo_id LIMIT 1),
                (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=d.repo_id LIMIT 1),
                (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=d.repo_id LIMIT 1),
                (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=d.repo_id LIMIT 1)) AS repo_full_name,
       COALESCE((SELECT CASE WHEN instr(rwi.repo_full_name,'/')>0 THEN substr(rwi.repo_full_name,1,instr(rwi.repo_full_name,'/')-1) ELSE rwi.repo_full_name END FROM repo_release_work_items rwi WHERE rwi.repo_id=d.repo_id LIMIT 1),
                (SELECT vr.owner_login FROM user_release_visible_repos vr WHERE vr.repo_id=d.repo_id LIMIT 1),
                (SELECT ura.owner_login FROM user_repo_associations ura WHERE ura.repo_id=d.repo_id LIMIT 1),
                (SELECT sr.owner_login FROM starred_repos sr WHERE sr.repo_id=d.repo_id LIMIT 1)) AS owner_login,
       CASE WHEN COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=d.repo_id LIMIT 1),
                          (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=d.repo_id LIMIT 1),
                          (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=d.repo_id LIMIT 1),
                          (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=d.repo_id LIMIT 1)) IS NOT NULL
            THEN '/' || COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=d.repo_id LIMIT 1),
                                 (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=d.repo_id LIMIT 1),
                                 (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=d.repo_id LIMIT 1),
                                 (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=d.repo_id LIMIT 1)) || '/releases/tag/' || rr.tag_name
       END AS target_path
FROM search_documents d
LEFT JOIN repo_releases rr ON rr.release_id=CAST(d.resource_id AS INTEGER)
WHERE d.resource_type='release';

CREATE TRIGGER IF NOT EXISTS search_owned_repo_star_baselines_ai AFTER INSERT ON owned_repo_star_baselines BEGIN
  UPDATE search_documents
  SET repo_full_name=(SELECT m.repo_full_name FROM search_release_metadata m WHERE m.id=search_documents.id),
      owner_login=(SELECT m.owner_login FROM search_release_metadata m WHERE m.id=search_documents.id),
      target_path=(SELECT m.target_path FROM search_release_metadata m WHERE m.id=search_documents.id)
  WHERE resource_type='release' AND repo_id=NEW.repo_id;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type='release' AND repo_id=NEW.repo_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type='release' AND repo_id=NEW.repo_id;
END;

CREATE TRIGGER IF NOT EXISTS search_owned_repo_star_baselines_au AFTER UPDATE ON owned_repo_star_baselines BEGIN
  UPDATE search_documents
  SET repo_full_name=(SELECT m.repo_full_name FROM search_release_metadata m WHERE m.id=search_documents.id),
      owner_login=(SELECT m.owner_login FROM search_release_metadata m WHERE m.id=search_documents.id),
      target_path=(SELECT m.target_path FROM search_release_metadata m WHERE m.id=search_documents.id)
  WHERE resource_type='release' AND repo_id IN (OLD.repo_id,NEW.repo_id);
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type='release' AND repo_id IN (OLD.repo_id,NEW.repo_id));
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type='release' AND repo_id IN (OLD.repo_id,NEW.repo_id);
END;

CREATE TRIGGER IF NOT EXISTS search_owned_repo_star_baselines_ad AFTER DELETE ON owned_repo_star_baselines BEGIN
  UPDATE search_documents
  SET repo_full_name=(SELECT m.repo_full_name FROM search_release_metadata m WHERE m.id=search_documents.id),
      owner_login=(SELECT m.owner_login FROM search_release_metadata m WHERE m.id=search_documents.id),
      target_path=(SELECT m.target_path FROM search_release_metadata m WHERE m.id=search_documents.id)
  WHERE resource_type='release' AND repo_id=OLD.repo_id;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type='release' AND repo_id=OLD.repo_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type='release' AND repo_id=OLD.repo_id;
END;

CREATE TRIGGER IF NOT EXISTS search_users_include_own_releases_au AFTER UPDATE OF include_own_releases ON users BEGIN
  UPDATE search_documents
  SET repo_full_name=(SELECT m.repo_full_name FROM search_release_metadata m WHERE m.id=search_documents.id),
      owner_login=(SELECT m.owner_login FROM search_release_metadata m WHERE m.id=search_documents.id),
      target_path=(SELECT m.target_path FROM search_release_metadata m WHERE m.id=search_documents.id)
  WHERE resource_type='release' AND repo_id IN (
    SELECT repo_id FROM user_repo_associations WHERE user_id=NEW.id AND repo_id IS NOT NULL
    UNION SELECT repo_id FROM starred_repos WHERE user_id=NEW.id
    UNION SELECT repo_id FROM owned_repo_star_baselines WHERE user_id=NEW.id
  );
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type='release' AND repo_id IN (SELECT repo_id FROM user_repo_associations WHERE user_id=NEW.id AND repo_id IS NOT NULL UNION SELECT repo_id FROM starred_repos WHERE user_id=NEW.id UNION SELECT repo_id FROM owned_repo_star_baselines WHERE user_id=NEW.id));
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type='release' AND repo_id IN (SELECT repo_id FROM user_repo_associations WHERE user_id=NEW.id AND repo_id IS NOT NULL UNION SELECT repo_id FROM starred_repos WHERE user_id=NEW.id UNION SELECT repo_id FROM owned_repo_star_baselines WHERE user_id=NEW.id);
END;

CREATE TRIGGER IF NOT EXISTS search_starred_repos_ai AFTER INSERT ON starred_repos BEGIN
  DELETE FROM search_documents_fts
  WHERE doc_id IN (
    SELECT id FROM search_documents
    WHERE user_id=NEW.user_id
      AND resource_type='repository'
      AND repo_id=NEW.repo_id
      AND id <> 'repository:'||NEW.user_id||':'||lower(NEW.full_name)
  );
  DELETE FROM search_documents
  WHERE user_id=NEW.user_id
    AND resource_type='repository'
    AND repo_id=NEW.repo_id
    AND id <> 'repository:'||NEW.user_id||':'||lower(NEW.full_name);
  INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at)
  SELECT 'repository:'||NEW.user_id||':'||lower(NEW.full_name),NEW.user_id,'repository',lower(NEW.full_name),NEW.repo_id,NEW.full_name,NEW.owner_login,NEW.name,NEW.description,NEW.updated_at,'/focus/repo/'||NEW.owner_login||'/'||NEW.name,NEW.html_url,NEW.updated_at,NEW.updated_at
  WHERE NOT EXISTS (SELECT 1 FROM user_repo_associations ura WHERE ura.user_id=NEW.user_id AND ura.repo_full_name_lower=lower(NEW.full_name))
  ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at;
  DELETE FROM search_documents_fts WHERE doc_id='repository:'||NEW.user_id||':'||lower(NEW.full_name);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='repository:'||NEW.user_id||':'||lower(NEW.full_name);
END;

CREATE TRIGGER IF NOT EXISTS search_starred_repos_au AFTER UPDATE ON starred_repos BEGIN
  DELETE FROM search_documents_fts
  WHERE doc_id IN (
    SELECT id FROM search_documents
    WHERE user_id=NEW.user_id
      AND resource_type='repository'
      AND repo_id=NEW.repo_id
      AND id <> 'repository:'||NEW.user_id||':'||lower(NEW.full_name)
  );
  DELETE FROM search_documents
  WHERE user_id=NEW.user_id
    AND resource_type='repository'
    AND repo_id=NEW.repo_id
    AND id <> 'repository:'||NEW.user_id||':'||lower(NEW.full_name);
  DELETE FROM search_documents_fts
  WHERE doc_id='repository:'||OLD.user_id||':'||lower(OLD.full_name)
    AND lower(OLD.full_name) <> lower(NEW.full_name)
    AND NOT EXISTS (SELECT 1 FROM user_repo_associations ura
                    WHERE ura.user_id=OLD.user_id
                      AND ura.repo_full_name_lower=lower(OLD.full_name));
  DELETE FROM search_documents
  WHERE id='repository:'||OLD.user_id||':'||lower(OLD.full_name)
    AND lower(OLD.full_name) <> lower(NEW.full_name)
    AND NOT EXISTS (SELECT 1 FROM user_repo_associations ura
                    WHERE ura.user_id=OLD.user_id
                      AND ura.repo_full_name_lower=lower(OLD.full_name));
  INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at)
  SELECT 'repository:'||NEW.user_id||':'||lower(NEW.full_name),NEW.user_id,'repository',lower(NEW.full_name),NEW.repo_id,NEW.full_name,NEW.owner_login,NEW.name,NEW.description,NEW.updated_at,'/focus/repo/'||NEW.owner_login||'/'||NEW.name,NEW.html_url,NEW.updated_at,NEW.updated_at
  WHERE NOT EXISTS (SELECT 1 FROM user_repo_associations ura WHERE ura.user_id=NEW.user_id AND ura.repo_full_name_lower=lower(NEW.full_name))
  ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at;
  DELETE FROM search_documents_fts WHERE doc_id='repository:'||NEW.user_id||':'||lower(NEW.full_name);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='repository:'||NEW.user_id||':'||lower(NEW.full_name);
END;

CREATE TRIGGER IF NOT EXISTS search_starred_repos_ad AFTER DELETE ON starred_repos BEGIN
  DELETE FROM search_documents_fts WHERE doc_id='repository:'||OLD.user_id||':'||lower(OLD.full_name)
    AND NOT EXISTS (SELECT 1 FROM user_repo_associations ura WHERE ura.user_id=OLD.user_id AND ura.repo_full_name_lower=lower(OLD.full_name));
  DELETE FROM search_documents WHERE id='repository:'||OLD.user_id||':'||lower(OLD.full_name)
    AND NOT EXISTS (SELECT 1 FROM user_repo_associations ura WHERE ura.user_id=OLD.user_id AND ura.repo_full_name_lower=lower(OLD.full_name));
END;

CREATE TRIGGER IF NOT EXISTS search_ai_translations_ai AFTER INSERT ON ai_translations BEGIN
  INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at)
  SELECT d.id,NEW.user_id,
    CASE WHEN lower(NEW.entity_type) LIKE '%smart%' THEN NULL ELSE trim(COALESCE(NEW.title,'') || ' ' || COALESCE(NEW.summary,'')) END,
    CASE WHEN lower(NEW.entity_type) LIKE '%smart%' THEN trim(COALESCE(NEW.title,'') || ' ' || COALESCE(NEW.summary,'')) ELSE NULL END,
    NEW.updated_at
  FROM search_documents d
  WHERE ((d.resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND d.resource_id=NEW.entity_id) OR (d.resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND d.resource_id=lower(NEW.entity_id)) OR (d.resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND d.resource_id=NEW.entity_id))
    AND NEW.lang='zh-CN'
    AND NEW.status IN ('ready','disabled','missing')
    AND (NEW.title IS NOT NULL OR NEW.summary IS NOT NULL)
    AND NOT EXISTS (
      SELECT 1 FROM ai_translations newer
      WHERE newer.user_id=NEW.user_id AND newer.entity_id=NEW.entity_id
        AND newer.lang=NEW.lang
        AND newer.status IN ('ready','disabled','missing')
        AND (newer.title IS NOT NULL OR newer.summary IS NOT NULL)
        AND ((lower(NEW.entity_type) LIKE '%smart%' AND lower(newer.entity_type) LIKE '%smart%')
          OR (lower(NEW.entity_type) NOT LIKE '%smart%' AND lower(newer.entity_type) NOT LIKE '%smart%'))
        AND ((lower(NEW.entity_type) LIKE 'release%' AND lower(newer.entity_type) LIKE 'release%')
          OR (lower(NEW.entity_type) LIKE 'announcement%' AND lower(newer.entity_type) LIKE 'announcement%')
          OR (lower(NEW.entity_type) LIKE 'notification%' AND lower(newer.entity_type) LIKE 'notification%'))
        AND (newer.updated_at > NEW.updated_at OR (newer.updated_at = NEW.updated_at AND newer.id > NEW.id))
    )
  ON CONFLICT(document_id,user_id) DO UPDATE SET
    translated_text=CASE WHEN excluded.translated_text IS NOT NULL THEN excluded.translated_text ELSE search_document_user_lanes.translated_text END,
    smart_text=CASE WHEN excluded.smart_text IS NOT NULL THEN excluded.smart_text ELSE search_document_user_lanes.smart_text END,
    updated_at=MAX(search_document_user_lanes.updated_at,excluded.updated_at);
  DELETE FROM search_document_user_lanes_fts WHERE user_id=NEW.user_id AND doc_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND resource_id=NEW.entity_id) OR (resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND resource_id=lower(NEW.entity_id)) OR (resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND resource_id=NEW.entity_id));
  INSERT INTO search_document_user_lanes_fts SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE user_id=NEW.user_id AND document_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND resource_id=NEW.entity_id) OR (resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND resource_id=lower(NEW.entity_id)) OR (resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND resource_id=NEW.entity_id));
END;

CREATE TRIGGER IF NOT EXISTS search_ai_translations_au AFTER UPDATE ON ai_translations BEGIN
  INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at)
  SELECT d.id,NEW.user_id,
    CASE WHEN lower(NEW.entity_type) LIKE '%smart%' THEN NULL ELSE trim(COALESCE(NEW.title,'') || ' ' || COALESCE(NEW.summary,'')) END,
    CASE WHEN lower(NEW.entity_type) LIKE '%smart%' THEN trim(COALESCE(NEW.title,'') || ' ' || COALESCE(NEW.summary,'')) ELSE NULL END,
    NEW.updated_at
  FROM search_documents d
  WHERE ((d.resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND d.resource_id=NEW.entity_id) OR (d.resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND d.resource_id=lower(NEW.entity_id)) OR (d.resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND d.resource_id=NEW.entity_id))
    AND NEW.lang='zh-CN'
    AND NEW.status IN ('ready','disabled','missing')
    AND (NEW.title IS NOT NULL OR NEW.summary IS NOT NULL)
    AND NOT EXISTS (
      SELECT 1 FROM ai_translations newer
      WHERE newer.user_id=NEW.user_id AND newer.entity_id=NEW.entity_id
        AND newer.lang=NEW.lang
        AND newer.status IN ('ready','disabled','missing')
        AND (newer.title IS NOT NULL OR newer.summary IS NOT NULL)
        AND ((lower(NEW.entity_type) LIKE '%smart%' AND lower(newer.entity_type) LIKE '%smart%')
          OR (lower(NEW.entity_type) NOT LIKE '%smart%' AND lower(newer.entity_type) NOT LIKE '%smart%'))
        AND ((lower(NEW.entity_type) LIKE 'release%' AND lower(newer.entity_type) LIKE 'release%')
          OR (lower(NEW.entity_type) LIKE 'announcement%' AND lower(newer.entity_type) LIKE 'announcement%')
          OR (lower(NEW.entity_type) LIKE 'notification%' AND lower(newer.entity_type) LIKE 'notification%'))
        AND (newer.updated_at > NEW.updated_at OR (newer.updated_at = NEW.updated_at AND newer.id > NEW.id))
    )
  ON CONFLICT(document_id,user_id) DO UPDATE SET
    translated_text=CASE WHEN excluded.translated_text IS NOT NULL THEN excluded.translated_text ELSE search_document_user_lanes.translated_text END,
    smart_text=CASE WHEN excluded.smart_text IS NOT NULL THEN excluded.smart_text ELSE search_document_user_lanes.smart_text END,
    updated_at=MAX(search_document_user_lanes.updated_at,excluded.updated_at);
  DELETE FROM search_document_user_lanes_fts WHERE user_id=NEW.user_id AND doc_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND resource_id=NEW.entity_id) OR (resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND resource_id=lower(NEW.entity_id)) OR (resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND resource_id=NEW.entity_id));
  INSERT INTO search_document_user_lanes_fts SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE user_id=NEW.user_id AND document_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND resource_id=NEW.entity_id) OR (resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND resource_id=lower(NEW.entity_id)) OR (resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND resource_id=NEW.entity_id));
END;

CREATE TRIGGER IF NOT EXISTS search_ai_translations_ad AFTER DELETE ON ai_translations BEGIN
  DELETE FROM search_document_user_lanes_fts WHERE user_id=OLD.user_id AND doc_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND resource_id=OLD.entity_id) OR (resource_type='announcement' AND resource_id=lower(OLD.entity_id)) OR (resource_type='notification' AND resource_id=OLD.entity_id));
  DELETE FROM search_document_user_lanes WHERE user_id=OLD.user_id AND document_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND resource_id=OLD.entity_id) OR (resource_type='announcement' AND resource_id=lower(OLD.entity_id)) OR (resource_type='notification' AND resource_id=OLD.entity_id));
  INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at)
  SELECT d.id,OLD.user_id,
    (SELECT trim(COALESCE(t.title,'')||' '||COALESCE(t.summary,'')) FROM ai_translations t
     WHERE t.user_id=OLD.user_id AND t.entity_id=OLD.entity_id
       AND t.lang='zh-CN'
       AND t.status IN ('ready','disabled','missing') AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
       AND lower(t.entity_type) NOT LIKE '%smart%'
       AND ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%')
         OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%')
         OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%'))
     ORDER BY t.updated_at DESC,t.id DESC LIMIT 1),
    (SELECT trim(COALESCE(t.title,'')||' '||COALESCE(t.summary,'')) FROM ai_translations t
     WHERE t.user_id=OLD.user_id AND t.entity_id=OLD.entity_id
       AND t.lang='zh-CN'
       AND t.status IN ('ready','disabled','missing') AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
       AND lower(t.entity_type) LIKE '%smart%'
       AND ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%')
         OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%')
         OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%'))
     ORDER BY t.updated_at DESC,t.id DESC LIMIT 1),
    COALESCE((SELECT MAX(t.updated_at) FROM ai_translations t
              WHERE t.user_id=OLD.user_id AND t.entity_id=OLD.entity_id
                AND t.lang='zh-CN'
                AND t.status IN ('ready','disabled','missing')
                AND ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%')
                  OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%')
                  OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%'))), CURRENT_TIMESTAMP)
  FROM search_documents d
  WHERE ((d.resource_type='release' AND lower(OLD.entity_type) LIKE 'release%' AND d.resource_id=OLD.entity_id)
      OR (d.resource_type='announcement' AND lower(OLD.entity_type) LIKE 'announcement%' AND d.resource_id=lower(OLD.entity_id))
      OR (d.resource_type='notification' AND lower(OLD.entity_type) LIKE 'notification%' AND d.resource_id=OLD.entity_id))
    AND EXISTS (SELECT 1 FROM ai_translations t
                WHERE t.user_id=OLD.user_id AND t.entity_id=OLD.entity_id
                  AND t.lang='zh-CN'
                  AND t.status IN ('ready','disabled','missing')
                  AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
                  AND ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%')
                    OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%')
                    OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%')))
  ON CONFLICT(document_id,user_id) DO UPDATE SET
    translated_text=excluded.translated_text,
    smart_text=excluded.smart_text,
    updated_at=excluded.updated_at;
  INSERT INTO search_document_user_lanes_fts (doc_id,user_id,translated_text,smart_text)
  SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'')
  FROM search_document_user_lanes
  WHERE user_id=OLD.user_id AND document_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND resource_id=OLD.entity_id) OR (resource_type='announcement' AND resource_id=lower(OLD.entity_id)) OR (resource_type='notification' AND resource_id=OLD.entity_id));
END;

CREATE TRIGGER IF NOT EXISTS search_content_projections_ai AFTER INSERT ON content_result_projections
WHEN NEW.pipeline IN ('translation', 'polishing') AND NEW.target_lang = 'zh-CN' BEGIN
  UPDATE search_documents SET
    translated_text = CASE WHEN NEW.pipeline = 'translation' THEN (SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = NEW.canonical_resource_type AND p.canonical_resource_id = NEW.canonical_resource_id AND p.pipeline = 'translation' AND p.target_lang = 'zh-CN' ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1) ELSE translated_text END,
    smart_text = CASE WHEN NEW.pipeline = 'polishing' THEN (SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = NEW.canonical_resource_type AND p.canonical_resource_id = NEW.canonical_resource_id AND p.pipeline = 'polishing' AND p.target_lang = 'zh-CN' ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1) ELSE smart_text END,
    updated_at = MAX(updated_at, NEW.updated_at)
  WHERE resource_type = NEW.canonical_resource_type AND resource_id = NEW.canonical_resource_id;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type = NEW.canonical_resource_type AND resource_id = NEW.canonical_resource_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type = NEW.canonical_resource_type AND resource_id = NEW.canonical_resource_id;
END;

CREATE TRIGGER IF NOT EXISTS search_content_projections_au AFTER UPDATE ON content_result_projections
WHEN NEW.pipeline IN ('translation', 'polishing') AND NEW.target_lang = 'zh-CN' BEGIN
  UPDATE search_documents SET
    translated_text = CASE WHEN NEW.pipeline = 'translation' THEN (SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = NEW.canonical_resource_type AND p.canonical_resource_id = NEW.canonical_resource_id AND p.pipeline = 'translation' AND p.target_lang = 'zh-CN' ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1) ELSE translated_text END,
    smart_text = CASE WHEN NEW.pipeline = 'polishing' THEN (SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = NEW.canonical_resource_type AND p.canonical_resource_id = NEW.canonical_resource_id AND p.pipeline = 'polishing' AND p.target_lang = 'zh-CN' ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1) ELSE smart_text END,
    updated_at = MAX(updated_at, NEW.updated_at)
  WHERE resource_type = NEW.canonical_resource_type AND resource_id = NEW.canonical_resource_id;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type = NEW.canonical_resource_type AND resource_id = NEW.canonical_resource_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type = NEW.canonical_resource_type AND resource_id = NEW.canonical_resource_id;
END;

CREATE TRIGGER IF NOT EXISTS search_content_projections_ad AFTER DELETE ON content_result_projections
WHEN OLD.pipeline IN ('translation', 'polishing') AND OLD.target_lang = 'zh-CN' BEGIN
  UPDATE search_documents SET
    translated_text = (SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = OLD.canonical_resource_type AND p.canonical_resource_id = OLD.canonical_resource_id AND p.pipeline = 'translation' AND p.target_lang = 'zh-CN' ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1),
    smart_text = (SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = OLD.canonical_resource_type AND p.canonical_resource_id = OLD.canonical_resource_id AND p.pipeline = 'polishing' AND p.target_lang = 'zh-CN' ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1)
  WHERE resource_type = OLD.canonical_resource_type AND resource_id = OLD.canonical_resource_id;
  DELETE FROM search_documents_fts WHERE doc_id IN (SELECT id FROM search_documents WHERE resource_type = OLD.canonical_resource_type AND resource_id = OLD.canonical_resource_id);
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE resource_type = OLD.canonical_resource_type AND resource_id = OLD.canonical_resource_id;
END;
