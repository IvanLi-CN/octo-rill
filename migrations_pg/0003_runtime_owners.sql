CREATE TABLE IF NOT EXISTS public.runtime_owners (
    runtime_owner_id TEXT PRIMARY KEY,
    lease_heartbeat_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_runtime_owners_pg_lease_heartbeat
    ON public.runtime_owners(lease_heartbeat_at);
