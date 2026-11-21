-- Forward repair: the imported legacy schema stores flag columns as BIGINT
-- 0/1 (the SQLite translation), while baseline 0006-0008 declared them BOOLEAN.
-- The application reads and writes these as integers, so normalize the type on
-- databases that already hold the imported schema. Fresh databases get the same
-- shape because this migration runs after the baseline at bootstrap.
DO $$
DECLARE
    target record;
BEGIN
    FOR target IN
        SELECT * FROM (VALUES
            ('content_work_items', 'cache_hit'),
            ('translation_work_items', 'cache_hit'),
            ('translation_attempt_events', 'retry_eligible'),
            ('llm_recovery_flags', 'llm_recovery_enabled')
        ) AS candidates(table_name, column_name)
    LOOP
        IF EXISTS (
            SELECT 1
            FROM information_schema.columns existing
            WHERE existing.table_schema = 'public'
              AND existing.table_name = target.table_name
              AND existing.column_name = target.column_name
              AND existing.data_type = 'boolean'
        ) THEN
            -- A boolean default cannot be cast implicitly, so drop it, change
            -- the type, then restore the equivalent 0 default.
            EXECUTE format(
                'ALTER TABLE public.%I ALTER COLUMN %I DROP DEFAULT',
                target.table_name,
                target.column_name
            );
            EXECUTE format(
                'ALTER TABLE public.%I ALTER COLUMN %I TYPE bigint USING (%I::int)::bigint',
                target.table_name,
                target.column_name,
                target.column_name
            );
            EXECUTE format(
                'ALTER TABLE public.%I ALTER COLUMN %I SET DEFAULT 0',
                target.table_name,
                target.column_name
            );
        END IF;
    END LOOP;
END
$$;
