CREATE TABLE admin_runtime_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  llm_max_concurrency INTEGER NOT NULL,
  translation_general_worker_concurrency INTEGER NOT NULL,
  translation_dedicated_worker_concurrency INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (llm_max_concurrency > 0),
  CHECK (translation_general_worker_concurrency > 0),
  CHECK (translation_dedicated_worker_concurrency > 0)
);

ALTER TABLE translation_batches
  ADD COLUMN worker_id TEXT NOT NULL DEFAULT '';

ALTER TABLE translation_batches
  ADD COLUMN worker_kind TEXT NOT NULL DEFAULT 'general'
  CHECK (worker_kind IN ('general', 'user_dedicated'));

UPDATE translation_batches
SET
  worker_id = CASE
    WHEN worker_slot = 4 THEN 'translation-worker-user-dedicated-1'
    ELSE 'translation-worker-general-' || CAST(worker_slot AS TEXT)
  END,
  worker_kind = CASE
    WHEN worker_slot = 4 THEN 'user_dedicated'
    ELSE 'general'
  END;
