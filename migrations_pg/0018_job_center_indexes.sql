-- Read indexes for the administrator job center.
--
-- The legacy SQLite runtime serves the same surface from
-- `idx_job_tasks_finished_status`, `idx_job_tasks_realtime_created` and
-- `idx_job_tasks_task_type_created_at`. Without them the PostgreSQL list sorts
-- the whole 216k-row task history for every page, and the SSE stream rescans
-- 2.6M task events on every poll. These index shapes mirror the legacy intent
-- with the PostgreSQL column types.
CREATE INDEX IF NOT EXISTS idx_job_tasks_pg_realtime_created
    ON public.job_tasks (created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_job_tasks_pg_task_type_created
    ON public.job_tasks (task_type, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_job_tasks_pg_finished_status
    ON public.job_tasks (status, finished_at DESC, id DESC)
    WHERE finished_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_job_task_events_pg_stream
    ON public.job_task_events (created_at, id);

-- The subscription diagnostics list/detail reads bound events per task with a
-- newest-first window; the legacy schema had this exact index.
CREATE INDEX IF NOT EXISTS idx_sync_subscription_events_pg_task_created
    ON public.sync_subscription_events (task_id, created_at DESC, id DESC);
