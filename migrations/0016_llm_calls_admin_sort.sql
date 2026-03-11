CREATE INDEX IF NOT EXISTS idx_llm_calls_admin_sort
  ON llm_calls(
    CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END,
    created_at DESC,
    id DESC
  );
