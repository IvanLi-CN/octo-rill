-- AI translations cache (SQLite)

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS ai_translations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  entity_type TEXT NOT NULL, -- release | notification
  entity_id TEXT NOT NULL,   -- release_id (text) | thread_id
  lang TEXT NOT NULL,        -- e.g. zh-CN
  source_hash TEXT NOT NULL,
  title TEXT,
  summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, entity_type, entity_id, lang),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

