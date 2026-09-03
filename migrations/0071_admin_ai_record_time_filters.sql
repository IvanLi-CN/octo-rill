CREATE INDEX IF NOT EXISTS idx_translation_requests_created_at
  ON translation_requests(created_at DESC, id DESC);
