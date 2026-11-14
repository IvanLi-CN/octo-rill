-- PostgreSQL-only runtime baseline. SQLite _sqlx_migrations is intentionally
-- not reused; this history belongs to the PostgreSQL deployment.
CREATE SCHEMA IF NOT EXISTS octo_rill;
SET search_path TO octo_rill, public;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version BIGINT PRIMARY KEY,
    checksum BYTEA NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS tower_sessions (
    id TEXT PRIMARY KEY,
    data BYTEA NOT NULL,
    expiry_date TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tower_sessions_expiry ON tower_sessions (expiry_date);

CREATE TABLE IF NOT EXISTS migration_manifests (
    id BIGSERIAL PRIMARY KEY,
    source_snapshot TEXT NOT NULL,
    manifest JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_catalog.pg_ts_config WHERE cfgname = 'octo_rill_simple') THEN
        CREATE TEXT SEARCH CONFIGURATION octo_rill_simple (COPY = simple);
    END IF;
END
$$;

CREATE EXTENSION IF NOT EXISTS pg_trgm;
