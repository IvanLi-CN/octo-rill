-- Audit fields retained by the existing admin read contract.
ALTER TABLE public.translation_attempt_events
    ADD COLUMN IF NOT EXISTS processing_stage TEXT,
    ADD COLUMN IF NOT EXISTS provider_status TEXT,
    ADD COLUMN IF NOT EXISTS output_contract_status TEXT,
    ADD COLUMN IF NOT EXISTS retry_disposition TEXT;
