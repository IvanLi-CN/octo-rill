-- PostgreSQL 18 baseline for the persistence/search cutover.
-- Business tables are imported by the migration tool; shared tables are
-- created first so the service can rebuild search immediately.

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    include_own_releases BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS user_release_visible_repos (
    user_id TEXT NOT NULL,
    repo_id BIGINT NOT NULL,
    full_name TEXT,
    owner_login TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, repo_id)
);

CREATE TABLE IF NOT EXISTS search_documents (
    id TEXT PRIMARY KEY,
    user_id TEXT,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    repo_id BIGINT,
    repo_full_name TEXT,
    owner_login TEXT,
    title TEXT,
    body TEXT,
    translated_text TEXT,
    smart_text TEXT,
    source_time TIMESTAMPTZ,
    target_path TEXT,
    target_url TEXT,
    unread BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    search_vector TSVECTOR GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', coalesce(title, '')), 'A') ||
        setweight(to_tsvector('simple', coalesce(body, '')), 'B') ||
        setweight(to_tsvector('simple', coalesce(repo_full_name, '')), 'A') ||
        setweight(to_tsvector('simple', coalesce(translated_text, '')), 'C') ||
        setweight(to_tsvector('simple', coalesce(smart_text, '')), 'C')
    ) STORED
);

CREATE INDEX IF NOT EXISTS idx_search_documents_vector ON search_documents USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_search_documents_user_time ON search_documents (user_id, source_time DESC, id DESC);

CREATE TABLE IF NOT EXISTS search_document_user_lanes (
    document_id TEXT NOT NULL REFERENCES search_documents(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    translated_text TEXT,
    smart_text TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (document_id, user_id)
);

CREATE TABLE IF NOT EXISTS search_rate_limits (
    user_id TEXT PRIMARY KEY,
    window_started_at BIGINT NOT NULL,
    request_count BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE SCHEMA IF NOT EXISTS tower_sessions;
CREATE TABLE IF NOT EXISTS tower_sessions.session (
    id TEXT PRIMARY KEY,
    data BYTEA NOT NULL,
    expiry_date TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS tower_sessions_expiry_idx ON tower_sessions.session (expiry_date);
