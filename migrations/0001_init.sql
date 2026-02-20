-- OctoRill initial schema (SQLite)

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  github_user_id INTEGER NOT NULL UNIQUE,
  login TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  email TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_tokens (
  user_id INTEGER PRIMARY KEY,
  access_token_ciphertext BLOB NOT NULL,
  access_token_nonce BLOB NOT NULL,
  scopes TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS starred_repos (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  repo_id INTEGER NOT NULL,
  full_name TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  html_url TEXT NOT NULL,
  stargazed_at TEXT,
  is_private INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS releases (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  repo_id INTEGER NOT NULL,
  release_id INTEGER NOT NULL,
  tag_name TEXT NOT NULL,
  name TEXT,
  body TEXT,
  html_url TEXT NOT NULL,
  published_at TEXT,
  created_at TEXT,
  is_prerelease INTEGER NOT NULL DEFAULT 0,
  is_draft INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, release_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS notifications (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  thread_id TEXT NOT NULL,
  repo_full_name TEXT,
  subject_title TEXT,
  subject_type TEXT,
  reason TEXT,
  updated_at TEXT,
  unread INTEGER NOT NULL DEFAULT 1,
  url TEXT,
  html_url TEXT,
  last_seen_at TEXT,
  UNIQUE(user_id, thread_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS briefs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  date TEXT NOT NULL,
  content_markdown TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(user_id, date),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS sync_state (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, key),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
