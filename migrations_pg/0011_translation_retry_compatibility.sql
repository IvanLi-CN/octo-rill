-- Preserve retry metadata used by the shared translation request read model.
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS retry_count BIGINT NOT NULL DEFAULT 0;
