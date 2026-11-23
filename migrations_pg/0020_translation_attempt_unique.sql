-- Imported SQLite snapshots carry `translation_attempt_events` without the
-- baseline `UNIQUE (work_item_id, attempt_no, event_type)` key: the table
-- pre-existed, so `CREATE TABLE IF NOT EXISTS` in the baseline skipped it.
-- The PostgreSQL runtime writes attempt events with
-- `ON CONFLICT (work_item_id, attempt_no, event_type) DO NOTHING`, which fails
-- on every `translation.generate` task without this key.
--
-- The baseline constraint's index is named by PostgreSQL's identifier
-- truncation: `translation_attempt_events_work_item_id_attempt_no_event_ty_key`
-- (63 bytes). Using that exact name keeps `IF NOT EXISTS` a true no-op on fresh
-- databases instead of adding a second identical index.
--
-- The legacy SQLite table never enforced this key, and its writer can record
-- the same `attempt_queued` triple twice before an attempt starts. Keep the
-- newest row per triple before creating the index; otherwise a duplicate in an
-- imported snapshot aborts startup for the whole service (same repair shape as
-- `0017_brief_user_date_unique.sql`).
DO $$
BEGIN
    DELETE FROM public.translation_attempt_events AS older
    USING public.translation_attempt_events AS newer
    WHERE older.work_item_id = newer.work_item_id
      AND older.attempt_no = newer.attempt_no
      AND older.event_type = newer.event_type
      AND (older.created_at, older.id) < (newer.created_at, newer.id);

    CREATE UNIQUE INDEX IF NOT EXISTS translation_attempt_events_work_item_id_attempt_no_event_ty_key
        ON public.translation_attempt_events (work_item_id, attempt_no, event_type);
END
$$;
