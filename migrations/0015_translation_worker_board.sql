ALTER TABLE translation_requests
  ADD COLUMN request_origin TEXT NOT NULL DEFAULT 'user'
  CHECK (request_origin IN ('user', 'system'));

ALTER TABLE translation_batches
  ADD COLUMN worker_slot INTEGER NOT NULL DEFAULT 0;

ALTER TABLE translation_batches
  ADD COLUMN request_count INTEGER NOT NULL DEFAULT 0;

UPDATE translation_batches
SET request_count = COALESCE((
  SELECT COUNT(DISTINCT tri.request_id)
  FROM translation_batch_items tbi
  JOIN translation_work_watchers tw ON tw.work_item_id = tbi.work_item_id
  JOIN translation_request_items tri ON tri.id = tw.request_item_id
  WHERE tbi.batch_id = translation_batches.id
), 0);
