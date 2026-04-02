ALTER TABLE ai_translations
  ADD COLUMN status TEXT NOT NULL DEFAULT 'ready';

ALTER TABLE ai_translations
  ADD COLUMN error_text TEXT;

ALTER TABLE ai_translations
  ADD COLUMN active_work_item_id TEXT;

UPDATE ai_translations
SET status = 'ready'
WHERE status IS NULL OR trim(status) = '';

CREATE INDEX IF NOT EXISTS idx_ai_translations_active_work_item_id
  ON ai_translations(active_work_item_id);
