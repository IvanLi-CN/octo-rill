ALTER TABLE users
  ADD COLUMN passkey_user_handle_uuid TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_passkey_user_handle_uuid
  ON users(passkey_user_handle_uuid)
  WHERE passkey_user_handle_uuid IS NOT NULL;

CREATE TABLE IF NOT EXISTS user_passkeys (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  credential_id TEXT NOT NULL UNIQUE,
  label TEXT NOT NULL,
  passkey_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_user_passkeys_user_created_at
  ON user_passkeys(user_id, created_at, id);
