ALTER TABLE webhook_push_deliveries
  ADD COLUMN processing_state TEXT NOT NULL DEFAULT 'pending'
  CHECK (processing_state IN ('pending', 'processing', 'ignored', 'queued'));

ALTER TABLE webhook_push_deliveries
  ADD COLUMN processing_started_at TEXT;

ALTER TABLE webhook_push_repos
  ADD COLUMN owner_github_user_id INTEGER;

UPDATE webhook_push_repos
SET owner_github_user_id = (
  SELECT pat.owner_github_user_id
  FROM reaction_pat_tokens pat
  WHERE pat.user_id = webhook_push_repos.user_id
);

UPDATE webhook_push_deliveries
SET processing_state = CASE
  WHEN queued_task_id IS NOT NULL THEN 'queued'
  ELSE 'pending'
END;

WITH ranked AS (
  SELECT id,
         ROW_NUMBER() OVER (
           PARTITION BY task_type, requested_by, payload_json
           ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END, created_at DESC, id DESC
         ) AS duplicate_rank
  FROM job_tasks
  WHERE task_type = 'webhook.push.manage'
    AND requested_by IS NOT NULL
    AND status IN ('queued', 'running')
)
UPDATE job_tasks
SET status = 'canceled',
    error_message = 'superseded during webhook idempotency migration',
    finished_at = COALESCE(finished_at, updated_at),
    runtime_owner_id = NULL,
    lease_heartbeat_at = NULL
WHERE id IN (SELECT id FROM ranked WHERE duplicate_rank > 1);

WITH ranked AS (
  SELECT id,
         ROW_NUMBER() OVER (
           PARTITION BY task_type
           ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END, created_at DESC, id DESC
         ) AS duplicate_rank
  FROM job_tasks
  WHERE task_type = 'webhook.push.audit'
    AND status IN ('queued', 'running')
)
UPDATE job_tasks
SET status = 'canceled',
    error_message = 'superseded during webhook idempotency migration',
    finished_at = COALESCE(finished_at, updated_at),
    runtime_owner_id = NULL,
    lease_heartbeat_at = NULL
WHERE id IN (SELECT id FROM ranked WHERE duplicate_rank > 1);

CREATE UNIQUE INDEX idx_webhook_push_manage_inflight
  ON job_tasks(task_type, requested_by, payload_json)
  WHERE task_type = 'webhook.push.manage'
    AND requested_by IS NOT NULL
    AND status IN ('queued', 'running');

CREATE UNIQUE INDEX idx_webhook_push_audit_inflight
  ON job_tasks(task_type)
  WHERE task_type = 'webhook.push.audit'
    AND status IN ('queued', 'running');

CREATE TABLE webhook_push_user_operation_leases (
  user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  task_id TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
