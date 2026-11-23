-- Imported SQLite snapshots carry `translation_attempt_events` without the
-- baseline `UNIQUE (work_item_id, attempt_no, event_type)` key: the table
-- pre-existed, so `CREATE TABLE IF NOT EXISTS` in the baseline skipped it.
-- The PostgreSQL runtime writes attempt events with
-- `ON CONFLICT (work_item_id, attempt_no, event_type) DO NOTHING`, which fails
-- on every `translation.generate` task without this key.
--
-- Fresh databases already get an index with this name from the baseline unique
-- constraint, so `IF NOT EXISTS` only repairs imported schemas.
CREATE UNIQUE INDEX IF NOT EXISTS translation_attempt_events_work_item_id_attempt_no_event_type_key
    ON public.translation_attempt_events (work_item_id, attempt_no, event_type);
