-- Preserve retry metadata used by the shared translation request read model.
--
-- The imported SQLite shape carries these columns, so a database created from a
-- snapshot already has them. A fresh PostgreSQL database only got them from the
-- import, which made the admin AI-records reads (`attempt_count`, retry state,
-- provider/contract status) fail on a bootstrap-only deployment.
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS retry_count BIGINT NOT NULL DEFAULT 0;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS failure_class TEXT;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS next_retry_at TEXT;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS retry_expires_at TEXT;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS attempt_count BIGINT NOT NULL DEFAULT 0;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS next_attempt_trigger TEXT NOT NULL DEFAULT 'initial';
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS error_code TEXT;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS processing_stage TEXT;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS provider_status TEXT;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS output_contract_status TEXT;
ALTER TABLE public.translation_work_items
    ADD COLUMN IF NOT EXISTS retry_disposition TEXT NOT NULL DEFAULT 'not_needed';
