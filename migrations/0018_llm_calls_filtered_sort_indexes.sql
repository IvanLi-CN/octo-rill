DROP INDEX IF EXISTS idx_llm_calls_status_created_at;
CREATE INDEX IF NOT EXISTS idx_llm_calls_status_created_at
  ON llm_calls(
    status,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );

DROP INDEX IF EXISTS idx_llm_calls_requested_by_created_at;
CREATE INDEX IF NOT EXISTS idx_llm_calls_requested_by_created_at
  ON llm_calls(
    requested_by,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );

DROP INDEX IF EXISTS idx_llm_calls_source_created_at;
CREATE INDEX IF NOT EXISTS idx_llm_calls_source_created_at
  ON llm_calls(
    source,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );

DROP INDEX IF EXISTS idx_llm_calls_parent_task_id;
CREATE INDEX IF NOT EXISTS idx_llm_calls_parent_task_id
  ON llm_calls(
    parent_task_id,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );

CREATE INDEX IF NOT EXISTS idx_llm_calls_source_admin_sort
  ON llm_calls(
    source,
    CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );

CREATE INDEX IF NOT EXISTS idx_llm_calls_requested_by_admin_sort
  ON llm_calls(
    requested_by,
    CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
