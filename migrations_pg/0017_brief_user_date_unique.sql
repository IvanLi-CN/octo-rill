-- The daily-brief executor upserts one row per (user, date) with
-- `ON CONFLICT (user_id, date)`. The legacy SQLite table enforced that
-- uniqueness until the window-snapshot rebuild replaced it with a partial
-- window index, so neither a fresh nor an imported database carries a matching
-- arbiter and every scheduled brief failed with 42P10.
--
-- A legacy snapshot can hold two rows for one (user, date) when a user changed
-- their brief time zone or local time: the window key changes while the display
-- date does not. Briefs are generated read models, so keep the newest row for
-- each pair before adding the index; otherwise the unique index would abort
-- startup for the whole service.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'briefs'
          AND column_name = 'date'
    ) THEN
        DELETE FROM public.briefs AS older
        USING public.briefs AS newer
        WHERE older.user_id = newer.user_id
          AND older.date = newer.date
          AND (older.updated_at, older.id) < (newer.updated_at, newer.id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_briefs_user_date_unique
            ON public.briefs USING btree (user_id, date);
    END IF;
END
$$;
