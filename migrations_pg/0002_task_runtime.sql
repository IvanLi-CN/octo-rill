-- PostgreSQL task coordination tables. The importer also creates these tables
-- when copying a production SQLite snapshot; IF NOT EXISTS keeps both paths
-- idempotent.
CREATE TABLE IF NOT EXISTS public.job_tasks (
    id TEXT PRIMARY KEY,
    task_type TEXT NOT NULL,
    status TEXT NOT NULL,
    source TEXT NOT NULL,
    requested_by TEXT,
    parent_task_id TEXT REFERENCES public.job_tasks(id) ON DELETE SET NULL,
    payload_json TEXT NOT NULL,
    result_json TEXT,
    error_message TEXT,
    cancel_requested BIGINT NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    runtime_owner_id TEXT,
    lease_heartbeat_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_job_tasks_pg_status_created_at
    ON public.job_tasks(status, created_at, id);
CREATE INDEX IF NOT EXISTS idx_job_tasks_pg_type_status
    ON public.job_tasks(task_type, status);

CREATE TABLE IF NOT EXISTS public.job_task_events (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES public.job_tasks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_job_task_events_pg_task_id_id
    ON public.job_task_events(task_id, created_at, id);
