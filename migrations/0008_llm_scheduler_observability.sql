PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS llm_calls (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  model TEXT NOT NULL,
  requested_by INTEGER,
  parent_task_id TEXT,
  parent_task_type TEXT,
  max_tokens INTEGER NOT NULL,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  scheduler_wait_ms INTEGER NOT NULL DEFAULT 0,
  duration_ms INTEGER,
  prompt_text TEXT NOT NULL,
  response_text TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_llm_calls_status_created_at
  ON llm_calls(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_llm_calls_requested_by_created_at
  ON llm_calls(requested_by, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_llm_calls_source_created_at
  ON llm_calls(source, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_llm_calls_parent_task_id
  ON llm_calls(parent_task_id);
CREATE INDEX IF NOT EXISTS idx_llm_calls_created_at
  ON llm_calls(created_at DESC);
