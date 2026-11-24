-- `content_work_items` was created by the runtime baseline migration without the
-- Release-evidence columns that the imported production schema already had.
-- PostgreSQL imports therefore carry them, while a fresh deployment from the
-- migration set does not, and any reader or worker that selects them fails.
--
-- Additive and idempotent: existing imported rows keep their values. Imported
-- rows can also carry NULL or a different default, which the claim predicate
-- (`evidence_status IN ('not_required', 'ready')`) would otherwise skip
-- forever, so the column is normalised before the constraint is applied.
ALTER TABLE public.content_work_items
    ADD COLUMN IF NOT EXISTS evidence_snapshot_id text;

ALTER TABLE public.content_work_items
    ADD COLUMN IF NOT EXISTS evidence_status text;

UPDATE public.content_work_items
    SET evidence_status = 'not_required'
    WHERE evidence_status IS NULL;

ALTER TABLE public.content_work_items
    ALTER COLUMN evidence_status SET DEFAULT 'not_required';

ALTER TABLE public.content_work_items
    ALTER COLUMN evidence_status SET NOT NULL;
