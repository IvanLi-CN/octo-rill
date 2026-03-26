ALTER TABLE job_tasks ADD COLUMN runtime_owner_id TEXT;
ALTER TABLE job_tasks ADD COLUMN lease_heartbeat_at TEXT;

ALTER TABLE llm_calls ADD COLUMN runtime_owner_id TEXT;
ALTER TABLE llm_calls ADD COLUMN lease_heartbeat_at TEXT;

ALTER TABLE translation_batches ADD COLUMN runtime_owner_id TEXT;
ALTER TABLE translation_batches ADD COLUMN lease_heartbeat_at TEXT;

CREATE INDEX IF NOT EXISTS idx_job_tasks_running_runtime_lease
  ON job_tasks(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';

CREATE INDEX IF NOT EXISTS idx_llm_calls_running_runtime_lease
  ON llm_calls(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';

CREATE INDEX IF NOT EXISTS idx_translation_batches_running_runtime_lease
  ON translation_batches(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';
