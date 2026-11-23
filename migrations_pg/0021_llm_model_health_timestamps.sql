-- The imported SQLite snapshot stores model-health timestamps as TEXT while the
-- baseline declares TIMESTAMPTZ. PostgreSQL translation routing reads and writes
-- these columns directly, so normalize the imported timestamp representation
-- instead of carrying a second representation.
--
-- The import only accepts RFC3339 snapshots, and the 2026-11-23 production
-- audit found zero invalid values before this migration.
DO $$
DECLARE
    target_column text;
BEGIN
    IF to_regclass('public.llm_model_health') IS NULL THEN
        RETURN;
    END IF;

    FOREACH target_column IN ARRAY ARRAY[
        'window_started_at',
        'cooldown_until',
        'last_failure_at',
        'created_at',
        'updated_at'
    ] LOOP
        IF EXISTS (
            SELECT 1
            FROM information_schema.columns c
            WHERE c.table_schema = 'public'
              AND c.table_name = 'llm_model_health'
              AND c.column_name = target_column
              AND c.data_type IN ('text', 'character varying')
        ) THEN
            IF target_column IN ('created_at', 'updated_at') THEN
                EXECUTE format(
                    'ALTER TABLE public.llm_model_health ALTER COLUMN %I TYPE timestamptz USING %I::timestamptz',
                    target_column,
                    target_column
                );
            ELSE
                EXECUTE format(
                    'ALTER TABLE public.llm_model_health ALTER COLUMN %I TYPE timestamptz USING NULLIF(%I, '''')::timestamptz',
                    target_column,
                    target_column
                );
            END IF;
        END IF;
    END LOOP;

    IF (
        SELECT count(*)
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'llm_model_health'
          AND column_name IN (
              'window_started_at',
              'cooldown_until',
              'last_failure_at',
              'created_at',
              'updated_at'
          )
          AND data_type = 'timestamp with time zone'
    ) <> 5 THEN
        RAISE EXCEPTION 'llm_model_health timestamp normalization incomplete';
    END IF;
END
$$;
