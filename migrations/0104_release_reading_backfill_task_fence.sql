ALTER TABLE release_reading_backfill_runs
  ADD COLUMN lease_task_id TEXT;

CREATE INDEX IF NOT EXISTS idx_release_reading_backfill_runs_task_lease
  ON release_reading_backfill_runs(lease_task_id, status);
