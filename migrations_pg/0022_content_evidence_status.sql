-- `content_work_items` was created by the runtime baseline migration without the
-- Release-evidence columns that the imported production schema already had.
-- PostgreSQL imports therefore carry them, while a fresh deployment from the
-- migration set does not, and any reader or worker that selects them fails.
--
-- Additive and idempotent: existing imported rows keep their values and the
-- default only applies to new rows.
ALTER TABLE public.content_work_items
    ADD COLUMN IF NOT EXISTS evidence_snapshot_id text;

ALTER TABLE public.content_work_items
    ADD COLUMN IF NOT EXISTS evidence_status text NOT NULL DEFAULT 'not_required';
