-- PostgreSQL indexes for stable ordering, ownership filters and worker leases.
CREATE INDEX idx_releases_user_published_at
  ON releases(user_id, published_at);
CREATE INDEX idx_job_tasks_status_created_at
  ON job_tasks(status, created_at DESC);
CREATE INDEX idx_job_tasks_task_type_created_at
  ON job_tasks(task_type, created_at DESC);
CREATE INDEX idx_job_task_events_task_id_created_at
  ON job_task_events(task_id, created_at, id);
CREATE INDEX idx_llm_calls_parent_translation_batch_id
  ON llm_calls(parent_translation_batch_id);
CREATE INDEX idx_llm_call_events_call_id_created_at
  ON llm_call_events(call_id, created_at, id);
CREATE INDEX idx_sync_subscription_events_task_id_created_at
  ON sync_subscription_events(task_id, created_at, id);
CREATE INDEX idx_sync_subscription_events_user_id_created_at
  ON sync_subscription_events(user_id, created_at DESC);
CREATE INDEX idx_sync_subscription_events_repo_id_created_at
  ON sync_subscription_events(repo_id, created_at DESC);
CREATE INDEX idx_sync_subscription_events_repo_full_name_created_at
  ON sync_subscription_events(repo_full_name, created_at DESC);
CREATE INDEX idx_llm_calls_admin_sort
  ON llm_calls(
    (CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END),
    created_at DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_created_at
  ON llm_calls(
    created_at DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_status_created_at
  ON llm_calls(
    status,
    created_at DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_requested_by_created_at
  ON llm_calls(
    requested_by,
    created_at DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_source_created_at
  ON llm_calls(
    source,
    created_at DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_parent_task_id
  ON llm_calls(
    parent_task_id,
    created_at DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_source_admin_sort
  ON llm_calls(
    source,
    (CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END),
    created_at DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_requested_by_admin_sort
  ON llm_calls(
    requested_by,
    (CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END),
    created_at DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_translation_work_items_status_partition
  ON translation_work_items(status, target_lang, protocol_version, model_profile, created_at ASC);
CREATE INDEX idx_translation_work_items_deadline
  ON translation_work_items(status, deadline_at ASC);
CREATE INDEX idx_translation_work_items_batch_id
  ON translation_work_items(batch_id);
CREATE INDEX idx_translation_requests_status_updated_at
  ON translation_requests(status, updated_at DESC, id DESC);
CREATE INDEX idx_translation_requests_work_item_id
  ON translation_requests(work_item_id);
CREATE INDEX idx_translation_requests_origin_work_item_id
  ON translation_requests(request_origin, work_item_id);
CREATE INDEX idx_translation_requests_scope_created_at
  ON translation_requests(scope_user_id, created_at DESC);
CREATE INDEX idx_translation_batches_status_created_at
  ON translation_batches(status, created_at DESC);
CREATE INDEX idx_translation_batches_target_lang_created_at
  ON translation_batches(target_lang, created_at DESC);
CREATE INDEX idx_translation_batch_items_batch_id
  ON translation_batch_items(batch_id, item_index ASC);
CREATE INDEX idx_job_tasks_running_runtime_lease
  ON job_tasks(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';
CREATE INDEX idx_llm_calls_running_runtime_lease
  ON llm_calls(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';
CREATE INDEX idx_translation_batches_running_runtime_lease
  ON translation_batches(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';
CREATE INDEX idx_runtime_owners_lease_heartbeat
  ON runtime_owners(lease_heartbeat_at);
CREATE INDEX idx_repo_releases_repo_published_at
  ON repo_releases(repo_id, published_at DESC, created_at DESC, release_id DESC);
CREATE INDEX idx_repo_releases_updated_at
  ON repo_releases(updated_at DESC, release_id DESC);
CREATE INDEX idx_repo_release_work_items_queue
  ON repo_release_work_items(
    status,
    priority DESC,
    has_new_repo_watchers DESC,
    deadline_at ASC,
    created_at ASC
  );
CREATE INDEX idx_repo_release_work_items_runtime
  ON repo_release_work_items(status, runtime_owner_id, lease_heartbeat_at);
CREATE INDEX idx_repo_release_watchers_task_status
  ON repo_release_watchers(task_id, status, created_at ASC);
CREATE INDEX idx_repo_release_watchers_work_item_status
  ON repo_release_watchers(work_item_id, status, created_at ASC);
CREATE INDEX idx_ai_translations_active_work_item_id
  ON ai_translations(active_work_item_id);
CREATE UNIQUE INDEX idx_translation_requests_request_key
  ON translation_requests(
    mode,
    request_origin,
    scope_user_id,
    producer_ref,
    kind,
    variant,
    entity_id,
    target_lang,
    source_hash
  );
CREATE INDEX idx_owned_repo_star_baselines_user
  ON owned_repo_star_baselines(user_id, repo_id);
CREATE INDEX idx_repo_star_current_members_user_repo
  ON repo_star_current_members(user_id, repo_id, actor_github_user_id);
CREATE INDEX idx_follower_current_members_user
  ON follower_current_members(user_id, actor_github_user_id);
CREATE INDEX idx_briefs_user_date_created_at
  ON briefs(user_id, date DESC, created_at DESC);
CREATE INDEX idx_briefs_user_window_end_created_at
  ON briefs(user_id, window_end_utc DESC, created_at DESC);
CREATE UNIQUE INDEX idx_briefs_user_window_unique
  ON briefs(user_id, window_start_utc, window_end_utc)
  WHERE window_start_utc IS NOT NULL AND window_end_utc IS NOT NULL;
CREATE INDEX idx_brief_release_memberships_release_id
  ON brief_release_memberships(release_id);
CREATE INDEX idx_brief_release_memberships_brief_ordinal
  ON brief_release_memberships(brief_id, ordinal ASC);
CREATE INDEX idx_admin_dashboard_rollups_tz_date
  ON admin_dashboard_daily_rollups(time_zone, rollup_date DESC);
CREATE INDEX idx_github_connections_user_login
  ON github_connections(user_id, login);
CREATE UNIQUE INDEX idx_users_passkey_user_handle_uuid
  ON users(passkey_user_handle_uuid)
  WHERE passkey_user_handle_uuid IS NOT NULL;
CREATE INDEX idx_user_passkeys_user_created_at
  ON user_passkeys(user_id, created_at, id);
CREATE INDEX idx_social_activity_events_user_sort
  ON social_activity_events(user_id, occurred_at DESC, created_at DESC, id DESC);
CREATE INDEX idx_social_activity_events_user_kind_sort
  ON social_activity_events(user_id, kind, occurred_at DESC, created_at DESC, id DESC);
CREATE UNIQUE INDEX idx_social_activity_events_dedupe
  ON social_activity_events(user_id, kind, COALESCE(repo_id, -1), actor_github_user_id, occurred_at)
  WHERE kind NOT IN ('announcement', 'repo_forked');
CREATE UNIQUE INDEX idx_social_activity_events_github_event_dedupe
  ON social_activity_events(user_id, kind, github_event_id)
  WHERE github_event_id IS NOT NULL;
CREATE INDEX idx_repo_release_sync_state_backoff
  ON repo_release_sync_state(backoff_until);
CREATE INDEX idx_public_repo_release_usage_repo_id
  ON public_repo_release_usage(repo_id);
CREATE INDEX idx_public_repo_release_usage_last_requested_at
  ON public_repo_release_usage(last_requested_at DESC);
CREATE INDEX idx_repo_releases_repo_sort_ts
  ON repo_releases(
    repo_id,
    COALESCE(published_at, created_at, updated_at),
    release_id
  );
CREATE INDEX idx_repo_refresh_governance_snapshots_priority
  ON repo_refresh_governance_snapshots(priority_rank ASC, repo_id ASC);
CREATE INDEX idx_repo_refresh_governance_snapshots_cycle
  ON repo_refresh_governance_snapshots(active_cycle_id, active_cycle_completed, active_cycle_window_index, priority_rank);
CREATE INDEX idx_repo_refresh_governance_cycles_status
  ON repo_refresh_governance_cycles(status, started_at DESC);
CREATE INDEX idx_repo_refresh_governance_cycle_members_status
  ON repo_refresh_governance_cycle_members(cycle_id, completed_at, removed_from_pool, repo_id);
CREATE INDEX idx_job_tasks_finished_status
  ON job_tasks(status, finished_at DESC, id DESC)
  WHERE finished_at IS NOT NULL;
CREATE INDEX idx_job_tasks_realtime_created
  ON job_tasks(created_at DESC, id DESC);
CREATE INDEX idx_job_tasks_subscription_chain
  ON job_tasks(parent_task_id, task_type, finished_at DESC)
  WHERE parent_task_id IS NOT NULL;
CREATE INDEX idx_repo_release_watchers_retention
  ON repo_release_watchers(status, updated_at ASC, id ASC);
CREATE INDEX idx_llm_calls_finished_updated_created
  ON llm_calls(
    COALESCE(finished_at, updated_at, created_at) DESC,
    status,
    source,
    id DESC
  );
CREATE INDEX idx_user_api_keys_user_created_at
  ON user_api_keys(user_id, created_at, id);
CREATE INDEX idx_user_api_keys_hash_active
  ON user_api_keys(key_hash)
  WHERE revoked_at IS NULL;
CREATE INDEX idx_public_repo_release_usage_access_kind
  ON public_repo_release_usage(access_kind, repo_id);
CREATE INDEX idx_public_repo_release_usage_publisher
  ON public_repo_release_usage(published_by_user_id, full_name_lower);
CREATE INDEX idx_user_repo_associations_user_following
  ON user_repo_associations(user_id, is_following, repo_full_name_lower);
CREATE INDEX idx_user_repo_associations_user_repo_id
  ON user_repo_associations(user_id, repo_id);
CREATE INDEX idx_user_repo_associations_repo_lower
  ON user_repo_associations(repo_full_name_lower);
CREATE INDEX idx_ai_translations_public_lookup
ON ai_translations(entity_type, lang, entity_id, status, updated_at DESC);
CREATE INDEX idx_starred_repos_repo_public_updated
ON starred_repos(repo_id, is_private, updated_at DESC);
CREATE INDEX idx_owned_repo_star_baselines_repo_updated
ON owned_repo_star_baselines(repo_id, updated_at DESC);
CREATE INDEX idx_social_activity_events_user_repo_discussion
  ON social_activity_events(user_id, kind, repo_full_name, discussion_number, occurred_at DESC, id DESC);
CREATE INDEX idx_starred_repos_public_full_name_updated
ON starred_repos(lower(full_name), updated_at DESC);
CREATE INDEX idx_owned_repo_star_baselines_public_full_name_updated
ON owned_repo_star_baselines(lower(repo_full_name), updated_at DESC);
CREATE INDEX idx_repo_refresh_governance_cycles_retention
  ON repo_refresh_governance_cycles(status, completed_at ASC, id ASC)
  WHERE status = 'completed';
CREATE INDEX idx_users_pause_maintenance
  ON users (is_disabled, paused_at, last_active_at, created_at);
CREATE INDEX idx_repo_release_watchers_task_freshness
  ON repo_release_watchers(task_id, freshness_decision, updated_at ASC, id ASC);
CREATE UNIQUE INDEX idx_users_webhook_push_callback_key
  ON users(webhook_push_callback_key)
  WHERE webhook_push_callback_key IS NOT NULL;
CREATE UNIQUE INDEX idx_webhook_push_repos_hook_id
  ON webhook_push_repos(hook_id)
  WHERE hook_id IS NOT NULL;
CREATE INDEX idx_webhook_push_repos_user_status
  ON webhook_push_repos(user_id, status, repo_full_name);
CREATE INDEX idx_webhook_push_deliveries_received_at
  ON webhook_push_deliveries(received_at);
CREATE UNIQUE INDEX idx_webhook_push_audit_inflight
  ON job_tasks(task_type)
  WHERE task_type = 'webhook.push.audit'
    AND status IN ('queued', 'running');
CREATE INDEX idx_llm_model_health_cooldown_until
  ON llm_model_health(cooldown_until);
CREATE INDEX idx_llm_calls_recovery_scan
  ON llm_calls(failure_class, retry_scheduled_at ASC, created_at ASC)
  WHERE failure_class IS NOT NULL AND retry_scheduled_at IS NOT NULL;
CREATE INDEX idx_translation_work_items_recovery_scan
  ON translation_work_items(failure_class, next_retry_at ASC, retry_expires_at ASC, id ASC)
  WHERE failure_class IS NOT NULL AND next_retry_at IS NOT NULL;
CREATE INDEX idx_repo_releases_dashboard_readable
  ON repo_releases(repo_id, published_at DESC, release_id DESC);
CREATE INDEX idx_social_activity_events_dashboard_readable
  ON social_activity_events(user_id, occurred_at DESC, id DESC);
CREATE INDEX idx_briefs_dashboard_readable
  ON briefs(user_id, date DESC, window_end_utc DESC, id DESC);
CREATE INDEX idx_translation_attempt_events_entity_created
  ON translation_attempt_events(entity_id, kind, variant, created_at ASC, id ASC);
CREATE INDEX idx_translation_attempt_events_work_item_created
  ON translation_attempt_events(work_item_id, created_at ASC, id ASC);
CREATE INDEX idx_translation_attempt_events_batch_id
  ON translation_attempt_events(batch_id, id ASC);
CREATE INDEX idx_translation_requests_created_at
  ON translation_requests(created_at DESC, id DESC);
CREATE INDEX idx_repo_releases_detected_at
  ON repo_releases(detected_at DESC, release_id DESC);
CREATE INDEX idx_llm_calls_parent_brief_id
  ON llm_calls(parent_brief_id, created_at ASC);
CREATE INDEX idx_repo_releases_admin_source_time
  ON repo_releases(
    COALESCE(published_at, created_at, updated_at) DESC,
    release_id DESC
  );
CREATE INDEX idx_social_activity_events_admin_announcement_time
  ON social_activity_events(
    kind,
    occurred_at DESC,
    repo_full_name,
    discussion_number
  );
CREATE INDEX idx_briefs_admin_created_at
  ON briefs(created_at DESC, id DESC);
CREATE INDEX idx_admin_collection_processing_coverage_lookup
  ON admin_collection_processing_coverage(record_kind, record_id, pipeline);
CREATE INDEX idx_translation_attempt_llm_calls_attempt
  ON translation_attempt_llm_calls(work_item_id, attempt_no, ordinal, id);
CREATE INDEX idx_translation_attempt_llm_calls_call
  ON translation_attempt_llm_calls(llm_call_id, created_at, id);
CREATE INDEX idx_llm_diagnostic_access_audit_call_created
  ON llm_diagnostic_access_audit(call_id, created_at DESC, id DESC);
CREATE INDEX idx_llm_diagnostic_access_audit_created
  ON llm_diagnostic_access_audit(created_at);
CREATE INDEX idx_translation_work_items_retry_disposition
  ON translation_work_items(retry_disposition, next_retry_at ASC, updated_at ASC)
  WHERE retry_disposition != 'not_needed';
CREATE INDEX idx_starred_repo_connection_memberships_epoch_prune
  ON starred_repo_connection_memberships(
    user_id,
    github_connection_id,
    last_seen_epoch_id,
    last_delta_seen_at
  );
CREATE INDEX idx_starred_repo_connection_memberships_user_repo
  ON starred_repo_connection_memberships(user_id, repo_id, stargazed_at DESC);
CREATE UNIQUE INDEX idx_star_sync_epochs_one_active_connection
  ON star_sync_epochs(github_connection_id)
  WHERE status IN ('queued', 'running');
CREATE INDEX idx_star_sync_epochs_connection_completed
  ON star_sync_epochs(github_connection_id, status, completed_at DESC);
CREATE INDEX idx_star_sync_epochs_slice_due
  ON star_sync_epochs(status, next_slice_not_before, lease_expires_at);
CREATE INDEX idx_content_work_items_status_priority
  ON content_work_items(status, priority DESC, created_at ASC);
CREATE INDEX idx_content_work_items_retry
  ON content_work_items(status, next_retry_at ASC);
CREATE INDEX idx_content_work_items_resource
  ON content_work_items(canonical_resource_type, canonical_resource_id, pipeline);
CREATE INDEX idx_content_batches_status_created
  ON content_batches(status, created_at ASC);
CREATE INDEX idx_content_batch_items_batch_order
  ON content_batch_items(batch_id, item_index ASC);
CREATE INDEX idx_content_result_projections_resource
  ON content_result_projections(canonical_resource_type, canonical_resource_id, pipeline);
CREATE INDEX idx_content_request_links_work_item
  ON content_request_links(work_item_id, created_at ASC);
CREATE INDEX idx_content_attempt_events_work_item
  ON content_attempt_events(work_item_id, created_at ASC, id ASC);
CREATE INDEX idx_content_attempt_llm_calls_event
  ON content_attempt_llm_calls(attempt_event_id, created_at ASC);
CREATE INDEX idx_content_legacy_observations_resource
  ON content_legacy_observations(canonical_resource_type, canonical_resource_id, pipeline);
CREATE INDEX idx_notifications_thread_id
  ON notifications(thread_id);
CREATE INDEX idx_notifications_admin_canonical_source
  ON notifications(thread_id, updated_at DESC, id DESC);
CREATE INDEX idx_translation_work_items_admin_entity_kind_attempt
  ON translation_work_items(entity_id, kind, attempt_count DESC);
CREATE UNIQUE INDEX idx_webhook_push_manage_inflight
  ON job_tasks(task_type, requested_by)
  WHERE task_type = 'webhook.push.manage'
    AND requested_by IS NOT NULL
    AND status IN ('queued', 'running');
CREATE INDEX idx_job_tasks_available_at
  ON job_tasks(status, available_at, created_at);
CREATE INDEX idx_search_documents_user_type_time
  ON search_documents(user_id, resource_type, source_time DESC, id DESC);
CREATE INDEX idx_search_documents_repo
  ON search_documents(repo_full_name, owner_login, resource_type);
CREATE INDEX idx_search_document_user_lanes_user
  ON search_document_user_lanes(user_id, document_id);
CREATE INDEX idx_webhook_push_reconcile_demands_pending
  ON webhook_push_reconcile_demands(requested_generation, completed_generation, requested_at)
  WHERE requested_generation > completed_generation;
CREATE UNIQUE INDEX idx_content_work_items_identity_fk
  ON content_work_items (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  );
CREATE INDEX idx_content_current_result_projections_work_item
  ON content_current_result_projections(work_item_id);
CREATE INDEX idx_repo_releases_admin_activity_source_time
  ON repo_releases(
    COALESCE(published_at, created_at, updated_at) DESC,
    release_id DESC
  );
CREATE INDEX idx_social_activity_events_admin_activity_time
  ON social_activity_events(
    kind,
    occurred_at DESC,
    lower(repo_full_name),
    discussion_number
  );
CREATE INDEX idx_social_activity_events_admin_activity_canonical
  ON social_activity_events(
    kind,
    lower(repo_full_name),
    discussion_number,
    occurred_at DESC
  );
CREATE INDEX idx_notifications_admin_activity_source_time
  ON notifications(
    updated_at DESC,
    thread_id,
    updated_at DESC,
    id DESC
  );
CREATE INDEX idx_briefs_admin_activity_source_time
  ON briefs(created_at DESC, id DESC);
CREATE INDEX idx_llm_calls_admin_brief_latest
  ON llm_calls(parent_brief_id, updated_at DESC, id DESC);
CREATE UNIQUE INDEX idx_content_work_admission_events_identity
  ON content_work_admission_events(
    work_item_id,
    event_type,
    COALESCE(replaced_by_work_item_id, ''),
    source_hash
  );
CREATE INDEX idx_content_work_admission_events_work_item
  ON content_work_admission_events(work_item_id, created_at ASC, id ASC);
CREATE INDEX idx_content_work_admission_events_retention
  ON content_work_admission_events(created_at ASC, id ASC);
CREATE UNIQUE INDEX idx_content_attempt_events_identity
  ON content_attempt_events(id, work_item_id, attempt_no);
CREATE INDEX idx_content_attempt_provider_admissions_work_item
  ON content_attempt_provider_admissions(work_item_id, attempt_no, call_ordinal);
CREATE INDEX idx_content_attempt_provider_admissions_attempt
  ON content_attempt_provider_admissions(attempt_event_id, call_ordinal);
CREATE INDEX idx_search_documents_resource_repo
  ON search_documents(resource_type, repo_id);
CREATE INDEX idx_search_metadata_backfill_queue_updated
  ON search_metadata_backfill_queue(updated_at, repo_id);
CREATE INDEX idx_user_repo_associations_repo_id
  ON user_repo_associations(repo_id, user_id);
CREATE INDEX idx_ai_translations_search_entity
  ON ai_translations(lang, entity_id, status, updated_at DESC, id DESC, user_id);
CREATE INDEX idx_ai_translations_search_entity_lower
  ON ai_translations(lang, lower(entity_id), status, updated_at DESC, id DESC, user_id);
CREATE INDEX idx_content_work_items_resource_scope_status
  ON content_work_items(
    canonical_resource_type,
    canonical_resource_id,
    pipeline,
    variant,
    target_lang,
    protocol_version,
    status,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_content_work_items_supersession_cursor
  ON content_work_items(created_at ASC, id ASC)
  WHERE status IN ('queued', 'failed', 'deferred_provider', 'blocked_config', 'ready');
CREATE INDEX idx_content_work_items_supersession_history
  ON content_work_items(
    canonical_resource_type,
    canonical_resource_id,
    pipeline,
    variant,
    target_lang,
    protocol_version,
    created_at ASC,
    id ASC
  )
  WHERE status NOT IN ('cancelled', 'superseded');
CREATE INDEX idx_search_projection_outbox_ready
  ON search_projection_outbox(available_at, created_at, projection_key)
  WHERE lease_expires_at IS NULL;
CREATE INDEX idx_search_projection_outbox_lease
  ON search_projection_outbox(lease_expires_at, available_at, created_at);
CREATE INDEX idx_search_documents_resource_key
  ON search_documents(resource_type, resource_id, id);
CREATE INDEX idx_search_projection_outbox_poll
  ON search_projection_outbox(available_at, created_at, projection_key, lease_expires_at);
CREATE INDEX idx_search_projection_outbox_failed_resource
  ON search_projection_outbox_failed(resource_type, canonical_resource_type, resource_id);
CREATE INDEX idx_job_tasks_star_connection_active
  ON job_tasks(task_type, status, payload_json);
CREATE INDEX idx_search_projection_outbox_failed_poll
  ON search_projection_outbox_failed(failed_at, projection_key);
CREATE INDEX idx_job_tasks_task_type_created_at_id
  ON job_tasks(task_type, created_at DESC, id DESC);
CREATE INDEX idx_job_task_events_task_id_event_type
  ON job_task_events(task_id, event_type);
CREATE INDEX idx_job_task_events_task_id_event_type_stage
  ON job_task_events(
    task_id,
    event_type,
    payload_json
  );
CREATE INDEX idx_job_tasks_sync_recent_roots
  ON job_tasks(created_at DESC, id DESC)
  WHERE task_type = 'sync.subscriptions';
CREATE INDEX idx_release_reading_backfill_runs_status
  ON release_reading_backfill_runs(status, created_at DESC);
CREATE INDEX idx_release_reading_backfill_items_cursor
  ON release_reading_backfill_items(run_id, status, published_at ASC, release_id ASC);
CREATE INDEX idx_release_reading_backfill_items_release
  ON release_reading_backfill_items(release_id, run_id);
CREATE INDEX idx_release_reading_backfill_runs_task_lease
  ON release_reading_backfill_runs(lease_task_id, status);
CREATE INDEX idx_release_evidence_snapshots_release_audience
  ON release_evidence_snapshots(
    release_id,
    audience,
    audience_user_id,
    status,
    updated_at DESC
  );
CREATE INDEX idx_release_evidence_snapshots_retry
  ON release_evidence_snapshots(status, next_retry_at, updated_at);
CREATE UNIQUE INDEX idx_release_evidence_index_scope_key
  ON release_evidence_index(
    repo_id,
    audience,
    COALESCE(audience_user_id, ''),
    resource_kind,
    resource_key,
    side
  );
CREATE INDEX idx_release_evidence_index_lookup
  ON release_evidence_index(
    repo_id,
    audience,
    audience_user_id,
    resource_kind,
    updated_at DESC
  );
CREATE INDEX idx_release_evidence_snapshot_items_order
  ON release_evidence_snapshot_items(snapshot_id, ordinal ASC, index_id ASC);
CREATE UNIQUE INDEX idx_release_evidence_task_active_snapshot
  ON job_tasks(
    task_type,
    payload_json
  )
  WHERE task_type = 'release.evidence' AND status IN ('queued', 'running');
CREATE INDEX idx_content_work_items_evidence
  ON content_work_items(evidence_snapshot_id, evidence_status, status);
CREATE INDEX IF NOT EXISTS idx_search_documents_vector ON search_documents USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_search_documents_user_time ON search_documents (user_id, source_time DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_tower_sessions_expiry ON tower_sessions.session (expiry_date);
CREATE INDEX IF NOT EXISTS idx_job_tasks_claim ON job_tasks (status, available_at, created_at, id) WHERE status IN ('queued', 'running');
CREATE INDEX IF NOT EXISTS idx_content_work_items_claim ON content_work_items (status, next_retry_at, created_at, id) WHERE status IN ('queued', 'running');
