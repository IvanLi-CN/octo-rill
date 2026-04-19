CREATE TABLE IF NOT EXISTS linuxdo_connections (
  user_id TEXT PRIMARY KEY,
  linuxdo_user_id INTEGER NOT NULL UNIQUE,
  username TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  trust_level INTEGER NOT NULL DEFAULT 0,
  active INTEGER NOT NULL DEFAULT 1,
  silenced INTEGER NOT NULL DEFAULT 0,
  linked_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
