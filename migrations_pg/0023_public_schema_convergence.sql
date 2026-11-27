-- Converge the runtime on the single `public` schema.
--
-- Before this migration the runtime owned two schemas: `public` held every live
-- application row, while `octo_rill` held the runtime migration history, the
-- session table, the import manifest and a set of stale duplicates whose
-- `content_processing_control` row even disagreed with `public`.
--
-- Order matters:
--   1. move the runtime migration history, so this migration is the last row
--      written under the old schema;
--   2. move the session table and the import manifest (spreadsheet evidence);
--   3. drop the stale duplicates;
--   4. drop the now-empty schema.
--
-- `octo_rill._sqlx_migrations` is moved with `ALTER TABLE ... SET SCHEMA`, so the
-- history rows survive untouched and no version is replayed.

DO $$
BEGIN
    IF to_regclass('public._sqlx_migrations') IS NULL
       AND to_regclass('octo_rill._sqlx_migrations') IS NOT NULL THEN
        EXECUTE 'ALTER TABLE octo_rill._sqlx_migrations SET SCHEMA public';
    END IF;

    IF to_regclass('public.tower_sessions') IS NULL
       AND to_regclass('octo_rill.tower_sessions') IS NOT NULL THEN
        EXECUTE 'ALTER TABLE octo_rill.tower_sessions SET SCHEMA public';
    END IF;

    IF to_regclass('public.migration_manifests') IS NULL
       AND to_regclass('octo_rill.migration_manifests') IS NOT NULL THEN
        EXECUTE 'ALTER TABLE octo_rill.migration_manifests SET SCHEMA public';
    END IF;
END
$$;

-- Stale duplicates. Every one of these either has no rows or duplicates a
-- `public` table that the runtime actually reads; `content_processing_control`
-- and `llm_recovery_flags` were the two with rows, and both disagree with the
-- authoritative `public` row, so they are dropped rather than merged.
DROP TABLE IF EXISTS octo_rill.content_processing_control;
DROP TABLE IF EXISTS octo_rill.llm_recovery_flags;
DROP TABLE IF EXISTS octo_rill.llm_model_health;
DROP TABLE IF EXISTS octo_rill.content_work_items;
DROP TABLE IF EXISTS octo_rill.content_batches;
DROP TABLE IF EXISTS octo_rill.content_batch_items;
DROP TABLE IF EXISTS octo_rill.content_result_projections;
DROP TABLE IF EXISTS octo_rill.translation_work_items;
DROP TABLE IF EXISTS octo_rill.translation_requests;
DROP TABLE IF EXISTS octo_rill.translation_batches;
DROP TABLE IF EXISTS octo_rill.translation_batch_items;
DROP TABLE IF EXISTS octo_rill.translation_attempt_events;
DROP TABLE IF EXISTS octo_rill.translation_attempt_llm_calls;
DROP TABLE IF EXISTS octo_rill.llm_diagnostic_access_audit;
DROP TABLE IF EXISTS octo_rill.briefs;
DROP TABLE IF EXISTS octo_rill.brief_release_memberships;
DROP TABLE IF EXISTS octo_rill.schema_migrations;

-- Two generated columns still resolve their text search configuration through
-- the retired schema: they were created while `search_path` was
-- `octo_rill, public`, so `'octo_rill_simple'::regconfig` bound to
-- `octo_rill.octo_rill_simple`. The expression cannot be altered in place, so
-- rebuild the column against the `public` configuration and restore its index.
DO $$
DECLARE
    has_public_config boolean;
BEGIN
    SELECT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_ts_config config
        JOIN pg_catalog.pg_namespace namespace ON namespace.oid = config.cfgnamespace
        WHERE config.cfgname = 'octo_rill_simple' AND namespace.nspname = 'public'
    ) INTO has_public_config;

    IF has_public_config THEN
        IF EXISTS (
            SELECT 1 FROM pg_attribute
            WHERE attrelid = to_regclass('public.search_documents')
              AND attname = 'search_vector' AND attgenerated <> ''
        ) THEN
            ALTER TABLE public.search_documents DROP COLUMN search_vector;
            ALTER TABLE public.search_documents
                ADD COLUMN search_vector TSVECTOR GENERATED ALWAYS AS (
                    to_tsvector('public.octo_rill_simple'::regconfig,
                        coalesce(title, '') || ' ' || coalesce(body, '') || ' ' ||
                        coalesce(repo_full_name, '') || ' ' || coalesce(translated_text, '') || ' ' ||
                        coalesce(smart_text, ''))
                ) STORED;
        END IF;

        IF EXISTS (
            SELECT 1 FROM pg_attribute
            WHERE attrelid = to_regclass('public.search_document_user_lanes')
              AND attname = 'search_vector' AND attgenerated <> ''
        ) THEN
            ALTER TABLE public.search_document_user_lanes DROP COLUMN search_vector;
            ALTER TABLE public.search_document_user_lanes
                ADD COLUMN search_vector TSVECTOR GENERATED ALWAYS AS (
                    to_tsvector('public.octo_rill_simple'::regconfig,
                        coalesce(translated_text, '') || ' ' || coalesce(smart_text, ''))
                ) STORED;
        END IF;
    END IF;
END
$$;

CREATE INDEX IF NOT EXISTS idx_search_documents_pg_vector
    ON public.search_documents USING gin (search_vector);
CREATE INDEX IF NOT EXISTS idx_search_lanes_pg_vector
    ON public.search_document_user_lanes USING gin (search_vector);

-- The retired configuration is now unused: both generated columns above bind to
-- `public.octo_rill_simple`. Dropping it is what lets the schema go.
DROP TEXT SEARCH CONFIGURATION IF EXISTS octo_rill.octo_rill_simple;

-- `pg_trgm` was installed into `octo_rill` by baseline 0001, so the extension
-- itself keeps the schema alive. Relocate it to `public` (the extension holds
-- no application data) and rebuild the trigram index against the public
-- operator class, because the index binds to `octo_rill.gin_trgm_ops`.
DROP INDEX IF EXISTS public.idx_search_documents_pg_trgm_title;
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_extension extension
        JOIN pg_namespace namespace ON namespace.oid = extension.extnamespace
        WHERE extension.extname = 'pg_trgm' AND namespace.nspname = 'octo_rill'
    ) THEN
        EXECUTE 'ALTER EXTENSION pg_trgm SET SCHEMA public';
    END IF;
END
$$;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX IF NOT EXISTS idx_search_documents_pg_trgm_title
    ON public.search_documents USING gin (title public.gin_trgm_ops);

DO $$
BEGIN
    IF to_regclass('octo_rill._sqlx_migrations') IS NULL
       AND to_regclass('octo_rill.tower_sessions') IS NULL
       AND to_regclass('octo_rill.migration_manifests') IS NULL THEN
        EXECUTE 'DROP SCHEMA IF EXISTS octo_rill';
    END IF;
END
$$;
