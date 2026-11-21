-- Forward migration: legacy application schema translated to PostgreSQL.
--
-- Generated from the pg_dump of a PostgreSQL database produced by importing the
-- production SQLite snapshot. Migrations 0001-0014 only create the runtime-owned
-- tables, so a brand-new deployment needs these application tables, their
-- constraints/indexes and the read views to serve the API without importing a
-- SQLite snapshot first.
--
-- Every statement below is idempotent: databases created by importing a snapshot
-- already contain these objects, so the migration must leave them untouched.
-- Tables and indexes use IF NOT EXISTS, views use CREATE OR REPLACE, and the
-- constraints are added dynamically with an exception guard (an already-present
-- constraint is a duplicate object and is skipped).
--
-- Types are the PostgreSQL translation of the legacy SQLite schema: timestamps
-- are TEXT and booleans are BIGINT, exactly as the application reads them.

-- ---------------------------------------------------------------- tables
-- Ordered by foreign-key dependency; the constraints themselves are added
-- once every table exists, which also handles self-references.

CREATE TABLE IF NOT EXISTS public.admin_collection_processing_coverage (
    record_kind text NOT NULL,
    record_id text NOT NULL,
    pipeline text NOT NULL,
    status_origin text NOT NULL,
    created_at text DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT admin_collection_processing_coverage_status_origin_check CHECK ((status_origin = ANY (ARRAY['never_started'::text, 'historical_unknown'::text])))
);

CREATE TABLE IF NOT EXISTS public.admin_dashboard_daily_rollups (
    rollup_date text NOT NULL,
    time_zone text NOT NULL,
    task_type text NOT NULL,
    total_users bigint NOT NULL,
    active_users bigint NOT NULL,
    queued_count bigint DEFAULT 0 NOT NULL,
    running_count bigint DEFAULT 0 NOT NULL,
    succeeded_count bigint DEFAULT 0 NOT NULL,
    failed_count bigint DEFAULT 0 NOT NULL,
    canceled_count bigint DEFAULT 0 NOT NULL,
    updated_at text NOT NULL,
    business_ok_count bigint DEFAULT 0 NOT NULL,
    business_partial_count bigint DEFAULT 0 NOT NULL,
    business_failed_count bigint DEFAULT 0 NOT NULL,
    business_disabled_count bigint DEFAULT 0 NOT NULL
);

CREATE TABLE IF NOT EXISTS public.admin_runtime_settings (
    id bigint NOT NULL,
    llm_max_concurrency bigint NOT NULL,
    translation_general_worker_concurrency bigint CONSTRAINT admin_runtime_settings_translation_general_worker_conc_not_null NOT NULL,
    translation_dedicated_worker_concurrency bigint CONSTRAINT admin_runtime_settings_translation_dedicated_worker_co_not_null NOT NULL,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    ai_model_context_limit bigint,
    ai_model_context_limit_migrated_at text,
    sync_auto_fetch_interval_minutes bigint DEFAULT 60 CONSTRAINT admin_runtime_settings_sync_auto_fetch_interval_minute_not_null NOT NULL,
    repo_release_worker_concurrency bigint DEFAULT 5 NOT NULL,
    retry_recent_failures_interval_minutes bigint DEFAULT 10 CONSTRAINT admin_runtime_settings_retry_recent_failures_interval__not_null NOT NULL,
    llm_models_json text DEFAULT '[]'::text NOT NULL,
    repo_refresh_system_budget_per_window bigint DEFAULT 1000 CONSTRAINT admin_runtime_settings_repo_refresh_system_budget_per__not_null NOT NULL,
    daily_brief_schedule_local_time text DEFAULT '06:00'::text NOT NULL,
    dashboard_release_freshness_profile text DEFAULT 'balanced'::text CONSTRAINT admin_runtime_settings_dashboard_release_freshness_pro_not_null NOT NULL,
    webhook_push_audit_interval_days bigint DEFAULT 7 CONSTRAINT admin_runtime_settings_webhook_push_audit_interval_day_not_null NOT NULL,
    webhook_push_audit_last_started_at text,
    sync_auto_fetch_effective_at text,
    star_sync_delta_interval_minutes bigint DEFAULT 30 CONSTRAINT admin_runtime_settings_star_sync_delta_interval_minute_not_null NOT NULL,
    star_sync_full_sweep_interval_minutes bigint DEFAULT 1440 CONSTRAINT admin_runtime_settings_star_sync_full_sweep_interval_m_not_null NOT NULL,
    CONSTRAINT admin_runtime_settings_ai_model_context_limit_check CHECK (((ai_model_context_limit IS NULL) OR (ai_model_context_limit > 0))),
    CONSTRAINT admin_runtime_settings_dashboard_release_freshness_profil_check CHECK ((dashboard_release_freshness_profile = ANY (ARRAY['latest'::text, 'balanced'::text, 'capacity'::text]))),
    CONSTRAINT admin_runtime_settings_id_check CHECK ((id = 1)),
    CONSTRAINT admin_runtime_settings_llm_max_concurrency_check CHECK ((llm_max_concurrency > 0)),
    CONSTRAINT admin_runtime_settings_repo_refresh_system_budget_per_win_check CHECK (((repo_refresh_system_budget_per_window >= 1) AND (repo_refresh_system_budget_per_window <= 20000))),
    CONSTRAINT admin_runtime_settings_repo_release_worker_concurrency_check CHECK (((repo_release_worker_concurrency >= 1) AND (repo_release_worker_concurrency <= 32))),
    CONSTRAINT admin_runtime_settings_retry_recent_failures_interval_min_check CHECK (((retry_recent_failures_interval_minutes >= 1) AND (retry_recent_failures_interval_minutes <= 120))),
    CONSTRAINT admin_runtime_settings_star_sync_delta_interval_minutes_check CHECK (((star_sync_delta_interval_minutes >= 1) AND (star_sync_delta_interval_minutes <= 120))),
    CONSTRAINT admin_runtime_settings_star_sync_full_sweep_interval_minu_check CHECK (((star_sync_full_sweep_interval_minutes >= 60) AND (star_sync_full_sweep_interval_minutes <= 10080))),
    CONSTRAINT admin_runtime_settings_sync_auto_fetch_interval_minutes_check CHECK (((sync_auto_fetch_interval_minutes >= 1) AND (sync_auto_fetch_interval_minutes <= 120))),
    CONSTRAINT admin_runtime_settings_translation_dedicated_worker_concu_check CHECK ((translation_dedicated_worker_concurrency > 0)),
    CONSTRAINT admin_runtime_settings_translation_general_worker_concurr_check CHECK ((translation_general_worker_concurrency > 0)),
    CONSTRAINT admin_runtime_settings_webhook_push_audit_interval_days_check CHECK (((webhook_push_audit_interval_days >= 1) AND (webhook_push_audit_interval_days <= 30)))
);

CREATE TABLE IF NOT EXISTS public.ai_translations (
    id text NOT NULL,
    user_id text NOT NULL,
    entity_type text NOT NULL,
    entity_id text NOT NULL,
    lang text NOT NULL,
    source_hash text NOT NULL,
    title text,
    summary text,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    status text DEFAULT 'ready'::text NOT NULL,
    error_text text,
    active_work_item_id text
);

CREATE TABLE IF NOT EXISTS public.content_attempt_events (
    id text NOT NULL,
    work_item_id text NOT NULL,
    attempt_no bigint NOT NULL,
    trigger text NOT NULL,
    event_type text NOT NULL,
    result_status text,
    error_code text,
    error_summary text,
    failure_class text,
    retry_disposition text,
    retry_eligible bigint DEFAULT 0 NOT NULL,
    next_retry_at text,
    duration_ms bigint,
    token_count bigint,
    cost_microunits bigint,
    created_at text NOT NULL,
    configuration_snapshot_json text,
    route_snapshot_json text,
    configuration_fingerprint text,
    CONSTRAINT content_attempt_events_attempt_no_check CHECK ((attempt_no >= 1)),
    CONSTRAINT content_attempt_events_cost_microunits_check CHECK (((cost_microunits IS NULL) OR (cost_microunits >= 0))),
    CONSTRAINT content_attempt_events_duration_ms_check CHECK (((duration_ms IS NULL) OR (duration_ms >= 0))),
    CONSTRAINT content_attempt_events_event_type_check CHECK ((event_type = ANY (ARRAY['attempt_queued'::text, 'attempt_started'::text, 'attempt_completed'::text, 'retry_scheduled'::text]))),
    CONSTRAINT content_attempt_events_retry_eligible_check CHECK ((retry_eligible = ANY (ARRAY[(0)::bigint, (1)::bigint]))),
    CONSTRAINT content_attempt_events_token_count_check CHECK (((token_count IS NULL) OR (token_count >= 0))),
    CONSTRAINT content_attempt_events_trigger_check CHECK ((trigger = ANY (ARRAY['initial'::text, 'manual_retry'::text, 'automatic_recovery'::text, 'system_requeue'::text])))
);

CREATE TABLE IF NOT EXISTS public.content_attempt_llm_calls (
    id text NOT NULL,
    attempt_event_id text NOT NULL,
    provider_call_id text NOT NULL,
    model text NOT NULL,
    status text NOT NULL,
    duration_ms bigint,
    input_tokens bigint,
    output_tokens bigint,
    cost_microunits bigint,
    error_code text,
    error_summary text,
    created_at text NOT NULL,
    CONSTRAINT content_attempt_llm_calls_cost_microunits_check CHECK (((cost_microunits IS NULL) OR (cost_microunits >= 0))),
    CONSTRAINT content_attempt_llm_calls_duration_ms_check CHECK (((duration_ms IS NULL) OR (duration_ms >= 0))),
    CONSTRAINT content_attempt_llm_calls_input_tokens_check CHECK (((input_tokens IS NULL) OR (input_tokens >= 0))),
    CONSTRAINT content_attempt_llm_calls_output_tokens_check CHECK (((output_tokens IS NULL) OR (output_tokens >= 0)))
);

CREATE TABLE IF NOT EXISTS public.content_attempt_provider_admissions (
    id text NOT NULL,
    attempt_event_id text NOT NULL,
    work_item_id text NOT NULL,
    attempt_no bigint NOT NULL,
    call_ordinal bigint NOT NULL,
    relation_role text NOT NULL,
    source_hash text NOT NULL,
    source_revision_json text DEFAULT '{}'::text CONSTRAINT content_attempt_provider_admissio_source_revision_json_not_null NOT NULL,
    admitted_at text NOT NULL,
    CONSTRAINT content_attempt_provider_admissions_attempt_no_check CHECK ((attempt_no >= 1)),
    CONSTRAINT content_attempt_provider_admissions_call_ordinal_check CHECK ((call_ordinal >= 0)),
    CONSTRAINT content_attempt_provider_admissions_relation_role_check CHECK ((relation_role = ANY (ARRAY['primary'::text, 'length_recovery'::text, 'fallback'::text])))
);

CREATE TABLE IF NOT EXISTS public.content_identity_upgrade_control (
    id bigint NOT NULL,
    generation bigint NOT NULL,
    status text NOT NULL,
    phase text NOT NULL,
    cursor text,
    last_error_code text,
    started_at text,
    completed_at text,
    updated_at text NOT NULL,
    CONSTRAINT content_identity_upgrade_control_generation_check CHECK ((generation >= 1)),
    CONSTRAINT content_identity_upgrade_control_id_check CHECK ((id = 1)),
    CONSTRAINT content_identity_upgrade_control_phase_check CHECK ((phase = ANY (ARRAY['work_identity_backfill'::text, 'projection_backfill'::text, 'blocked_config_recovery'::text, 'complete'::text]))),
    CONSTRAINT content_identity_upgrade_control_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'running'::text, 'paused'::text, 'completed'::text, 'failed'::text])))
);

CREATE TABLE IF NOT EXISTS public.content_legacy_observations (
    id text NOT NULL,
    legacy_table text NOT NULL,
    legacy_primary_key text NOT NULL,
    legacy_source_hash text NOT NULL,
    canonical_resource_type text,
    canonical_resource_id text,
    pipeline text,
    classification text NOT NULL,
    observation_basis_json text NOT NULL,
    observed_at text NOT NULL,
    CONSTRAINT content_legacy_observations_classification_check CHECK ((classification = ANY (ARRAY['legacy_cached'::text, 'legacy_conflict'::text]))),
    CONSTRAINT content_legacy_observations_pipeline_check CHECK (((pipeline IS NULL) OR (pipeline = ANY (ARRAY['translation'::text, 'polishing'::text]))))
);

CREATE TABLE IF NOT EXISTS public.content_request_links (
    id text NOT NULL,
    request_id text NOT NULL,
    work_item_id text NOT NULL,
    requester_type text NOT NULL,
    requester_id text,
    authorization_snapshot_json text NOT NULL,
    producer_ref text NOT NULL,
    request_source text NOT NULL,
    delivery_mode text NOT NULL,
    response_status text,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT content_request_links_delivery_mode_check CHECK ((delivery_mode = ANY (ARRAY['async'::text, 'wait'::text, 'stream'::text]))),
    CONSTRAINT content_request_links_requester_type_check CHECK ((requester_type = ANY (ARRAY['user'::text, 'system'::text])))
);

CREATE TABLE IF NOT EXISTS public.content_work_admission_events (
    id text NOT NULL,
    work_item_id text NOT NULL,
    event_type text NOT NULL,
    replaced_by_work_item_id text,
    source_hash text NOT NULL,
    source_revision_json text DEFAULT '{}'::text NOT NULL,
    producer_ref text DEFAULT ''::text NOT NULL,
    requester_id text,
    reason_code text DEFAULT ''::text NOT NULL,
    created_at text NOT NULL,
    CONSTRAINT content_work_admission_events_event_type_check CHECK ((event_type = ANY (ARRAY['admission_accepted'::text, 'admission_noop'::text, 'admission_rejected_superseded'::text, 'source_superseded'::text, 'reconciliation_superseded'::text])))
);

CREATE TABLE IF NOT EXISTS public.content_work_identities (
    id text NOT NULL,
    canonical_resource_type text NOT NULL,
    canonical_resource_id text NOT NULL,
    pipeline text NOT NULL,
    variant text NOT NULL,
    target_lang text NOT NULL,
    source_hash text NOT NULL,
    protocol_version text NOT NULL,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT content_work_identities_canonical_resource_type_check CHECK ((canonical_resource_type = ANY (ARRAY['release'::text, 'announcement'::text, 'notification'::text]))),
    CONSTRAINT content_work_identities_pipeline_check CHECK ((pipeline = ANY (ARRAY['translation'::text, 'polishing'::text])))
);

CREATE TABLE IF NOT EXISTS public.content_work_identity_members (
    work_item_id text NOT NULL,
    identity_id text NOT NULL,
    canonical_resource_type text NOT NULL,
    canonical_resource_id text NOT NULL,
    pipeline text NOT NULL,
    variant text NOT NULL,
    target_lang text NOT NULL,
    source_hash text NOT NULL,
    protocol_version text NOT NULL,
    linked_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.content_current_result_projections (
    identity_id text NOT NULL,
    work_item_id text NOT NULL,
    active_work_item_id text,
    source_projection_id text,
    payload_json text NOT NULL,
    published_at text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.follower_current_members (
    id text NOT NULL,
    user_id text NOT NULL,
    actor_github_user_id bigint NOT NULL,
    actor_login text NOT NULL,
    actor_avatar_url text,
    actor_html_url text,
    created_at text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.follower_sync_baselines (
    id text NOT NULL,
    user_id text NOT NULL,
    initialized_at text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.llm_calls (
    id text NOT NULL,
    status text NOT NULL,
    source text NOT NULL,
    model text NOT NULL,
    requested_by text,
    parent_task_id text,
    parent_task_type text,
    max_tokens bigint NOT NULL,
    attempt_count bigint DEFAULT 0 NOT NULL,
    scheduler_wait_ms bigint DEFAULT 0 NOT NULL,
    duration_ms bigint,
    prompt_text text NOT NULL,
    response_text text,
    error_text text,
    created_at text NOT NULL,
    started_at text,
    finished_at text,
    updated_at text NOT NULL,
    input_messages_json text,
    output_messages_json text,
    input_tokens bigint,
    output_tokens bigint,
    cached_input_tokens bigint,
    total_tokens bigint,
    first_token_wait_ms bigint,
    parent_translation_batch_id text,
    runtime_owner_id text,
    lease_heartbeat_at text,
    failure_class text,
    final_model text,
    fallback_count bigint DEFAULT 0 NOT NULL,
    retry_scheduled_at text,
    recovery_attempt_count bigint DEFAULT 0 NOT NULL,
    parent_brief_id text,
    finish_reason text,
    provider_request_id text,
    provider_http_status bigint,
    CONSTRAINT llm_calls_failure_class_check CHECK (((failure_class IS NULL) OR (failure_class = ANY (ARRAY['empty_content'::text, 'transient'::text, 'rate_limited'::text, 'configuration'::text])))),
    CONSTRAINT llm_calls_fallback_count_check CHECK ((fallback_count >= 0)),
    CONSTRAINT llm_calls_recovery_attempt_count_check CHECK ((recovery_attempt_count >= 0)),
    CONSTRAINT llm_calls_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'running'::text, 'succeeded'::text, 'failed'::text])))
);

CREATE TABLE IF NOT EXISTS public.llm_call_events (
    id text NOT NULL,
    call_id text NOT NULL,
    event_type text NOT NULL,
    status text NOT NULL,
    source text NOT NULL,
    requested_by text,
    parent_task_id text,
    payload_json text,
    created_at text NOT NULL,
    failure_class text,
    model text,
    attempt bigint,
    retry_after_ms bigint,
    from_model text,
    to_model text,
    fallback_count bigint,
    CONSTRAINT llm_call_events_attempt_check CHECK (((attempt IS NULL) OR (attempt >= 0))),
    CONSTRAINT llm_call_events_failure_class_check CHECK (((failure_class IS NULL) OR (failure_class = ANY (ARRAY['empty_content'::text, 'transient'::text, 'rate_limited'::text, 'configuration'::text])))),
    CONSTRAINT llm_call_events_fallback_count_check CHECK (((fallback_count IS NULL) OR (fallback_count >= 0))),
    CONSTRAINT llm_call_events_retry_after_ms_check CHECK (((retry_after_ms IS NULL) OR (retry_after_ms >= 0))),
    CONSTRAINT llm_call_events_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'running'::text, 'succeeded'::text, 'failed'::text])))
);

CREATE TABLE IF NOT EXISTS public.notifications (
    id text NOT NULL,
    user_id text NOT NULL,
    thread_id text NOT NULL,
    repo_full_name text,
    subject_title text,
    subject_type text,
    reason text,
    updated_at text,
    unread bigint DEFAULT 1 NOT NULL,
    url text,
    html_url text,
    last_seen_at text
);

CREATE TABLE IF NOT EXISTS public.online_migration_leases (
    lease_name text NOT NULL,
    owner_id text NOT NULL,
    lease_expires_at text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.owned_repo_star_baselines (
    id text NOT NULL,
    user_id text NOT NULL,
    repo_id bigint NOT NULL,
    repo_full_name text NOT NULL,
    initialized_at text NOT NULL,
    updated_at text NOT NULL,
    members_snapshot_initialized bigint DEFAULT 1 NOT NULL,
    owner_avatar_url text,
    open_graph_image_url text,
    uses_custom_open_graph_image bigint DEFAULT 0 NOT NULL,
    repo_stargazer_count bigint,
    repo_stargazer_count_updated_at text,
    is_private bigint
);

CREATE TABLE IF NOT EXISTS public.public_metrics_hourly_snapshots (
    sampled_hour text NOT NULL,
    observed_at text NOT NULL,
    deduplicated_repositories bigint CONSTRAINT public_metrics_hourly_snapsh_deduplicated_repositories_not_null NOT NULL,
    pressure double precision NOT NULL,
    CONSTRAINT public_metrics_hourly_snapshots_deduplicated_repositories_check CHECK ((deduplicated_repositories >= 0)),
    CONSTRAINT public_metrics_hourly_snapshots_pressure_check CHECK ((pressure >= (0)::double precision))
);

CREATE TABLE IF NOT EXISTS public.public_repo_release_usage (
    id text NOT NULL,
    repo_id bigint,
    owner_login text NOT NULL,
    repo_name text NOT NULL,
    full_name text NOT NULL,
    full_name_lower text NOT NULL,
    first_registered_at text NOT NULL,
    last_requested_at text NOT NULL,
    last_list_requested_at text,
    last_detail_requested_at text,
    api_list_requests bigint DEFAULT 0 NOT NULL,
    api_detail_requests bigint DEFAULT 0 NOT NULL,
    page_list_requests bigint DEFAULT 0 NOT NULL,
    page_detail_requests bigint DEFAULT 0 NOT NULL,
    last_sync_status text DEFAULT 'pending'::text NOT NULL,
    last_sync_error text,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    access_kind text DEFAULT 'github_public'::text NOT NULL,
    published_by_user_id text,
    published_at text,
    CONSTRAINT public_repo_release_usage_access_kind_check CHECK ((access_kind = ANY (ARRAY['github_public'::text, 'private_owner_published'::text]))),
    CONSTRAINT public_repo_release_usage_last_sync_status_check CHECK ((last_sync_status = ANY (ARRAY['pending'::text, 'ready'::text, 'failed'::text, 'inaccessible'::text])))
);

CREATE TABLE IF NOT EXISTS public.reaction_pat_tokens (
    user_id text NOT NULL,
    token_ciphertext bytea NOT NULL,
    token_nonce bytea NOT NULL,
    masked_token text NOT NULL,
    last_check_state text DEFAULT 'unknown'::text NOT NULL,
    last_check_message text,
    last_checked_at text,
    updated_at text NOT NULL,
    owner_github_connection_id text,
    owner_github_user_id bigint,
    owner_login text,
    webhook_push_allows_private_repos bigint,
    CONSTRAINT reaction_pat_tokens_webhook_push_allows_private_repos_check CHECK (((webhook_push_allows_private_repos IS NULL) OR (webhook_push_allows_private_repos = ANY (ARRAY[(0)::bigint, (1)::bigint]))))
);

CREATE TABLE IF NOT EXISTS public.release_evidence_index (
    id text NOT NULL,
    repo_id bigint NOT NULL,
    repo_full_name text NOT NULL,
    audience text NOT NULL,
    audience_user_id text,
    resource_kind text NOT NULL,
    resource_key text NOT NULL,
    side text DEFAULT ''::text NOT NULL,
    status text NOT NULL,
    payload_json text NOT NULL,
    content_hash text NOT NULL,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT release_evidence_index_audience_check CHECK ((audience = ANY (ARRAY['public'::text, 'authorized'::text]))),
    CONSTRAINT release_evidence_index_check CHECK ((((audience = 'public'::text) AND (audience_user_id IS NULL)) OR ((audience = 'authorized'::text) AND (audience_user_id IS NOT NULL)))),
    CONSTRAINT release_evidence_index_resource_kind_check CHECK ((resource_kind = ANY (ARRAY['compare'::text, 'commit'::text, 'tree'::text, 'blob'::text, 'file'::text]))),
    CONSTRAINT release_evidence_index_status_check CHECK ((status = ANY (ARRAY['ready'::text, 'failed'::text])))
);

CREATE TABLE IF NOT EXISTS public.release_evidence_snapshots (
    id text NOT NULL,
    request_key text NOT NULL,
    release_id bigint NOT NULL,
    repo_id bigint NOT NULL,
    repo_full_name text NOT NULL,
    audience text NOT NULL,
    audience_user_id text,
    source_revision text NOT NULL,
    base_tag text NOT NULL,
    head_tag text NOT NULL,
    status text NOT NULL,
    snapshot_json text,
    content_hash text,
    attempt_count bigint DEFAULT 0 NOT NULL,
    next_retry_at text,
    retry_expires_at text,
    task_id text,
    lease_owner text,
    lease_expires_at text,
    last_error_code text,
    last_error_summary text,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT release_evidence_snapshots_attempt_count_check CHECK ((attempt_count >= 0)),
    CONSTRAINT release_evidence_snapshots_audience_check CHECK ((audience = ANY (ARRAY['public'::text, 'authorized'::text]))),
    CONSTRAINT release_evidence_snapshots_check CHECK ((((audience = 'public'::text) AND (audience_user_id IS NULL)) OR ((audience = 'authorized'::text) AND (audience_user_id IS NOT NULL)))),
    CONSTRAINT release_evidence_snapshots_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'running'::text, 'ready'::text, 'failed'::text])))
);

CREATE TABLE IF NOT EXISTS public.release_evidence_snapshot_items (
    snapshot_id text NOT NULL,
    index_id text NOT NULL,
    ordinal bigint NOT NULL,
    created_at text NOT NULL,
    CONSTRAINT release_evidence_snapshot_items_ordinal_check CHECK ((ordinal >= 0))
);

CREATE TABLE IF NOT EXISTS public.release_reading_backfill_runs (
    id text NOT NULL,
    started_at text NOT NULL,
    window_start_utc text NOT NULL,
    window_end_utc text NOT NULL,
    status text NOT NULL,
    cursor_published_at text,
    cursor_release_id bigint,
    total bigint DEFAULT 0 NOT NULL,
    succeeded bigint DEFAULT 0 NOT NULL,
    failed bigint DEFAULT 0 NOT NULL,
    waiting bigint DEFAULT 0 NOT NULL,
    skipped bigint DEFAULT 0 NOT NULL,
    lease_owner text,
    lease_expires_at text,
    error_text text,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    lease_task_id text,
    CONSTRAINT release_reading_backfill_runs_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'running'::text, 'waiting'::text, 'paused'::text, 'succeeded'::text, 'failed'::text])))
);

CREATE TABLE IF NOT EXISTS public.releases (
    id text NOT NULL,
    user_id text NOT NULL,
    repo_id bigint NOT NULL,
    release_id bigint NOT NULL,
    tag_name text NOT NULL,
    name text,
    body text,
    html_url text NOT NULL,
    published_at text,
    created_at text,
    is_prerelease bigint DEFAULT 0 NOT NULL,
    is_draft bigint DEFAULT 0 NOT NULL,
    updated_at text NOT NULL,
    node_id text,
    react_plus1 bigint DEFAULT 0 NOT NULL,
    react_laugh bigint DEFAULT 0 NOT NULL,
    react_heart bigint DEFAULT 0 NOT NULL,
    react_hooray bigint DEFAULT 0 NOT NULL,
    react_rocket bigint DEFAULT 0 NOT NULL,
    react_eyes bigint DEFAULT 0 NOT NULL
);

CREATE TABLE IF NOT EXISTS public.repo_refresh_governance_cycles (
    id text NOT NULL,
    status text NOT NULL,
    window_budget bigint NOT NULL,
    frozen_repo_count bigint NOT NULL,
    completed_repo_count bigint DEFAULT 0 NOT NULL,
    window_index_started_at bigint NOT NULL,
    window_index_completed_at bigint,
    started_at text NOT NULL,
    completed_at text,
    updated_at text NOT NULL,
    window_minutes bigint DEFAULT 10 NOT NULL,
    last_selection_window_index bigint,
    CONSTRAINT repo_refresh_governance_cycles_status_check CHECK ((status = ANY (ARRAY['active'::text, 'completed'::text]))),
    CONSTRAINT repo_refresh_governance_cycles_window_minutes_check CHECK (((window_minutes >= 1) AND (window_minutes <= 120)))
);

CREATE TABLE IF NOT EXISTS public.repo_refresh_governance_cycle_members (
    cycle_id text NOT NULL,
    repo_id bigint NOT NULL,
    repo_full_name text NOT NULL,
    completed_at text,
    removed_from_pool bigint DEFAULT 0 CONSTRAINT repo_refresh_governance_cycle_member_removed_from_pool_not_null NOT NULL,
    updated_at text NOT NULL,
    attempt_status text,
    attempt_error text
);

CREATE TABLE IF NOT EXISTS public.repo_refresh_governance_retention_state (
    id bigint NOT NULL,
    window_started_at text CONSTRAINT repo_refresh_governance_retention_st_window_started_at_not_null NOT NULL,
    deleted_rows bigint DEFAULT 0 NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT repo_refresh_governance_retention_state_id_check CHECK ((id = 1))
);

CREATE TABLE IF NOT EXISTS public.repo_refresh_governance_snapshots (
    repo_id bigint NOT NULL,
    repo_full_name text NOT NULL,
    is_private bigint DEFAULT 0 NOT NULL,
    watcher_user_count bigint NOT NULL,
    watcher_repo_total_sum bigint CONSTRAINT repo_refresh_governance_snapsho_watcher_repo_total_sum_not_null NOT NULL,
    cached_stargazer_count bigint,
    cached_stargazer_count_updated_at text,
    priority_rank bigint NOT NULL,
    target_window bigint NOT NULL,
    target_interval_minutes bigint CONSTRAINT repo_refresh_governance_snapsh_target_interval_minutes_not_null NOT NULL,
    urgency_score double precision DEFAULT 1 NOT NULL,
    urgency_bucket text DEFAULT 'active'::text NOT NULL,
    system_last_selected_at text,
    system_last_success_at text,
    actual_last_success_at text,
    actual_last_success_source text,
    active_cycle_id text,
    active_cycle_window_index bigint,
    active_cycle_completed bigint DEFAULT 0 CONSTRAINT repo_refresh_governance_snapsho_active_cycle_completed_not_null NOT NULL,
    updated_at text NOT NULL,
    system_last_attempt_at text,
    system_last_attempt_status text,
    system_last_attempt_error text
);

CREATE TABLE IF NOT EXISTS public.repo_release_sync_state (
    repo_id bigint NOT NULL,
    etag text,
    last_modified text,
    last_success_at text,
    last_attempt_at text,
    last_not_modified_at text,
    last_error_text text,
    backoff_until text,
    updated_at text NOT NULL,
    last_page_count bigint DEFAULT 0 NOT NULL,
    last_fetched_count bigint DEFAULT 0 NOT NULL,
    last_inserted_count bigint DEFAULT 0 NOT NULL,
    last_updated_count bigint DEFAULT 0 NOT NULL,
    last_unchanged_count bigint DEFAULT 0 NOT NULL,
    last_stopped_reason text
);

CREATE TABLE IF NOT EXISTS public.repo_release_work_items (
    id text NOT NULL,
    repo_id bigint NOT NULL,
    repo_full_name text NOT NULL,
    status text NOT NULL,
    request_origin text NOT NULL,
    priority bigint DEFAULT 0 NOT NULL,
    has_new_repo_watchers bigint DEFAULT 0 NOT NULL,
    deadline_at text NOT NULL,
    last_release_count bigint DEFAULT 0 NOT NULL,
    last_candidate_failures bigint DEFAULT 0 NOT NULL,
    last_success_at text,
    error_text text,
    created_at text NOT NULL,
    started_at text,
    finished_at text,
    updated_at text NOT NULL,
    runtime_owner_id text,
    lease_heartbeat_at text,
    last_fetched_count bigint DEFAULT 0 NOT NULL,
    last_inserted_count bigint DEFAULT 0 NOT NULL,
    last_updated_count bigint DEFAULT 0 NOT NULL,
    last_unchanged_count bigint DEFAULT 0 NOT NULL,
    last_pages_fetched bigint DEFAULT 0 NOT NULL,
    last_stopped_reason text,
    last_new_release_ids_json text
);

CREATE TABLE IF NOT EXISTS public.repo_release_watchers (
    id text NOT NULL,
    work_item_id text NOT NULL,
    task_id text NOT NULL,
    user_id text,
    origin text NOT NULL,
    priority bigint DEFAULT 0 NOT NULL,
    reason text NOT NULL,
    is_new_repo bigint DEFAULT 0 NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    error_text text,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    reused_fresh bigint DEFAULT 0 NOT NULL,
    freshness_window_minutes bigint,
    freshness_decision text,
    freshness_assessment_json text,
    CONSTRAINT repo_release_watchers_freshness_decision_check CHECK (((freshness_decision IS NULL) OR (freshness_decision = ANY (ARRAY['fetch'::text, 'reused_fresh'::text, 'reused_running'::text])))),
    CONSTRAINT repo_release_watchers_freshness_window_minutes_check CHECK (((freshness_window_minutes IS NULL) OR ((freshness_window_minutes >= 1) AND (freshness_window_minutes <= 30))))
);

CREATE TABLE IF NOT EXISTS public.repo_releases (
    id text NOT NULL,
    repo_id bigint NOT NULL,
    release_id bigint NOT NULL,
    node_id text,
    tag_name text NOT NULL,
    name text,
    body text,
    html_url text NOT NULL,
    published_at text,
    created_at text,
    is_prerelease bigint DEFAULT 0 NOT NULL,
    is_draft bigint DEFAULT 0 NOT NULL,
    updated_at text NOT NULL,
    react_plus1 bigint DEFAULT 0 NOT NULL,
    react_laugh bigint DEFAULT 0 NOT NULL,
    react_heart bigint DEFAULT 0 NOT NULL,
    react_hooray bigint DEFAULT 0 NOT NULL,
    react_rocket bigint DEFAULT 0 NOT NULL,
    react_eyes bigint DEFAULT 0 NOT NULL,
    detected_at text
);

CREATE TABLE IF NOT EXISTS public.release_reading_backfill_items (
    run_id text NOT NULL,
    release_id bigint NOT NULL,
    authorized_user_id text,
    published_at text NOT NULL,
    status text NOT NULL,
    attempt_count bigint DEFAULT 0 NOT NULL,
    error_text text,
    updated_at text NOT NULL,
    CONSTRAINT release_reading_backfill_items_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'running'::text, 'waiting'::text, 'succeeded'::text, 'failed'::text, 'skipped'::text])))
);

CREATE TABLE IF NOT EXISTS public.repo_star_current_members (
    id text NOT NULL,
    user_id text NOT NULL,
    repo_id bigint NOT NULL,
    repo_full_name text NOT NULL,
    actor_github_user_id bigint NOT NULL,
    actor_login text NOT NULL,
    actor_avatar_url text,
    actor_html_url text,
    starred_at text,
    created_at text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.repo_star_sync_baselines (
    id text NOT NULL,
    user_id text NOT NULL,
    initialized_at text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.scheduled_task_dispatch_state (
    schedule_name text NOT NULL,
    last_dispatch_key text,
    last_task_id text,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.search_fts_document_rows (
    fts_rowid bigint NOT NULL,
    document_id text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.search_fts_user_lane_rows (
    fts_rowid bigint NOT NULL,
    document_id text NOT NULL,
    user_id text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.search_metadata_backfill_queue (
    repo_id bigint NOT NULL,
    updated_at text NOT NULL,
    release_cursor bigint DEFAULT 0 NOT NULL
);

CREATE TABLE IF NOT EXISTS public.search_projection_backfill_state (
    id bigint NOT NULL,
    phase text NOT NULL,
    cursor bigint DEFAULT 0 NOT NULL,
    status text NOT NULL,
    last_error text,
    updated_at text NOT NULL,
    CONSTRAINT search_projection_backfill_state_id_check CHECK ((id = 1)),
    CONSTRAINT search_projection_backfill_state_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'building'::text, 'ready'::text, 'paused_low_disk'::text, 'failed'::text])))
);

CREATE TABLE IF NOT EXISTS public.search_projection_outbox (
    projection_key text NOT NULL,
    operation text NOT NULL,
    resource_type text NOT NULL,
    resource_id text NOT NULL,
    canonical_resource_type text,
    user_id text,
    attempt_count bigint DEFAULT 0 NOT NULL,
    available_at text NOT NULL,
    lease_owner text,
    lease_expires_at text,
    last_error text,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    previous_resource_id text,
    fanout_cursor text,
    CONSTRAINT search_projection_outbox_attempt_count_check CHECK ((attempt_count >= 0)),
    CONSTRAINT search_projection_outbox_operation_check CHECK ((operation = ANY (ARRAY['upsert'::text, 'delete'::text]))),
    CONSTRAINT search_projection_outbox_resource_type_check CHECK ((resource_type = ANY (ARRAY['announcement'::text, 'content'::text, 'notification'::text])))
);

CREATE TABLE IF NOT EXISTS public.search_projection_outbox_failed (
    projection_key text NOT NULL,
    operation text NOT NULL,
    resource_type text NOT NULL,
    resource_id text NOT NULL,
    previous_resource_id text,
    canonical_resource_type text,
    user_id text,
    attempt_count bigint NOT NULL,
    last_error text NOT NULL,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    failed_at text NOT NULL,
    CONSTRAINT search_projection_outbox_failed_attempt_count_check CHECK ((attempt_count >= 0)),
    CONSTRAINT search_projection_outbox_failed_operation_check CHECK ((operation = ANY (ARRAY['upsert'::text, 'delete'::text]))),
    CONSTRAINT search_projection_outbox_failed_resource_type_check CHECK ((resource_type = ANY (ARRAY['announcement'::text, 'content'::text, 'notification'::text])))
);

CREATE TABLE IF NOT EXISTS public.search_rate_limits (
    user_id text NOT NULL,
    window_started_at bigint NOT NULL,
    request_count bigint NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT search_rate_limits_request_count_check CHECK ((request_count >= 0))
);

CREATE TABLE IF NOT EXISTS public.social_activity_events (
    id text NOT NULL,
    user_id text NOT NULL,
    kind text NOT NULL,
    repo_id bigint,
    repo_full_name text,
    repo_owner_avatar_url text,
    repo_open_graph_image_url text,
    repo_uses_custom_open_graph_image bigint,
    title text,
    body text,
    html_url text,
    github_event_id text,
    actor_github_user_id bigint NOT NULL,
    actor_login text NOT NULL,
    actor_avatar_url text,
    actor_html_url text,
    occurred_at text NOT NULL,
    detected_at text NOT NULL,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    discussion_number bigint
);

CREATE TABLE IF NOT EXISTS public.star_sync_epochs (
    id text NOT NULL,
    user_id text NOT NULL,
    github_connection_id text NOT NULL,
    status text NOT NULL,
    started_at text NOT NULL,
    next_cursor text,
    total_count_at_start bigint,
    processed_pages bigint DEFAULT 0 NOT NULL,
    processed_items bigint DEFAULT 0 NOT NULL,
    next_slice_not_before text,
    lease_owner_id text,
    lease_expires_at text,
    completed_at text,
    failure_reason text,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT star_sync_epochs_status_check CHECK ((status = ANY (ARRAY['queued'::text, 'running'::text, 'succeeded'::text, 'failed'::text, 'cancelled'::text])))
);

CREATE TABLE IF NOT EXISTS public.starred_repo_connection_memberships (
    id text NOT NULL,
    user_id text NOT NULL,
    github_connection_id text CONSTRAINT starred_repo_connection_membershi_github_connection_id_not_null NOT NULL,
    repo_id bigint NOT NULL,
    full_name text NOT NULL,
    owner_login text NOT NULL,
    name text NOT NULL,
    description text,
    html_url text NOT NULL,
    stargazed_at text,
    is_private bigint DEFAULT 0 NOT NULL,
    owner_avatar_url text,
    open_graph_image_url text,
    uses_custom_open_graph_image bigint DEFAULT 0 CONSTRAINT starred_repo_connection_mem_uses_custom_open_graph_ima_not_null NOT NULL,
    repo_stargazer_count bigint,
    last_seen_epoch_id text,
    last_delta_seen_at text,
    created_at text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.starred_repos (
    id text NOT NULL,
    user_id text NOT NULL,
    repo_id bigint NOT NULL,
    full_name text NOT NULL,
    owner_login text NOT NULL,
    name text NOT NULL,
    description text,
    html_url text NOT NULL,
    stargazed_at text,
    is_private bigint DEFAULT 0 NOT NULL,
    updated_at text NOT NULL,
    owner_avatar_url text,
    open_graph_image_url text,
    uses_custom_open_graph_image bigint DEFAULT 0 NOT NULL,
    repo_stargazer_count bigint,
    repo_stargazer_count_updated_at text
);

CREATE TABLE IF NOT EXISTS public.sync_state (
    id text NOT NULL,
    user_id text NOT NULL,
    key text NOT NULL,
    value text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.sync_subscription_events (
    id text NOT NULL,
    task_id text NOT NULL,
    stage text NOT NULL,
    event_type text NOT NULL,
    severity text NOT NULL,
    recoverable bigint DEFAULT 0 NOT NULL,
    attempt bigint DEFAULT 0 NOT NULL,
    user_id text,
    repo_id bigint,
    repo_full_name text,
    payload_json text,
    created_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.user_repo_associations (
    id text NOT NULL,
    user_id text NOT NULL,
    repo_id bigint,
    repo_full_name text NOT NULL,
    repo_full_name_lower text NOT NULL,
    owner_login text NOT NULL,
    repo_name text NOT NULL,
    html_url text,
    description text,
    is_private bigint,
    owner_avatar_url text,
    open_graph_image_url text,
    uses_custom_open_graph_image bigint DEFAULT 0 NOT NULL,
    first_source text NOT NULL,
    first_associated_at text NOT NULL,
    last_seen_at text NOT NULL,
    is_following bigint DEFAULT 0 NOT NULL,
    follow_state_source text DEFAULT 'system_default'::text NOT NULL,
    has_personal_owned_source bigint DEFAULT 0 NOT NULL,
    has_github_star_source bigint DEFAULT 0 NOT NULL,
    has_manual_feed_source bigint DEFAULT 0 NOT NULL,
    created_at text NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT user_repo_associations_first_source_check CHECK ((first_source = ANY (ARRAY['personal_owned'::text, 'github_star'::text, 'manual_feed'::text]))),
    CONSTRAINT user_repo_associations_follow_state_source_check CHECK ((follow_state_source = ANY (ARRAY['system_default'::text, 'user_explicit'::text])))
);

CREATE TABLE IF NOT EXISTS public.webhook_push_deliveries (
    delivery_id text NOT NULL,
    hook_id bigint,
    repo_id bigint,
    event text NOT NULL,
    action text,
    received_at text NOT NULL,
    queued_task_id text,
    processing_state text DEFAULT 'pending'::text NOT NULL,
    processing_started_at text,
    CONSTRAINT webhook_push_deliveries_processing_state_check CHECK ((processing_state = ANY (ARRAY['pending'::text, 'processing'::text, 'ignored'::text, 'queued'::text])))
);

CREATE TABLE IF NOT EXISTS public.webhook_push_reconcile_demands (
    user_id text NOT NULL,
    requested_generation bigint DEFAULT 0 NOT NULL,
    completed_generation bigint DEFAULT 0 NOT NULL,
    requested_at text NOT NULL,
    updated_at text NOT NULL,
    CONSTRAINT webhook_push_reconcile_demands_check CHECK ((completed_generation <= requested_generation)),
    CONSTRAINT webhook_push_reconcile_demands_completed_generation_check CHECK ((completed_generation >= 0)),
    CONSTRAINT webhook_push_reconcile_demands_requested_generation_check CHECK ((requested_generation >= 0))
);

CREATE TABLE IF NOT EXISTS public.webhook_push_repos (
    user_id text NOT NULL,
    repo_id bigint NOT NULL,
    owner_login text NOT NULL,
    repo_name text NOT NULL,
    repo_full_name text NOT NULL,
    hook_id bigint,
    callback_url text NOT NULL,
    status text DEFAULT 'unknown'::text NOT NULL,
    error_kind text,
    error_message text,
    permission_paused bigint DEFAULT 0 NOT NULL,
    last_checked_at text,
    last_registered_at text,
    updated_at text NOT NULL,
    owner_github_user_id bigint,
    CONSTRAINT webhook_push_repos_permission_paused_check CHECK ((permission_paused = ANY (ARRAY[(0)::bigint, (1)::bigint]))),
    CONSTRAINT webhook_push_repos_status_check CHECK ((status = ANY (ARRAY['unknown'::text, 'missing'::text, 'registered'::text, 'conflict'::text, 'permission_paused'::text, 'error'::text, 'delete_pending'::text])))
);

CREATE TABLE IF NOT EXISTS public.webhook_push_user_operation_leases (
    user_id text NOT NULL,
    task_id text NOT NULL,
    expires_at text NOT NULL
);

-- ------------------------------------- primary keys and unique constraints
DO $$
DECLARE
    statement text;
BEGIN
    FOREACH statement IN ARRAY ARRAY[
        'ALTER TABLE public.admin_collection_processing_coverage ADD CONSTRAINT admin_collection_processing_coverage_pkey PRIMARY KEY (record_kind, record_id, pipeline)',
        'ALTER TABLE public.admin_dashboard_daily_rollups ADD CONSTRAINT admin_dashboard_daily_rollups_pkey PRIMARY KEY (rollup_date, time_zone, task_type)',
        'ALTER TABLE public.admin_runtime_settings ADD CONSTRAINT admin_runtime_settings_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.ai_translations ADD CONSTRAINT ai_translations_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.ai_translations ADD CONSTRAINT ai_translations_user_id_entity_type_entity_id_lang_key UNIQUE (user_id, entity_type, entity_id, lang)',
        'ALTER TABLE public.content_attempt_events ADD CONSTRAINT content_attempt_events_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.content_attempt_events ADD CONSTRAINT content_attempt_events_work_item_id_attempt_no_event_type_key UNIQUE (work_item_id, attempt_no, event_type)',
        'ALTER TABLE public.content_attempt_llm_calls ADD CONSTRAINT content_attempt_llm_calls_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.content_attempt_llm_calls ADD CONSTRAINT content_attempt_llm_calls_attempt_event_id_provider_call_id_key UNIQUE (attempt_event_id, provider_call_id)',
        'ALTER TABLE public.content_attempt_provider_admissions ADD CONSTRAINT content_attempt_provider_admissions_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.content_attempt_provider_admissions ADD CONSTRAINT content_attempt_provider_admi_attempt_event_id_call_ordinal_key UNIQUE (attempt_event_id, call_ordinal)',
        'ALTER TABLE public.content_identity_upgrade_control ADD CONSTRAINT content_identity_upgrade_control_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.content_legacy_observations ADD CONSTRAINT content_legacy_observations_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.content_request_links ADD CONSTRAINT content_request_links_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.content_request_links ADD CONSTRAINT content_request_links_request_id_key UNIQUE (request_id)',
        'ALTER TABLE public.content_work_admission_events ADD CONSTRAINT content_work_admission_events_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.content_work_identities ADD CONSTRAINT content_work_identities_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.content_work_identities ADD CONSTRAINT content_work_identities_canonical_resource_type_canonical_r_key UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version)',
        'ALTER TABLE public.content_work_identities ADD CONSTRAINT content_work_identities_id_canonical_resource_type_canonica_key UNIQUE (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version)',
        'ALTER TABLE public.content_work_identity_members ADD CONSTRAINT content_work_identity_members_pkey PRIMARY KEY (work_item_id)',
        'ALTER TABLE public.content_work_identity_members ADD CONSTRAINT content_work_identity_members_identity_id_work_item_id_key UNIQUE (identity_id, work_item_id)',
        'ALTER TABLE public.content_current_result_projections ADD CONSTRAINT content_current_result_projections_pkey PRIMARY KEY (identity_id)',
        'ALTER TABLE public.follower_current_members ADD CONSTRAINT follower_current_members_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.follower_current_members ADD CONSTRAINT follower_current_members_user_id_actor_github_user_id_key UNIQUE (user_id, actor_github_user_id)',
        'ALTER TABLE public.follower_sync_baselines ADD CONSTRAINT follower_sync_baselines_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.follower_sync_baselines ADD CONSTRAINT follower_sync_baselines_user_id_key UNIQUE (user_id)',
        'ALTER TABLE public.llm_calls ADD CONSTRAINT llm_calls_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.llm_call_events ADD CONSTRAINT llm_call_events_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.notifications ADD CONSTRAINT notifications_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.notifications ADD CONSTRAINT notifications_user_id_thread_id_key UNIQUE (user_id, thread_id)',
        'ALTER TABLE public.online_migration_leases ADD CONSTRAINT online_migration_leases_pkey PRIMARY KEY (lease_name)',
        'ALTER TABLE public.owned_repo_star_baselines ADD CONSTRAINT owned_repo_star_baselines_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.owned_repo_star_baselines ADD CONSTRAINT owned_repo_star_baselines_user_id_repo_id_key UNIQUE (user_id, repo_id)',
        'ALTER TABLE public.public_metrics_hourly_snapshots ADD CONSTRAINT public_metrics_hourly_snapshots_pkey PRIMARY KEY (sampled_hour)',
        'ALTER TABLE public.public_repo_release_usage ADD CONSTRAINT public_repo_release_usage_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.public_repo_release_usage ADD CONSTRAINT public_repo_release_usage_full_name_lower_key UNIQUE (full_name_lower)',
        'ALTER TABLE public.reaction_pat_tokens ADD CONSTRAINT reaction_pat_tokens_pkey PRIMARY KEY (user_id)',
        'ALTER TABLE public.release_evidence_index ADD CONSTRAINT release_evidence_index_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.release_evidence_snapshots ADD CONSTRAINT release_evidence_snapshots_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.release_evidence_snapshots ADD CONSTRAINT release_evidence_snapshots_request_key_key UNIQUE (request_key)',
        'ALTER TABLE public.release_evidence_snapshot_items ADD CONSTRAINT release_evidence_snapshot_items_pkey PRIMARY KEY (snapshot_id, index_id)',
        'ALTER TABLE public.release_reading_backfill_runs ADD CONSTRAINT release_reading_backfill_runs_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.releases ADD CONSTRAINT releases_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.releases ADD CONSTRAINT releases_user_id_release_id_key UNIQUE (user_id, release_id)',
        'ALTER TABLE public.repo_refresh_governance_cycles ADD CONSTRAINT repo_refresh_governance_cycles_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.repo_refresh_governance_cycle_members ADD CONSTRAINT repo_refresh_governance_cycle_members_pkey PRIMARY KEY (cycle_id, repo_id)',
        'ALTER TABLE public.repo_refresh_governance_retention_state ADD CONSTRAINT repo_refresh_governance_retention_state_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.repo_refresh_governance_snapshots ADD CONSTRAINT repo_refresh_governance_snapshots_pkey PRIMARY KEY (repo_id)',
        'ALTER TABLE public.repo_release_sync_state ADD CONSTRAINT repo_release_sync_state_pkey PRIMARY KEY (repo_id)',
        'ALTER TABLE public.repo_release_work_items ADD CONSTRAINT repo_release_work_items_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.repo_release_work_items ADD CONSTRAINT repo_release_work_items_repo_id_key UNIQUE (repo_id)',
        'ALTER TABLE public.repo_release_watchers ADD CONSTRAINT repo_release_watchers_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.repo_release_watchers ADD CONSTRAINT repo_release_watchers_task_id_work_item_id_key UNIQUE (task_id, work_item_id)',
        'ALTER TABLE public.repo_releases ADD CONSTRAINT repo_releases_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.repo_releases ADD CONSTRAINT repo_releases_release_id_key UNIQUE (release_id)',
        'ALTER TABLE public.release_reading_backfill_items ADD CONSTRAINT release_reading_backfill_items_pkey PRIMARY KEY (run_id, release_id)',
        'ALTER TABLE public.repo_star_current_members ADD CONSTRAINT repo_star_current_members_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.repo_star_current_members ADD CONSTRAINT repo_star_current_members_user_id_repo_id_actor_github_user_key UNIQUE (user_id, repo_id, actor_github_user_id)',
        'ALTER TABLE public.repo_star_sync_baselines ADD CONSTRAINT repo_star_sync_baselines_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.repo_star_sync_baselines ADD CONSTRAINT repo_star_sync_baselines_user_id_key UNIQUE (user_id)',
        'ALTER TABLE public.scheduled_task_dispatch_state ADD CONSTRAINT scheduled_task_dispatch_state_pkey PRIMARY KEY (schedule_name)',
        'ALTER TABLE public.search_fts_document_rows ADD CONSTRAINT search_fts_document_rows_pkey PRIMARY KEY (fts_rowid)',
        'ALTER TABLE public.search_fts_document_rows ADD CONSTRAINT search_fts_document_rows_document_id_key UNIQUE (document_id)',
        'ALTER TABLE public.search_fts_user_lane_rows ADD CONSTRAINT search_fts_user_lane_rows_pkey PRIMARY KEY (fts_rowid)',
        'ALTER TABLE public.search_fts_user_lane_rows ADD CONSTRAINT search_fts_user_lane_rows_document_id_user_id_key UNIQUE (document_id, user_id)',
        'ALTER TABLE public.search_metadata_backfill_queue ADD CONSTRAINT search_metadata_backfill_queue_pkey PRIMARY KEY (repo_id)',
        'ALTER TABLE public.search_projection_backfill_state ADD CONSTRAINT search_projection_backfill_state_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.search_projection_outbox ADD CONSTRAINT search_projection_outbox_pkey PRIMARY KEY (projection_key)',
        'ALTER TABLE public.search_projection_outbox_failed ADD CONSTRAINT search_projection_outbox_failed_pkey PRIMARY KEY (projection_key)',
        'ALTER TABLE public.search_rate_limits ADD CONSTRAINT search_rate_limits_pkey PRIMARY KEY (user_id)',
        'ALTER TABLE public.social_activity_events ADD CONSTRAINT social_activity_events_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.star_sync_epochs ADD CONSTRAINT star_sync_epochs_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.starred_repo_connection_memberships ADD CONSTRAINT starred_repo_connection_memberships_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.starred_repo_connection_memberships ADD CONSTRAINT starred_repo_connection_membe_user_id_github_connection_id__key UNIQUE (user_id, github_connection_id, repo_id)',
        'ALTER TABLE public.starred_repos ADD CONSTRAINT starred_repos_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.starred_repos ADD CONSTRAINT starred_repos_user_id_repo_id_key UNIQUE (user_id, repo_id)',
        'ALTER TABLE public.sync_state ADD CONSTRAINT sync_state_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.sync_state ADD CONSTRAINT sync_state_user_id_key_key UNIQUE (user_id, key)',
        'ALTER TABLE public.sync_subscription_events ADD CONSTRAINT sync_subscription_events_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.user_repo_associations ADD CONSTRAINT user_repo_associations_pkey PRIMARY KEY (id)',
        'ALTER TABLE public.user_repo_associations ADD CONSTRAINT user_repo_associations_user_id_repo_full_name_lower_key UNIQUE (user_id, repo_full_name_lower)',
        'ALTER TABLE public.webhook_push_deliveries ADD CONSTRAINT webhook_push_deliveries_pkey PRIMARY KEY (delivery_id)',
        'ALTER TABLE public.webhook_push_reconcile_demands ADD CONSTRAINT webhook_push_reconcile_demands_pkey PRIMARY KEY (user_id)',
        'ALTER TABLE public.webhook_push_repos ADD CONSTRAINT webhook_push_repos_pkey PRIMARY KEY (user_id, repo_id)',
        'ALTER TABLE public.webhook_push_user_operation_leases ADD CONSTRAINT webhook_push_user_operation_leases_pkey PRIMARY KEY (user_id)'
    ] LOOP
        BEGIN
            EXECUTE statement;
        EXCEPTION
            WHEN duplicate_object THEN NULL;
            -- Imported databases already carry a primary key under the name the
            -- legacy translation produced, so the name guard above cannot see
            -- it and PostgreSQL rejects a second primary key (42P16).
            WHEN invalid_table_definition THEN NULL;
            -- Imported databases already own a unique index with the constraint
            -- name, which PostgreSQL reports as an existing relation (42P07).
            WHEN duplicate_table THEN NULL;
        END;
    END LOOP;
END
$$;

-- The observation identity includes the source hash, which the earliest
-- imported shape predates. Guard the constraint on the column so a pre-hash
-- database still starts instead of failing on an unknown column.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = 'public'
          AND table_name = 'content_legacy_observations'
          AND column_name = 'legacy_source_hash'
    ) THEN
        BEGIN
            ALTER TABLE public.content_legacy_observations
                ADD CONSTRAINT content_legacy_observations_legacy_table_legacy_primary_key_key
                UNIQUE (legacy_table, legacy_primary_key, legacy_source_hash);
        EXCEPTION
            WHEN duplicate_object THEN NULL;
            WHEN invalid_table_definition THEN NULL;
            WHEN duplicate_table THEN NULL;
        END;
    END IF;
END
$$;

-- --------------------------------------------------------------- indexes
-- Needed before the foreign keys below: composite FKs reference these unique
-- indexes. Indexes that baseline migrations already created are skipped.
CREATE UNIQUE INDEX IF NOT EXISTS idx_briefs_user_window_unique ON public.briefs USING btree (user_id, window_start_utc, window_end_utc) WHERE ((window_start_utc IS NOT NULL) AND (window_end_utc IS NOT NULL));
CREATE UNIQUE INDEX IF NOT EXISTS idx_content_attempt_events_identity ON public.content_attempt_events USING btree (id, work_item_id, attempt_no);
CREATE UNIQUE INDEX IF NOT EXISTS idx_content_work_admission_events_identity ON public.content_work_admission_events USING btree (work_item_id, event_type, COALESCE(replaced_by_work_item_id, ''::text), source_hash);
CREATE UNIQUE INDEX IF NOT EXISTS idx_content_work_items_identity_fk ON public.content_work_items USING btree (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version);
CREATE INDEX IF NOT EXISTS idx_github_connections_user_login ON public.github_connections USING btree (user_id, login);
CREATE INDEX IF NOT EXISTS idx_job_task_events_pg_task_id_id ON public.job_task_events USING btree (task_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_job_tasks_pg_status_created_at ON public.job_tasks USING btree (status, created_at, id);
CREATE INDEX IF NOT EXISTS idx_job_tasks_pg_type_status ON public.job_tasks USING btree (task_type, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_release_evidence_index_scope_key ON public.release_evidence_index USING btree (repo_id, audience, COALESCE(audience_user_id, ''::text), resource_kind, resource_key, side);
CREATE INDEX IF NOT EXISTS idx_runtime_owners_pg_lease_heartbeat ON public.runtime_owners USING btree (lease_heartbeat_at);
CREATE INDEX IF NOT EXISTS idx_search_documents_pg_trgm_title ON public.search_documents USING gin (title octo_rill.gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_search_documents_pg_user_type_time ON public.search_documents USING btree (user_id, resource_type, source_time DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_search_documents_pg_vector ON public.search_documents USING gin (search_vector);
CREATE INDEX IF NOT EXISTS idx_search_lanes_pg_vector ON public.search_document_user_lanes USING gin (search_vector);
CREATE UNIQUE INDEX IF NOT EXISTS idx_social_activity_events_github_event_dedupe ON public.social_activity_events USING btree (user_id, kind, github_event_id) WHERE (github_event_id IS NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS idx_star_sync_epochs_one_active_connection ON public.star_sync_epochs USING btree (github_connection_id) WHERE (status = ANY (ARRAY['queued'::text, 'running'::text]));
CREATE UNIQUE INDEX IF NOT EXISTS idx_translation_requests_request_key ON public.translation_requests USING btree (mode, request_origin, scope_user_id, producer_ref, kind, variant, entity_id, target_lang, source_hash);
CREATE INDEX IF NOT EXISTS idx_user_api_keys_hash_active ON public.user_api_keys USING btree (key_hash) WHERE (revoked_at IS NULL);
CREATE INDEX IF NOT EXISTS idx_user_api_keys_user_created_at ON public.user_api_keys USING btree (user_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_user_passkeys_user_created_at ON public.user_passkeys USING btree (user_id, created_at, id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_passkey_user_handle_uuid ON public.users USING btree (passkey_user_handle_uuid) WHERE (passkey_user_handle_uuid IS NOT NULL);
CREATE INDEX IF NOT EXISTS idx_users_pause_maintenance ON public.users USING btree (is_disabled, paused_at, last_active_at, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_webhook_push_callback_key ON public.users USING btree (webhook_push_callback_key) WHERE (webhook_push_callback_key IS NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS idx_webhook_push_audit_inflight ON public.job_tasks USING btree (task_type) WHERE ((task_type = 'webhook.push.audit'::text) AND (status = ANY (ARRAY['queued'::text, 'running'::text])));
CREATE UNIQUE INDEX IF NOT EXISTS idx_webhook_push_manage_inflight ON public.job_tasks USING btree (task_type, requested_by) WHERE ((task_type = 'webhook.push.manage'::text) AND (requested_by IS NOT NULL) AND (status = ANY (ARRAY['queued'::text, 'running'::text])));
CREATE UNIQUE INDEX IF NOT EXISTS idx_webhook_push_repos_hook_id ON public.webhook_push_repos USING btree (hook_id) WHERE (hook_id IS NOT NULL);

-- ----------------------------------------------------------- foreign keys
-- Guarded so re-running the migration over an imported database is a no-op.
DO $$
DECLARE
    statement text;
BEGIN
    FOREACH statement IN ARRAY ARRAY[
        'ALTER TABLE public.ai_translations ADD CONSTRAINT ai_translations_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.content_attempt_events ADD CONSTRAINT content_attempt_events_work_item_id_fkey FOREIGN KEY (work_item_id) REFERENCES public.content_work_items(id) ON DELETE CASCADE',
        'ALTER TABLE public.content_attempt_llm_calls ADD CONSTRAINT content_attempt_llm_calls_attempt_event_id_fkey FOREIGN KEY (attempt_event_id) REFERENCES public.content_attempt_events(id) ON DELETE CASCADE',
        'ALTER TABLE public.content_attempt_provider_admissions ADD CONSTRAINT content_attempt_provider_admi_attempt_event_id_work_item_i_fkey FOREIGN KEY (attempt_event_id, work_item_id, attempt_no) REFERENCES public.content_attempt_events(id, work_item_id, attempt_no) ON DELETE CASCADE',
        'ALTER TABLE public.content_attempt_provider_admissions ADD CONSTRAINT content_attempt_provider_admissions_work_item_id_fkey FOREIGN KEY (work_item_id) REFERENCES public.content_work_items(id) ON DELETE CASCADE',
        'ALTER TABLE public.content_request_links ADD CONSTRAINT content_request_links_work_item_id_fkey FOREIGN KEY (work_item_id) REFERENCES public.content_work_items(id)',
        'ALTER TABLE public.content_work_admission_events ADD CONSTRAINT content_work_admission_events_replaced_by_work_item_id_fkey FOREIGN KEY (replaced_by_work_item_id) REFERENCES public.content_work_items(id)',
        'ALTER TABLE public.content_work_admission_events ADD CONSTRAINT content_work_admission_events_work_item_id_fkey FOREIGN KEY (work_item_id) REFERENCES public.content_work_items(id)',
        'ALTER TABLE public.content_work_identity_members ADD CONSTRAINT content_work_identity_members_identity_id_canonical_resour_fkey FOREIGN KEY (identity_id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version) REFERENCES public.content_work_identities(id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version) ON DELETE RESTRICT',
        'ALTER TABLE public.content_work_identity_members ADD CONSTRAINT content_work_identity_members_work_item_id_canonical_resou_fkey FOREIGN KEY (work_item_id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version) REFERENCES public.content_work_items(id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version) ON DELETE RESTRICT',
        'ALTER TABLE public.content_current_result_projections ADD CONSTRAINT content_current_result_projections_identity_id_fkey FOREIGN KEY (identity_id) REFERENCES public.content_work_identities(id) ON DELETE RESTRICT',
        'ALTER TABLE public.content_current_result_projections ADD CONSTRAINT content_current_result_projection_identity_id_work_item_id_fkey FOREIGN KEY (identity_id, work_item_id) REFERENCES public.content_work_identity_members(identity_id, work_item_id) ON DELETE RESTRICT',
        'ALTER TABLE public.content_current_result_projections ADD CONSTRAINT content_current_result_projections_active_work_item_id_fkey FOREIGN KEY (active_work_item_id) REFERENCES public.content_work_items(id) ON DELETE RESTRICT',
        'ALTER TABLE public.content_current_result_projections ADD CONSTRAINT content_current_result_projections_source_projection_id_fkey FOREIGN KEY (source_projection_id) REFERENCES public.content_result_projections(id) ON DELETE RESTRICT',
        'ALTER TABLE public.follower_current_members ADD CONSTRAINT follower_current_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.follower_sync_baselines ADD CONSTRAINT follower_sync_baselines_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.llm_calls ADD CONSTRAINT llm_calls_parent_task_id_fkey FOREIGN KEY (parent_task_id) REFERENCES public.job_tasks(id) ON DELETE SET NULL',
        'ALTER TABLE public.llm_calls ADD CONSTRAINT llm_calls_requested_by_fkey FOREIGN KEY (requested_by) REFERENCES public.users(id) ON DELETE SET NULL',
        'ALTER TABLE public.llm_call_events ADD CONSTRAINT llm_call_events_call_id_fkey FOREIGN KEY (call_id) REFERENCES public.llm_calls(id) ON DELETE CASCADE',
        'ALTER TABLE public.notifications ADD CONSTRAINT notifications_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.owned_repo_star_baselines ADD CONSTRAINT owned_repo_star_baselines_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.reaction_pat_tokens ADD CONSTRAINT reaction_pat_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.release_evidence_snapshot_items ADD CONSTRAINT release_evidence_snapshot_items_index_id_fkey FOREIGN KEY (index_id) REFERENCES public.release_evidence_index(id) ON DELETE RESTRICT',
        'ALTER TABLE public.release_evidence_snapshot_items ADD CONSTRAINT release_evidence_snapshot_items_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES public.release_evidence_snapshots(id) ON DELETE CASCADE',
        'ALTER TABLE public.releases ADD CONSTRAINT releases_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.repo_refresh_governance_cycle_members ADD CONSTRAINT repo_refresh_governance_cycle_members_cycle_id_fkey FOREIGN KEY (cycle_id) REFERENCES public.repo_refresh_governance_cycles(id) ON DELETE CASCADE',
        'ALTER TABLE public.repo_release_watchers ADD CONSTRAINT repo_release_watchers_work_item_id_fkey FOREIGN KEY (work_item_id) REFERENCES public.repo_release_work_items(id) ON DELETE CASCADE',
        'ALTER TABLE public.repo_release_watchers ADD CONSTRAINT repo_release_watchers_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.job_tasks(id) ON DELETE CASCADE',
        'ALTER TABLE public.repo_release_watchers ADD CONSTRAINT repo_release_watchers_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL',
        'ALTER TABLE public.release_reading_backfill_items ADD CONSTRAINT release_reading_backfill_items_run_id_fkey FOREIGN KEY (run_id) REFERENCES public.release_reading_backfill_runs(id) ON DELETE CASCADE',
        'ALTER TABLE public.release_reading_backfill_items ADD CONSTRAINT release_reading_backfill_items_release_id_fkey FOREIGN KEY (release_id) REFERENCES public.repo_releases(release_id) ON DELETE CASCADE',
        'ALTER TABLE public.repo_star_current_members ADD CONSTRAINT repo_star_current_members_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.repo_star_sync_baselines ADD CONSTRAINT repo_star_sync_baselines_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.search_rate_limits ADD CONSTRAINT search_rate_limits_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.social_activity_events ADD CONSTRAINT social_activity_events_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.star_sync_epochs ADD CONSTRAINT star_sync_epochs_github_connection_id_fkey FOREIGN KEY (github_connection_id) REFERENCES public.github_connections(id) ON DELETE CASCADE',
        'ALTER TABLE public.star_sync_epochs ADD CONSTRAINT star_sync_epochs_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.starred_repo_connection_memberships ADD CONSTRAINT starred_repo_connection_memberships_github_connection_id_fkey FOREIGN KEY (github_connection_id) REFERENCES public.github_connections(id) ON DELETE CASCADE',
        'ALTER TABLE public.starred_repo_connection_memberships ADD CONSTRAINT starred_repo_connection_memberships_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.starred_repos ADD CONSTRAINT starred_repos_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.sync_state ADD CONSTRAINT sync_state_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.sync_subscription_events ADD CONSTRAINT sync_subscription_events_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.job_tasks(id) ON DELETE CASCADE',
        'ALTER TABLE public.user_repo_associations ADD CONSTRAINT user_repo_associations_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.webhook_push_reconcile_demands ADD CONSTRAINT webhook_push_reconcile_demands_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.webhook_push_repos ADD CONSTRAINT webhook_push_repos_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.webhook_push_user_operation_leases ADD CONSTRAINT webhook_push_user_operation_leases_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE',
        'ALTER TABLE public.brief_release_memberships ADD CONSTRAINT brief_release_memberships_release_id_fkey FOREIGN KEY (release_id) REFERENCES public.repo_releases(release_id) ON DELETE CASCADE'
    ] LOOP
        BEGIN
            EXECUTE statement;
        EXCEPTION
            WHEN duplicate_object THEN NULL;
            -- Imported databases already carry a primary key under the name the
            -- legacy translation produced, so the name guard above cannot see
            -- it and PostgreSQL rejects a second primary key (42P16).
            WHEN invalid_table_definition THEN NULL;
            -- Imported databases already own a unique index with the constraint
            -- name, which PostgreSQL reports as an existing relation (42P07).
            WHEN duplicate_table THEN NULL;
        END;
    END LOOP;
END
$$;

-- ---------------------------------------------------------------- views
-- Ordered so a view is replaced only after the views it reads exist.

CREATE OR REPLACE VIEW public.user_release_visible_repos AS
 SELECT ura.user_id,
    ura.repo_id,
    ura.repo_full_name AS full_name,
    ura.owner_login,
    ura.repo_name AS name,
    ura.description,
    COALESCE(ura.html_url, ('https://github.com/'::text || ura.repo_full_name)) AS html_url,
    sr.stargazed_at,
    ura.is_private,
    ura.last_seen_at AS updated_at,
    ura.owner_avatar_url,
    ura.open_graph_image_url,
    ura.uses_custom_open_graph_image
   FROM ((public.user_repo_associations ura
     JOIN public.users u ON ((u.id = ura.user_id)))
     LEFT JOIN public.starred_repos sr ON (((sr.user_id = ura.user_id) AND (lower(sr.full_name) = ura.repo_full_name_lower))))
  WHERE ((ura.is_following <> 0) AND (ura.repo_id IS NOT NULL) AND ((ura.has_personal_owned_source = 0) OR (ura.has_github_star_source <> 0) OR (u.include_own_releases <> 0)))
UNION ALL
 SELECT sr.user_id,
    sr.repo_id,
    sr.full_name,
    sr.owner_login,
    sr.name,
    sr.description,
    sr.html_url,
    sr.stargazed_at,
    sr.is_private,
    sr.updated_at,
    sr.owner_avatar_url,
    sr.open_graph_image_url,
    sr.uses_custom_open_graph_image
   FROM public.starred_repos sr
  WHERE (NOT (EXISTS ( SELECT 1
           FROM public.user_repo_associations ura
          WHERE ((ura.user_id = sr.user_id) AND (ura.repo_full_name_lower = lower(sr.full_name))))))
UNION ALL
 SELECT ob.user_id,
    ob.repo_id,
    ob.repo_full_name AS full_name,
        CASE
            WHEN (strpos(ob.repo_full_name, '/'::text) > 0) THEN substr(ob.repo_full_name, 1, (strpos(ob.repo_full_name, '/'::text) - 1))
            ELSE ob.repo_full_name
        END AS owner_login,
        CASE
            WHEN (strpos(ob.repo_full_name, '/'::text) > 0) THEN substr(ob.repo_full_name, (strpos(ob.repo_full_name, '/'::text) + 1))
            ELSE ob.repo_full_name
        END AS name,
    NULL::text AS description,
    ('https://github.com/'::text || ob.repo_full_name) AS html_url,
    NULL::text AS stargazed_at,
    ob.is_private,
    ob.updated_at,
    ob.owner_avatar_url,
    ob.open_graph_image_url,
    ob.uses_custom_open_graph_image
   FROM (public.owned_repo_star_baselines ob
     JOIN public.users u ON ((u.id = ob.user_id)))
  WHERE ((u.include_own_releases <> 0) AND (NOT (EXISTS ( SELECT 1
           FROM public.user_repo_associations ura
          WHERE ((ura.user_id = ob.user_id) AND (ura.repo_full_name_lower = lower(ob.repo_full_name)))))) AND (NOT (EXISTS ( SELECT 1
           FROM public.starred_repos sr
          WHERE ((sr.user_id = ob.user_id) AND (sr.repo_id = ob.repo_id))))));

CREATE OR REPLACE VIEW public.search_release_metadata AS
 SELECT d.id,
    d.repo_id,
    COALESCE(( SELECT rwi.repo_full_name
           FROM public.repo_release_work_items rwi
          WHERE (rwi.repo_id = d.repo_id)
         LIMIT 1), ( SELECT vr.full_name
           FROM public.user_release_visible_repos vr
          WHERE (vr.repo_id = d.repo_id)
         LIMIT 1), ( SELECT ura.repo_full_name
           FROM public.user_repo_associations ura
          WHERE (ura.repo_id = d.repo_id)
         LIMIT 1), ( SELECT sr.full_name
           FROM public.starred_repos sr
          WHERE (sr.repo_id = d.repo_id)
         LIMIT 1)) AS repo_full_name,
    COALESCE(( SELECT
                CASE
                    WHEN (strpos(rwi.repo_full_name, '/'::text) > 0) THEN substr(rwi.repo_full_name, 1, (strpos(rwi.repo_full_name, '/'::text) - 1))
                    ELSE rwi.repo_full_name
                END AS repo_full_name
           FROM public.repo_release_work_items rwi
          WHERE (rwi.repo_id = d.repo_id)
         LIMIT 1), ( SELECT vr.owner_login
           FROM public.user_release_visible_repos vr
          WHERE (vr.repo_id = d.repo_id)
         LIMIT 1), ( SELECT ura.owner_login
           FROM public.user_repo_associations ura
          WHERE (ura.repo_id = d.repo_id)
         LIMIT 1), ( SELECT sr.owner_login
           FROM public.starred_repos sr
          WHERE (sr.repo_id = d.repo_id)
         LIMIT 1)) AS owner_login,
        CASE
            WHEN (COALESCE(( SELECT rwi.repo_full_name
               FROM public.repo_release_work_items rwi
              WHERE (rwi.repo_id = d.repo_id)
             LIMIT 1), ( SELECT vr.full_name
               FROM public.user_release_visible_repos vr
              WHERE (vr.repo_id = d.repo_id)
             LIMIT 1), ( SELECT ura.repo_full_name
               FROM public.user_repo_associations ura
              WHERE (ura.repo_id = d.repo_id)
             LIMIT 1), ( SELECT sr.full_name
               FROM public.starred_repos sr
              WHERE (sr.repo_id = d.repo_id)
             LIMIT 1)) IS NOT NULL) THEN ((('/'::text || COALESCE(( SELECT rwi.repo_full_name
               FROM public.repo_release_work_items rwi
              WHERE (rwi.repo_id = d.repo_id)
             LIMIT 1), ( SELECT vr.full_name
               FROM public.user_release_visible_repos vr
              WHERE (vr.repo_id = d.repo_id)
             LIMIT 1), ( SELECT ura.repo_full_name
               FROM public.user_repo_associations ura
              WHERE (ura.repo_id = d.repo_id)
             LIMIT 1), ( SELECT sr.full_name
               FROM public.starred_repos sr
              WHERE (sr.repo_id = d.repo_id)
             LIMIT 1))) || '/releases/tag/'::text) || rr.tag_name)
            ELSE NULL::text
        END AS target_path
   FROM (public.search_documents d
     LEFT JOIN public.repo_releases rr ON ((rr.release_id = (d.resource_id)::bigint)))
  WHERE (d.resource_type = 'release'::text);

CREATE OR REPLACE VIEW public.user_following_repos AS
 SELECT user_repo_associations.user_id,
    user_repo_associations.repo_id,
    user_repo_associations.repo_full_name AS full_name,
    user_repo_associations.owner_login,
    user_repo_associations.repo_name AS name,
    user_repo_associations.html_url,
    user_repo_associations.description,
    user_repo_associations.is_private,
    user_repo_associations.last_seen_at AS updated_at,
    user_repo_associations.owner_avatar_url,
    user_repo_associations.open_graph_image_url,
    user_repo_associations.uses_custom_open_graph_image,
    user_repo_associations.first_source,
    user_repo_associations.first_associated_at,
    user_repo_associations.follow_state_source,
    user_repo_associations.has_personal_owned_source,
    user_repo_associations.has_github_star_source,
    user_repo_associations.has_manual_feed_source
   FROM public.user_repo_associations
  WHERE ((user_repo_associations.is_following <> 0) AND (user_repo_associations.repo_id IS NOT NULL))
UNION ALL
 SELECT sr.user_id,
    sr.repo_id,
    sr.full_name,
    sr.owner_login,
    sr.name,
    sr.html_url,
    sr.description,
    sr.is_private,
    sr.updated_at,
    sr.owner_avatar_url,
    sr.open_graph_image_url,
    sr.uses_custom_open_graph_image,
    'github_star'::text AS first_source,
    sr.stargazed_at AS first_associated_at,
    'system_default'::text AS follow_state_source,
    0 AS has_personal_owned_source,
    1 AS has_github_star_source,
    0 AS has_manual_feed_source
   FROM public.starred_repos sr
  WHERE (NOT (EXISTS ( SELECT 1
           FROM public.user_repo_associations ura
          WHERE ((ura.user_id = sr.user_id) AND (ura.repo_full_name_lower = lower(sr.full_name))))))
UNION ALL
 SELECT ob.user_id,
    ob.repo_id,
    ob.repo_full_name AS full_name,
        CASE
            WHEN (strpos(ob.repo_full_name, '/'::text) > 0) THEN substr(ob.repo_full_name, 1, (strpos(ob.repo_full_name, '/'::text) - 1))
            ELSE ob.repo_full_name
        END AS owner_login,
        CASE
            WHEN (strpos(ob.repo_full_name, '/'::text) > 0) THEN substr(ob.repo_full_name, (strpos(ob.repo_full_name, '/'::text) + 1))
            ELSE ob.repo_full_name
        END AS name,
    ('https://github.com/'::text || ob.repo_full_name) AS html_url,
    NULL::text AS description,
    ob.is_private,
    ob.updated_at,
    ob.owner_avatar_url,
    ob.open_graph_image_url,
    ob.uses_custom_open_graph_image,
    'personal_owned'::text AS first_source,
    COALESCE(ob.initialized_at, ob.updated_at) AS first_associated_at,
    'system_default'::text AS follow_state_source,
    1 AS has_personal_owned_source,
    0 AS has_github_star_source,
    0 AS has_manual_feed_source
   FROM public.owned_repo_star_baselines ob
  WHERE (NOT (EXISTS ( SELECT 1
           FROM public.user_repo_associations ura
          WHERE ((ura.user_id = ob.user_id) AND (ura.repo_full_name_lower = lower(ob.repo_full_name))))));
