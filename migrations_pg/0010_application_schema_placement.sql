-- Forward repair: baseline migrations 0006-0009 inherited octo_rill's
-- search_path on fresh databases, while application queries use public.
-- Imported public tables remain authoritative and are never replaced.
DO $$
DECLARE
    table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'content_processing_control', 'content_work_items', 'content_batches',
        'content_batch_items', 'content_result_projections',
        'translation_work_items', 'translation_requests', 'translation_batches',
        'translation_batch_items', 'translation_attempt_events',
        'translation_attempt_llm_calls', 'llm_diagnostic_access_audit',
        'llm_recovery_flags', 'llm_model_health', 'briefs', 'brief_release_memberships'
    ] LOOP
        IF to_regclass(format('public.%I', table_name)) IS NULL
           AND to_regclass(format('octo_rill.%I', table_name)) IS NOT NULL THEN
            EXECUTE format('ALTER TABLE octo_rill.%I SET SCHEMA public', table_name);
        END IF;
    END LOOP;
END
$$;
