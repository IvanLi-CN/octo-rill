-- Keep historical migration progress readable after import. This does not
-- activate SQLite migration executors on PostgreSQL.
CREATE TABLE IF NOT EXISTS public.online_migration_runs (
    migration_id TEXT PRIMARY KEY,
    definition_checksum TEXT NOT NULL,
    status TEXT NOT NULL,
    pause_requested BIGINT NOT NULL DEFAULT 0,
    owner_id TEXT,
    lease_heartbeat_at TEXT,
    last_error TEXT,
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS public.online_migration_operations (
    migration_id TEXT NOT NULL REFERENCES public.online_migration_runs(migration_id),
    operation_id TEXT NOT NULL,
    definition_checksum TEXT NOT NULL,
    operation_kind TEXT NOT NULL,
    operation_order BIGINT NOT NULL,
    status TEXT NOT NULL,
    cursor TEXT NOT NULL DEFAULT '',
    rows_processed BIGINT NOT NULL DEFAULT 0,
    last_error TEXT,
    owner_id TEXT,
    lease_heartbeat_at TEXT,
    started_at TEXT,
    completed_at TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(migration_id,operation_id)
);
