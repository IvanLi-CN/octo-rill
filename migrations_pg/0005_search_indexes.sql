-- PostgreSQL search projection indexes.
-- The generated vector uses the explicit `simple` configuration from the
-- baseline. Trigram indexes provide the second candidate path for Chinese,
-- code identifiers, punctuation and translation lanes that are not usefully
-- tokenized by the simple parser.
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX IF NOT EXISTS idx_search_documents_vector
  ON search_documents USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_search_documents_title_trgm
  ON search_documents USING GIN (lower(title) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_documents_body_trgm
  ON search_documents USING GIN (lower(body) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_documents_repo_trgm
  ON search_documents USING GIN (lower(repo_full_name) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_documents_translated_trgm
  ON search_documents USING GIN (lower(translated_text) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_documents_smart_trgm
  ON search_documents USING GIN (lower(smart_text) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_document_lanes_user_document
  ON search_document_user_lanes (user_id, document_id);
CREATE INDEX IF NOT EXISTS idx_search_document_lanes_translated_trgm
  ON search_document_user_lanes USING GIN (lower(translated_text) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_document_lanes_smart_trgm
  ON search_document_user_lanes USING GIN (lower(smart_text) gin_trgm_ops);

-- Visibility and current repository metadata are authorization inputs. Keep
-- them indexed independently from the rebuildable search projection.
CREATE INDEX IF NOT EXISTS idx_user_repo_associations_user_repo_updated
  ON user_repo_associations (user_id, repo_id, updated_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_user_repo_associations_user_name_updated
  ON user_repo_associations (user_id, repo_full_name_lower, updated_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_search_projection_outbox_claim
  ON search_projection_outbox (available_at, created_at, projection_key)
  WHERE lease_expires_at IS NULL;

