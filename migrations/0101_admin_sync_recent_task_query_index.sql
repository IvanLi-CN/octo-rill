CREATE INDEX IF NOT EXISTS idx_job_tasks_sync_recent_roots
  ON job_tasks(created_at DESC, id DESC)
  WHERE task_type = 'sync.subscriptions'
    AND COALESCE(json_extract(result_json, '$.skipped'), 0) != 1;
