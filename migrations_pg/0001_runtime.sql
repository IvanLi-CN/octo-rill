-- PostgreSQL-only runtime baseline. SQLite _sqlx_migrations is intentionally
-- not reused; this history belongs to the PostgreSQL deployment.
CREATE SCHEMA IF NOT EXISTS octo_rill;
SET search_path TO octo_rill, public;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version BIGINT PRIMARY KEY,
    checksum BYTEA NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Identity and account tables are part of the PostgreSQL runtime baseline.
-- They must exist before later migrations (for example translation and brief
-- scope tables) add foreign keys to users.
CREATE TABLE IF NOT EXISTS public.users (
    id TEXT PRIMARY KEY,
    github_user_id BIGINT UNIQUE,
    login TEXT,
    name TEXT,
    avatar_url TEXT,
    email TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    is_admin BIGINT NOT NULL DEFAULT 0,
    is_disabled BIGINT NOT NULL DEFAULT 0,
    daily_brief_utc_time TEXT NOT NULL DEFAULT '00:00',
    daily_brief_local_time TEXT,
    daily_brief_time_zone TEXT,
    daily_brief_schedule_local_time TEXT NOT NULL DEFAULT '06:00',
    include_own_releases BIGINT NOT NULL DEFAULT 0,
    last_active_at TEXT,
    paused_at TEXT,
    passkey_user_handle_uuid TEXT UNIQUE,
    webhook_push_enabled BIGINT NOT NULL DEFAULT 0,
    webhook_push_secret_ciphertext BYTEA,
    webhook_push_secret_nonce BYTEA,
    webhook_push_callback_key TEXT UNIQUE,
    webhook_push_desired_state TEXT NOT NULL DEFAULT 'deleted',
    webhook_push_last_completed_check_at TEXT,
    CONSTRAINT users_webhook_push_enabled_ck CHECK (webhook_push_enabled IN (0, 1)),
    CONSTRAINT users_webhook_push_desired_state_ck CHECK (webhook_push_desired_state IN ('enabled', 'paused', 'deleted'))
);

CREATE INDEX IF NOT EXISTS idx_users_pause_maintenance
    ON public.users (is_disabled, paused_at, last_active_at, created_at);

CREATE TABLE IF NOT EXISTS public.user_tokens (
    user_id TEXT PRIMARY KEY REFERENCES public.users(id) ON DELETE CASCADE,
    access_token_ciphertext BYTEA NOT NULL,
    access_token_nonce BYTEA NOT NULL,
    scopes TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS public.github_connections (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    github_user_id BIGINT NOT NULL UNIQUE,
    login TEXT NOT NULL,
    name TEXT,
    avatar_url TEXT,
    email TEXT,
    access_token_ciphertext BYTEA NOT NULL,
    access_token_nonce BYTEA NOT NULL,
    scopes TEXT NOT NULL,
    linked_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_github_connections_user_login
    ON public.github_connections(user_id, login);

CREATE TABLE IF NOT EXISTS public.linuxdo_connections (
    user_id TEXT PRIMARY KEY REFERENCES public.users(id) ON DELETE CASCADE,
    linuxdo_user_id BIGINT NOT NULL UNIQUE,
    username TEXT NOT NULL,
    name TEXT,
    avatar_url TEXT,
    trust_level BIGINT NOT NULL DEFAULT 0,
    active BIGINT NOT NULL DEFAULT 1,
    silenced BIGINT NOT NULL DEFAULT 0,
    linked_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS public.user_passkeys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    label TEXT NOT NULL,
    passkey_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_used_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_user_passkeys_user_created_at
    ON public.user_passkeys(user_id, created_at, id);

CREATE TABLE IF NOT EXISTS public.user_api_keys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    key_ciphertext BYTEA NOT NULL,
    key_nonce BYTEA NOT NULL,
    key_prefix TEXT NOT NULL,
    masked_key TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    revoked_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_user_api_keys_user_created_at
    ON public.user_api_keys(user_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_user_api_keys_hash_active
    ON public.user_api_keys(key_hash) WHERE revoked_at IS NULL;

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
