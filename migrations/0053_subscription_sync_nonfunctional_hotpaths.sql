CREATE INDEX IF NOT EXISTS idx_job_tasks_finished_status
  ON job_tasks(status, finished_at DESC, id DESC)
  WHERE finished_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_job_tasks_realtime_created
  ON job_tasks(created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_job_tasks_subscription_chain
  ON job_tasks(parent_task_id, task_type, finished_at DESC)
  WHERE parent_task_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_repo_release_watchers_retention
  ON repo_release_watchers(status, updated_at ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_llm_calls_finished_updated_created
  ON llm_calls(
    COALESCE(finished_at, updated_at, created_at) DESC,
    status,
    source,
    id DESC
  );
