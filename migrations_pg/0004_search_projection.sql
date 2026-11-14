-- PostgreSQL search projection. SQLite FTS5 shadow tables are intentionally
-- not imported; these generated vectors are rebuilt from authoritative rows.
CREATE TABLE IF NOT EXISTS public.search_documents (
    id TEXT PRIMARY KEY,
    user_id TEXT,
    resource_type TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    repo_id BIGINT,
    repo_full_name TEXT,
    owner_login TEXT,
    title TEXT,
    body TEXT,
    translated_text TEXT,
    smart_text TEXT,
    source_time TEXT,
    unread BIGINT,
    target_path TEXT,
    target_url TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    search_vector TSVECTOR GENERATED ALWAYS AS (
        to_tsvector('octo_rill_simple'::regconfig,
            coalesce(title, '') || ' ' || coalesce(body, '') || ' ' ||
            coalesce(repo_full_name, '') || ' ' || coalesce(translated_text, '') || ' ' ||
            coalesce(smart_text, ''))
    ) STORED,
    UNIQUE (user_id, resource_type, resource_id)
);

CREATE INDEX IF NOT EXISTS idx_search_documents_pg_user_type_time
    ON public.search_documents(user_id, resource_type, source_time DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_search_documents_pg_vector
    ON public.search_documents USING GIN(search_vector);
CREATE INDEX IF NOT EXISTS idx_search_documents_pg_trgm_title
    ON public.search_documents USING GIN(title gin_trgm_ops);

CREATE TABLE IF NOT EXISTS public.search_document_user_lanes (
    document_id TEXT NOT NULL REFERENCES public.search_documents(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    translated_text TEXT,
    smart_text TEXT,
    updated_at TEXT NOT NULL,
    search_vector TSVECTOR GENERATED ALWAYS AS (
        to_tsvector('octo_rill_simple'::regconfig,
            coalesce(translated_text, '') || ' ' || coalesce(smart_text, ''))
    ) STORED,
    PRIMARY KEY (document_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_search_lanes_pg_vector
    ON public.search_document_user_lanes USING GIN(search_vector);
