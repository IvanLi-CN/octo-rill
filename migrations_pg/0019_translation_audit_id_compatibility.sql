-- Normalize imported translation audit ids to the baseline text shape.
--
-- The imported SQLite snapshot represents these primary keys as BIGINT, while
-- the PostgreSQL runtime generates `local_id` text values for the same columns.
-- Without this repair, every attempt insert fails with
-- `column "id" is of type bigint but expression is of type text`, which makes
-- the `translation.generate` executor fail before the first provider call.
-- Both tables are audit-only; no other table references these ids.
DO $$
DECLARE
    target record;
BEGIN
    FOR target IN
        SELECT * FROM (VALUES
            ('translation_attempt_events'),
            ('translation_attempt_llm_calls')
        ) AS candidates(table_name)
    LOOP
        IF EXISTS (
            SELECT 1
            FROM information_schema.columns existing
            WHERE existing.table_schema = 'public'
              AND existing.table_name = target.table_name
              AND existing.column_name = 'id'
              AND existing.data_type IN ('bigint', 'integer', 'smallint')
        ) THEN
            EXECUTE format(
                'ALTER TABLE public.%I ALTER COLUMN id TYPE text USING id::text',
                target.table_name
            );
        END IF;
    END LOOP;
END
$$;
