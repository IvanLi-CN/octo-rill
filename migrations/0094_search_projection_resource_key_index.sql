-- Keep realtime projection updates point-addressable by logical resource key.
CREATE INDEX IF NOT EXISTS idx_search_documents_resource_key
  ON search_documents(resource_type, resource_id, id);
