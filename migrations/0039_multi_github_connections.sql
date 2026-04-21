CREATE TABLE IF NOT EXISTS github_connections (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_user_id INTEGER NOT NULL UNIQUE,
  login TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  email TEXT,
  access_token_ciphertext BLOB NOT NULL,
  access_token_nonce BLOB NOT NULL,
  scopes TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_github_connections_user_login
  ON github_connections(user_id, login);

ALTER TABLE reaction_pat_tokens
  ADD COLUMN owner_github_connection_id TEXT;

ALTER TABLE reaction_pat_tokens
  ADD COLUMN owner_github_user_id INTEGER;

ALTER TABLE reaction_pat_tokens
  ADD COLUMN owner_login TEXT;
