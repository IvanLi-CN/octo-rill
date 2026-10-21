-- Rebuild scheduler and terminal-recovery lookup indexes without evaluating
-- malformed legacy task payloads during startup or subsequent inserts.
DROP INDEX IF EXISTS idx_job_tasks_star_connection_active;
CREATE INDEX idx_job_tasks_star_connection_active
  ON job_tasks(
    task_type,
    status,
    json_extract(payload_json, '$.github_connection_id')
  )
  WHERE json_valid(payload_json);

CREATE INDEX IF NOT EXISTS idx_search_projection_outbox_failed_poll
  ON search_projection_outbox_failed(failed_at, projection_key);
