CREATE TABLE IF NOT EXISTS runtime_owners (
  runtime_owner_id TEXT PRIMARY KEY,
  lease_heartbeat_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runtime_owners_lease_heartbeat
  ON runtime_owners(lease_heartbeat_at);
