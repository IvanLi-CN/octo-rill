ALTER TABLE reaction_pat_tokens
  ADD COLUMN webhook_push_allows_private_repos INTEGER
  CHECK (webhook_push_allows_private_repos IS NULL OR webhook_push_allows_private_repos IN (0, 1));

CREATE TABLE IF NOT EXISTS webhook_push_reconcile_demands (
  user_id TEXT PRIMARY KEY,
  requested_generation INTEGER NOT NULL DEFAULT 0 CHECK (requested_generation >= 0),
  completed_generation INTEGER NOT NULL DEFAULT 0 CHECK (completed_generation >= 0),
  requested_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  CHECK (completed_generation <= requested_generation)
);

CREATE INDEX IF NOT EXISTS idx_webhook_push_reconcile_demands_pending
  ON webhook_push_reconcile_demands(requested_generation, completed_generation, requested_at)
  WHERE requested_generation > completed_generation;
