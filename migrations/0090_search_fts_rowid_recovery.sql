-- Replace doc_id scans in the command-palette FTS tables with stable rowid
-- lookups. The old FTS corpus is a rebuildable cache, so this migration only
-- changes the structure and leaves population to the resumable worker.

CREATE TABLE IF NOT EXISTS search_fts_document_rows (
  fts_rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS search_fts_user_lane_rows (
  fts_rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  UNIQUE (document_id, user_id)
);

CREATE TABLE IF NOT EXISTS search_metadata_backfill_queue (
  repo_id INTEGER PRIMARY KEY,
  updated_at TEXT NOT NULL,
  release_cursor INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_search_metadata_backfill_queue_updated
  ON search_metadata_backfill_queue(updated_at, repo_id);

-- FTS5 does not support changing the rowid strategy in place. These tables are
-- only a cache: while the new tables are empty, search falls back to the
-- projection columns and the worker repopulates them after the listener starts.
DROP TABLE IF EXISTS search_documents_fts;
DROP TABLE IF EXISTS search_document_user_lanes_fts;

CREATE VIRTUAL TABLE search_documents_fts_v2 USING fts5(
  doc_id UNINDEXED,
  title,
  body,
  repo_full_name,
  translated_text,
  smart_text,
  tokenize = 'trigram'
);

CREATE VIRTUAL TABLE search_document_user_lanes_fts_v2 USING fts5(
  doc_id UNINDEXED,
  user_id UNINDEXED,
  translated_text,
  smart_text,
  tokenize = 'trigram'
);

-- Keep the old names writable for the already-deployed source triggers. The
-- views are backed by indexed mapping tables, and their INSTEAD OF triggers
-- translate every maintenance operation into an FTS rowid point update.
CREATE VIEW search_documents_fts(
  doc_id, title, body, repo_full_name, translated_text, smart_text
) AS
SELECT r.document_id, f.title, f.body, f.repo_full_name,
       f.translated_text, f.smart_text
FROM search_fts_document_rows r
JOIN search_documents_fts_v2 f ON f.rowid = r.fts_rowid;

CREATE VIEW search_document_user_lanes_fts(
  doc_id, user_id, translated_text, smart_text
) AS
SELECT r.document_id, r.user_id, f.translated_text, f.smart_text
FROM search_fts_user_lane_rows r
JOIN search_document_user_lanes_fts_v2 f ON f.rowid = r.fts_rowid;

CREATE TRIGGER search_documents_fts_compat_ai
INSTEAD OF INSERT ON search_documents_fts BEGIN
  INSERT INTO search_fts_document_rows (document_id)
  VALUES (NEW.doc_id)
  ON CONFLICT(document_id) DO NOTHING;
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.doc_id);
  INSERT INTO search_documents_fts_v2
    (rowid, doc_id, title, body, repo_full_name, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.doc_id),
    NEW.doc_id, NEW.title, NEW.body, NEW.repo_full_name,
    NEW.translated_text, NEW.smart_text
  );
END;

CREATE TRIGGER search_documents_fts_compat_au
INSTEAD OF UPDATE ON search_documents_fts BEGIN
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = OLD.doc_id);
  INSERT INTO search_fts_document_rows (document_id)
  VALUES (NEW.doc_id)
  ON CONFLICT(document_id) DO NOTHING;
  INSERT INTO search_documents_fts_v2
    (rowid, doc_id, title, body, repo_full_name, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.doc_id),
    NEW.doc_id, NEW.title, NEW.body, NEW.repo_full_name,
    NEW.translated_text, NEW.smart_text
  );
END;

CREATE TRIGGER search_documents_fts_compat_ad
INSTEAD OF DELETE ON search_documents_fts BEGIN
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = OLD.doc_id);
END;

CREATE TRIGGER search_document_user_lanes_fts_compat_ai
INSTEAD OF INSERT ON search_document_user_lanes_fts BEGIN
  INSERT INTO search_fts_user_lane_rows (document_id, user_id)
  VALUES (NEW.doc_id, NEW.user_id)
  ON CONFLICT(document_id, user_id) DO NOTHING;
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = NEW.doc_id AND user_id = NEW.user_id
  );
  INSERT INTO search_document_user_lanes_fts_v2
    (rowid, doc_id, user_id, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_user_lane_rows
     WHERE document_id = NEW.doc_id AND user_id = NEW.user_id),
    NEW.doc_id, NEW.user_id, NEW.translated_text, NEW.smart_text
  );
END;

CREATE TRIGGER search_document_user_lanes_fts_compat_au
INSTEAD OF UPDATE ON search_document_user_lanes_fts BEGIN
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = OLD.doc_id AND user_id = OLD.user_id
  );
  INSERT INTO search_fts_user_lane_rows (document_id, user_id)
  VALUES (NEW.doc_id, NEW.user_id)
  ON CONFLICT(document_id, user_id) DO NOTHING;
  INSERT INTO search_document_user_lanes_fts_v2
    (rowid, doc_id, user_id, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_user_lane_rows
     WHERE document_id = NEW.doc_id AND user_id = NEW.user_id),
    NEW.doc_id, NEW.user_id, NEW.translated_text, NEW.smart_text
  );
END;

CREATE TRIGGER search_document_user_lanes_fts_compat_ad
INSTEAD OF DELETE ON search_document_user_lanes_fts BEGIN
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = OLD.doc_id AND user_id = OLD.user_id
  );
END;

-- Direct projection writes are also indexed document-specific. The old source
-- triggers still issue explicit FTS writes, so these triggers make deletes and
-- updates that only touch search_documents/search_document_user_lanes safe.
CREATE TRIGGER search_documents_fts_v2_ai
AFTER INSERT ON search_documents BEGIN
  INSERT INTO search_fts_document_rows (document_id)
  VALUES (NEW.id)
  ON CONFLICT(document_id) DO NOTHING;
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.id);
  INSERT INTO search_documents_fts_v2
    (rowid, doc_id, title, body, repo_full_name, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.id),
    NEW.id, COALESCE(NEW.title, ''), COALESCE(NEW.body, ''),
    COALESCE(NEW.repo_full_name, ''), COALESCE(NEW.translated_text, ''),
    COALESCE(NEW.smart_text, '')
  );
END;

CREATE TRIGGER search_documents_fts_v2_au
AFTER UPDATE ON search_documents BEGIN
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = OLD.id);
  INSERT INTO search_fts_document_rows (document_id)
  VALUES (NEW.id)
  ON CONFLICT(document_id) DO NOTHING;
  INSERT INTO search_documents_fts_v2
    (rowid, doc_id, title, body, repo_full_name, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.id),
    NEW.id, COALESCE(NEW.title, ''), COALESCE(NEW.body, ''),
    COALESCE(NEW.repo_full_name, ''), COALESCE(NEW.translated_text, ''),
    COALESCE(NEW.smart_text, '')
  );
END;

CREATE TRIGGER search_documents_fts_v2_ad
AFTER DELETE ON search_documents BEGIN
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = OLD.id);
  DELETE FROM search_fts_document_rows WHERE document_id = OLD.id;
END;

CREATE TRIGGER search_document_user_lanes_fts_v2_ai
AFTER INSERT ON search_document_user_lanes BEGIN
  INSERT INTO search_fts_user_lane_rows (document_id, user_id)
  VALUES (NEW.document_id, NEW.user_id)
  ON CONFLICT(document_id, user_id) DO NOTHING;
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = NEW.document_id AND user_id = NEW.user_id
  );
  INSERT INTO search_document_user_lanes_fts_v2
    (rowid, doc_id, user_id, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_user_lane_rows
     WHERE document_id = NEW.document_id AND user_id = NEW.user_id),
    NEW.document_id, NEW.user_id, COALESCE(NEW.translated_text, ''),
    COALESCE(NEW.smart_text, '')
  );
END;

CREATE TRIGGER search_document_user_lanes_fts_v2_au
AFTER UPDATE ON search_document_user_lanes BEGIN
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = OLD.document_id AND user_id = OLD.user_id
  );
  INSERT INTO search_fts_user_lane_rows (document_id, user_id)
  VALUES (NEW.document_id, NEW.user_id)
  ON CONFLICT(document_id, user_id) DO NOTHING;
  INSERT INTO search_document_user_lanes_fts_v2
    (rowid, doc_id, user_id, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_user_lane_rows
     WHERE document_id = NEW.document_id AND user_id = NEW.user_id),
    NEW.document_id, NEW.user_id, COALESCE(NEW.translated_text, ''),
    COALESCE(NEW.smart_text, '')
  );
END;

CREATE TRIGGER search_document_user_lanes_fts_v2_ad
AFTER DELETE ON search_document_user_lanes BEGIN
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = OLD.document_id AND user_id = OLD.user_id
  );
  DELETE FROM search_fts_user_lane_rows
  WHERE document_id = OLD.document_id AND user_id = OLD.user_id;
END;

-- Repo metadata affects every cached release for a repo. Queue the repo and
-- let the Background worker refresh at most 100 release rows per transaction.
DROP TRIGGER IF EXISTS search_repo_release_work_items_ai;
DROP TRIGGER IF EXISTS search_repo_release_work_items_au;
DROP TRIGGER IF EXISTS search_repo_release_work_items_ad;
DROP TRIGGER IF EXISTS search_repo_associations_ai;
DROP TRIGGER IF EXISTS search_repo_associations_au;
DROP TRIGGER IF EXISTS search_repo_associations_ad;
DROP TRIGGER IF EXISTS search_owned_repo_star_baselines_ai;
DROP TRIGGER IF EXISTS search_owned_repo_star_baselines_au;
DROP TRIGGER IF EXISTS search_owned_repo_star_baselines_ad;
DROP TRIGGER IF EXISTS search_users_include_own_releases_au;
DROP TRIGGER IF EXISTS search_starred_repos_ai;
DROP TRIGGER IF EXISTS search_starred_repos_au;
DROP TRIGGER IF EXISTS search_starred_repos_ad;

CREATE TRIGGER search_repo_release_work_items_ai
AFTER INSERT ON repo_release_work_items
WHEN NEW.repo_id IS NOT NULL BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  VALUES (NEW.repo_id, CURRENT_TIMESTAMP)
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_repo_release_work_items_au
AFTER UPDATE OF repo_id, repo_full_name ON repo_release_work_items BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL AND OLD.repo_id IS NOT NEW.repo_id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_repo_release_work_items_ad
AFTER DELETE ON repo_release_work_items
WHEN OLD.repo_id IS NOT NULL BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  VALUES (OLD.repo_id, CURRENT_TIMESTAMP)
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_repo_associations_ai
AFTER INSERT ON user_repo_associations BEGIN
  DELETE FROM search_documents
  WHERE user_id = NEW.user_id
    AND resource_type = 'repository'
    AND repo_id = NEW.repo_id
    AND id <> 'repository:' || NEW.user_id || ':' || NEW.repo_full_name_lower;
  INSERT INTO search_documents (
    id, user_id, resource_type, resource_id, repo_id, repo_full_name,
    owner_login, title, body, source_time, target_path, target_url,
    created_at, updated_at
  ) VALUES (
    'repository:' || NEW.user_id || ':' || NEW.repo_full_name_lower,
    NEW.user_id, 'repository', NEW.repo_full_name_lower, NEW.repo_id,
    NEW.repo_full_name, NEW.owner_login, NEW.repo_name, NEW.description,
    NEW.updated_at, '/focus/repo/' || NEW.owner_login || '/' || NEW.repo_name,
    NEW.html_url, NEW.created_at, NEW.updated_at
  ) ON CONFLICT(user_id, resource_type, resource_id) DO UPDATE SET
    repo_id = excluded.repo_id,
    repo_full_name = excluded.repo_full_name,
    owner_login = excluded.owner_login,
    title = excluded.title,
    body = excluded.body,
    source_time = excluded.source_time,
    target_path = excluded.target_path,
    target_url = excluded.target_url,
    updated_at = excluded.updated_at;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_repo_associations_au
AFTER UPDATE OF repo_id, repo_full_name, repo_full_name_lower, owner_login,
                repo_name, html_url, description ON user_repo_associations
WHEN OLD.repo_id IS NOT NEW.repo_id
  OR OLD.repo_full_name IS NOT NEW.repo_full_name
  OR OLD.repo_full_name_lower IS NOT NEW.repo_full_name_lower
  OR OLD.owner_login IS NOT NEW.owner_login
  OR OLD.repo_name IS NOT NEW.repo_name
  OR OLD.html_url IS NOT NEW.html_url
  OR OLD.description IS NOT NEW.description
BEGIN
  DELETE FROM search_documents
  WHERE user_id = NEW.user_id
    AND resource_type = 'repository'
    AND repo_id = NEW.repo_id
    AND id <> 'repository:' || NEW.user_id || ':' || NEW.repo_full_name_lower;
  DELETE FROM search_documents
  WHERE id = 'repository:' || OLD.user_id || ':' || OLD.repo_full_name_lower
    AND lower(OLD.repo_full_name_lower) <> lower(NEW.repo_full_name_lower)
    AND NOT EXISTS (
      SELECT 1 FROM starred_repos sr
      WHERE sr.user_id = OLD.user_id
        AND lower(sr.full_name) = lower(OLD.repo_full_name_lower)
    );
  INSERT INTO search_documents (
    id, user_id, resource_type, resource_id, repo_id, repo_full_name,
    owner_login, title, body, source_time, target_path, target_url,
    created_at, updated_at
  ) VALUES (
    'repository:' || NEW.user_id || ':' || NEW.repo_full_name_lower,
    NEW.user_id, 'repository', NEW.repo_full_name_lower, NEW.repo_id,
    NEW.repo_full_name, NEW.owner_login, NEW.repo_name, NEW.description,
    NEW.updated_at, '/focus/repo/' || NEW.owner_login || '/' || NEW.repo_name,
    NEW.html_url, NEW.created_at, NEW.updated_at
  ) ON CONFLICT(user_id, resource_type, resource_id) DO UPDATE SET
    repo_id = excluded.repo_id,
    repo_full_name = excluded.repo_full_name,
    owner_login = excluded.owner_login,
    title = excluded.title,
    body = excluded.body,
    source_time = excluded.source_time,
    target_path = excluded.target_path,
    target_url = excluded.target_url,
    updated_at = excluded.updated_at;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL AND OLD.repo_id IS NOT NEW.repo_id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_repo_associations_ad
AFTER DELETE ON user_repo_associations BEGIN
  DELETE FROM search_documents
  WHERE id = 'repository:' || OLD.user_id || ':' || OLD.repo_full_name_lower
    AND NOT EXISTS (
      SELECT 1 FROM starred_repos ura
      WHERE ura.user_id = OLD.user_id
        AND lower(ura.full_name) = lower(OLD.repo_full_name_lower)
    );
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_owned_repo_star_baselines_ai
AFTER INSERT ON owned_repo_star_baselines
WHEN NEW.repo_id IS NOT NULL BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  VALUES (NEW.repo_id, CURRENT_TIMESTAMP)
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_owned_repo_star_baselines_au
AFTER UPDATE ON owned_repo_star_baselines BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL AND OLD.repo_id IS NOT NEW.repo_id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_owned_repo_star_baselines_ad
AFTER DELETE ON owned_repo_star_baselines
WHEN OLD.repo_id IS NOT NULL BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  VALUES (OLD.repo_id, CURRENT_TIMESTAMP)
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_users_include_own_releases_au
AFTER UPDATE OF include_own_releases ON users BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT repo_id, CURRENT_TIMESTAMP
  FROM user_repo_associations
  WHERE user_id = NEW.id AND repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT repo_id, CURRENT_TIMESTAMP FROM starred_repos WHERE user_id = NEW.id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT repo_id, CURRENT_TIMESTAMP FROM owned_repo_star_baselines WHERE user_id = NEW.id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_starred_repos_ai
AFTER INSERT ON starred_repos BEGIN
  DELETE FROM search_documents
  WHERE user_id = NEW.user_id
    AND resource_type = 'repository'
    AND repo_id = NEW.repo_id
    AND id <> 'repository:' || NEW.user_id || ':' || lower(NEW.full_name);
  INSERT INTO search_documents (
    id, user_id, resource_type, resource_id, repo_id, repo_full_name,
    owner_login, title, body, source_time, target_path, target_url,
    created_at, updated_at
  )
  SELECT 'repository:' || NEW.user_id || ':' || lower(NEW.full_name),
         NEW.user_id, 'repository', lower(NEW.full_name), NEW.repo_id,
         NEW.full_name, NEW.owner_login, NEW.name, NEW.description,
         NEW.updated_at, '/focus/repo/' || NEW.owner_login || '/' || NEW.name,
         NEW.html_url, NEW.updated_at, NEW.updated_at
  WHERE NOT EXISTS (
    SELECT 1 FROM user_repo_associations ura
    WHERE ura.user_id = NEW.user_id
      AND ura.repo_full_name_lower = lower(NEW.full_name)
  ) ON CONFLICT(user_id, resource_type, resource_id) DO UPDATE SET
    repo_id = excluded.repo_id,
    repo_full_name = excluded.repo_full_name,
    owner_login = excluded.owner_login,
    title = excluded.title,
    body = excluded.body,
    source_time = excluded.source_time,
    target_path = excluded.target_path,
    target_url = excluded.target_url,
    updated_at = excluded.updated_at;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_starred_repos_au
AFTER UPDATE ON starred_repos BEGIN
  DELETE FROM search_documents
  WHERE user_id = NEW.user_id
    AND resource_type = 'repository'
    AND repo_id = NEW.repo_id
    AND id <> 'repository:' || NEW.user_id || ':' || lower(NEW.full_name);
  DELETE FROM search_documents
  WHERE id = 'repository:' || OLD.user_id || ':' || lower(OLD.full_name)
    AND lower(OLD.full_name) <> lower(NEW.full_name)
    AND NOT EXISTS (
      SELECT 1 FROM user_repo_associations ura
      WHERE ura.user_id = OLD.user_id
        AND ura.repo_full_name_lower = lower(OLD.full_name)
    );
  INSERT INTO search_documents (
    id, user_id, resource_type, resource_id, repo_id, repo_full_name,
    owner_login, title, body, source_time, target_path, target_url,
    created_at, updated_at
  )
  SELECT 'repository:' || NEW.user_id || ':' || lower(NEW.full_name),
         NEW.user_id, 'repository', lower(NEW.full_name), NEW.repo_id,
         NEW.full_name, NEW.owner_login, NEW.name, NEW.description,
         NEW.updated_at, '/focus/repo/' || NEW.owner_login || '/' || NEW.name,
         NEW.html_url, NEW.updated_at, NEW.updated_at
  WHERE NOT EXISTS (
    SELECT 1 FROM user_repo_associations ura
    WHERE ura.user_id = NEW.user_id
      AND ura.repo_full_name_lower = lower(NEW.full_name)
  ) ON CONFLICT(user_id, resource_type, resource_id) DO UPDATE SET
    repo_id = excluded.repo_id,
    repo_full_name = excluded.repo_full_name,
    owner_login = excluded.owner_login,
    title = excluded.title,
    body = excluded.body,
    source_time = excluded.source_time,
    target_path = excluded.target_path,
    target_url = excluded.target_url,
    updated_at = excluded.updated_at;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL AND OLD.repo_id IS NOT NEW.repo_id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

CREATE TRIGGER search_starred_repos_ad
AFTER DELETE ON starred_repos BEGIN
  DELETE FROM search_documents
  WHERE id = 'repository:' || OLD.user_id || ':' || lower(OLD.full_name)
    AND NOT EXISTS (
      SELECT 1 FROM user_repo_associations ura
      WHERE ura.user_id = OLD.user_id
        AND ura.repo_full_name_lower = lower(OLD.full_name)
    );
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;

-- A database already marked ready has an old FTS corpus. Make the rebuild
-- explicit and resumable; pending/in-flight source phases continue normally.
UPDATE search_projection_backfill_state
SET phase = 'fts_documents', cursor = 0, status = 'pending',
    last_error = NULL, updated_at = CURRENT_TIMESTAMP
WHERE id = 1 AND status = 'ready';
