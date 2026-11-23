-- The imported SQLite snapshot stores model-health timestamps as TEXT while the
-- baseline declares TIMESTAMPTZ. PostgreSQL translation routing reads and writes
-- these columns directly, so normalize the imported shape instead of carrying a
-- second timestamp representation.
--
-- The import only accepts RFC3339 snapshots, and the 2026-11-23 production
-- audit found zero invalid values before this migration.
DO $$
BEGIN
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
          AND data_type IN ('text', 'character varying')
    ) = 5 THEN
        ALTER TABLE public.llm_model_health
            ALTER COLUMN window_started_at TYPE timestamptz
                USING NULLIF(window_started_at, '')::timestamptz,
            ALTER COLUMN cooldown_until TYPE timestamptz
                USING NULLIF(cooldown_until, '')::timestamptz,
            ALTER COLUMN last_failure_at TYPE timestamptz
                USING NULLIF(last_failure_at, '')::timestamptz,
            ALTER COLUMN created_at TYPE timestamptz
                USING created_at::timestamptz,
            ALTER COLUMN updated_at TYPE timestamptz
                USING updated_at::timestamptz;
    END IF;
END
$$;
