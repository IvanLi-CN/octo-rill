PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS user_api_keys (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  name TEXT NOT NULL,
  key_hash TEXT NOT NULL UNIQUE,
  key_prefix TEXT NOT NULL,
  masked_key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked_at TEXT,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_user_api_keys_user_created_at
  ON user_api_keys(user_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_user_api_keys_hash_active
  ON user_api_keys(key_hash)
  WHERE revoked_at IS NULL;
