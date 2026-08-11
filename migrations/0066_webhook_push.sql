ALTER TABLE users
  ADD COLUMN webhook_push_enabled INTEGER NOT NULL DEFAULT 0
  CHECK (webhook_push_enabled IN (0, 1));

ALTER TABLE users ADD COLUMN webhook_push_secret_ciphertext BLOB;
ALTER TABLE users ADD COLUMN webhook_push_secret_nonce BLOB;
ALTER TABLE users ADD COLUMN webhook_push_callback_key TEXT;

CREATE UNIQUE INDEX idx_users_webhook_push_callback_key
  ON users(webhook_push_callback_key)
  WHERE webhook_push_callback_key IS NOT NULL;

CREATE TABLE webhook_push_repos (
  user_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  repo_full_name TEXT NOT NULL,
  hook_id INTEGER,
  callback_url TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'unknown'
    CHECK (status IN ('unknown', 'missing', 'registered', 'conflict', 'permission_paused', 'error', 'delete_pending')),
  error_kind TEXT,
  error_message TEXT,
  permission_paused INTEGER NOT NULL DEFAULT 0 CHECK (permission_paused IN (0, 1)),
  last_checked_at TEXT,
  last_registered_at TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_webhook_push_repos_hook_id
  ON webhook_push_repos(hook_id)
  WHERE hook_id IS NOT NULL;

CREATE INDEX idx_webhook_push_repos_user_status
  ON webhook_push_repos(user_id, status, repo_full_name);

CREATE TABLE webhook_push_deliveries (
  delivery_id TEXT PRIMARY KEY,
  hook_id INTEGER,
  repo_id INTEGER,
  event TEXT NOT NULL,
  action TEXT,
  received_at TEXT NOT NULL,
  queued_task_id TEXT
);

CREATE INDEX idx_webhook_push_deliveries_received_at
  ON webhook_push_deliveries(received_at);

ALTER TABLE admin_runtime_settings
  ADD COLUMN webhook_push_audit_interval_days INTEGER NOT NULL DEFAULT 7
  CHECK (webhook_push_audit_interval_days BETWEEN 1 AND 30);

ALTER TABLE admin_runtime_settings
  ADD COLUMN webhook_push_audit_last_started_at TEXT;
