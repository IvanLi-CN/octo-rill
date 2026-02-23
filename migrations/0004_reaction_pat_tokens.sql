PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS reaction_pat_tokens (
  user_id INTEGER PRIMARY KEY,
  token_ciphertext BLOB NOT NULL,
  token_nonce BLOB NOT NULL,
  masked_token TEXT NOT NULL,
  last_check_state TEXT NOT NULL DEFAULT 'unknown',
  last_check_message TEXT,
  last_checked_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
