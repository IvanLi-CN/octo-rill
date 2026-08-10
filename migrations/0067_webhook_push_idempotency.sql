ALTER TABLE webhook_push_deliveries
  ADD COLUMN processing_state TEXT NOT NULL DEFAULT 'pending'
  CHECK (processing_state IN ('pending', 'processing', 'ignored', 'queued'));

ALTER TABLE webhook_push_deliveries
  ADD COLUMN processing_started_at TEXT;

UPDATE webhook_push_deliveries
SET processing_state = CASE
  WHEN queued_task_id IS NOT NULL THEN 'queued'
  ELSE 'ignored'
END;

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
