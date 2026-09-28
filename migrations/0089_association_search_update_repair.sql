-- Association source and follow-state updates do not change the repository
-- search projection. Keep the trigger for repository metadata changes only.
DROP TRIGGER IF EXISTS search_repo_associations_au;

CREATE INDEX IF NOT EXISTS idx_search_documents_resource_repo
  ON search_documents(resource_type, repo_id);

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
