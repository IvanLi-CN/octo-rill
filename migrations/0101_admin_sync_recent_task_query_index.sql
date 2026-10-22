CREATE INDEX IF NOT EXISTS idx_job_tasks_sync_recent_roots
  ON job_tasks(created_at DESC, id DESC)
  WHERE task_type = 'sync.subscriptions'
    AND CASE
      WHEN json_valid(result_json)
        THEN COALESCE(json_extract(result_json, '$.skipped'), 0) != 1
      ELSE 1
    END;
