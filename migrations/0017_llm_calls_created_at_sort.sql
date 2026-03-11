DROP INDEX IF EXISTS idx_llm_calls_admin_sort;
CREATE INDEX IF NOT EXISTS idx_llm_calls_admin_sort
  ON llm_calls(
    CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );

DROP INDEX IF EXISTS idx_llm_calls_created_at;
CREATE INDEX IF NOT EXISTS idx_llm_calls_created_at
  ON llm_calls(
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
