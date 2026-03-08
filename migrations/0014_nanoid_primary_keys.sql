PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS translation_batch_items;
DROP TABLE IF EXISTS translation_batches;
DROP TABLE IF EXISTS translation_work_watchers;
DROP TABLE IF EXISTS translation_work_items;
DROP TABLE IF EXISTS translation_request_items;
DROP TABLE IF EXISTS translation_requests;
DROP TABLE IF EXISTS sync_subscription_events;
DROP TABLE IF EXISTS scheduled_task_dispatch_state;
DROP TABLE IF EXISTS llm_call_events;
DROP TABLE IF EXISTS llm_calls;
DROP TABLE IF EXISTS daily_brief_hour_slots;
DROP TABLE IF EXISTS job_task_events;
DROP TABLE IF EXISTS job_tasks;
DROP TABLE IF EXISTS reaction_pat_tokens;
DROP TABLE IF EXISTS ai_translations;
DROP TABLE IF EXISTS sync_state;
DROP TABLE IF EXISTS briefs;
DROP TABLE IF EXISTS notifications;
DROP TABLE IF EXISTS releases;
DROP TABLE IF EXISTS starred_repos;
DROP TABLE IF EXISTS user_tokens;
DROP TABLE IF EXISTS users;

PRAGMA foreign_keys = ON;

CREATE TABLE users (
  id TEXT PRIMARY KEY,
  github_user_id INTEGER NOT NULL UNIQUE,
  login TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  email TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  is_disabled INTEGER NOT NULL DEFAULT 0,
  daily_brief_utc_time TEXT NOT NULL DEFAULT '00:00',
  last_active_at TEXT
);

CREATE TABLE user_tokens (
  user_id TEXT PRIMARY KEY,
  access_token_ciphertext BLOB NOT NULL,
  access_token_nonce BLOB NOT NULL,
  scopes TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE starred_repos (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
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

CREATE TABLE releases (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
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
  node_id TEXT,
  react_plus1 INTEGER NOT NULL DEFAULT 0,
  react_laugh INTEGER NOT NULL DEFAULT 0,
  react_heart INTEGER NOT NULL DEFAULT 0,
  react_hooray INTEGER NOT NULL DEFAULT 0,
  react_rocket INTEGER NOT NULL DEFAULT 0,
  react_eyes INTEGER NOT NULL DEFAULT 0,
  UNIQUE(user_id, release_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_releases_user_published_at
  ON releases(user_id, published_at);

CREATE TABLE notifications (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
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

CREATE TABLE briefs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  date TEXT NOT NULL,
  content_markdown TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(user_id, date),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE sync_state (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, key),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE ai_translations (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  lang TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  title TEXT,
  summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, entity_type, entity_id, lang),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE reaction_pat_tokens (
  user_id TEXT PRIMARY KEY,
  token_ciphertext BLOB NOT NULL,
  token_nonce BLOB NOT NULL,
  masked_token TEXT NOT NULL,
  last_check_state TEXT NOT NULL DEFAULT 'unknown',
  last_check_message TEXT,
  last_checked_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE job_tasks (
  id TEXT PRIMARY KEY,
  task_type TEXT NOT NULL,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  requested_by TEXT,
  parent_task_id TEXT,
  payload_json TEXT NOT NULL,
  result_json TEXT,
  error_message TEXT,
  cancel_requested INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  log_file_path TEXT,
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id) ON DELETE SET NULL
);

CREATE INDEX idx_job_tasks_status_created_at
  ON job_tasks(status, created_at DESC);
CREATE INDEX idx_job_tasks_task_type_created_at
  ON job_tasks(task_type, created_at DESC);

CREATE TABLE job_task_events (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE
);

CREATE INDEX idx_job_task_events_task_id_created_at
  ON job_task_events(task_id, created_at, id);

CREATE TABLE daily_brief_hour_slots (
  hour_utc INTEGER PRIMARY KEY,
  enabled INTEGER NOT NULL DEFAULT 1,
  last_dispatch_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (hour_utc >= 0 AND hour_utc <= 23)
);

WITH RECURSIVE hours(n) AS (
  SELECT 0
  UNION ALL
  SELECT n + 1 FROM hours WHERE n < 23
)
INSERT INTO daily_brief_hour_slots (hour_utc, enabled, last_dispatch_at, updated_at)
SELECT n, 1, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now') FROM hours;

CREATE TABLE llm_calls (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  model TEXT NOT NULL,
  requested_by TEXT,
  parent_task_id TEXT,
  parent_task_type TEXT,
  max_tokens INTEGER NOT NULL,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  scheduler_wait_ms INTEGER NOT NULL DEFAULT 0,
  duration_ms INTEGER,
  prompt_text TEXT NOT NULL,
  response_text TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  input_messages_json TEXT,
  output_messages_json TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cached_input_tokens INTEGER,
  total_tokens INTEGER,
  first_token_wait_ms INTEGER,
  parent_translation_batch_id TEXT,
  CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id) ON DELETE SET NULL
);

CREATE INDEX idx_llm_calls_status_created_at
  ON llm_calls(status, created_at DESC);
CREATE INDEX idx_llm_calls_requested_by_created_at
  ON llm_calls(requested_by, created_at DESC);
CREATE INDEX idx_llm_calls_source_created_at
  ON llm_calls(source, created_at DESC);
CREATE INDEX idx_llm_calls_parent_task_id
  ON llm_calls(parent_task_id);
CREATE INDEX idx_llm_calls_created_at
  ON llm_calls(created_at DESC);
CREATE INDEX idx_llm_calls_parent_translation_batch_id
  ON llm_calls(parent_translation_batch_id);

CREATE TABLE llm_call_events (
  id TEXT PRIMARY KEY,
  call_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  requested_by TEXT,
  parent_task_id TEXT,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  FOREIGN KEY(call_id) REFERENCES llm_calls(id) ON DELETE CASCADE
);

CREATE INDEX idx_llm_call_events_call_id_created_at
  ON llm_call_events(call_id, created_at, id);

CREATE TABLE scheduled_task_dispatch_state (
  schedule_name TEXT PRIMARY KEY,
  last_dispatch_key TEXT,
  last_task_id TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE sync_subscription_events (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  event_type TEXT NOT NULL,
  severity TEXT NOT NULL,
  recoverable INTEGER NOT NULL DEFAULT 0,
  attempt INTEGER NOT NULL DEFAULT 0,
  user_id TEXT,
  repo_id INTEGER,
  repo_full_name TEXT,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE
);

CREATE INDEX idx_sync_subscription_events_task_id_created_at
  ON sync_subscription_events(task_id, created_at, id);
CREATE INDEX idx_sync_subscription_events_user_id_created_at
  ON sync_subscription_events(user_id, created_at DESC);
CREATE INDEX idx_sync_subscription_events_repo_id_created_at
  ON sync_subscription_events(repo_id, created_at DESC);
CREATE INDEX idx_sync_subscription_events_repo_full_name_created_at
  ON sync_subscription_events(repo_full_name, created_at DESC);

CREATE TABLE translation_requests (
  id TEXT PRIMARY KEY,
  mode TEXT NOT NULL,
  source TEXT NOT NULL,
  requested_by TEXT,
  scope_user_id TEXT NOT NULL,
  status TEXT NOT NULL,
  item_count INTEGER NOT NULL DEFAULT 0,
  completed_item_count INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (mode IN ('async', 'wait', 'stream')),
  CHECK (status IN ('queued', 'running', 'completed', 'failed')),
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(scope_user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE translation_request_items (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL,
  producer_ref TEXT NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  max_wait_ms INTEGER NOT NULL,
  source_hash TEXT NOT NULL,
  source_blocks_json TEXT NOT NULL,
  target_slots_json TEXT NOT NULL,
  work_item_id TEXT,
  result_status TEXT,
  title_zh TEXT,
  summary_md TEXT,
  body_md TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  FOREIGN KEY(request_id) REFERENCES translation_requests(id) ON DELETE CASCADE
);

CREATE INDEX idx_translation_request_items_request_id
  ON translation_request_items(request_id, created_at, id);
CREATE INDEX idx_translation_request_items_work_item_id
  ON translation_request_items(work_item_id);

CREATE TABLE translation_work_items (
  id TEXT PRIMARY KEY,
  dedupe_key TEXT NOT NULL UNIQUE,
  scope_user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  source_blocks_json TEXT NOT NULL,
  target_slots_json TEXT NOT NULL,
  token_estimate INTEGER NOT NULL,
  deadline_at TEXT NOT NULL,
  status TEXT NOT NULL,
  batch_id TEXT,
  result_status TEXT,
  title_zh TEXT,
  summary_md TEXT,
  body_md TEXT,
  error_text TEXT,
  cache_hit INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (status IN ('queued', 'batched', 'running', 'completed', 'failed')),
  CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  FOREIGN KEY(scope_user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_translation_work_items_status_partition
  ON translation_work_items(status, target_lang, protocol_version, model_profile, created_at ASC);
CREATE INDEX idx_translation_work_items_deadline
  ON translation_work_items(status, deadline_at ASC);
CREATE INDEX idx_translation_work_items_batch_id
  ON translation_work_items(batch_id);

CREATE TABLE translation_work_watchers (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  request_item_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(work_item_id, request_item_id),
  FOREIGN KEY(work_item_id) REFERENCES translation_work_items(id) ON DELETE CASCADE,
  FOREIGN KEY(request_item_id) REFERENCES translation_request_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_translation_work_watchers_request_item_id
  ON translation_work_watchers(request_item_id);

CREATE TABLE translation_batches (
  id TEXT PRIMARY KEY,
  partition_key TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  trigger_reason TEXT NOT NULL,
  item_count INTEGER NOT NULL,
  estimated_input_tokens INTEGER NOT NULL,
  status TEXT NOT NULL,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (status IN ('queued', 'running', 'completed', 'failed'))
);

CREATE INDEX idx_translation_batches_status_created_at
  ON translation_batches(status, created_at DESC);
CREATE INDEX idx_translation_batches_target_lang_created_at
  ON translation_batches(target_lang, created_at DESC);

CREATE TABLE translation_batch_items (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  item_index INTEGER NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  producer_count INTEGER NOT NULL DEFAULT 0,
  token_estimate INTEGER NOT NULL,
  result_status TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(batch_id, work_item_id),
  CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  FOREIGN KEY(batch_id) REFERENCES translation_batches(id) ON DELETE CASCADE,
  FOREIGN KEY(work_item_id) REFERENCES translation_work_items(id) ON DELETE CASCADE
);

CREATE INDEX idx_translation_batch_items_batch_id
  ON translation_batch_items(batch_id, item_index ASC);
