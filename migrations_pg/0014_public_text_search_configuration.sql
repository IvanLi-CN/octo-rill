-- Forward repair: baseline 0001 created the text search configuration while the
-- session search_path targeted octo_rill, so runtime connections that only see
-- public could not resolve 'octo_rill_simple'. Own the configuration in public
-- and keep the existing octo_rill copy untouched for generated columns that
-- already bind to it.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_ts_config config
        JOIN pg_catalog.pg_namespace namespace ON namespace.oid = config.cfgnamespace
        WHERE config.cfgname = 'octo_rill_simple' AND namespace.nspname = 'public'
    ) THEN
        CREATE TEXT SEARCH CONFIGURATION public.octo_rill_simple (COPY = pg_catalog.simple);
    END IF;
END
$$;
