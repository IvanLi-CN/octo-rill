-- Intentionally rebuild the translation scheduler tables from scratch; runtime history is discarded.
PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS translation_batch_items;
DROP TABLE IF EXISTS translation_batches;
DROP TABLE IF EXISTS translation_work_watchers;
DROP TABLE IF EXISTS translation_request_items;
DROP TABLE IF EXISTS translation_requests;
DROP TABLE IF EXISTS translation_work_items;

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

CREATE TABLE translation_requests (
  id TEXT PRIMARY KEY,
  mode TEXT NOT NULL,
  source TEXT NOT NULL,
  request_origin TEXT NOT NULL CHECK (request_origin IN ('user', 'system')),
  requested_by TEXT,
  scope_user_id TEXT NOT NULL,
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
  status TEXT NOT NULL,
  result_status TEXT,
  title_zh TEXT,
  summary_md TEXT,
  body_md TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (mode IN ('async', 'wait', 'stream')),
  CHECK (status IN ('queued', 'running', 'completed', 'failed')),
  CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(scope_user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY(work_item_id) REFERENCES translation_work_items(id) ON DELETE SET NULL
);

CREATE INDEX idx_translation_requests_status_updated_at
  ON translation_requests(status, updated_at DESC, id DESC);
CREATE INDEX idx_translation_requests_work_item_id
  ON translation_requests(work_item_id);
CREATE INDEX idx_translation_requests_origin_work_item_id
  ON translation_requests(request_origin, work_item_id);
CREATE INDEX idx_translation_requests_scope_created_at
  ON translation_requests(scope_user_id, created_at DESC);

CREATE TABLE translation_batches (
  id TEXT PRIMARY KEY,
  partition_key TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  trigger_reason TEXT NOT NULL,
  worker_slot INTEGER NOT NULL,
  request_count INTEGER NOT NULL,
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

PRAGMA foreign_keys = ON;
