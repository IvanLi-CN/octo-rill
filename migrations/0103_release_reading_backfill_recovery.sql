-- Keep release deletes point-addressable for the backfill item's cascading FK.
CREATE INDEX IF NOT EXISTS idx_release_reading_backfill_items_release
  ON release_reading_backfill_items(release_id, run_id);
