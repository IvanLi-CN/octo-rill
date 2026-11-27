CREATE TABLE admin_collection_processing_coverage (
  record_kind TEXT NOT NULL,
  record_id TEXT NOT NULL,
  pipeline TEXT NOT NULL,
  status_origin TEXT NOT NULL CHECK (status_origin IN ('never_started', 'historical_unknown')),
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (record_kind, record_id, pipeline)
);
CREATE TABLE admin_dashboard_daily_rollups (
  rollup_date TEXT NOT NULL,
  time_zone TEXT NOT NULL,
  task_type TEXT NOT NULL,
  total_users INTEGER NOT NULL,
  active_users INTEGER NOT NULL,
  queued_count INTEGER NOT NULL DEFAULT 0,
  running_count INTEGER NOT NULL DEFAULT 0,
  succeeded_count INTEGER NOT NULL DEFAULT 0,
  failed_count INTEGER NOT NULL DEFAULT 0,
  canceled_count INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL, business_ok_count INTEGER NOT NULL DEFAULT 0, business_partial_count INTEGER NOT NULL DEFAULT 0, business_failed_count INTEGER NOT NULL DEFAULT 0, business_disabled_count INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (rollup_date, time_zone, task_type)
);
CREATE TABLE admin_runtime_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  llm_max_concurrency INTEGER NOT NULL,
  translation_general_worker_concurrency INTEGER NOT NULL,
  translation_dedicated_worker_concurrency INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, ai_model_context_limit INTEGER
  CHECK (ai_model_context_limit IS NULL OR ai_model_context_limit > 0), ai_model_context_limit_migrated_at TEXT, sync_auto_fetch_interval_minutes INTEGER NOT NULL DEFAULT 60
  CHECK (sync_auto_fetch_interval_minutes >= 1 AND sync_auto_fetch_interval_minutes <= 120), repo_release_worker_concurrency INTEGER NOT NULL DEFAULT 5
CHECK (repo_release_worker_concurrency BETWEEN 1 AND 32), retry_recent_failures_interval_minutes INTEGER NOT NULL DEFAULT 10
  CHECK (retry_recent_failures_interval_minutes >= 1 AND retry_recent_failures_interval_minutes <= 120), llm_models_json TEXT NOT NULL DEFAULT '[]', repo_refresh_system_budget_per_window INTEGER NOT NULL DEFAULT 1000
  CHECK (repo_refresh_system_budget_per_window BETWEEN 1 AND 20000), daily_brief_schedule_local_time TEXT NOT NULL DEFAULT '06:00', dashboard_release_freshness_profile TEXT NOT NULL DEFAULT 'balanced'
  CHECK (dashboard_release_freshness_profile IN ('latest', 'balanced', 'capacity')), webhook_push_audit_interval_days INTEGER NOT NULL DEFAULT 7
  CHECK (webhook_push_audit_interval_days BETWEEN 1 AND 30), webhook_push_audit_last_started_at TEXT, sync_auto_fetch_effective_at TEXT, star_sync_delta_interval_minutes INTEGER NOT NULL DEFAULT 30
  CHECK (star_sync_delta_interval_minutes BETWEEN 1 AND 120), star_sync_full_sweep_interval_minutes INTEGER NOT NULL DEFAULT 1440
  CHECK (star_sync_full_sweep_interval_minutes BETWEEN 60 AND 10080),
  CHECK (llm_max_concurrency > 0),
  CHECK (translation_general_worker_concurrency > 0),
  CHECK (translation_dedicated_worker_concurrency > 0)
);
CREATE TABLE ai_translations (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  lang TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  title TEXT,
  summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'ready', error_text TEXT, active_work_item_id TEXT,
  UNIQUE(user_id, entity_type, entity_id, lang),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE brief_release_memberships (
  brief_id TEXT NOT NULL,
  release_id INTEGER NOT NULL,
  release_ts_utc TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (brief_id, release_id),
  FOREIGN KEY(brief_id) REFERENCES briefs(id) ON DELETE CASCADE,
  FOREIGN KEY(release_id) REFERENCES repo_releases(release_id) ON DELETE CASCADE
);
CREATE TABLE "briefs" (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  date TEXT NOT NULL,
  window_start_utc TEXT,
  window_end_utc TEXT,
  effective_time_zone TEXT,
  effective_local_boundary TEXT,
  generation_source TEXT NOT NULL DEFAULT 'legacy',
  content_markdown TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE content_attempt_events (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no >= 1),
  trigger TEXT NOT NULL
    CHECK (trigger IN ('initial', 'manual_retry', 'automatic_recovery', 'system_requeue')),
  event_type TEXT NOT NULL
    CHECK (event_type IN ('attempt_queued', 'attempt_started', 'attempt_completed', 'retry_scheduled')),
  result_status TEXT,
  error_code TEXT,
  error_summary TEXT,
  failure_class TEXT,
  retry_disposition TEXT,
  retry_eligible INTEGER NOT NULL DEFAULT 0 CHECK (retry_eligible IN (0, 1)),
  next_retry_at TEXT,
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  token_count INTEGER CHECK (token_count IS NULL OR token_count >= 0),
  cost_microunits INTEGER CHECK (cost_microunits IS NULL OR cost_microunits >= 0),
  created_at TEXT NOT NULL, configuration_snapshot_json TEXT, route_snapshot_json TEXT, configuration_fingerprint TEXT,
  UNIQUE (work_item_id, attempt_no, event_type),
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE CASCADE
);
CREATE TABLE content_attempt_llm_calls (
  id TEXT PRIMARY KEY,
  attempt_event_id TEXT NOT NULL,
  provider_call_id TEXT NOT NULL,
  model TEXT NOT NULL,
  status TEXT NOT NULL,
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  input_tokens INTEGER CHECK (input_tokens IS NULL OR input_tokens >= 0),
  output_tokens INTEGER CHECK (output_tokens IS NULL OR output_tokens >= 0),
  cost_microunits INTEGER CHECK (cost_microunits IS NULL OR cost_microunits >= 0),
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  UNIQUE (attempt_event_id, provider_call_id),
  FOREIGN KEY (attempt_event_id) REFERENCES content_attempt_events(id) ON DELETE CASCADE
);
CREATE TABLE content_attempt_provider_admissions (
  id TEXT PRIMARY KEY,
  attempt_event_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no >= 1),
  call_ordinal INTEGER NOT NULL CHECK (call_ordinal >= 0),
  relation_role TEXT NOT NULL CHECK (
    relation_role IN ('primary', 'length_recovery', 'fallback')
  ),
  source_hash TEXT NOT NULL,
  source_revision_json TEXT NOT NULL DEFAULT '{}',
  admitted_at TEXT NOT NULL,
  UNIQUE (attempt_event_id, call_ordinal),
  FOREIGN KEY (attempt_event_id, work_item_id, attempt_no)
    REFERENCES content_attempt_events(id, work_item_id, attempt_no)
    ON DELETE CASCADE,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE CASCADE
);
CREATE TABLE content_batch_items (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  item_index INTEGER NOT NULL CHECK (item_index >= 0),
  request_count INTEGER NOT NULL DEFAULT 0 CHECK (request_count >= 0),
  token_estimate INTEGER NOT NULL DEFAULT 0 CHECK (token_estimate >= 0),
  result_status TEXT,
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (batch_id, work_item_id),
  FOREIGN KEY (batch_id) REFERENCES content_batches(id) ON DELETE CASCADE,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE CASCADE
);
CREATE TABLE content_batches (
  id TEXT PRIMARY KEY,
  partition_key TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  trigger_reason TEXT NOT NULL,
  worker_id TEXT,
  worker_kind TEXT NOT NULL DEFAULT 'general'
    CHECK (worker_kind = 'general'),
  request_count INTEGER NOT NULL DEFAULT 0 CHECK (request_count >= 0),
  item_count INTEGER NOT NULL DEFAULT 0 CHECK (item_count >= 0),
  estimated_input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (estimated_input_tokens >= 0),
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed')),
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE content_current_result_projections (
  identity_id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  active_work_item_id TEXT,
  source_projection_id TEXT,
  payload_json TEXT NOT NULL,
  published_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (identity_id) REFERENCES content_work_identities(id) ON DELETE RESTRICT,
  FOREIGN KEY (identity_id, work_item_id)
    REFERENCES content_work_identity_members(identity_id, work_item_id) ON DELETE RESTRICT,
  FOREIGN KEY (active_work_item_id) REFERENCES content_work_items(id) ON DELETE RESTRICT,
  FOREIGN KEY (source_projection_id) REFERENCES content_result_projections(id) ON DELETE RESTRICT
);
CREATE TABLE content_identity_upgrade_control (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  generation INTEGER NOT NULL CHECK (generation >= 1),
  status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'paused', 'completed', 'failed')),
  phase TEXT NOT NULL CHECK (
    phase IN ('work_identity_backfill', 'projection_backfill', 'blocked_config_recovery', 'complete')
  ),
  cursor TEXT,
  last_error_code TEXT,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE content_legacy_observations (
  id TEXT PRIMARY KEY,
  legacy_table TEXT NOT NULL,
  legacy_primary_key TEXT NOT NULL,
  canonical_resource_type TEXT,
  canonical_resource_id TEXT,
  pipeline TEXT CHECK (pipeline IS NULL OR pipeline IN ('translation', 'polishing')),
  classification TEXT NOT NULL CHECK (classification IN ('legacy_cached', 'legacy_conflict')),
  observation_basis_json TEXT NOT NULL,
  observed_at TEXT NOT NULL,
  UNIQUE (legacy_table, legacy_primary_key)
);
CREATE TABLE content_processing_control (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  mode TEXT NOT NULL CHECK (mode IN ('legacy', 'rollback_freeze', 'global')),
  switch_token TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE content_request_links (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE,
  work_item_id TEXT NOT NULL,
  requester_type TEXT NOT NULL CHECK (requester_type IN ('user', 'system')),
  requester_id TEXT,
  authorization_snapshot_json TEXT NOT NULL,
  producer_ref TEXT NOT NULL,
  request_source TEXT NOT NULL,
  delivery_mode TEXT NOT NULL CHECK (delivery_mode IN ('async', 'wait', 'stream')),
  response_status TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id)
);
CREATE TABLE content_result_projections (
  id TEXT PRIMARY KEY,
  canonical_resource_type TEXT NOT NULL
    CHECK (canonical_resource_type IN ('release', 'announcement', 'notification')),
  canonical_resource_id TEXT NOT NULL,
  pipeline TEXT NOT NULL CHECK (pipeline IN ('translation', 'polishing')),
  variant TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  active_work_item_id TEXT,
  payload_json TEXT NOT NULL,
  published_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (
    canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, protocol_version, model_profile, source_hash
  ),
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id),
  FOREIGN KEY (active_work_item_id) REFERENCES content_work_items(id)
);
CREATE TABLE content_work_admission_events (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  event_type TEXT NOT NULL CHECK (
    event_type IN (
      'admission_accepted',
      'admission_noop',
      'admission_rejected_superseded',
      'source_superseded',
      'reconciliation_superseded'
    )
  ),
  replaced_by_work_item_id TEXT,
  source_hash TEXT NOT NULL,
  source_revision_json TEXT NOT NULL DEFAULT '{}',
  producer_ref TEXT NOT NULL DEFAULT '',
  requester_id TEXT,
  reason_code TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id),
  FOREIGN KEY (replaced_by_work_item_id) REFERENCES content_work_items(id)
);
CREATE TABLE content_work_identities (
  id TEXT PRIMARY KEY,
  canonical_resource_type TEXT NOT NULL
    CHECK (canonical_resource_type IN ('release', 'announcement', 'notification')),
  canonical_resource_id TEXT NOT NULL,
  pipeline TEXT NOT NULL CHECK (pipeline IN ('translation', 'polishing')),
  variant TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (
    canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  ),
  UNIQUE (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  )
);
CREATE TABLE content_work_identity_members (
  work_item_id TEXT PRIMARY KEY,
  identity_id TEXT NOT NULL,
  canonical_resource_type TEXT NOT NULL,
  canonical_resource_id TEXT NOT NULL,
  pipeline TEXT NOT NULL,
  variant TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  UNIQUE (identity_id, work_item_id),
  FOREIGN KEY (
    work_item_id, canonical_resource_type, canonical_resource_id, pipeline,
    variant, target_lang, source_hash, protocol_version
  ) REFERENCES content_work_items (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  ) ON DELETE RESTRICT,
  FOREIGN KEY (
    identity_id, canonical_resource_type, canonical_resource_id, pipeline,
    variant, target_lang, source_hash, protocol_version
  ) REFERENCES content_work_identities (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  ) ON DELETE RESTRICT
);
CREATE TABLE content_work_items (
  id TEXT PRIMARY KEY,
  canonical_resource_type TEXT NOT NULL
    CHECK (canonical_resource_type IN ('release', 'announcement', 'notification')),
  canonical_resource_id TEXT NOT NULL,
  pipeline TEXT NOT NULL CHECK (pipeline IN ('translation', 'polishing')),
  variant TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  source_snapshot_json TEXT NOT NULL,
  configuration_fingerprint TEXT NOT NULL,
  status TEXT NOT NULL CHECK (
    status IN (
      'queued', 'running', 'ready', 'failed', 'not_applicable',
      'deferred_provider', 'blocked_config', 'cancelled', 'superseded'
    )
  ),
  priority INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),
  cache_hit INTEGER NOT NULL DEFAULT 0 CHECK (cache_hit IN (0, 1)),
  token_estimate INTEGER NOT NULL DEFAULT 0 CHECK (token_estimate >= 0),
  batch_id TEXT,
  lease_owner TEXT,
  lease_expires_at TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  next_retry_at TEXT,
  retry_expires_at TEXT,
  retry_after_at TEXT,
  failure_class TEXT,
  supersedes_work_item_id TEXT,
  cancelled_at TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL, evidence_snapshot_id TEXT, evidence_status TEXT NOT NULL DEFAULT 'not_required'
    CHECK (evidence_status IN ('not_required', 'pending', 'ready', 'failed')),
  UNIQUE (
    canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version, model_profile
  ),
  FOREIGN KEY (supersedes_work_item_id) REFERENCES content_work_items(id)
);
CREATE TABLE daily_brief_hour_slots (
  hour_utc INTEGER PRIMARY KEY,
  enabled INTEGER NOT NULL DEFAULT 1,
  last_dispatch_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (hour_utc >= 0 AND hour_utc <= 23)
);
CREATE TABLE follower_current_members (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  actor_github_user_id INTEGER NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, actor_github_user_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE follower_sync_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE github_connections (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_user_id INTEGER NOT NULL UNIQUE,
  login TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  email TEXT,
  access_token_ciphertext BLOB NOT NULL,
  access_token_nonce BLOB NOT NULL,
  scopes TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE job_task_events (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE
);
CREATE TABLE job_tasks (
  id TEXT PRIMARY KEY,
  task_type TEXT NOT NULL,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  requested_by TEXT,
  parent_task_id TEXT,
  payload_json TEXT NOT NULL,
  result_json TEXT,
  error_message TEXT,
  cancel_requested INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  log_file_path TEXT, runtime_owner_id TEXT, lease_heartbeat_at TEXT, available_at TEXT,
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id) ON DELETE SET NULL
);
CREATE TABLE linuxdo_connections (
  user_id TEXT PRIMARY KEY,
  linuxdo_user_id INTEGER NOT NULL UNIQUE,
  username TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  trust_level INTEGER NOT NULL DEFAULT 0,
  active INTEGER NOT NULL DEFAULT 1,
  silenced INTEGER NOT NULL DEFAULT 0,
  linked_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE llm_call_events (
  id TEXT PRIMARY KEY,
  call_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  requested_by TEXT,
  parent_task_id TEXT,
  payload_json TEXT,
  created_at TEXT NOT NULL, failure_class TEXT
  CHECK (
    failure_class IS NULL
    OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
  ), model TEXT, attempt INTEGER
  CHECK (attempt IS NULL OR attempt >= 0), retry_after_ms INTEGER
  CHECK (retry_after_ms IS NULL OR retry_after_ms >= 0), from_model TEXT, to_model TEXT, fallback_count INTEGER
  CHECK (fallback_count IS NULL OR fallback_count >= 0),
  CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  FOREIGN KEY(call_id) REFERENCES llm_calls(id) ON DELETE CASCADE
);
CREATE TABLE llm_calls (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  model TEXT NOT NULL,
  requested_by TEXT,
  parent_task_id TEXT,
  parent_task_type TEXT,
  max_tokens INTEGER NOT NULL,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  scheduler_wait_ms INTEGER NOT NULL DEFAULT 0,
  duration_ms INTEGER,
  prompt_text TEXT NOT NULL,
  response_text TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  input_messages_json TEXT,
  output_messages_json TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cached_input_tokens INTEGER,
  total_tokens INTEGER,
  first_token_wait_ms INTEGER,
  parent_translation_batch_id TEXT, runtime_owner_id TEXT, lease_heartbeat_at TEXT, failure_class TEXT
  CHECK (
    failure_class IS NULL
    OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
  ), final_model TEXT, fallback_count INTEGER NOT NULL DEFAULT 0
  CHECK (fallback_count >= 0), retry_scheduled_at TEXT, recovery_attempt_count INTEGER NOT NULL DEFAULT 0
  CHECK (recovery_attempt_count >= 0), parent_brief_id TEXT, finish_reason TEXT, provider_request_id TEXT, provider_http_status INTEGER,
  CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id) ON DELETE SET NULL
);
CREATE TABLE llm_diagnostic_access_audit (
  id TEXT PRIMARY KEY,
  call_id TEXT NOT NULL,
  actor_user_id TEXT,
  action TEXT NOT NULL
    CHECK (action IN ('reveal', 'copy')),
  created_at TEXT NOT NULL
);
CREATE TABLE llm_model_health (
  model TEXT PRIMARY KEY,
  relevant_failure_count INTEGER NOT NULL DEFAULT 0
    CHECK (relevant_failure_count >= 0),
  window_started_at TEXT,
  cooldown_until TEXT,
  last_failure_class TEXT
    CHECK (
      last_failure_class IS NULL
      OR last_failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
    ),
  last_failure_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE llm_recovery_flags (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  llm_recovery_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (llm_recovery_enabled IN (0, 1)),
  llm_recovery_rollout_percent INTEGER NOT NULL DEFAULT 0
    CHECK (llm_recovery_rollout_percent BETWEEN 0 AND 100),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE notifications (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  thread_id TEXT NOT NULL,
  repo_full_name TEXT,
  subject_title TEXT,
  subject_type TEXT,
  reason TEXT,
  updated_at TEXT,
  unread INTEGER NOT NULL DEFAULT 1,
  url TEXT,
  html_url TEXT,
  last_seen_at TEXT,
  UNIQUE(user_id, thread_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE owned_repo_star_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, members_snapshot_initialized INTEGER NOT NULL DEFAULT 1, owner_avatar_url TEXT, open_graph_image_url TEXT, uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0, repo_stargazer_count INTEGER, repo_stargazer_count_updated_at TEXT, is_private INTEGER,
  UNIQUE(user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE public_metrics_hourly_snapshots (
  sampled_hour TEXT PRIMARY KEY,
  observed_at TEXT NOT NULL,
  deduplicated_repositories INTEGER NOT NULL CHECK (deduplicated_repositories >= 0),
  pressure REAL NOT NULL CHECK (pressure >= 0)
);
CREATE TABLE public_repo_release_usage (
  id TEXT PRIMARY KEY,
  repo_id INTEGER,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  full_name TEXT NOT NULL,
  full_name_lower TEXT NOT NULL UNIQUE,
  first_registered_at TEXT NOT NULL,
  last_requested_at TEXT NOT NULL,
  last_list_requested_at TEXT,
  last_detail_requested_at TEXT,
  api_list_requests INTEGER NOT NULL DEFAULT 0,
  api_detail_requests INTEGER NOT NULL DEFAULT 0,
  page_list_requests INTEGER NOT NULL DEFAULT 0,
  page_detail_requests INTEGER NOT NULL DEFAULT 0,
  last_sync_status TEXT NOT NULL DEFAULT 'pending',
  last_sync_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, access_kind TEXT NOT NULL DEFAULT 'github_public'
  CHECK (access_kind IN ('github_public', 'private_owner_published')), published_by_user_id TEXT, published_at TEXT,
  CHECK (last_sync_status IN ('pending', 'ready', 'failed', 'inaccessible'))
);
CREATE TABLE reaction_pat_tokens (
  user_id TEXT PRIMARY KEY,
  token_ciphertext BLOB NOT NULL,
  token_nonce BLOB NOT NULL,
  masked_token TEXT NOT NULL,
  last_check_state TEXT NOT NULL DEFAULT 'unknown',
  last_check_message TEXT,
  last_checked_at TEXT,
  updated_at TEXT NOT NULL, owner_github_connection_id TEXT, owner_github_user_id INTEGER, owner_login TEXT, webhook_push_allows_private_repos INTEGER
  CHECK (webhook_push_allows_private_repos IS NULL OR webhook_push_allows_private_repos IN (0, 1)),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE release_evidence_index (
  id TEXT PRIMARY KEY,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  audience TEXT NOT NULL CHECK (audience IN ('public', 'authorized')),
  audience_user_id TEXT,
  resource_kind TEXT NOT NULL CHECK (
    resource_kind IN ('compare', 'commit', 'tree', 'blob', 'file')
  ),
  resource_key TEXT NOT NULL,
  side TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL CHECK (status IN ('ready', 'failed')),
  payload_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (audience = 'public' AND audience_user_id IS NULL)
    OR (audience = 'authorized' AND audience_user_id IS NOT NULL)
  )
);
CREATE TABLE release_evidence_snapshot_items (
  snapshot_id TEXT NOT NULL,
  index_id TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  created_at TEXT NOT NULL,
  PRIMARY KEY (snapshot_id, index_id),
  FOREIGN KEY(snapshot_id) REFERENCES release_evidence_snapshots(id) ON DELETE CASCADE,
  FOREIGN KEY(index_id) REFERENCES release_evidence_index(id) ON DELETE RESTRICT
);
CREATE TABLE release_evidence_snapshots (
  id TEXT PRIMARY KEY,
  request_key TEXT NOT NULL UNIQUE,
  release_id INTEGER NOT NULL,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  audience TEXT NOT NULL CHECK (audience IN ('public', 'authorized')),
  audience_user_id TEXT,
  source_revision TEXT NOT NULL,
  base_tag TEXT NOT NULL,
  head_tag TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'ready', 'failed')),
  snapshot_json TEXT,
  content_hash TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  next_retry_at TEXT,
  retry_expires_at TEXT,
  task_id TEXT,
  lease_owner TEXT,
  lease_expires_at TEXT,
  last_error_code TEXT,
  last_error_summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (audience = 'public' AND audience_user_id IS NULL)
    OR (audience = 'authorized' AND audience_user_id IS NOT NULL)
  )
);
CREATE TABLE release_reading_backfill_items (
  run_id TEXT NOT NULL,
  release_id INTEGER NOT NULL,
  authorized_user_id TEXT,
  published_at TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'waiting', 'succeeded', 'failed', 'skipped')),
  attempt_count INTEGER NOT NULL DEFAULT 0,
  error_text TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (run_id, release_id),
  FOREIGN KEY(run_id) REFERENCES release_reading_backfill_runs(id) ON DELETE CASCADE,
  FOREIGN KEY(release_id) REFERENCES repo_releases(release_id) ON DELETE CASCADE
);
CREATE TABLE release_reading_backfill_runs (
  id TEXT PRIMARY KEY,
  started_at TEXT NOT NULL,
  window_start_utc TEXT NOT NULL,
  window_end_utc TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'waiting', 'paused', 'succeeded', 'failed')),
  cursor_published_at TEXT,
  cursor_release_id INTEGER,
  total INTEGER NOT NULL DEFAULT 0,
  succeeded INTEGER NOT NULL DEFAULT 0,
  failed INTEGER NOT NULL DEFAULT 0,
  waiting INTEGER NOT NULL DEFAULT 0,
  skipped INTEGER NOT NULL DEFAULT 0,
  lease_owner TEXT,
  lease_expires_at TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
, lease_task_id TEXT);
CREATE TABLE releases (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  release_id INTEGER NOT NULL,
  tag_name TEXT NOT NULL,
  name TEXT,
  body TEXT,
  html_url TEXT NOT NULL,
  published_at TEXT,
  created_at TEXT,
  is_prerelease INTEGER NOT NULL DEFAULT 0,
  is_draft INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  node_id TEXT,
  react_plus1 INTEGER NOT NULL DEFAULT 0,
  react_laugh INTEGER NOT NULL DEFAULT 0,
  react_heart INTEGER NOT NULL DEFAULT 0,
  react_hooray INTEGER NOT NULL DEFAULT 0,
  react_rocket INTEGER NOT NULL DEFAULT 0,
  react_eyes INTEGER NOT NULL DEFAULT 0,
  UNIQUE(user_id, release_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE repo_refresh_governance_cycle_members (
  cycle_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  completed_at TEXT,
  removed_from_pool INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL, attempt_status TEXT, attempt_error TEXT,
  PRIMARY KEY (cycle_id, repo_id),
  FOREIGN KEY(cycle_id) REFERENCES repo_refresh_governance_cycles(id) ON DELETE CASCADE
);
CREATE TABLE repo_refresh_governance_cycles (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL CHECK (status IN ('active', 'completed')),
  window_budget INTEGER NOT NULL,
  frozen_repo_count INTEGER NOT NULL,
  completed_repo_count INTEGER NOT NULL DEFAULT 0,
  window_index_started_at INTEGER NOT NULL,
  window_index_completed_at INTEGER,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  updated_at TEXT NOT NULL
, window_minutes INTEGER NOT NULL DEFAULT 10
  CHECK (window_minutes BETWEEN 1 AND 120), last_selection_window_index INTEGER);
CREATE TABLE repo_refresh_governance_retention_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  window_started_at TEXT NOT NULL,
  deleted_rows INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);
CREATE TABLE repo_refresh_governance_snapshots (
  repo_id INTEGER PRIMARY KEY,
  repo_full_name TEXT NOT NULL,
  is_private INTEGER NOT NULL DEFAULT 0,
  watcher_user_count INTEGER NOT NULL,
  watcher_repo_total_sum INTEGER NOT NULL,
  cached_stargazer_count INTEGER,
  cached_stargazer_count_updated_at TEXT,
  priority_rank INTEGER NOT NULL,
  target_window INTEGER NOT NULL,
  target_interval_minutes INTEGER NOT NULL,
  urgency_score REAL NOT NULL DEFAULT 1,
  urgency_bucket TEXT NOT NULL DEFAULT 'active',
  system_last_selected_at TEXT,
  system_last_success_at TEXT,
  actual_last_success_at TEXT,
  actual_last_success_source TEXT,
  active_cycle_id TEXT,
  active_cycle_window_index INTEGER,
  active_cycle_completed INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
, system_last_attempt_at TEXT, system_last_attempt_status TEXT, system_last_attempt_error TEXT);
CREATE TABLE repo_release_sync_state (
  repo_id INTEGER PRIMARY KEY,
  etag TEXT NULL,
  last_modified TEXT NULL,
  last_success_at TEXT NULL,
  last_attempt_at TEXT NULL,
  last_not_modified_at TEXT NULL,
  last_error_text TEXT NULL,
  backoff_until TEXT NULL,
  updated_at TEXT NOT NULL
, last_page_count INTEGER NOT NULL DEFAULT 0, last_fetched_count INTEGER NOT NULL DEFAULT 0, last_inserted_count INTEGER NOT NULL DEFAULT 0, last_updated_count INTEGER NOT NULL DEFAULT 0, last_unchanged_count INTEGER NOT NULL DEFAULT 0, last_stopped_reason TEXT);
CREATE TABLE repo_release_watchers (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  user_id TEXT,
  origin TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 0,
  reason TEXT NOT NULL,
  is_new_repo INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'pending',
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, reused_fresh INTEGER NOT NULL DEFAULT 0, freshness_window_minutes INTEGER
  CHECK (freshness_window_minutes IS NULL OR freshness_window_minutes BETWEEN 1 AND 30), freshness_decision TEXT
  CHECK (
    freshness_decision IS NULL
    OR freshness_decision IN ('fetch', 'reused_fresh', 'reused_running')
  ), freshness_assessment_json TEXT,
  UNIQUE(task_id, work_item_id),
  FOREIGN KEY(work_item_id) REFERENCES repo_release_work_items(id) ON DELETE CASCADE,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE SET NULL
);
CREATE TABLE repo_release_work_items (
  id TEXT PRIMARY KEY,
  repo_id INTEGER NOT NULL UNIQUE,
  repo_full_name TEXT NOT NULL,
  status TEXT NOT NULL,
  request_origin TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 0,
  has_new_repo_watchers INTEGER NOT NULL DEFAULT 0,
  deadline_at TEXT NOT NULL,
  last_release_count INTEGER NOT NULL DEFAULT 0,
  last_candidate_failures INTEGER NOT NULL DEFAULT 0,
  last_success_at TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  runtime_owner_id TEXT,
  lease_heartbeat_at TEXT
, last_fetched_count INTEGER NOT NULL DEFAULT 0, last_inserted_count INTEGER NOT NULL DEFAULT 0, last_updated_count INTEGER NOT NULL DEFAULT 0, last_unchanged_count INTEGER NOT NULL DEFAULT 0, last_pages_fetched INTEGER NOT NULL DEFAULT 0, last_stopped_reason TEXT, last_new_release_ids_json TEXT);
CREATE TABLE repo_releases (
  id TEXT PRIMARY KEY,
  repo_id INTEGER NOT NULL,
  release_id INTEGER NOT NULL UNIQUE,
  node_id TEXT,
  tag_name TEXT NOT NULL,
  name TEXT,
  body TEXT,
  html_url TEXT NOT NULL,
  published_at TEXT,
  created_at TEXT,
  is_prerelease INTEGER NOT NULL DEFAULT 0,
  is_draft INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  react_plus1 INTEGER NOT NULL DEFAULT 0,
  react_laugh INTEGER NOT NULL DEFAULT 0,
  react_heart INTEGER NOT NULL DEFAULT 0,
  react_hooray INTEGER NOT NULL DEFAULT 0,
  react_rocket INTEGER NOT NULL DEFAULT 0,
  react_eyes INTEGER NOT NULL DEFAULT 0
, detected_at TEXT);
CREATE TABLE repo_star_current_members (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  repo_full_name TEXT NOT NULL,
  actor_github_user_id INTEGER NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  starred_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, repo_id, actor_github_user_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE repo_star_sync_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE runtime_owners (
  runtime_owner_id TEXT PRIMARY KEY,
  lease_heartbeat_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE scheduled_task_dispatch_state (
  schedule_name TEXT PRIMARY KEY,
  last_dispatch_key TEXT,
  last_task_id TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE search_document_user_lanes (
  document_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  translated_text TEXT,
  smart_text TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (document_id, user_id),
  FOREIGN KEY (document_id) REFERENCES search_documents(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE VIRTUAL TABLE search_document_user_lanes_fts_v2 USING fts5(
  doc_id UNINDEXED,
  user_id UNINDEXED,
  translated_text,
  smart_text,
  tokenize = 'trigram'
);
CREATE TABLE search_documents (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  resource_type TEXT NOT NULL CHECK (resource_type IN ('release', 'announcement', 'brief', 'notification', 'repository')),
  resource_id TEXT NOT NULL,
  repo_id INTEGER,
  repo_full_name TEXT,
  owner_login TEXT,
  title TEXT,
  body TEXT,
  translated_text TEXT,
  smart_text TEXT,
  source_time TEXT,
  unread INTEGER,
  target_path TEXT,
  target_url TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (user_id, resource_type, resource_id)
);
CREATE VIRTUAL TABLE search_documents_fts_v2 USING fts5(
  doc_id UNINDEXED,
  title,
  body,
  repo_full_name,
  translated_text,
  smart_text,
  tokenize = 'trigram'
);
CREATE TABLE search_fts_document_rows (
  fts_rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id TEXT NOT NULL UNIQUE
);
CREATE TABLE search_fts_user_lane_rows (
  fts_rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  UNIQUE (document_id, user_id)
);
CREATE TABLE search_metadata_backfill_queue (
  repo_id INTEGER PRIMARY KEY,
  updated_at TEXT NOT NULL,
  release_cursor INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE search_projection_backfill_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  phase TEXT NOT NULL,
  cursor INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK (status IN ('pending', 'building', 'ready', 'paused_low_disk', 'failed')),
  last_error TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE search_projection_outbox (
  projection_key TEXT PRIMARY KEY,
  operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
  resource_type TEXT NOT NULL CHECK (resource_type IN ('announcement', 'content', 'notification')),
  resource_id TEXT NOT NULL,
  canonical_resource_type TEXT,
  user_id TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  available_at TEXT NOT NULL,
  lease_owner TEXT,
  lease_expires_at TEXT,
  last_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
, previous_resource_id TEXT, fanout_cursor TEXT);
CREATE TABLE search_projection_outbox_failed (
  projection_key TEXT PRIMARY KEY,
  operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
  resource_type TEXT NOT NULL CHECK (resource_type IN ('announcement', 'content', 'notification')),
  resource_id TEXT NOT NULL,
  previous_resource_id TEXT,
  canonical_resource_type TEXT,
  user_id TEXT,
  attempt_count INTEGER NOT NULL CHECK (attempt_count >= 0),
  last_error TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  failed_at TEXT NOT NULL
);
CREATE TABLE search_rate_limits (
  user_id TEXT PRIMARY KEY,
  window_started_at INTEGER NOT NULL,
  request_count INTEGER NOT NULL CHECK (request_count >= 0),
  updated_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE "social_activity_events" (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  repo_id INTEGER,
  repo_full_name TEXT,
  repo_owner_avatar_url TEXT,
  repo_open_graph_image_url TEXT,
  repo_uses_custom_open_graph_image INTEGER,
  title TEXT,
  body TEXT,
  html_url TEXT,
  github_event_id TEXT,
  actor_github_user_id INTEGER NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  occurred_at TEXT NOT NULL,
  detected_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, discussion_number INTEGER,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE star_sync_epochs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_connection_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  started_at TEXT NOT NULL,
  next_cursor TEXT,
  total_count_at_start INTEGER,
  processed_pages INTEGER NOT NULL DEFAULT 0,
  processed_items INTEGER NOT NULL DEFAULT 0,
  next_slice_not_before TEXT,
  lease_owner_id TEXT,
  lease_expires_at TEXT,
  completed_at TEXT,
  failure_reason TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY(github_connection_id) REFERENCES github_connections(id) ON DELETE CASCADE
);
CREATE TABLE starred_repo_connection_memberships (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_connection_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  full_name TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  html_url TEXT NOT NULL,
  stargazed_at TEXT,
  is_private INTEGER NOT NULL DEFAULT 0,
  owner_avatar_url TEXT,
  open_graph_image_url TEXT,
  uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0,
  repo_stargazer_count INTEGER,
  last_seen_epoch_id TEXT,
  last_delta_seen_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, github_connection_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY(github_connection_id) REFERENCES github_connections(id) ON DELETE CASCADE
);
CREATE TABLE starred_repos (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  full_name TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  html_url TEXT NOT NULL,
  stargazed_at TEXT,
  is_private INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL, owner_avatar_url TEXT, open_graph_image_url TEXT, uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0, repo_stargazer_count INTEGER, repo_stargazer_count_updated_at TEXT,
  UNIQUE(user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE sync_state (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, key),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE sync_subscription_events (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  event_type TEXT NOT NULL,
  severity TEXT NOT NULL,
  recoverable INTEGER NOT NULL DEFAULT 0,
  attempt INTEGER NOT NULL DEFAULT 0,
  user_id TEXT,
  repo_id INTEGER,
  repo_full_name TEXT,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE
);
CREATE TABLE tower_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    data BLOB NOT NULL,
    expiry_date INTEGER NOT NULL
);
CREATE TABLE translation_attempt_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  work_item_id TEXT NOT NULL,
  request_id TEXT,
  batch_id TEXT,
  scope_user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no >= 1),
  trigger TEXT NOT NULL
    CHECK (
      trigger IN (
        'initial',
        'manual_retry',
        'automatic_recovery',
        'system_requeue'
      )
    ),
  event_type TEXT NOT NULL
    CHECK (
      event_type IN (
        'attempt_queued',
        'attempt_started',
        'attempt_completed',
        'retry_scheduled'
      )
    ),
  result_status TEXT
    CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  error_code TEXT,
  error_summary TEXT,
  failure_class TEXT
    CHECK (
      failure_class IS NULL
      OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
    ),
  retry_eligible INTEGER NOT NULL DEFAULT 0 CHECK (retry_eligible IN (0, 1)),
  next_retry_at TEXT,
  llm_call_ids_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL
, processing_stage TEXT, provider_status TEXT
  CHECK (
    provider_status IS NULL
    OR provider_status IN ('not_started', 'succeeded', 'failed')
  ), output_contract_status TEXT
  CHECK (
    output_contract_status IS NULL
    OR output_contract_status IN ('not_run', 'passed', 'failed', 'recovered')
  ), retry_disposition TEXT NOT NULL DEFAULT 'not_needed'
  CHECK (
    retry_disposition IN ('not_needed', 'scheduled', 'manual_only', 'in_attempt_recovered')
  ));
CREATE TABLE translation_attempt_llm_calls (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  work_item_id TEXT NOT NULL,
  attempt_no INTEGER NOT NULL CHECK (attempt_no >= 1),
  llm_call_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  relation_role TEXT NOT NULL
    CHECK (relation_role IN ('primary', 'length_recovery', 'schema_repair', 'fallback', 'legacy_unverified')),
  evidence_availability TEXT NOT NULL DEFAULT 'available'
    CHECK (evidence_availability IN ('available', 'expired', 'not_captured')),
  created_at TEXT NOT NULL,
  UNIQUE(work_item_id, attempt_no, llm_call_id, stage, ordinal)
);
CREATE TABLE translation_batch_items (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  item_index INTEGER NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  producer_count INTEGER NOT NULL DEFAULT 0,
  token_estimate INTEGER NOT NULL,
  result_status TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(batch_id, work_item_id),
  CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  FOREIGN KEY(batch_id) REFERENCES translation_batches(id) ON DELETE CASCADE,
  FOREIGN KEY(work_item_id) REFERENCES translation_work_items(id) ON DELETE CASCADE
);
CREATE TABLE translation_batches (
  id TEXT PRIMARY KEY,
  partition_key TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  trigger_reason TEXT NOT NULL,
  worker_slot INTEGER NOT NULL,
  request_count INTEGER NOT NULL,
  item_count INTEGER NOT NULL,
  estimated_input_tokens INTEGER NOT NULL,
  status TEXT NOT NULL,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL, runtime_owner_id TEXT, lease_heartbeat_at TEXT, worker_id TEXT NOT NULL DEFAULT '', worker_kind TEXT NOT NULL DEFAULT 'general'
  CHECK (worker_kind IN ('general', 'user_dedicated')),
  CHECK (status IN ('queued', 'running', 'completed', 'failed'))
);
CREATE TABLE translation_requests (
  id TEXT PRIMARY KEY,
  mode TEXT NOT NULL,
  source TEXT NOT NULL,
  request_origin TEXT NOT NULL CHECK (request_origin IN ('user', 'system')),
  requested_by TEXT,
  scope_user_id TEXT NOT NULL,
  producer_ref TEXT NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  max_wait_ms INTEGER NOT NULL,
  source_hash TEXT NOT NULL,
  source_blocks_json TEXT NOT NULL,
  target_slots_json TEXT NOT NULL,
  work_item_id TEXT,
  status TEXT NOT NULL,
  result_status TEXT,
  title_zh TEXT,
  summary_md TEXT,
  body_md TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (mode IN ('async', 'wait', 'stream')),
  CHECK (status IN ('queued', 'running', 'completed', 'failed')),
  CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(scope_user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY(work_item_id) REFERENCES translation_work_items(id) ON DELETE SET NULL
);
CREATE TABLE translation_work_items (
  id TEXT PRIMARY KEY,
  dedupe_key TEXT NOT NULL UNIQUE,
  scope_user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  source_hash TEXT NOT NULL,
  source_blocks_json TEXT NOT NULL,
  target_slots_json TEXT NOT NULL,
  token_estimate INTEGER NOT NULL,
  deadline_at TEXT NOT NULL,
  status TEXT NOT NULL,
  batch_id TEXT,
  result_status TEXT,
  title_zh TEXT,
  summary_md TEXT,
  body_md TEXT,
  error_text TEXT,
  cache_hit INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL, failure_class TEXT
  CHECK (
    failure_class IS NULL
    OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
  ), retry_count INTEGER NOT NULL DEFAULT 0
  CHECK (retry_count >= 0), next_retry_at TEXT, retry_expires_at TEXT, attempt_count INTEGER NOT NULL DEFAULT 0
  CHECK (attempt_count >= 0), next_attempt_trigger TEXT NOT NULL DEFAULT 'initial'
  CHECK (
    next_attempt_trigger IN (
      'initial',
      'manual_retry',
      'automatic_recovery',
      'system_requeue'
    )
  ), error_code TEXT, processing_stage TEXT, provider_status TEXT
  CHECK (
    provider_status IS NULL
    OR provider_status IN ('not_started', 'succeeded', 'failed')
  ), output_contract_status TEXT
  CHECK (
    output_contract_status IS NULL
    OR output_contract_status IN ('not_run', 'passed', 'failed', 'recovered')
  ), retry_disposition TEXT NOT NULL DEFAULT 'not_needed'
  CHECK (
    retry_disposition IN ('not_needed', 'scheduled', 'manual_only', 'in_attempt_recovered')
  ),
  CHECK (status IN ('queued', 'batched', 'running', 'completed', 'failed')),
  CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  FOREIGN KEY(scope_user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE user_api_keys (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  name TEXT NOT NULL,
  key_hash TEXT NOT NULL UNIQUE,
  key_ciphertext BLOB NOT NULL,
  key_nonce BLOB NOT NULL,
  key_prefix TEXT NOT NULL,
  masked_key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked_at TEXT,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE user_passkeys (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  credential_id TEXT NOT NULL UNIQUE,
  label TEXT NOT NULL,
  passkey_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE user_repo_associations (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id INTEGER,
  repo_full_name TEXT NOT NULL,
  repo_full_name_lower TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  html_url TEXT,
  description TEXT,
  is_private INTEGER,
  owner_avatar_url TEXT,
  open_graph_image_url TEXT,
  uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0,
  first_source TEXT NOT NULL
    CHECK (first_source IN ('personal_owned', 'github_star', 'manual_feed')),
  first_associated_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  is_following INTEGER NOT NULL DEFAULT 0,
  follow_state_source TEXT NOT NULL DEFAULT 'system_default'
    CHECK (follow_state_source IN ('system_default', 'user_explicit')),
  has_personal_owned_source INTEGER NOT NULL DEFAULT 0,
  has_github_star_source INTEGER NOT NULL DEFAULT 0,
  has_manual_feed_source INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, repo_full_name_lower),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE user_tokens (
  user_id TEXT PRIMARY KEY,
  access_token_ciphertext BLOB NOT NULL,
  access_token_nonce BLOB NOT NULL,
  scopes TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  github_user_id INTEGER NOT NULL UNIQUE,
  login TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  email TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  is_disabled INTEGER NOT NULL DEFAULT 0,
  daily_brief_utc_time TEXT NOT NULL DEFAULT '00:00',
  last_active_at TEXT
, daily_brief_local_time TEXT, daily_brief_time_zone TEXT, include_own_releases INTEGER NOT NULL DEFAULT 0, passkey_user_handle_uuid TEXT, paused_at TEXT, webhook_push_enabled INTEGER NOT NULL DEFAULT 0
  CHECK (webhook_push_enabled IN (0, 1)), webhook_push_secret_ciphertext BLOB, webhook_push_secret_nonce BLOB, webhook_push_callback_key TEXT, webhook_push_desired_state TEXT NOT NULL DEFAULT 'deleted'
  CHECK (webhook_push_desired_state IN ('enabled', 'paused', 'deleted')), webhook_push_last_completed_check_at TEXT);
CREATE TABLE webhook_push_deliveries (
  delivery_id TEXT PRIMARY KEY,
  hook_id INTEGER,
  repo_id INTEGER,
  event TEXT NOT NULL,
  action TEXT,
  received_at TEXT NOT NULL,
  queued_task_id TEXT
, processing_state TEXT NOT NULL DEFAULT 'pending'
  CHECK (processing_state IN ('pending', 'processing', 'ignored', 'queued')), processing_started_at TEXT);
CREATE TABLE webhook_push_reconcile_demands (
  user_id TEXT PRIMARY KEY,
  requested_generation INTEGER NOT NULL DEFAULT 0 CHECK (requested_generation >= 0),
  completed_generation INTEGER NOT NULL DEFAULT 0 CHECK (completed_generation >= 0),
  requested_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  CHECK (completed_generation <= requested_generation)
);
CREATE TABLE webhook_push_repos (
  user_id TEXT NOT NULL,
  repo_id INTEGER NOT NULL,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  repo_full_name TEXT NOT NULL,
  hook_id INTEGER,
  callback_url TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'unknown'
    CHECK (status IN ('unknown', 'missing', 'registered', 'conflict', 'permission_paused', 'error', 'delete_pending')),
  error_kind TEXT,
  error_message TEXT,
  permission_paused INTEGER NOT NULL DEFAULT 0 CHECK (permission_paused IN (0, 1)),
  last_checked_at TEXT,
  last_registered_at TEXT,
  updated_at TEXT NOT NULL, owner_github_user_id INTEGER,
  PRIMARY KEY (user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE webhook_push_user_operation_leases (
  user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  task_id TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
CREATE INDEX idx_admin_collection_processing_coverage_lookup
  ON admin_collection_processing_coverage(record_kind, record_id, pipeline);
CREATE INDEX idx_admin_dashboard_rollups_tz_date
  ON admin_dashboard_daily_rollups(time_zone, rollup_date DESC);
CREATE INDEX idx_ai_translations_active_work_item_id
  ON ai_translations(active_work_item_id);
CREATE INDEX idx_ai_translations_public_lookup
ON ai_translations(entity_type, lang, entity_id, status, updated_at DESC);
CREATE INDEX idx_ai_translations_search_entity
  ON ai_translations(lang, entity_id, status, updated_at DESC, id DESC, user_id);
CREATE INDEX idx_ai_translations_search_entity_lower
  ON ai_translations(lang, lower(entity_id), status, updated_at DESC, id DESC, user_id);
CREATE INDEX idx_brief_release_memberships_brief_ordinal
  ON brief_release_memberships(brief_id, ordinal ASC);
CREATE INDEX idx_brief_release_memberships_release_id
  ON brief_release_memberships(release_id);
CREATE INDEX idx_briefs_admin_activity_source_time
  ON briefs(julianday(created_at) DESC, id DESC);
CREATE INDEX idx_briefs_admin_created_at
  ON briefs(created_at DESC, id DESC);
CREATE INDEX idx_briefs_dashboard_readable
  ON briefs(user_id, date DESC, window_end_utc DESC, id DESC);
CREATE INDEX idx_briefs_user_date_created_at
  ON briefs(user_id, date DESC, created_at DESC);
CREATE INDEX idx_briefs_user_window_end_created_at
  ON briefs(user_id, window_end_utc DESC, created_at DESC);
CREATE UNIQUE INDEX idx_briefs_user_window_unique
  ON briefs(user_id, window_start_utc, window_end_utc)
  WHERE window_start_utc IS NOT NULL AND window_end_utc IS NOT NULL;
CREATE UNIQUE INDEX idx_content_attempt_events_identity
  ON content_attempt_events(id, work_item_id, attempt_no);
CREATE INDEX idx_content_attempt_events_work_item
  ON content_attempt_events(work_item_id, created_at ASC, id ASC);
CREATE INDEX idx_content_attempt_llm_calls_event
  ON content_attempt_llm_calls(attempt_event_id, created_at ASC);
CREATE INDEX idx_content_attempt_provider_admissions_attempt
  ON content_attempt_provider_admissions(attempt_event_id, call_ordinal);
CREATE INDEX idx_content_attempt_provider_admissions_work_item
  ON content_attempt_provider_admissions(work_item_id, attempt_no, call_ordinal);
CREATE INDEX idx_content_batch_items_batch_order
  ON content_batch_items(batch_id, item_index ASC);
CREATE INDEX idx_content_batches_status_created
  ON content_batches(status, created_at ASC);
CREATE INDEX idx_content_current_result_projections_work_item
  ON content_current_result_projections(work_item_id);
CREATE INDEX idx_content_legacy_observations_resource
  ON content_legacy_observations(canonical_resource_type, canonical_resource_id, pipeline);
CREATE INDEX idx_content_request_links_work_item
  ON content_request_links(work_item_id, created_at ASC);
CREATE INDEX idx_content_result_projections_resource
  ON content_result_projections(canonical_resource_type, canonical_resource_id, pipeline);
CREATE UNIQUE INDEX idx_content_work_admission_events_identity
  ON content_work_admission_events(
    work_item_id,
    event_type,
    COALESCE(replaced_by_work_item_id, ''),
    source_hash
  );
CREATE INDEX idx_content_work_admission_events_retention
  ON content_work_admission_events(created_at ASC, id ASC);
CREATE INDEX idx_content_work_admission_events_work_item
  ON content_work_admission_events(work_item_id, created_at ASC, id ASC);
CREATE INDEX idx_content_work_items_evidence
  ON content_work_items(evidence_snapshot_id, evidence_status, status);
CREATE UNIQUE INDEX idx_content_work_items_identity_fk
  ON content_work_items (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  );
CREATE INDEX idx_content_work_items_resource
  ON content_work_items(canonical_resource_type, canonical_resource_id, pipeline);
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
CREATE INDEX idx_content_work_items_retry
  ON content_work_items(status, next_retry_at ASC);
CREATE INDEX idx_content_work_items_status_priority
  ON content_work_items(status, priority DESC, created_at ASC);
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
CREATE INDEX idx_follower_current_members_user
  ON follower_current_members(user_id, actor_github_user_id);
CREATE INDEX idx_github_connections_user_login
  ON github_connections(user_id, login);
CREATE INDEX idx_job_task_events_task_id_created_at
  ON job_task_events(task_id, created_at, id);
CREATE INDEX idx_job_task_events_task_id_event_type
  ON job_task_events(task_id, event_type);
CREATE INDEX idx_job_task_events_task_id_event_type_stage
  ON job_task_events(
    task_id,
    event_type,
    CASE WHEN json_valid(payload_json)
      THEN json_extract(payload_json, '$.stage')
      ELSE NULL
    END
  );
CREATE INDEX idx_job_tasks_available_at
  ON job_tasks(status, available_at, created_at);
CREATE INDEX idx_job_tasks_finished_status
  ON job_tasks(status, finished_at DESC, id DESC)
  WHERE finished_at IS NOT NULL;
CREATE INDEX idx_job_tasks_realtime_created
  ON job_tasks(created_at DESC, id DESC);
CREATE INDEX idx_job_tasks_running_runtime_lease
  ON job_tasks(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';
CREATE INDEX idx_job_tasks_star_connection_active
  ON job_tasks(
    task_type,
    status,
    json_extract(payload_json, '$.github_connection_id')
  )
  WHERE json_valid(payload_json);
CREATE INDEX idx_job_tasks_status_created_at
  ON job_tasks(status, created_at DESC);
CREATE INDEX idx_job_tasks_subscription_chain
  ON job_tasks(parent_task_id, task_type, finished_at DESC)
  WHERE parent_task_id IS NOT NULL;
CREATE INDEX idx_job_tasks_sync_recent_roots
  ON job_tasks(created_at DESC, id DESC)
  WHERE task_type = 'sync.subscriptions'
    AND CASE
      WHEN json_valid(result_json)
        THEN COALESCE(json_extract(result_json, '$.skipped'), 0) != 1
      ELSE 1
    END;
CREATE INDEX idx_job_tasks_task_type_created_at
  ON job_tasks(task_type, created_at DESC);
CREATE INDEX idx_job_tasks_task_type_created_at_id
  ON job_tasks(task_type, created_at DESC, id DESC);
CREATE INDEX idx_llm_call_events_call_id_created_at
  ON llm_call_events(call_id, created_at, id);
CREATE INDEX idx_llm_calls_admin_brief_latest
  ON llm_calls(parent_brief_id, updated_at DESC, id DESC);
CREATE INDEX idx_llm_calls_admin_sort
  ON llm_calls(
    CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_created_at
  ON llm_calls(
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_finished_updated_created
  ON llm_calls(
    COALESCE(finished_at, updated_at, created_at) DESC,
    status,
    source,
    id DESC
  );
CREATE INDEX idx_llm_calls_parent_brief_id
  ON llm_calls(parent_brief_id, created_at ASC);
CREATE INDEX idx_llm_calls_parent_task_id
  ON llm_calls(
    parent_task_id,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_parent_translation_batch_id
  ON llm_calls(parent_translation_batch_id);
CREATE INDEX idx_llm_calls_recovery_scan
  ON llm_calls(failure_class, retry_scheduled_at ASC, created_at ASC)
  WHERE failure_class IS NOT NULL AND retry_scheduled_at IS NOT NULL;
CREATE INDEX idx_llm_calls_requested_by_admin_sort
  ON llm_calls(
    requested_by,
    CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_requested_by_created_at
  ON llm_calls(
    requested_by,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_running_runtime_lease
  ON llm_calls(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';
CREATE INDEX idx_llm_calls_source_admin_sort
  ON llm_calls(
    source,
    CASE
      WHEN status = 'running' THEN 0
      WHEN status = 'queued' THEN 1
      ELSE 2
    END,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_source_created_at
  ON llm_calls(
    source,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_calls_status_created_at
  ON llm_calls(
    status,
    julianday(created_at) DESC,
    created_at DESC,
    id DESC
  );
CREATE INDEX idx_llm_diagnostic_access_audit_call_created
  ON llm_diagnostic_access_audit(call_id, created_at DESC, id DESC);
CREATE INDEX idx_llm_diagnostic_access_audit_created
  ON llm_diagnostic_access_audit(created_at);
CREATE INDEX idx_llm_model_health_cooldown_until
  ON llm_model_health(cooldown_until);
CREATE INDEX idx_notifications_admin_activity_source_time
  ON notifications(
    julianday(updated_at) DESC,
    thread_id,
    updated_at DESC,
    id DESC
  );
CREATE INDEX idx_notifications_admin_canonical_source
  ON notifications(thread_id, updated_at DESC, id DESC);
CREATE INDEX idx_notifications_thread_id
  ON notifications(thread_id);
CREATE INDEX idx_owned_repo_star_baselines_public_full_name_updated
ON owned_repo_star_baselines(lower(repo_full_name), updated_at DESC);
CREATE INDEX idx_owned_repo_star_baselines_repo_updated
ON owned_repo_star_baselines(repo_id, updated_at DESC);
CREATE INDEX idx_owned_repo_star_baselines_user
  ON owned_repo_star_baselines(user_id, repo_id);
CREATE INDEX idx_public_repo_release_usage_access_kind
  ON public_repo_release_usage(access_kind, repo_id);
CREATE INDEX idx_public_repo_release_usage_last_requested_at
  ON public_repo_release_usage(last_requested_at DESC);
CREATE INDEX idx_public_repo_release_usage_publisher
  ON public_repo_release_usage(published_by_user_id, full_name_lower);
CREATE INDEX idx_public_repo_release_usage_repo_id
  ON public_repo_release_usage(repo_id);
CREATE INDEX idx_release_evidence_index_lookup
  ON release_evidence_index(
    repo_id,
    audience,
    audience_user_id,
    resource_kind,
    updated_at DESC
  );
CREATE UNIQUE INDEX idx_release_evidence_index_scope_key
  ON release_evidence_index(
    repo_id,
    audience,
    COALESCE(audience_user_id, ''),
    resource_kind,
    resource_key,
    side
  );
CREATE INDEX idx_release_evidence_snapshot_items_order
  ON release_evidence_snapshot_items(snapshot_id, ordinal ASC, index_id ASC);
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
CREATE UNIQUE INDEX idx_release_evidence_task_active_snapshot
  ON job_tasks(
    task_type,
    json_extract(payload_json, '$.snapshot_id')
  )
  WHERE task_type = 'release.evidence' AND status IN ('queued', 'running');
CREATE INDEX idx_release_reading_backfill_items_cursor
  ON release_reading_backfill_items(run_id, status, published_at ASC, release_id ASC);
CREATE INDEX idx_release_reading_backfill_items_release
  ON release_reading_backfill_items(release_id, run_id);
CREATE INDEX idx_release_reading_backfill_runs_status
  ON release_reading_backfill_runs(status, created_at DESC);
CREATE INDEX idx_release_reading_backfill_runs_task_lease
  ON release_reading_backfill_runs(lease_task_id, status);
CREATE INDEX idx_releases_user_published_at
  ON releases(user_id, published_at);
CREATE INDEX idx_repo_refresh_governance_cycle_members_status
  ON repo_refresh_governance_cycle_members(cycle_id, completed_at, removed_from_pool, repo_id);
CREATE INDEX idx_repo_refresh_governance_cycles_retention
  ON repo_refresh_governance_cycles(status, completed_at ASC, id ASC)
  WHERE status = 'completed';
CREATE INDEX idx_repo_refresh_governance_cycles_status
  ON repo_refresh_governance_cycles(status, started_at DESC);
CREATE INDEX idx_repo_refresh_governance_snapshots_cycle
  ON repo_refresh_governance_snapshots(active_cycle_id, active_cycle_completed, active_cycle_window_index, priority_rank);
CREATE INDEX idx_repo_refresh_governance_snapshots_priority
  ON repo_refresh_governance_snapshots(priority_rank ASC, repo_id ASC);
CREATE INDEX idx_repo_release_sync_state_backoff
  ON repo_release_sync_state(backoff_until);
CREATE INDEX idx_repo_release_watchers_retention
  ON repo_release_watchers(status, updated_at ASC, id ASC);
CREATE INDEX idx_repo_release_watchers_task_freshness
  ON repo_release_watchers(task_id, freshness_decision, updated_at ASC, id ASC);
CREATE INDEX idx_repo_release_watchers_task_status
  ON repo_release_watchers(task_id, status, created_at ASC);
CREATE INDEX idx_repo_release_watchers_work_item_status
  ON repo_release_watchers(work_item_id, status, created_at ASC);
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
CREATE INDEX idx_repo_releases_admin_activity_source_time
  ON repo_releases(
    julianday(COALESCE(published_at, created_at, updated_at)) DESC,
    release_id DESC
  );
CREATE INDEX idx_repo_releases_admin_source_time
  ON repo_releases(
    COALESCE(published_at, created_at, updated_at) DESC,
    release_id DESC
  );
CREATE INDEX idx_repo_releases_dashboard_readable
  ON repo_releases(repo_id, published_at DESC, release_id DESC);
CREATE INDEX idx_repo_releases_detected_at
  ON repo_releases(detected_at DESC, release_id DESC);
CREATE INDEX idx_repo_releases_repo_published_at
  ON repo_releases(repo_id, published_at DESC, created_at DESC, release_id DESC);
CREATE INDEX idx_repo_releases_repo_sort_ts
  ON repo_releases(
    repo_id,
    COALESCE(published_at, created_at, updated_at),
    release_id
  );
CREATE INDEX idx_repo_releases_updated_at
  ON repo_releases(updated_at DESC, release_id DESC);
CREATE INDEX idx_repo_star_current_members_user_repo
  ON repo_star_current_members(user_id, repo_id, actor_github_user_id);
CREATE INDEX idx_runtime_owners_lease_heartbeat
  ON runtime_owners(lease_heartbeat_at);
CREATE INDEX idx_search_document_user_lanes_user
  ON search_document_user_lanes(user_id, document_id);
CREATE INDEX idx_search_documents_repo
  ON search_documents(repo_full_name, owner_login, resource_type);
CREATE INDEX idx_search_documents_resource_key
  ON search_documents(resource_type, resource_id, id);
CREATE INDEX idx_search_documents_resource_repo
  ON search_documents(resource_type, repo_id);
CREATE INDEX idx_search_documents_user_type_time
  ON search_documents(user_id, resource_type, source_time DESC, id DESC);
CREATE INDEX idx_search_metadata_backfill_queue_updated
  ON search_metadata_backfill_queue(updated_at, repo_id);
CREATE INDEX idx_search_projection_outbox_failed_poll
  ON search_projection_outbox_failed(failed_at, projection_key);
CREATE INDEX idx_search_projection_outbox_failed_resource
  ON search_projection_outbox_failed(resource_type, canonical_resource_type, resource_id);
CREATE INDEX idx_search_projection_outbox_lease
  ON search_projection_outbox(lease_expires_at, available_at, created_at);
CREATE INDEX idx_search_projection_outbox_poll
  ON search_projection_outbox(available_at, created_at, projection_key, lease_expires_at);
CREATE INDEX idx_search_projection_outbox_ready
  ON search_projection_outbox(available_at, created_at, projection_key)
  WHERE lease_expires_at IS NULL;
CREATE INDEX idx_social_activity_events_admin_activity_canonical
  ON social_activity_events(
    kind,
    lower(repo_full_name),
    discussion_number,
    occurred_at DESC
  );
CREATE INDEX idx_social_activity_events_admin_activity_time
  ON social_activity_events(
    kind,
    julianday(occurred_at) DESC,
    lower(repo_full_name),
    discussion_number
  );
CREATE INDEX idx_social_activity_events_admin_announcement_time
  ON social_activity_events(
    kind,
    occurred_at DESC,
    repo_full_name,
    discussion_number
  );
CREATE INDEX idx_social_activity_events_dashboard_readable
  ON social_activity_events(user_id, occurred_at DESC, id DESC);
CREATE UNIQUE INDEX idx_social_activity_events_dedupe
  ON social_activity_events(user_id, kind, IFNULL(repo_id, -1), actor_github_user_id, occurred_at)
  WHERE kind NOT IN ('announcement', 'repo_forked');
CREATE UNIQUE INDEX idx_social_activity_events_github_event_dedupe
  ON social_activity_events(user_id, kind, github_event_id)
  WHERE github_event_id IS NOT NULL;
CREATE INDEX idx_social_activity_events_user_kind_sort
  ON social_activity_events(user_id, kind, occurred_at DESC, created_at DESC, id DESC);
CREATE INDEX idx_social_activity_events_user_repo_discussion
  ON social_activity_events(user_id, kind, repo_full_name, discussion_number, occurred_at DESC, id DESC);
CREATE INDEX idx_social_activity_events_user_sort
  ON social_activity_events(user_id, occurred_at DESC, created_at DESC, id DESC);
CREATE INDEX idx_star_sync_epochs_connection_completed
  ON star_sync_epochs(github_connection_id, status, completed_at DESC);
CREATE UNIQUE INDEX idx_star_sync_epochs_one_active_connection
  ON star_sync_epochs(github_connection_id)
  WHERE status IN ('queued', 'running');
CREATE INDEX idx_star_sync_epochs_slice_due
  ON star_sync_epochs(status, next_slice_not_before, lease_expires_at);
CREATE INDEX idx_starred_repo_connection_memberships_epoch_prune
  ON starred_repo_connection_memberships(
    user_id,
    github_connection_id,
    last_seen_epoch_id,
    last_delta_seen_at
  );
CREATE INDEX idx_starred_repo_connection_memberships_user_repo
  ON starred_repo_connection_memberships(user_id, repo_id, stargazed_at DESC);
CREATE INDEX idx_starred_repos_public_full_name_updated
ON starred_repos(lower(full_name), updated_at DESC);
CREATE INDEX idx_starred_repos_repo_public_updated
ON starred_repos(repo_id, is_private, updated_at DESC);
CREATE INDEX idx_sync_subscription_events_repo_full_name_created_at
  ON sync_subscription_events(repo_full_name, created_at DESC);
CREATE INDEX idx_sync_subscription_events_repo_id_created_at
  ON sync_subscription_events(repo_id, created_at DESC);
CREATE INDEX idx_sync_subscription_events_task_id_created_at
  ON sync_subscription_events(task_id, created_at, id);
CREATE INDEX idx_sync_subscription_events_user_id_created_at
  ON sync_subscription_events(user_id, created_at DESC);
CREATE INDEX idx_translation_attempt_events_batch_id
  ON translation_attempt_events(batch_id, id ASC);
CREATE INDEX idx_translation_attempt_events_entity_created
  ON translation_attempt_events(entity_id, kind, variant, created_at ASC, id ASC);
CREATE INDEX idx_translation_attempt_events_work_item_created
  ON translation_attempt_events(work_item_id, created_at ASC, id ASC);
CREATE INDEX idx_translation_attempt_llm_calls_attempt
  ON translation_attempt_llm_calls(work_item_id, attempt_no, ordinal, id);
CREATE INDEX idx_translation_attempt_llm_calls_call
  ON translation_attempt_llm_calls(llm_call_id, created_at, id);
CREATE INDEX idx_translation_batch_items_batch_id
  ON translation_batch_items(batch_id, item_index ASC);
CREATE INDEX idx_translation_batches_running_runtime_lease
  ON translation_batches(status, runtime_owner_id, lease_heartbeat_at)
  WHERE status = 'running';
CREATE INDEX idx_translation_batches_status_created_at
  ON translation_batches(status, created_at DESC);
CREATE INDEX idx_translation_batches_target_lang_created_at
  ON translation_batches(target_lang, created_at DESC);
CREATE INDEX idx_translation_requests_created_at
  ON translation_requests(created_at DESC, id DESC);
CREATE INDEX idx_translation_requests_origin_work_item_id
  ON translation_requests(request_origin, work_item_id);
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
CREATE INDEX idx_translation_requests_scope_created_at
  ON translation_requests(scope_user_id, created_at DESC);
CREATE INDEX idx_translation_requests_status_updated_at
  ON translation_requests(status, updated_at DESC, id DESC);
CREATE INDEX idx_translation_requests_work_item_id
  ON translation_requests(work_item_id);
CREATE INDEX idx_translation_work_items_admin_entity_kind_attempt
  ON translation_work_items(entity_id, kind, attempt_count DESC);
CREATE INDEX idx_translation_work_items_batch_id
  ON translation_work_items(batch_id);
CREATE INDEX idx_translation_work_items_deadline
  ON translation_work_items(status, deadline_at ASC);
CREATE INDEX idx_translation_work_items_recovery_scan
  ON translation_work_items(failure_class, next_retry_at ASC, retry_expires_at ASC, id ASC)
  WHERE failure_class IS NOT NULL AND next_retry_at IS NOT NULL;
CREATE INDEX idx_translation_work_items_retry_disposition
  ON translation_work_items(retry_disposition, next_retry_at ASC, updated_at ASC)
  WHERE retry_disposition != 'not_needed';
CREATE INDEX idx_translation_work_items_status_partition
  ON translation_work_items(status, target_lang, protocol_version, model_profile, created_at ASC);
CREATE INDEX idx_user_api_keys_hash_active
  ON user_api_keys(key_hash)
  WHERE revoked_at IS NULL;
CREATE INDEX idx_user_api_keys_user_created_at
  ON user_api_keys(user_id, created_at, id);
CREATE INDEX idx_user_passkeys_user_created_at
  ON user_passkeys(user_id, created_at, id);
CREATE INDEX idx_user_repo_associations_repo_id
  ON user_repo_associations(repo_id, user_id);
CREATE INDEX idx_user_repo_associations_repo_lower
  ON user_repo_associations(repo_full_name_lower);
CREATE INDEX idx_user_repo_associations_user_following
  ON user_repo_associations(user_id, is_following, repo_full_name_lower);
CREATE INDEX idx_user_repo_associations_user_repo_id
  ON user_repo_associations(user_id, repo_id);
CREATE UNIQUE INDEX idx_users_passkey_user_handle_uuid
  ON users(passkey_user_handle_uuid)
  WHERE passkey_user_handle_uuid IS NOT NULL;
CREATE INDEX idx_users_pause_maintenance
  ON users (is_disabled, paused_at, last_active_at, created_at);
CREATE UNIQUE INDEX idx_users_webhook_push_callback_key
  ON users(webhook_push_callback_key)
  WHERE webhook_push_callback_key IS NOT NULL;
CREATE UNIQUE INDEX idx_webhook_push_audit_inflight
  ON job_tasks(task_type)
  WHERE task_type = 'webhook.push.audit'
    AND status IN ('queued', 'running');
CREATE INDEX idx_webhook_push_deliveries_received_at
  ON webhook_push_deliveries(received_at);
CREATE UNIQUE INDEX idx_webhook_push_manage_inflight
  ON job_tasks(task_type, requested_by)
  WHERE task_type = 'webhook.push.manage'
    AND requested_by IS NOT NULL
    AND status IN ('queued', 'running');
CREATE INDEX idx_webhook_push_reconcile_demands_pending
  ON webhook_push_reconcile_demands(requested_generation, completed_generation, requested_at)
  WHERE requested_generation > completed_generation;
CREATE UNIQUE INDEX idx_webhook_push_repos_hook_id
  ON webhook_push_repos(hook_id)
  WHERE hook_id IS NOT NULL;
CREATE INDEX idx_webhook_push_repos_user_status
  ON webhook_push_repos(user_id, status, repo_full_name);
CREATE TRIGGER admin_collection_coverage_announcement_insert
AFTER INSERT ON social_activity_events
WHEN NEW.kind = 'announcement'
  AND NEW.repo_full_name IS NOT NULL
  AND NEW.discussion_number IS NOT NULL
BEGIN
  INSERT OR IGNORE INTO admin_collection_processing_coverage
    (record_kind, record_id, pipeline, status_origin)
  VALUES
    ('announcement', lower(NEW.repo_full_name) || '#' || CAST(NEW.discussion_number AS TEXT), 'translation', 'never_started'),
    ('announcement', lower(NEW.repo_full_name) || '#' || CAST(NEW.discussion_number AS TEXT), 'polish', 'never_started');
END;
CREATE TRIGGER admin_collection_coverage_brief_insert
AFTER INSERT ON briefs
BEGIN
  INSERT OR IGNORE INTO admin_collection_processing_coverage
    (record_kind, record_id, pipeline, status_origin)
  VALUES ('brief', NEW.id, 'polish', 'never_started');
END;
CREATE TRIGGER admin_collection_coverage_notification_insert
AFTER INSERT ON notifications
BEGIN
  INSERT OR IGNORE INTO admin_collection_processing_coverage
    (record_kind, record_id, pipeline, status_origin)
  VALUES
    ('notification', NEW.thread_id, 'translation', 'never_started'),
    ('notification', NEW.thread_id, 'polish', 'never_started');
END;
CREATE TRIGGER admin_collection_coverage_repo_release_insert
AFTER INSERT ON repo_releases
BEGIN
  INSERT OR IGNORE INTO admin_collection_processing_coverage
    (record_kind, record_id, pipeline, status_origin)
  VALUES
    ('release', CAST(NEW.release_id AS TEXT), 'translation', 'never_started'),
    ('release', CAST(NEW.release_id AS TEXT), 'polish', 'never_started');
END;
CREATE TRIGGER search_ai_translations_ad AFTER DELETE ON ai_translations BEGIN
  DELETE FROM search_document_user_lanes_fts WHERE user_id=OLD.user_id AND doc_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND resource_id=OLD.entity_id) OR (resource_type='announcement' AND resource_id=lower(OLD.entity_id)) OR (resource_type='notification' AND resource_id=OLD.entity_id));
  DELETE FROM search_document_user_lanes WHERE user_id=OLD.user_id AND document_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND resource_id=OLD.entity_id) OR (resource_type='announcement' AND resource_id=lower(OLD.entity_id)) OR (resource_type='notification' AND resource_id=OLD.entity_id));
  INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at)
  SELECT d.id,OLD.user_id,
    (SELECT trim(COALESCE(t.title,'')||' '||COALESCE(t.summary,'')) FROM ai_translations t
     WHERE t.user_id=OLD.user_id AND t.entity_id=OLD.entity_id
       AND t.lang='zh-CN'
       AND t.status IN ('ready','disabled','missing') AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
       AND lower(t.entity_type) NOT LIKE '%smart%'
       AND ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%')
         OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%')
         OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%'))
     ORDER BY t.updated_at DESC,t.id DESC LIMIT 1),
    (SELECT trim(COALESCE(t.title,'')||' '||COALESCE(t.summary,'')) FROM ai_translations t
     WHERE t.user_id=OLD.user_id AND t.entity_id=OLD.entity_id
       AND t.lang='zh-CN'
       AND t.status IN ('ready','disabled','missing') AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
       AND lower(t.entity_type) LIKE '%smart%'
       AND ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%')
         OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%')
         OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%'))
     ORDER BY t.updated_at DESC,t.id DESC LIMIT 1),
    COALESCE((SELECT MAX(t.updated_at) FROM ai_translations t
              WHERE t.user_id=OLD.user_id AND t.entity_id=OLD.entity_id
                AND t.lang='zh-CN'
                AND t.status IN ('ready','disabled','missing')
                AND ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%')
                  OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%')
                  OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%'))), CURRENT_TIMESTAMP)
  FROM search_documents d
  WHERE ((d.resource_type='release' AND lower(OLD.entity_type) LIKE 'release%' AND d.resource_id=OLD.entity_id)
      OR (d.resource_type='announcement' AND lower(OLD.entity_type) LIKE 'announcement%' AND d.resource_id=lower(OLD.entity_id))
      OR (d.resource_type='notification' AND lower(OLD.entity_type) LIKE 'notification%' AND d.resource_id=OLD.entity_id))
    AND EXISTS (SELECT 1 FROM ai_translations t
                WHERE t.user_id=OLD.user_id AND t.entity_id=OLD.entity_id
                  AND t.lang='zh-CN'
                  AND t.status IN ('ready','disabled','missing')
                  AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
                  AND ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%')
                    OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%')
                    OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%')))
  ON CONFLICT(document_id,user_id) DO UPDATE SET
    translated_text=excluded.translated_text,
    smart_text=excluded.smart_text,
    updated_at=excluded.updated_at;
  INSERT INTO search_document_user_lanes_fts (doc_id,user_id,translated_text,smart_text)
  SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'')
  FROM search_document_user_lanes
  WHERE user_id=OLD.user_id AND document_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND resource_id=OLD.entity_id) OR (resource_type='announcement' AND resource_id=lower(OLD.entity_id)) OR (resource_type='notification' AND resource_id=OLD.entity_id));
END;
CREATE TRIGGER search_ai_translations_ai AFTER INSERT ON ai_translations BEGIN
  INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at)
  SELECT d.id,NEW.user_id,
    CASE WHEN lower(NEW.entity_type) LIKE '%smart%' THEN NULL ELSE trim(COALESCE(NEW.title,'') || ' ' || COALESCE(NEW.summary,'')) END,
    CASE WHEN lower(NEW.entity_type) LIKE '%smart%' THEN trim(COALESCE(NEW.title,'') || ' ' || COALESCE(NEW.summary,'')) ELSE NULL END,
    NEW.updated_at
  FROM search_documents d
  WHERE ((d.resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND d.resource_id=NEW.entity_id) OR (d.resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND d.resource_id=lower(NEW.entity_id)) OR (d.resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND d.resource_id=NEW.entity_id))
    AND NEW.lang='zh-CN'
    AND NEW.status IN ('ready','disabled','missing')
    AND (NEW.title IS NOT NULL OR NEW.summary IS NOT NULL)
    AND NOT EXISTS (
      SELECT 1 FROM ai_translations newer
      WHERE newer.user_id=NEW.user_id AND newer.entity_id=NEW.entity_id
        AND newer.lang=NEW.lang
        AND newer.status IN ('ready','disabled','missing')
        AND (newer.title IS NOT NULL OR newer.summary IS NOT NULL)
        AND ((lower(NEW.entity_type) LIKE '%smart%' AND lower(newer.entity_type) LIKE '%smart%')
          OR (lower(NEW.entity_type) NOT LIKE '%smart%' AND lower(newer.entity_type) NOT LIKE '%smart%'))
        AND ((lower(NEW.entity_type) LIKE 'release%' AND lower(newer.entity_type) LIKE 'release%')
          OR (lower(NEW.entity_type) LIKE 'announcement%' AND lower(newer.entity_type) LIKE 'announcement%')
          OR (lower(NEW.entity_type) LIKE 'notification%' AND lower(newer.entity_type) LIKE 'notification%'))
        AND (newer.updated_at > NEW.updated_at OR (newer.updated_at = NEW.updated_at AND newer.id > NEW.id))
    )
  ON CONFLICT(document_id,user_id) DO UPDATE SET
    translated_text=CASE WHEN excluded.translated_text IS NOT NULL THEN excluded.translated_text ELSE search_document_user_lanes.translated_text END,
    smart_text=CASE WHEN excluded.smart_text IS NOT NULL THEN excluded.smart_text ELSE search_document_user_lanes.smart_text END,
    updated_at=MAX(search_document_user_lanes.updated_at,excluded.updated_at);
  DELETE FROM search_document_user_lanes_fts WHERE user_id=NEW.user_id AND doc_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND resource_id=NEW.entity_id) OR (resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND resource_id=lower(NEW.entity_id)) OR (resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND resource_id=NEW.entity_id));
  INSERT INTO search_document_user_lanes_fts SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE user_id=NEW.user_id AND document_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND resource_id=NEW.entity_id) OR (resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND resource_id=lower(NEW.entity_id)) OR (resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND resource_id=NEW.entity_id));
END;
CREATE TRIGGER search_ai_translations_au AFTER UPDATE ON ai_translations BEGIN
  INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at)
  SELECT d.id,NEW.user_id,
    CASE WHEN lower(NEW.entity_type) LIKE '%smart%' THEN NULL ELSE trim(COALESCE(NEW.title,'') || ' ' || COALESCE(NEW.summary,'')) END,
    CASE WHEN lower(NEW.entity_type) LIKE '%smart%' THEN trim(COALESCE(NEW.title,'') || ' ' || COALESCE(NEW.summary,'')) ELSE NULL END,
    NEW.updated_at
  FROM search_documents d
  WHERE ((d.resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND d.resource_id=NEW.entity_id) OR (d.resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND d.resource_id=lower(NEW.entity_id)) OR (d.resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND d.resource_id=NEW.entity_id))
    AND NEW.lang='zh-CN'
    AND NEW.status IN ('ready','disabled','missing')
    AND (NEW.title IS NOT NULL OR NEW.summary IS NOT NULL)
    AND NOT EXISTS (
      SELECT 1 FROM ai_translations newer
      WHERE newer.user_id=NEW.user_id AND newer.entity_id=NEW.entity_id
        AND newer.lang=NEW.lang
        AND newer.status IN ('ready','disabled','missing')
        AND (newer.title IS NOT NULL OR newer.summary IS NOT NULL)
        AND ((lower(NEW.entity_type) LIKE '%smart%' AND lower(newer.entity_type) LIKE '%smart%')
          OR (lower(NEW.entity_type) NOT LIKE '%smart%' AND lower(newer.entity_type) NOT LIKE '%smart%'))
        AND ((lower(NEW.entity_type) LIKE 'release%' AND lower(newer.entity_type) LIKE 'release%')
          OR (lower(NEW.entity_type) LIKE 'announcement%' AND lower(newer.entity_type) LIKE 'announcement%')
          OR (lower(NEW.entity_type) LIKE 'notification%' AND lower(newer.entity_type) LIKE 'notification%'))
        AND (newer.updated_at > NEW.updated_at OR (newer.updated_at = NEW.updated_at AND newer.id > NEW.id))
    )
  ON CONFLICT(document_id,user_id) DO UPDATE SET
    translated_text=CASE WHEN excluded.translated_text IS NOT NULL THEN excluded.translated_text ELSE search_document_user_lanes.translated_text END,
    smart_text=CASE WHEN excluded.smart_text IS NOT NULL THEN excluded.smart_text ELSE search_document_user_lanes.smart_text END,
    updated_at=MAX(search_document_user_lanes.updated_at,excluded.updated_at);
  DELETE FROM search_document_user_lanes_fts WHERE user_id=NEW.user_id AND doc_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND resource_id=NEW.entity_id) OR (resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND resource_id=lower(NEW.entity_id)) OR (resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND resource_id=NEW.entity_id));
  INSERT INTO search_document_user_lanes_fts SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE user_id=NEW.user_id AND document_id IN (SELECT id FROM search_documents WHERE (resource_type='release' AND lower(NEW.entity_type) LIKE 'release%' AND resource_id=NEW.entity_id) OR (resource_type='announcement' AND lower(NEW.entity_type) LIKE 'announcement%' AND resource_id=lower(NEW.entity_id)) OR (resource_type='notification' AND lower(NEW.entity_type) LIKE 'notification%' AND resource_id=NEW.entity_id));
END;
CREATE TRIGGER search_announcements_outbox_ad AFTER DELETE ON social_activity_events
WHEN OLD.kind = 'announcement' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'announcement:' || OLD.id,
    'delete', 'announcement', OLD.id, OLD.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_announcements_outbox_ai AFTER INSERT ON social_activity_events
WHEN NEW.kind = 'announcement' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'announcement:' || NEW.id,
    'upsert', 'announcement', NEW.id, NEW.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_announcements_outbox_au AFTER UPDATE ON social_activity_events
WHEN NEW.kind = 'announcement' OR OLD.kind = 'announcement' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, previous_resource_id,
    user_id, available_at, created_at, updated_at
  ) VALUES (
    'announcement:' || COALESCE(NEW.id, OLD.id),
    CASE WHEN NEW.kind = 'announcement' THEN 'upsert' ELSE 'delete' END,
    'announcement', COALESCE(NEW.id, OLD.id),
    CASE WHEN NEW.kind = 'announcement' AND OLD.kind = 'announcement' THEN
      CASE WHEN OLD.repo_full_name IS NOT NULL AND OLD.discussion_number IS NOT NULL
           THEN lower(OLD.repo_full_name) || '#' || OLD.discussion_number
           ELSE OLD.id
      END
    END,
    COALESCE(NEW.user_id, OLD.user_id),
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    previous_resource_id = CASE
      WHEN excluded.operation = 'upsert'
      THEN COALESCE(search_projection_outbox.previous_resource_id, excluded.previous_resource_id)
      ELSE search_projection_outbox.previous_resource_id
    END,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_briefs_ad AFTER DELETE ON briefs BEGIN
  DELETE FROM search_documents_fts WHERE doc_id='brief:'||OLD.id;
  DELETE FROM search_documents WHERE id='brief:'||OLD.id;
END;
CREATE TRIGGER search_briefs_ai AFTER INSERT ON briefs BEGIN
  INSERT INTO search_documents (id,user_id,resource_type,resource_id,title,body,source_time,target_path,created_at,updated_at)
  VALUES ('brief:'||NEW.id,NEW.user_id,'brief',NEW.id,'日报 '||NEW.date,NEW.content_markdown,COALESCE(NEW.window_end_utc,NEW.created_at),'/briefs?brief='||NEW.id,NEW.created_at,NEW.updated_at)
  ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,updated_at=excluded.updated_at;
  DELETE FROM search_documents_fts WHERE doc_id='brief:'||NEW.id;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='brief:'||NEW.id;
END;
CREATE TRIGGER search_briefs_au AFTER UPDATE ON briefs BEGIN
  UPDATE search_documents SET title='日报 '||NEW.date,body=NEW.content_markdown,source_time=COALESCE(NEW.window_end_utc,NEW.updated_at),target_path='/briefs?brief='||NEW.id,updated_at=NEW.updated_at WHERE id='brief:'||NEW.id;
  DELETE FROM search_documents_fts WHERE doc_id='brief:'||NEW.id;
  INSERT INTO search_documents_fts SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id='brief:'||NEW.id;
END;
CREATE TRIGGER search_content_projections_ad AFTER DELETE ON content_result_projections
WHEN OLD.pipeline IN ('translation', 'polishing') AND OLD.target_lang = 'zh-CN' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, canonical_resource_type,
    available_at, created_at, updated_at
  ) VALUES (
    'content:' || OLD.canonical_resource_type || ':' || OLD.canonical_resource_id,
    'upsert', 'content', OLD.canonical_resource_id, OLD.canonical_resource_type,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    canonical_resource_type = excluded.canonical_resource_type,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_content_projections_outbox_ai AFTER INSERT ON content_result_projections
WHEN NEW.pipeline IN ('translation', 'polishing') AND NEW.target_lang = 'zh-CN' BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, canonical_resource_type,
    available_at, created_at, updated_at
  ) VALUES (
    'content:' || NEW.canonical_resource_type || ':' || NEW.canonical_resource_id,
    'upsert', 'content', NEW.canonical_resource_id, NEW.canonical_resource_type,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    canonical_resource_type = excluded.canonical_resource_type,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_content_projections_outbox_au_new
AFTER UPDATE ON content_result_projections
WHEN NEW.pipeline IN ('translation', 'polishing')
 AND NEW.target_lang = 'zh-CN'
BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, canonical_resource_type,
    available_at, created_at, updated_at
  ) VALUES (
    'content:' || NEW.canonical_resource_type || ':' || NEW.canonical_resource_id,
    'upsert', 'content', NEW.canonical_resource_id, NEW.canonical_resource_type,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    canonical_resource_type = excluded.canonical_resource_type,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_content_projections_outbox_au_old
AFTER UPDATE ON content_result_projections
WHEN OLD.pipeline IN ('translation', 'polishing')
 AND OLD.target_lang = 'zh-CN'
 AND (
   NEW.pipeline NOT IN ('translation', 'polishing')
   OR NEW.target_lang <> 'zh-CN'
   OR NEW.canonical_resource_type IS NOT OLD.canonical_resource_type
   OR NEW.canonical_resource_id IS NOT OLD.canonical_resource_id
 )
BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, canonical_resource_type,
    available_at, created_at, updated_at
  ) VALUES (
    'content:' || OLD.canonical_resource_type || ':' || OLD.canonical_resource_id,
    'upsert', 'content', OLD.canonical_resource_id, OLD.canonical_resource_type,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    canonical_resource_type = excluded.canonical_resource_type,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE VIEW search_document_user_lanes_fts(
  doc_id, user_id, translated_text, smart_text
) AS
SELECT r.document_id, r.user_id, f.translated_text, f.smart_text
FROM search_fts_user_lane_rows r
JOIN search_document_user_lanes_fts_v2 f ON f.rowid = r.fts_rowid;
CREATE TRIGGER search_document_user_lanes_fts_compat_ad
INSTEAD OF DELETE ON search_document_user_lanes_fts BEGIN
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = OLD.doc_id AND user_id = OLD.user_id
  );
END;
CREATE TRIGGER search_document_user_lanes_fts_compat_ai
INSTEAD OF INSERT ON search_document_user_lanes_fts BEGIN
  INSERT INTO search_fts_user_lane_rows (document_id, user_id)
  VALUES (NEW.doc_id, NEW.user_id)
  ON CONFLICT(document_id, user_id) DO NOTHING;
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = NEW.doc_id AND user_id = NEW.user_id
  );
  INSERT INTO search_document_user_lanes_fts_v2
    (rowid, doc_id, user_id, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_user_lane_rows
     WHERE document_id = NEW.doc_id AND user_id = NEW.user_id),
    NEW.doc_id, NEW.user_id, NEW.translated_text, NEW.smart_text
  );
END;
CREATE TRIGGER search_document_user_lanes_fts_compat_au
INSTEAD OF UPDATE ON search_document_user_lanes_fts BEGIN
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = OLD.doc_id AND user_id = OLD.user_id
  );
  INSERT INTO search_fts_user_lane_rows (document_id, user_id)
  VALUES (NEW.doc_id, NEW.user_id)
  ON CONFLICT(document_id, user_id) DO NOTHING;
  INSERT INTO search_document_user_lanes_fts_v2
    (rowid, doc_id, user_id, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_user_lane_rows
     WHERE document_id = NEW.doc_id AND user_id = NEW.user_id),
    NEW.doc_id, NEW.user_id, NEW.translated_text, NEW.smart_text
  );
END;
CREATE TRIGGER search_document_user_lanes_fts_v2_ad
AFTER DELETE ON search_document_user_lanes BEGIN
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = OLD.document_id AND user_id = OLD.user_id
  );
  DELETE FROM search_fts_user_lane_rows
  WHERE document_id = OLD.document_id AND user_id = OLD.user_id;
END;
CREATE TRIGGER search_document_user_lanes_fts_v2_ai
AFTER INSERT ON search_document_user_lanes BEGIN
  INSERT INTO search_fts_user_lane_rows (document_id, user_id)
  VALUES (NEW.document_id, NEW.user_id)
  ON CONFLICT(document_id, user_id) DO NOTHING;
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = NEW.document_id AND user_id = NEW.user_id
  );
  INSERT INTO search_document_user_lanes_fts_v2
    (rowid, doc_id, user_id, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_user_lane_rows
     WHERE document_id = NEW.document_id AND user_id = NEW.user_id),
    NEW.document_id, NEW.user_id, COALESCE(NEW.translated_text, ''),
    COALESCE(NEW.smart_text, '')
  );
END;
CREATE TRIGGER search_document_user_lanes_fts_v2_au
AFTER UPDATE ON search_document_user_lanes BEGIN
  DELETE FROM search_document_user_lanes_fts_v2
  WHERE rowid = (
    SELECT fts_rowid FROM search_fts_user_lane_rows
    WHERE document_id = OLD.document_id AND user_id = OLD.user_id
  );
  INSERT INTO search_fts_user_lane_rows (document_id, user_id)
  VALUES (NEW.document_id, NEW.user_id)
  ON CONFLICT(document_id, user_id) DO NOTHING;
  INSERT INTO search_document_user_lanes_fts_v2
    (rowid, doc_id, user_id, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_user_lane_rows
     WHERE document_id = NEW.document_id AND user_id = NEW.user_id),
    NEW.document_id, NEW.user_id, COALESCE(NEW.translated_text, ''),
    COALESCE(NEW.smart_text, '')
  );
END;
CREATE TRIGGER search_documents_ai AFTER INSERT ON search_documents
WHEN NEW.resource_type = 'release' BEGIN
  INSERT INTO search_document_user_lanes (
    document_id, user_id, translated_text, updated_at
  )
  SELECT NEW.id, ranked.user_id, ranked.translated_text, ranked.updated_at
  FROM (
    SELECT
      t.user_id,
      trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, '')) AS translated_text,
      t.updated_at,
      ROW_NUMBER() OVER (
        PARTITION BY t.user_id
        ORDER BY t.updated_at DESC, t.id DESC
      ) AS lane_rank
    FROM ai_translations t
    WHERE t.lang = 'zh-CN'
      AND t.status IN ('ready', 'disabled', 'missing')
      AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
      AND lower(t.entity_type) NOT LIKE '%smart%'
      AND lower(t.entity_type) LIKE 'release%'
      AND t.entity_id = NEW.resource_id
  ) ranked
  WHERE ranked.lane_rank = 1
  ON CONFLICT(document_id, user_id) DO UPDATE SET
    translated_text = excluded.translated_text,
    updated_at = MAX(search_document_user_lanes.updated_at, excluded.updated_at);

  INSERT INTO search_document_user_lanes (
    document_id, user_id, smart_text, updated_at
  )
  SELECT NEW.id, ranked.user_id, ranked.smart_text, ranked.updated_at
  FROM (
    SELECT
      t.user_id,
      trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, '')) AS smart_text,
      t.updated_at,
      ROW_NUMBER() OVER (
        PARTITION BY t.user_id
        ORDER BY t.updated_at DESC, t.id DESC
      ) AS lane_rank
    FROM ai_translations t
    WHERE t.lang = 'zh-CN'
      AND t.status IN ('ready', 'disabled', 'missing')
      AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
      AND lower(t.entity_type) LIKE '%smart%'
      AND lower(t.entity_type) LIKE 'release%'
      AND t.entity_id = NEW.resource_id
  ) ranked
  WHERE ranked.lane_rank = 1
  ON CONFLICT(document_id, user_id) DO UPDATE SET
    smart_text = excluded.smart_text,
    updated_at = MAX(search_document_user_lanes.updated_at, excluded.updated_at);

  DELETE FROM search_document_user_lanes_fts WHERE doc_id = NEW.id;
  INSERT INTO search_document_user_lanes_fts
    (doc_id, user_id, translated_text, smart_text)
  SELECT document_id, user_id, COALESCE(translated_text, ''), COALESCE(smart_text, '')
  FROM search_document_user_lanes
  WHERE document_id = NEW.id;
END;
CREATE VIEW search_documents_fts(
  doc_id, title, body, repo_full_name, translated_text, smart_text
) AS
SELECT r.document_id, f.title, f.body, f.repo_full_name,
       f.translated_text, f.smart_text
FROM search_fts_document_rows r
JOIN search_documents_fts_v2 f ON f.rowid = r.fts_rowid;
CREATE TRIGGER search_documents_fts_compat_ad
INSTEAD OF DELETE ON search_documents_fts BEGIN
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = OLD.doc_id);
END;
CREATE TRIGGER search_documents_fts_compat_ai
INSTEAD OF INSERT ON search_documents_fts BEGIN
  INSERT INTO search_fts_document_rows (document_id)
  VALUES (NEW.doc_id)
  ON CONFLICT(document_id) DO NOTHING;
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.doc_id);
  INSERT INTO search_documents_fts_v2
    (rowid, doc_id, title, body, repo_full_name, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.doc_id),
    NEW.doc_id, NEW.title, NEW.body, NEW.repo_full_name,
    NEW.translated_text, NEW.smart_text
  );
END;
CREATE TRIGGER search_documents_fts_compat_au
INSTEAD OF UPDATE ON search_documents_fts BEGIN
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = OLD.doc_id);
  INSERT INTO search_fts_document_rows (document_id)
  VALUES (NEW.doc_id)
  ON CONFLICT(document_id) DO NOTHING;
  INSERT INTO search_documents_fts_v2
    (rowid, doc_id, title, body, repo_full_name, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.doc_id),
    NEW.doc_id, NEW.title, NEW.body, NEW.repo_full_name,
    NEW.translated_text, NEW.smart_text
  );
END;
CREATE TRIGGER search_documents_fts_v2_ad
AFTER DELETE ON search_documents BEGIN
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = OLD.id);
  DELETE FROM search_fts_document_rows WHERE document_id = OLD.id;
END;
CREATE TRIGGER search_documents_fts_v2_ai
AFTER INSERT ON search_documents BEGIN
  INSERT INTO search_fts_document_rows (document_id)
  VALUES (NEW.id)
  ON CONFLICT(document_id) DO NOTHING;
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.id);
  INSERT INTO search_documents_fts_v2
    (rowid, doc_id, title, body, repo_full_name, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.id),
    NEW.id, COALESCE(NEW.title, ''), COALESCE(NEW.body, ''),
    COALESCE(NEW.repo_full_name, ''), COALESCE(NEW.translated_text, ''),
    COALESCE(NEW.smart_text, '')
  );
END;
CREATE TRIGGER search_documents_fts_v2_au
AFTER UPDATE ON search_documents BEGIN
  DELETE FROM search_documents_fts_v2
  WHERE rowid = (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = OLD.id);
  INSERT INTO search_fts_document_rows (document_id)
  VALUES (NEW.id)
  ON CONFLICT(document_id) DO NOTHING;
  INSERT INTO search_documents_fts_v2
    (rowid, doc_id, title, body, repo_full_name, translated_text, smart_text)
  VALUES (
    (SELECT fts_rowid FROM search_fts_document_rows WHERE document_id = NEW.id),
    NEW.id, COALESCE(NEW.title, ''), COALESCE(NEW.body, ''),
    COALESCE(NEW.repo_full_name, ''), COALESCE(NEW.translated_text, ''),
    COALESCE(NEW.smart_text, '')
  );
END;
CREATE TRIGGER search_notifications_outbox_ad AFTER DELETE ON notifications BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'notification:' || OLD.user_id || ':' || OLD.thread_id,
    'delete', 'notification', OLD.thread_id, OLD.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_notifications_outbox_ai AFTER INSERT ON notifications BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'notification:' || NEW.user_id || ':' || NEW.thread_id,
    'upsert', 'notification', NEW.thread_id, NEW.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_notifications_outbox_au AFTER UPDATE ON notifications BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'notification:' || OLD.user_id || ':' || OLD.thread_id,
    'delete', 'notification', OLD.thread_id, OLD.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;

  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, user_id,
    available_at, created_at, updated_at
  ) VALUES (
    'notification:' || NEW.user_id || ':' || NEW.thread_id,
    'upsert', 'notification', NEW.thread_id, NEW.user_id,
    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
  ) ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    user_id = excluded.user_id,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_owned_repo_star_baselines_ad
AFTER DELETE ON owned_repo_star_baselines
WHEN OLD.repo_id IS NOT NULL BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  VALUES (OLD.repo_id, CURRENT_TIMESTAMP)
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_owned_repo_star_baselines_ai
AFTER INSERT ON owned_repo_star_baselines
WHEN NEW.repo_id IS NOT NULL BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  VALUES (NEW.repo_id, CURRENT_TIMESTAMP)
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_owned_repo_star_baselines_au
AFTER UPDATE ON owned_repo_star_baselines BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL AND OLD.repo_id IS NOT NEW.repo_id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_projection_outbox_reactivate_content_document
AFTER INSERT ON search_documents
WHEN EXISTS (
  SELECT 1
  FROM search_projection_outbox_failed f
  WHERE f.resource_type = 'content'
    AND f.canonical_resource_type = NEW.resource_type
    AND f.resource_id = NEW.resource_id
)
BEGIN
  INSERT INTO search_projection_outbox (
    projection_key, operation, resource_type, resource_id, canonical_resource_type,
    user_id, attempt_count, available_at, lease_owner, lease_expires_at,
    last_error, created_at, updated_at
  )
  SELECT projection_key, operation, resource_type, resource_id, canonical_resource_type,
         user_id, 0, CURRENT_TIMESTAMP, NULL, NULL, NULL, created_at, CURRENT_TIMESTAMP
  FROM search_projection_outbox_failed
  WHERE resource_type = 'content'
    AND canonical_resource_type = NEW.resource_type
    AND resource_id = NEW.resource_id
  ON CONFLICT(projection_key) DO UPDATE SET
    operation = excluded.operation,
    resource_type = excluded.resource_type,
    resource_id = excluded.resource_id,
    canonical_resource_type = excluded.canonical_resource_type,
    user_id = excluded.user_id,
    attempt_count = excluded.attempt_count,
    available_at = excluded.available_at,
    lease_owner = NULL,
    lease_expires_at = NULL,
    last_error = NULL,
    updated_at = excluded.updated_at;
END;
CREATE TRIGGER search_projection_outbox_reactivate_failed
AFTER INSERT ON search_projection_outbox
BEGIN
  DELETE FROM search_projection_outbox_failed
  WHERE projection_key = NEW.projection_key;
END;
CREATE TRIGGER search_projection_outbox_reset_source_state
AFTER UPDATE ON search_projection_outbox
WHEN NEW.lease_owner IS NULL
 AND NEW.last_error IS NULL
 AND NEW.fanout_cursor IS OLD.fanout_cursor
 AND (NEW.attempt_count > 0 OR NEW.fanout_cursor IS NOT NULL)
BEGIN
  UPDATE search_projection_outbox
  SET attempt_count = 0,
      fanout_cursor = NULL,
      updated_at = CURRENT_TIMESTAMP
  WHERE projection_key = NEW.projection_key;
END;
CREATE VIEW search_release_metadata AS
SELECT d.id,
       d.repo_id,
       COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=d.repo_id LIMIT 1),
                (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=d.repo_id LIMIT 1),
                (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=d.repo_id LIMIT 1),
                (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=d.repo_id LIMIT 1)) AS repo_full_name,
       COALESCE((SELECT CASE WHEN instr(rwi.repo_full_name,'/')>0 THEN substr(rwi.repo_full_name,1,instr(rwi.repo_full_name,'/')-1) ELSE rwi.repo_full_name END FROM repo_release_work_items rwi WHERE rwi.repo_id=d.repo_id LIMIT 1),
                (SELECT vr.owner_login FROM user_release_visible_repos vr WHERE vr.repo_id=d.repo_id LIMIT 1),
                (SELECT ura.owner_login FROM user_repo_associations ura WHERE ura.repo_id=d.repo_id LIMIT 1),
                (SELECT sr.owner_login FROM starred_repos sr WHERE sr.repo_id=d.repo_id LIMIT 1)) AS owner_login,
       CASE WHEN COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=d.repo_id LIMIT 1),
                          (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=d.repo_id LIMIT 1),
                          (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=d.repo_id LIMIT 1),
                          (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=d.repo_id LIMIT 1)) IS NOT NULL
            THEN '/' || COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=d.repo_id LIMIT 1),
                                 (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=d.repo_id LIMIT 1),
                                 (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=d.repo_id LIMIT 1),
                                 (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=d.repo_id LIMIT 1)) || '/releases/tag/' || rr.tag_name
       END AS target_path
FROM search_documents d
LEFT JOIN repo_releases rr ON rr.release_id=CAST(d.resource_id AS INTEGER)
WHERE d.resource_type='release';
CREATE TRIGGER search_repo_associations_ad
AFTER DELETE ON user_repo_associations BEGIN
  DELETE FROM search_documents
  WHERE id = 'repository:' || OLD.user_id || ':' || OLD.repo_full_name_lower
    AND NOT EXISTS (
      SELECT 1 FROM starred_repos ura
      WHERE ura.user_id = OLD.user_id
        AND lower(ura.full_name) = lower(OLD.repo_full_name_lower)
    );
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_repo_associations_ai
AFTER INSERT ON user_repo_associations BEGIN
  DELETE FROM search_documents
  WHERE user_id = NEW.user_id
    AND resource_type = 'repository'
    AND repo_id = NEW.repo_id
    AND id <> 'repository:' || NEW.user_id || ':' || NEW.repo_full_name_lower;
  INSERT INTO search_documents (
    id, user_id, resource_type, resource_id, repo_id, repo_full_name,
    owner_login, title, body, source_time, target_path, target_url,
    created_at, updated_at
  ) VALUES (
    'repository:' || NEW.user_id || ':' || NEW.repo_full_name_lower,
    NEW.user_id, 'repository', NEW.repo_full_name_lower, NEW.repo_id,
    NEW.repo_full_name, NEW.owner_login, NEW.repo_name, NEW.description,
    NEW.updated_at, '/focus/repo/' || NEW.owner_login || '/' || NEW.repo_name,
    NEW.html_url, NEW.created_at, NEW.updated_at
  ) ON CONFLICT(user_id, resource_type, resource_id) DO UPDATE SET
    repo_id = excluded.repo_id,
    repo_full_name = excluded.repo_full_name,
    owner_login = excluded.owner_login,
    title = excluded.title,
    body = excluded.body,
    source_time = excluded.source_time,
    target_path = excluded.target_path,
    target_url = excluded.target_url,
    updated_at = excluded.updated_at;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_repo_associations_au
AFTER UPDATE OF repo_id, repo_full_name, repo_full_name_lower, owner_login,
                repo_name, html_url, description ON user_repo_associations
WHEN OLD.repo_id IS NOT NEW.repo_id
  OR OLD.repo_full_name IS NOT NEW.repo_full_name
  OR OLD.repo_full_name_lower IS NOT NEW.repo_full_name_lower
  OR OLD.owner_login IS NOT NEW.owner_login
  OR OLD.repo_name IS NOT NEW.repo_name
  OR OLD.html_url IS NOT NEW.html_url
  OR OLD.description IS NOT NEW.description
BEGIN
  DELETE FROM search_documents
  WHERE user_id = NEW.user_id
    AND resource_type = 'repository'
    AND repo_id = NEW.repo_id
    AND id <> 'repository:' || NEW.user_id || ':' || NEW.repo_full_name_lower;
  DELETE FROM search_documents
  WHERE id = 'repository:' || OLD.user_id || ':' || OLD.repo_full_name_lower
    AND lower(OLD.repo_full_name_lower) <> lower(NEW.repo_full_name_lower)
    AND NOT EXISTS (
      SELECT 1 FROM starred_repos sr
      WHERE sr.user_id = OLD.user_id
        AND lower(sr.full_name) = lower(OLD.repo_full_name_lower)
    );
  INSERT INTO search_documents (
    id, user_id, resource_type, resource_id, repo_id, repo_full_name,
    owner_login, title, body, source_time, target_path, target_url,
    created_at, updated_at
  ) VALUES (
    'repository:' || NEW.user_id || ':' || NEW.repo_full_name_lower,
    NEW.user_id, 'repository', NEW.repo_full_name_lower, NEW.repo_id,
    NEW.repo_full_name, NEW.owner_login, NEW.repo_name, NEW.description,
    NEW.updated_at, '/focus/repo/' || NEW.owner_login || '/' || NEW.repo_name,
    NEW.html_url, NEW.created_at, NEW.updated_at
  ) ON CONFLICT(user_id, resource_type, resource_id) DO UPDATE SET
    repo_id = excluded.repo_id,
    repo_full_name = excluded.repo_full_name,
    owner_login = excluded.owner_login,
    title = excluded.title,
    body = excluded.body,
    source_time = excluded.source_time,
    target_path = excluded.target_path,
    target_url = excluded.target_url,
    updated_at = excluded.updated_at;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL AND OLD.repo_id IS NOT NEW.repo_id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_repo_release_work_items_ad
AFTER DELETE ON repo_release_work_items
WHEN OLD.repo_id IS NOT NULL BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  VALUES (OLD.repo_id, CURRENT_TIMESTAMP)
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_repo_release_work_items_ai
AFTER INSERT ON repo_release_work_items
WHEN NEW.repo_id IS NOT NULL BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  VALUES (NEW.repo_id, CURRENT_TIMESTAMP)
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_repo_release_work_items_au
AFTER UPDATE OF repo_id, repo_full_name ON repo_release_work_items BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL AND OLD.repo_id IS NOT NEW.repo_id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_repo_releases_ad AFTER DELETE ON repo_releases BEGIN
  DELETE FROM search_documents_fts WHERE doc_id = 'release:' || OLD.release_id;
  DELETE FROM search_documents WHERE id = 'release:' || OLD.release_id;
END;
CREATE TRIGGER search_repo_releases_ai AFTER INSERT ON repo_releases BEGIN
  INSERT INTO search_documents (id, resource_type, resource_id, repo_id, repo_full_name, owner_login, title, body, source_time, target_path, target_url, created_at, updated_at)
  VALUES ('release:' || NEW.release_id, 'release', CAST(NEW.release_id AS TEXT), NEW.repo_id,
    COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)),
    COALESCE((SELECT CASE WHEN instr(rwi.repo_full_name, '/') > 0 THEN substr(rwi.repo_full_name, 1, instr(rwi.repo_full_name, '/') - 1) ELSE rwi.repo_full_name END FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.owner_login FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.owner_login FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.owner_login FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)),
    COALESCE(NEW.name, NEW.tag_name), NEW.body, COALESCE(NEW.published_at, NEW.created_at, NEW.updated_at),
    CASE WHEN COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)) IS NOT NULL
         THEN '/' || COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)) || '/releases/tag/' || NEW.tag_name END,
    NEW.html_url, NEW.updated_at, NEW.updated_at)
  ON CONFLICT(id) DO UPDATE SET repo_id=excluded.repo_id, repo_full_name=excluded.repo_full_name, owner_login=excluded.owner_login, title=excluded.title, body=excluded.body, source_time=excluded.source_time, target_path=excluded.target_path, target_url=excluded.target_url, updated_at=excluded.updated_at;
  DELETE FROM search_documents_fts WHERE doc_id = 'release:' || NEW.release_id;
  INSERT INTO search_documents_fts SELECT id, COALESCE(title,''), COALESCE(body,''), COALESCE(repo_full_name,''), COALESCE(translated_text,''), COALESCE(smart_text,'') FROM search_documents WHERE id = 'release:' || NEW.release_id;
END;
CREATE TRIGGER search_repo_releases_au AFTER UPDATE ON repo_releases BEGIN
  UPDATE search_documents SET repo_id=NEW.repo_id, repo_full_name=COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)), owner_login=COALESCE((SELECT CASE WHEN instr(rwi.repo_full_name, '/') > 0 THEN substr(rwi.repo_full_name, 1, instr(rwi.repo_full_name, '/') - 1) ELSE rwi.repo_full_name END FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.owner_login FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.owner_login FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.owner_login FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)), title=COALESCE(NEW.name,NEW.tag_name), body=NEW.body, source_time=COALESCE(NEW.published_at,NEW.created_at,NEW.updated_at), target_path=CASE WHEN COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)) IS NOT NULL THEN '/' || COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id = NEW.repo_id LIMIT 1), (SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id = NEW.repo_id LIMIT 1), (SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id = NEW.repo_id LIMIT 1), (SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id = NEW.repo_id LIMIT 1)) || '/releases/tag/' || NEW.tag_name END, target_url=NEW.html_url, updated_at=NEW.updated_at WHERE id='release:' || NEW.release_id;
  DELETE FROM search_documents_fts WHERE doc_id = 'release:' || NEW.release_id;
  INSERT INTO search_documents_fts SELECT id, COALESCE(title,''), COALESCE(body,''), COALESCE(repo_full_name,''), COALESCE(translated_text,''), COALESCE(smart_text,'') FROM search_documents WHERE id = 'release:' || NEW.release_id;
END;
CREATE TRIGGER search_starred_repos_ad
AFTER DELETE ON starred_repos BEGIN
  DELETE FROM search_documents
  WHERE id = 'repository:' || OLD.user_id || ':' || lower(OLD.full_name)
    AND NOT EXISTS (
      SELECT 1 FROM user_repo_associations ura
      WHERE ura.user_id = OLD.user_id
        AND ura.repo_full_name_lower = lower(OLD.full_name)
    );
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_starred_repos_ai
AFTER INSERT ON starred_repos BEGIN
  DELETE FROM search_documents
  WHERE user_id = NEW.user_id
    AND resource_type = 'repository'
    AND repo_id = NEW.repo_id
    AND id <> 'repository:' || NEW.user_id || ':' || lower(NEW.full_name);
  INSERT INTO search_documents (
    id, user_id, resource_type, resource_id, repo_id, repo_full_name,
    owner_login, title, body, source_time, target_path, target_url,
    created_at, updated_at
  )
  SELECT 'repository:' || NEW.user_id || ':' || lower(NEW.full_name),
         NEW.user_id, 'repository', lower(NEW.full_name), NEW.repo_id,
         NEW.full_name, NEW.owner_login, NEW.name, NEW.description,
         NEW.updated_at, '/focus/repo/' || NEW.owner_login || '/' || NEW.name,
         NEW.html_url, NEW.updated_at, NEW.updated_at
  WHERE NOT EXISTS (
    SELECT 1 FROM user_repo_associations ura
    WHERE ura.user_id = NEW.user_id
      AND ura.repo_full_name_lower = lower(NEW.full_name)
  ) ON CONFLICT(user_id, resource_type, resource_id) DO UPDATE SET
    repo_id = excluded.repo_id,
    repo_full_name = excluded.repo_full_name,
    owner_login = excluded.owner_login,
    title = excluded.title,
    body = excluded.body,
    source_time = excluded.source_time,
    target_path = excluded.target_path,
    target_url = excluded.target_url,
    updated_at = excluded.updated_at;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_starred_repos_au
AFTER UPDATE ON starred_repos BEGIN
  DELETE FROM search_documents
  WHERE user_id = NEW.user_id
    AND resource_type = 'repository'
    AND repo_id = NEW.repo_id
    AND id <> 'repository:' || NEW.user_id || ':' || lower(NEW.full_name);
  DELETE FROM search_documents
  WHERE id = 'repository:' || OLD.user_id || ':' || lower(OLD.full_name)
    AND lower(OLD.full_name) <> lower(NEW.full_name)
    AND NOT EXISTS (
      SELECT 1 FROM user_repo_associations ura
      WHERE ura.user_id = OLD.user_id
        AND ura.repo_full_name_lower = lower(OLD.full_name)
    );
  INSERT INTO search_documents (
    id, user_id, resource_type, resource_id, repo_id, repo_full_name,
    owner_login, title, body, source_time, target_path, target_url,
    created_at, updated_at
  )
  SELECT 'repository:' || NEW.user_id || ':' || lower(NEW.full_name),
         NEW.user_id, 'repository', lower(NEW.full_name), NEW.repo_id,
         NEW.full_name, NEW.owner_login, NEW.name, NEW.description,
         NEW.updated_at, '/focus/repo/' || NEW.owner_login || '/' || NEW.name,
         NEW.html_url, NEW.updated_at, NEW.updated_at
  WHERE NOT EXISTS (
    SELECT 1 FROM user_repo_associations ura
    WHERE ura.user_id = NEW.user_id
      AND ura.repo_full_name_lower = lower(NEW.full_name)
  ) ON CONFLICT(user_id, resource_type, resource_id) DO UPDATE SET
    repo_id = excluded.repo_id,
    repo_full_name = excluded.repo_full_name,
    owner_login = excluded.owner_login,
    title = excluded.title,
    body = excluded.body,
    source_time = excluded.source_time,
    target_path = excluded.target_path,
    target_url = excluded.target_url,
    updated_at = excluded.updated_at;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT NEW.repo_id, CURRENT_TIMESTAMP
  WHERE NEW.repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT OLD.repo_id, CURRENT_TIMESTAMP
  WHERE OLD.repo_id IS NOT NULL AND OLD.repo_id IS NOT NEW.repo_id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER search_users_include_own_releases_au
AFTER UPDATE OF include_own_releases ON users BEGIN
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT repo_id, CURRENT_TIMESTAMP
  FROM user_repo_associations
  WHERE user_id = NEW.id AND repo_id IS NOT NULL
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT repo_id, CURRENT_TIMESTAMP FROM starred_repos WHERE user_id = NEW.id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
  INSERT INTO search_metadata_backfill_queue (repo_id, updated_at)
  SELECT repo_id, CURRENT_TIMESTAMP FROM owned_repo_star_baselines WHERE user_id = NEW.id
  ON CONFLICT(repo_id) DO UPDATE SET updated_at = excluded.updated_at, release_cursor = 0;
END;
CREATE TRIGGER trg_content_current_result_projections_active_scope_insert
BEFORE INSERT ON content_current_result_projections
WHEN NEW.active_work_item_id IS NOT NULL
 AND NOT EXISTS (
   SELECT 1
   FROM content_work_identities AS identity
   JOIN content_work_items AS work ON work.id = NEW.active_work_item_id
   WHERE identity.id = NEW.identity_id
     AND work.canonical_resource_type = identity.canonical_resource_type
     AND work.canonical_resource_id = identity.canonical_resource_id
     AND work.pipeline = identity.pipeline
     AND work.variant = identity.variant
     AND work.target_lang = identity.target_lang
     AND work.protocol_version = identity.protocol_version
 )
BEGIN
  SELECT RAISE(ABORT, 'active work item identity scope mismatch');
END;
CREATE TRIGGER trg_content_current_result_projections_active_scope_update
BEFORE UPDATE OF identity_id, active_work_item_id ON content_current_result_projections
WHEN NEW.active_work_item_id IS NOT NULL
 AND NOT EXISTS (
   SELECT 1
   FROM content_work_identities AS identity
   JOIN content_work_items AS work ON work.id = NEW.active_work_item_id
   WHERE identity.id = NEW.identity_id
     AND work.canonical_resource_type = identity.canonical_resource_type
     AND work.canonical_resource_id = identity.canonical_resource_id
     AND work.pipeline = identity.pipeline
     AND work.variant = identity.variant
     AND work.target_lang = identity.target_lang
     AND work.protocol_version = identity.protocol_version
 )
BEGIN
  SELECT RAISE(ABORT, 'active work item identity scope mismatch');
END;
CREATE TRIGGER trg_content_current_result_projections_source_scope_insert
BEFORE INSERT ON content_current_result_projections
WHEN NEW.source_projection_id IS NOT NULL
 AND NOT EXISTS (
   SELECT 1
   FROM content_work_identities AS identity
   JOIN content_result_projections AS source
     ON source.id = NEW.source_projection_id
   WHERE identity.id = NEW.identity_id
     AND source.canonical_resource_type = identity.canonical_resource_type
     AND source.canonical_resource_id = identity.canonical_resource_id
     AND source.pipeline = identity.pipeline
     AND source.variant = identity.variant
     AND source.target_lang = identity.target_lang
     AND source.source_hash = identity.source_hash
     AND source.protocol_version = identity.protocol_version
 )
BEGIN
  SELECT RAISE(ABORT, 'source projection identity mismatch');
END;
CREATE TRIGGER trg_content_current_result_projections_source_scope_update
BEFORE UPDATE OF identity_id, source_projection_id ON content_current_result_projections
WHEN NEW.source_projection_id IS NOT NULL
 AND NOT EXISTS (
   SELECT 1
   FROM content_work_identities AS identity
   JOIN content_result_projections AS source
     ON source.id = NEW.source_projection_id
   WHERE identity.id = NEW.identity_id
     AND source.canonical_resource_type = identity.canonical_resource_type
     AND source.canonical_resource_id = identity.canonical_resource_id
     AND source.pipeline = identity.pipeline
     AND source.variant = identity.variant
     AND source.target_lang = identity.target_lang
     AND source.source_hash = identity.source_hash
     AND source.protocol_version = identity.protocol_version
 )
BEGIN
  SELECT RAISE(ABORT, 'source projection identity mismatch');
END;
CREATE VIEW user_following_repos AS
SELECT
  user_id,
  repo_id,
  repo_full_name AS full_name,
  owner_login,
  repo_name AS name,
  html_url,
  description,
  is_private,
  last_seen_at AS updated_at,
  owner_avatar_url,
  open_graph_image_url,
  uses_custom_open_graph_image,
  first_source,
  first_associated_at,
  follow_state_source,
  has_personal_owned_source,
  has_github_star_source,
  has_manual_feed_source
FROM user_repo_associations
WHERE is_following != 0
  AND repo_id IS NOT NULL

UNION ALL

SELECT
  sr.user_id AS user_id,
  sr.repo_id AS repo_id,
  sr.full_name AS full_name,
  sr.owner_login AS owner_login,
  sr.name AS name,
  sr.html_url AS html_url,
  sr.description AS description,
  sr.is_private AS is_private,
  sr.updated_at AS updated_at,
  sr.owner_avatar_url AS owner_avatar_url,
  sr.open_graph_image_url AS open_graph_image_url,
  sr.uses_custom_open_graph_image AS uses_custom_open_graph_image,
  'github_star' AS first_source,
  sr.stargazed_at AS first_associated_at,
  'system_default' AS follow_state_source,
  0 AS has_personal_owned_source,
  1 AS has_github_star_source,
  0 AS has_manual_feed_source
FROM starred_repos sr
WHERE NOT EXISTS (
  SELECT 1
  FROM user_repo_associations ura
  WHERE ura.user_id = sr.user_id
    AND ura.repo_full_name_lower = lower(sr.full_name)
)

UNION ALL

SELECT
  ob.user_id AS user_id,
  ob.repo_id AS repo_id,
  ob.repo_full_name AS full_name,
  CASE
    WHEN instr(ob.repo_full_name, '/') > 0
      THEN substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)
    ELSE ob.repo_full_name
  END AS owner_login,
  CASE
    WHEN instr(ob.repo_full_name, '/') > 0
      THEN substr(ob.repo_full_name, instr(ob.repo_full_name, '/') + 1)
    ELSE ob.repo_full_name
  END AS name,
  'https://github.com/' || ob.repo_full_name AS html_url,
  NULL AS description,
  ob.is_private AS is_private,
  ob.updated_at AS updated_at,
  ob.owner_avatar_url AS owner_avatar_url,
  ob.open_graph_image_url AS open_graph_image_url,
  ob.uses_custom_open_graph_image AS uses_custom_open_graph_image,
  'personal_owned' AS first_source,
  COALESCE(ob.initialized_at, ob.updated_at) AS first_associated_at,
  'system_default' AS follow_state_source,
  1 AS has_personal_owned_source,
  0 AS has_github_star_source,
  0 AS has_manual_feed_source
FROM owned_repo_star_baselines ob
WHERE NOT EXISTS (
  SELECT 1
  FROM user_repo_associations ura
  WHERE ura.user_id = ob.user_id
    AND ura.repo_full_name_lower = lower(ob.repo_full_name)
);
CREATE VIEW user_release_visible_repos AS
SELECT
  ura.user_id AS user_id,
  ura.repo_id AS repo_id,
  ura.repo_full_name AS full_name,
  ura.owner_login AS owner_login,
  ura.repo_name AS name,
  ura.description AS description,
  COALESCE(ura.html_url, 'https://github.com/' || ura.repo_full_name) AS html_url,
  sr.stargazed_at AS stargazed_at,
  ura.is_private AS is_private,
  ura.last_seen_at AS updated_at,
  ura.owner_avatar_url AS owner_avatar_url,
  ura.open_graph_image_url AS open_graph_image_url,
  ura.uses_custom_open_graph_image AS uses_custom_open_graph_image
FROM user_repo_associations ura
JOIN users u
  ON u.id = ura.user_id
LEFT JOIN starred_repos sr
  ON sr.user_id = ura.user_id
 AND lower(sr.full_name) = ura.repo_full_name_lower
WHERE ura.is_following != 0
  AND ura.repo_id IS NOT NULL
  AND (
    ura.has_personal_owned_source = 0
    OR ura.has_github_star_source != 0
    OR u.include_own_releases != 0
  )

UNION ALL

SELECT
  sr.user_id AS user_id,
  sr.repo_id AS repo_id,
  sr.full_name AS full_name,
  sr.owner_login AS owner_login,
  sr.name AS name,
  sr.description AS description,
  sr.html_url AS html_url,
  sr.stargazed_at AS stargazed_at,
  sr.is_private AS is_private,
  sr.updated_at AS updated_at,
  sr.owner_avatar_url AS owner_avatar_url,
  sr.open_graph_image_url AS open_graph_image_url,
  sr.uses_custom_open_graph_image AS uses_custom_open_graph_image
FROM starred_repos sr
WHERE NOT EXISTS (
  SELECT 1
  FROM user_repo_associations ura
  WHERE ura.user_id = sr.user_id
    AND ura.repo_full_name_lower = lower(sr.full_name)
)

UNION ALL

SELECT
  ob.user_id AS user_id,
  ob.repo_id AS repo_id,
  ob.repo_full_name AS full_name,
  CASE
    WHEN instr(ob.repo_full_name, '/') > 0
      THEN substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)
    ELSE ob.repo_full_name
  END AS owner_login,
  CASE
    WHEN instr(ob.repo_full_name, '/') > 0
      THEN substr(ob.repo_full_name, instr(ob.repo_full_name, '/') + 1)
    ELSE ob.repo_full_name
  END AS name,
  NULL AS description,
  'https://github.com/' || ob.repo_full_name AS html_url,
  NULL AS stargazed_at,
  ob.is_private AS is_private,
  ob.updated_at AS updated_at,
  ob.owner_avatar_url AS owner_avatar_url,
  ob.open_graph_image_url AS open_graph_image_url,
  ob.uses_custom_open_graph_image AS uses_custom_open_graph_image
FROM owned_repo_star_baselines ob
JOIN users u
  ON u.id = ob.user_id
WHERE u.include_own_releases != 0
  AND NOT EXISTS (
    SELECT 1
    FROM user_repo_associations ura
    WHERE ura.user_id = ob.user_id
      AND ura.repo_full_name_lower = lower(ob.repo_full_name)
  )
  AND NOT EXISTS (
    SELECT 1
    FROM starred_repos sr
    WHERE sr.user_id = ob.user_id
      AND sr.repo_id = ob.repo_id
  );
