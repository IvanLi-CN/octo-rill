ALTER TABLE users ADD COLUMN webhook_push_desired_state TEXT NOT NULL DEFAULT 'deleted'
  CHECK (webhook_push_desired_state IN ('enabled', 'paused', 'deleted'));

ALTER TABLE users ADD COLUMN webhook_push_last_completed_check_at TEXT;

ALTER TABLE job_tasks ADD COLUMN available_at TEXT;

UPDATE users
SET webhook_push_desired_state = CASE
  WHEN webhook_push_enabled != 0 THEN 'enabled'
  WHEN EXISTS (
    SELECT 1
    FROM webhook_push_repos
    WHERE webhook_push_repos.user_id = users.id
      AND webhook_push_repos.hook_id IS NOT NULL
  ) THEN 'paused'
  ELSE 'deleted'
END;

DROP INDEX IF EXISTS idx_webhook_push_manage_inflight;

WITH ranked AS (
  SELECT id,
         ROW_NUMBER() OVER (
           PARTITION BY task_type, requested_by
           ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END,
                    created_at DESC,
                    id DESC
         ) AS duplicate_rank
  FROM job_tasks
  WHERE task_type = 'webhook.push.manage'
    AND requested_by IS NOT NULL
    AND status IN ('queued', 'running')
)
UPDATE job_tasks
SET status = 'canceled',
    cancel_requested = 1,
    error_message = 'superseded during webhook desired-state migration',
    finished_at = COALESCE(finished_at, updated_at),
    runtime_owner_id = NULL,
    lease_heartbeat_at = NULL,
    updated_at = COALESCE(updated_at, created_at)
WHERE id IN (SELECT id FROM ranked WHERE duplicate_rank > 1);

DELETE FROM webhook_push_user_operation_leases
WHERE task_id NOT IN (
  SELECT id
  FROM job_tasks
  WHERE task_type IN ('webhook.push.manage', 'webhook.push.audit')
    AND status IN ('queued', 'running')
);

CREATE UNIQUE INDEX idx_webhook_push_manage_inflight
  ON job_tasks(task_type, requested_by)
  WHERE task_type = 'webhook.push.manage'
    AND requested_by IS NOT NULL
    AND status IN ('queued', 'running');

CREATE INDEX idx_job_tasks_available_at
  ON job_tasks(status, available_at, created_at);
