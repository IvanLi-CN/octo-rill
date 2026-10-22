CREATE INDEX IF NOT EXISTS idx_job_tasks_task_type_created_at_id
  ON job_tasks(task_type, created_at DESC, id DESC);
