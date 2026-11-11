-- PostgreSQL 18 schema baseline derived from the final SQLite schema.
-- This file is owned by the PostgreSQL SQLx migrator; SQLite history is never copied.
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE SCHEMA IF NOT EXISTS tower_sessions;

CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY,
  github_user_id BIGINT NOT NULL UNIQUE,
  login TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  email TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  is_admin BIGINT NOT NULL DEFAULT 0,
  is_disabled BIGINT NOT NULL DEFAULT 0,
  daily_brief_utc_time TEXT NOT NULL DEFAULT '00:00',
  last_active_at TEXT
, daily_brief_local_time TEXT, daily_brief_time_zone TEXT, include_own_releases BIGINT NOT NULL DEFAULT 0, passkey_user_handle_uuid TEXT, paused_at TEXT, webhook_push_enabled BIGINT NOT NULL DEFAULT 0
  CHECK (webhook_push_enabled IN (0, 1)), webhook_push_secret_ciphertext BYTEA, webhook_push_secret_nonce BYTEA, webhook_push_callback_key TEXT, webhook_push_desired_state TEXT NOT NULL DEFAULT 'deleted'
  CHECK (webhook_push_desired_state IN ('enabled', 'paused', 'deleted')), webhook_push_last_completed_check_at TEXT);

CREATE TABLE IF NOT EXISTS user_tokens (
  user_id TEXT PRIMARY KEY,
  access_token_ciphertext BYTEA NOT NULL,
  access_token_nonce BYTEA NOT NULL,
  scopes TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS starred_repos (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id BIGINT NOT NULL,
  full_name TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  html_url TEXT NOT NULL,
  stargazed_at TEXT,
  is_private BIGINT NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL, owner_avatar_url TEXT, open_graph_image_url TEXT, uses_custom_open_graph_image BIGINT NOT NULL DEFAULT 0, repo_stargazer_count BIGINT, repo_stargazer_count_updated_at TEXT,
  UNIQUE(user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS releases (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id BIGINT NOT NULL,
  release_id BIGINT NOT NULL,
  tag_name TEXT NOT NULL,
  name TEXT,
  body TEXT,
  html_url TEXT NOT NULL,
  published_at TEXT,
  created_at TEXT,
  is_prerelease BIGINT NOT NULL DEFAULT 0,
  is_draft BIGINT NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  node_id TEXT,
  react_plus1 BIGINT NOT NULL DEFAULT 0,
  react_laugh BIGINT NOT NULL DEFAULT 0,
  react_heart BIGINT NOT NULL DEFAULT 0,
  react_hooray BIGINT NOT NULL DEFAULT 0,
  react_rocket BIGINT NOT NULL DEFAULT 0,
  react_eyes BIGINT NOT NULL DEFAULT 0,
  UNIQUE(user_id, release_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS notifications (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  thread_id TEXT NOT NULL,
  repo_full_name TEXT,
  subject_title TEXT,
  subject_type TEXT,
  reason TEXT,
  updated_at TEXT,
  unread BIGINT NOT NULL DEFAULT 1,
  url TEXT,
  html_url TEXT,
  last_seen_at TEXT,
  UNIQUE(user_id, thread_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS sync_state (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, key),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS ai_translations (
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

CREATE TABLE IF NOT EXISTS reaction_pat_tokens (
  user_id TEXT PRIMARY KEY,
  token_ciphertext BYTEA NOT NULL,
  token_nonce BYTEA NOT NULL,
  masked_token TEXT NOT NULL,
  last_check_state TEXT NOT NULL DEFAULT 'unknown',
  last_check_message TEXT,
  last_checked_at TEXT,
  updated_at TEXT NOT NULL, owner_github_connection_id TEXT, owner_github_user_id BIGINT, owner_login TEXT, webhook_push_allows_private_repos BIGINT
  CHECK (webhook_push_allows_private_repos IS NULL OR webhook_push_allows_private_repos IN (0, 1)),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS job_tasks (
  id TEXT PRIMARY KEY,
  task_type TEXT NOT NULL,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  requested_by TEXT,
  parent_task_id TEXT,
  payload_json TEXT NOT NULL,
  result_json TEXT,
  error_message TEXT,
  cancel_requested BIGINT NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  log_file_path TEXT, runtime_owner_id TEXT, lease_heartbeat_at TEXT, available_at TEXT,
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS job_task_events (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS daily_brief_hour_slots (
  hour_utc BIGINT PRIMARY KEY,
  enabled BIGINT NOT NULL DEFAULT 1,
  last_dispatch_at TEXT,
  updated_at TEXT NOT NULL,
  CHECK (hour_utc >= 0 AND hour_utc <= 23)
);

CREATE TABLE IF NOT EXISTS llm_calls (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  model TEXT NOT NULL,
  requested_by TEXT,
  parent_task_id TEXT,
  parent_task_type TEXT,
  max_tokens BIGINT NOT NULL,
  attempt_count BIGINT NOT NULL DEFAULT 0,
  scheduler_wait_ms BIGINT NOT NULL DEFAULT 0,
  duration_ms BIGINT,
  prompt_text TEXT NOT NULL,
  response_text TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  input_messages_json TEXT,
  output_messages_json TEXT,
  input_tokens BIGINT,
  output_tokens BIGINT,
  cached_input_tokens BIGINT,
  total_tokens BIGINT,
  first_token_wait_ms BIGINT,
  parent_translation_batch_id TEXT, runtime_owner_id TEXT, lease_heartbeat_at TEXT, failure_class TEXT
  CHECK (
    failure_class IS NULL
    OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
  ), final_model TEXT, fallback_count BIGINT NOT NULL DEFAULT 0
  CHECK (fallback_count >= 0), retry_scheduled_at TEXT, recovery_attempt_count BIGINT NOT NULL DEFAULT 0
  CHECK (recovery_attempt_count >= 0), parent_brief_id TEXT, finish_reason TEXT, provider_request_id TEXT, provider_http_status BIGINT,
  CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  FOREIGN KEY(requested_by) REFERENCES users(id) ON DELETE SET NULL,
  FOREIGN KEY(parent_task_id) REFERENCES job_tasks(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS llm_call_events (
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
  ), model TEXT, attempt BIGINT
  CHECK (attempt IS NULL OR attempt >= 0), retry_after_ms BIGINT
  CHECK (retry_after_ms IS NULL OR retry_after_ms >= 0), from_model TEXT, to_model TEXT, fallback_count BIGINT
  CHECK (fallback_count IS NULL OR fallback_count >= 0),
  CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  FOREIGN KEY(call_id) REFERENCES llm_calls(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS scheduled_task_dispatch_state (
  schedule_name TEXT PRIMARY KEY,
  last_dispatch_key TEXT,
  last_task_id TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_subscription_events (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  event_type TEXT NOT NULL,
  severity TEXT NOT NULL,
  recoverable BIGINT NOT NULL DEFAULT 0,
  attempt BIGINT NOT NULL DEFAULT 0,
  user_id TEXT,
  repo_id BIGINT,
  repo_full_name TEXT,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(task_id) REFERENCES job_tasks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS translation_work_items (
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
  token_estimate BIGINT NOT NULL,
  deadline_at TEXT NOT NULL,
  status TEXT NOT NULL,
  batch_id TEXT,
  result_status TEXT,
  title_zh TEXT,
  summary_md TEXT,
  body_md TEXT,
  error_text TEXT,
  cache_hit BIGINT NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL, failure_class TEXT
  CHECK (
    failure_class IS NULL
    OR failure_class IN ('empty_content', 'transient', 'rate_limited', 'configuration')
  ), retry_count BIGINT NOT NULL DEFAULT 0
  CHECK (retry_count >= 0), next_retry_at TEXT, retry_expires_at TEXT, attempt_count BIGINT NOT NULL DEFAULT 0
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

CREATE TABLE IF NOT EXISTS translation_requests (
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
  max_wait_ms BIGINT NOT NULL,
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

CREATE TABLE IF NOT EXISTS translation_batches (
  id TEXT PRIMARY KEY,
  partition_key TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  trigger_reason TEXT NOT NULL,
  worker_slot BIGINT NOT NULL,
  request_count BIGINT NOT NULL,
  item_count BIGINT NOT NULL,
  estimated_input_tokens BIGINT NOT NULL,
  status TEXT NOT NULL,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL, runtime_owner_id TEXT, lease_heartbeat_at TEXT, worker_id TEXT NOT NULL DEFAULT '', worker_kind TEXT NOT NULL DEFAULT 'general'
  CHECK (worker_kind IN ('general', 'user_dedicated')),
  CHECK (status IN ('queued', 'running', 'completed', 'failed'))
);

CREATE TABLE IF NOT EXISTS translation_batch_items (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  item_index BIGINT NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  producer_count BIGINT NOT NULL DEFAULT 0,
  token_estimate BIGINT NOT NULL,
  result_status TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(batch_id, work_item_id),
  CHECK (result_status IS NULL OR result_status IN ('ready', 'disabled', 'missing', 'error')),
  FOREIGN KEY(batch_id) REFERENCES translation_batches(id) ON DELETE CASCADE,
  FOREIGN KEY(work_item_id) REFERENCES translation_work_items(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS runtime_owners (
  runtime_owner_id TEXT PRIMARY KEY,
  lease_heartbeat_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS repo_releases (
  id TEXT PRIMARY KEY,
  repo_id BIGINT NOT NULL,
  release_id BIGINT NOT NULL UNIQUE,
  node_id TEXT,
  tag_name TEXT NOT NULL,
  name TEXT,
  body TEXT,
  html_url TEXT NOT NULL,
  published_at TEXT,
  created_at TEXT,
  is_prerelease BIGINT NOT NULL DEFAULT 0,
  is_draft BIGINT NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  react_plus1 BIGINT NOT NULL DEFAULT 0,
  react_laugh BIGINT NOT NULL DEFAULT 0,
  react_heart BIGINT NOT NULL DEFAULT 0,
  react_hooray BIGINT NOT NULL DEFAULT 0,
  react_rocket BIGINT NOT NULL DEFAULT 0,
  react_eyes BIGINT NOT NULL DEFAULT 0
, detected_at TEXT);

CREATE TABLE IF NOT EXISTS repo_release_work_items (
  id TEXT PRIMARY KEY,
  repo_id BIGINT NOT NULL UNIQUE,
  repo_full_name TEXT NOT NULL,
  status TEXT NOT NULL,
  request_origin TEXT NOT NULL,
  priority BIGINT NOT NULL DEFAULT 0,
  has_new_repo_watchers BIGINT NOT NULL DEFAULT 0,
  deadline_at TEXT NOT NULL,
  last_release_count BIGINT NOT NULL DEFAULT 0,
  last_candidate_failures BIGINT NOT NULL DEFAULT 0,
  last_success_at TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL,
  runtime_owner_id TEXT,
  lease_heartbeat_at TEXT
, last_fetched_count BIGINT NOT NULL DEFAULT 0, last_inserted_count BIGINT NOT NULL DEFAULT 0, last_updated_count BIGINT NOT NULL DEFAULT 0, last_unchanged_count BIGINT NOT NULL DEFAULT 0, last_pages_fetched BIGINT NOT NULL DEFAULT 0, last_stopped_reason TEXT, last_new_release_ids_json TEXT);

CREATE TABLE IF NOT EXISTS repo_release_watchers (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  user_id TEXT,
  origin TEXT NOT NULL,
  priority BIGINT NOT NULL DEFAULT 0,
  reason TEXT NOT NULL,
  is_new_repo BIGINT NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'pending',
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, reused_fresh BIGINT NOT NULL DEFAULT 0, freshness_window_minutes BIGINT
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

CREATE TABLE IF NOT EXISTS admin_runtime_settings (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  llm_max_concurrency BIGINT NOT NULL,
  translation_general_worker_concurrency BIGINT NOT NULL,
  translation_dedicated_worker_concurrency BIGINT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, ai_model_context_limit BIGINT
  CHECK (ai_model_context_limit IS NULL OR ai_model_context_limit > 0), ai_model_context_limit_migrated_at TEXT, sync_auto_fetch_interval_minutes BIGINT NOT NULL DEFAULT 60
  CHECK (sync_auto_fetch_interval_minutes >= 1 AND sync_auto_fetch_interval_minutes <= 120), repo_release_worker_concurrency BIGINT NOT NULL DEFAULT 5
CHECK (repo_release_worker_concurrency BETWEEN 1 AND 32), retry_recent_failures_interval_minutes BIGINT NOT NULL DEFAULT 10
  CHECK (retry_recent_failures_interval_minutes >= 1 AND retry_recent_failures_interval_minutes <= 120), llm_models_json TEXT NOT NULL DEFAULT '[]', repo_refresh_system_budget_per_window BIGINT NOT NULL DEFAULT 1000
  CHECK (repo_refresh_system_budget_per_window BETWEEN 1 AND 20000), daily_brief_schedule_local_time TEXT NOT NULL DEFAULT '06:00', dashboard_release_freshness_profile TEXT NOT NULL DEFAULT 'balanced'
  CHECK (dashboard_release_freshness_profile IN ('latest', 'balanced', 'capacity')), webhook_push_audit_interval_days BIGINT NOT NULL DEFAULT 7
  CHECK (webhook_push_audit_interval_days BETWEEN 1 AND 30), webhook_push_audit_last_started_at TEXT, sync_auto_fetch_effective_at TEXT, star_sync_delta_interval_minutes BIGINT NOT NULL DEFAULT 30
  CHECK (star_sync_delta_interval_minutes BETWEEN 1 AND 120), star_sync_full_sweep_interval_minutes BIGINT NOT NULL DEFAULT 1440
  CHECK (star_sync_full_sweep_interval_minutes BETWEEN 60 AND 10080),
  CHECK (llm_max_concurrency > 0),
  CHECK (translation_general_worker_concurrency > 0),
  CHECK (translation_dedicated_worker_concurrency > 0)
);

CREATE TABLE IF NOT EXISTS owned_repo_star_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id BIGINT NOT NULL,
  repo_full_name TEXT NOT NULL,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, members_snapshot_initialized BIGINT NOT NULL DEFAULT 1, owner_avatar_url TEXT, open_graph_image_url TEXT, uses_custom_open_graph_image BIGINT NOT NULL DEFAULT 0, repo_stargazer_count BIGINT, repo_stargazer_count_updated_at TEXT, is_private BIGINT,
  UNIQUE(user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS repo_star_current_members (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id BIGINT NOT NULL,
  repo_full_name TEXT NOT NULL,
  actor_github_user_id BIGINT NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  starred_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, repo_id, actor_github_user_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS follower_sync_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS follower_current_members (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  actor_github_user_id BIGINT NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, actor_github_user_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS repo_star_sync_baselines (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL UNIQUE,
  initialized_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS briefs (
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

CREATE TABLE IF NOT EXISTS brief_release_memberships (
  brief_id TEXT NOT NULL,
  release_id BIGINT NOT NULL,
  release_ts_utc TEXT NOT NULL,
  ordinal BIGINT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (brief_id, release_id),
  FOREIGN KEY(brief_id) REFERENCES briefs(id) ON DELETE CASCADE,
  FOREIGN KEY(release_id) REFERENCES repo_releases(release_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS linuxdo_connections (
  user_id TEXT PRIMARY KEY,
  linuxdo_user_id BIGINT NOT NULL UNIQUE,
  username TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  trust_level BIGINT NOT NULL DEFAULT 0,
  active BIGINT NOT NULL DEFAULT 1,
  silenced BIGINT NOT NULL DEFAULT 0,
  linked_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS admin_dashboard_daily_rollups (
  rollup_date TEXT NOT NULL,
  time_zone TEXT NOT NULL,
  task_type TEXT NOT NULL,
  total_users BIGINT NOT NULL,
  active_users BIGINT NOT NULL,
  queued_count BIGINT NOT NULL DEFAULT 0,
  running_count BIGINT NOT NULL DEFAULT 0,
  succeeded_count BIGINT NOT NULL DEFAULT 0,
  failed_count BIGINT NOT NULL DEFAULT 0,
  canceled_count BIGINT NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL, business_ok_count BIGINT NOT NULL DEFAULT 0, business_partial_count BIGINT NOT NULL DEFAULT 0, business_failed_count BIGINT NOT NULL DEFAULT 0, business_disabled_count BIGINT NOT NULL DEFAULT 0,
  PRIMARY KEY (rollup_date, time_zone, task_type)
);

CREATE TABLE IF NOT EXISTS github_connections (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_user_id BIGINT NOT NULL UNIQUE,
  login TEXT NOT NULL,
  name TEXT,
  avatar_url TEXT,
  email TEXT,
  access_token_ciphertext BYTEA NOT NULL,
  access_token_nonce BYTEA NOT NULL,
  scopes TEXT NOT NULL,
  linked_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_passkeys (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  credential_id TEXT NOT NULL UNIQUE,
  label TEXT NOT NULL,
  passkey_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS social_activity_events (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  repo_id BIGINT,
  repo_full_name TEXT,
  repo_owner_avatar_url TEXT,
  repo_open_graph_image_url TEXT,
  repo_uses_custom_open_graph_image BIGINT,
  title TEXT,
  body TEXT,
  html_url TEXT,
  github_event_id TEXT,
  actor_github_user_id BIGINT NOT NULL,
  actor_login TEXT NOT NULL,
  actor_avatar_url TEXT,
  actor_html_url TEXT,
  occurred_at TEXT NOT NULL,
  detected_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, discussion_number BIGINT,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS repo_release_sync_state (
  repo_id BIGINT PRIMARY KEY,
  etag TEXT NULL,
  last_modified TEXT NULL,
  last_success_at TEXT NULL,
  last_attempt_at TEXT NULL,
  last_not_modified_at TEXT NULL,
  last_error_text TEXT NULL,
  backoff_until TEXT NULL,
  updated_at TEXT NOT NULL
, last_page_count BIGINT NOT NULL DEFAULT 0, last_fetched_count BIGINT NOT NULL DEFAULT 0, last_inserted_count BIGINT NOT NULL DEFAULT 0, last_updated_count BIGINT NOT NULL DEFAULT 0, last_unchanged_count BIGINT NOT NULL DEFAULT 0, last_stopped_reason TEXT);

CREATE TABLE IF NOT EXISTS public_repo_release_usage (
  id TEXT PRIMARY KEY,
  repo_id BIGINT,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  full_name TEXT NOT NULL,
  full_name_lower TEXT NOT NULL UNIQUE,
  first_registered_at TEXT NOT NULL,
  last_requested_at TEXT NOT NULL,
  last_list_requested_at TEXT,
  last_detail_requested_at TEXT,
  api_list_requests BIGINT NOT NULL DEFAULT 0,
  api_detail_requests BIGINT NOT NULL DEFAULT 0,
  page_list_requests BIGINT NOT NULL DEFAULT 0,
  page_detail_requests BIGINT NOT NULL DEFAULT 0,
  last_sync_status TEXT NOT NULL DEFAULT 'pending',
  last_sync_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL, access_kind TEXT NOT NULL DEFAULT 'github_public'
  CHECK (access_kind IN ('github_public', 'private_owner_published')), published_by_user_id TEXT, published_at TEXT,
  CHECK (last_sync_status IN ('pending', 'ready', 'failed', 'inaccessible'))
);

CREATE TABLE IF NOT EXISTS repo_refresh_governance_snapshots (
  repo_id BIGINT PRIMARY KEY,
  repo_full_name TEXT NOT NULL,
  is_private BIGINT NOT NULL DEFAULT 0,
  watcher_user_count BIGINT NOT NULL,
  watcher_repo_total_sum BIGINT NOT NULL,
  cached_stargazer_count BIGINT,
  cached_stargazer_count_updated_at TEXT,
  priority_rank BIGINT NOT NULL,
  target_window BIGINT NOT NULL,
  target_interval_minutes BIGINT NOT NULL,
  urgency_score DOUBLE PRECISION NOT NULL DEFAULT 1,
  urgency_bucket TEXT NOT NULL DEFAULT 'active',
  system_last_selected_at TEXT,
  system_last_success_at TEXT,
  actual_last_success_at TEXT,
  actual_last_success_source TEXT,
  active_cycle_id TEXT,
  active_cycle_window_index BIGINT,
  active_cycle_completed BIGINT NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
, system_last_attempt_at TEXT, system_last_attempt_status TEXT, system_last_attempt_error TEXT);

CREATE TABLE IF NOT EXISTS repo_refresh_governance_cycles (
  id TEXT PRIMARY KEY,
  status TEXT NOT NULL CHECK (status IN ('active', 'completed')),
  window_budget BIGINT NOT NULL,
  frozen_repo_count BIGINT NOT NULL,
  completed_repo_count BIGINT NOT NULL DEFAULT 0,
  window_index_started_at BIGINT NOT NULL,
  window_index_completed_at BIGINT,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  updated_at TEXT NOT NULL
, window_minutes BIGINT NOT NULL DEFAULT 10
  CHECK (window_minutes BETWEEN 1 AND 120), last_selection_window_index BIGINT);

CREATE TABLE IF NOT EXISTS repo_refresh_governance_cycle_members (
  cycle_id TEXT NOT NULL,
  repo_id BIGINT NOT NULL,
  repo_full_name TEXT NOT NULL,
  completed_at TEXT,
  removed_from_pool BIGINT NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL, attempt_status TEXT, attempt_error TEXT,
  PRIMARY KEY (cycle_id, repo_id),
  FOREIGN KEY(cycle_id) REFERENCES repo_refresh_governance_cycles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_api_keys (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  name TEXT NOT NULL,
  key_hash TEXT NOT NULL UNIQUE,
  key_ciphertext BYTEA NOT NULL,
  key_nonce BYTEA NOT NULL,
  key_prefix TEXT NOT NULL,
  masked_key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked_at TEXT,
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user_repo_associations (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id BIGINT,
  repo_full_name TEXT NOT NULL,
  repo_full_name_lower TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  html_url TEXT,
  description TEXT,
  is_private BIGINT,
  owner_avatar_url TEXT,
  open_graph_image_url TEXT,
  uses_custom_open_graph_image BIGINT NOT NULL DEFAULT 0,
  first_source TEXT NOT NULL
    CHECK (first_source IN ('personal_owned', 'github_star', 'manual_feed')),
  first_associated_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  is_following BIGINT NOT NULL DEFAULT 0,
  follow_state_source TEXT NOT NULL DEFAULT 'system_default'
    CHECK (follow_state_source IN ('system_default', 'user_explicit')),
  has_personal_owned_source BIGINT NOT NULL DEFAULT 0,
  has_github_star_source BIGINT NOT NULL DEFAULT 0,
  has_manual_feed_source BIGINT NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, repo_full_name_lower),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS repo_refresh_governance_retention_state (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  window_started_at TEXT NOT NULL,
  deleted_rows BIGINT NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS webhook_push_repos (
  user_id TEXT NOT NULL,
  repo_id BIGINT NOT NULL,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  repo_full_name TEXT NOT NULL,
  hook_id BIGINT,
  callback_url TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'unknown'
    CHECK (status IN ('unknown', 'missing', 'registered', 'conflict', 'permission_paused', 'error', 'delete_pending')),
  error_kind TEXT,
  error_message TEXT,
  permission_paused BIGINT NOT NULL DEFAULT 0 CHECK (permission_paused IN (0, 1)),
  last_checked_at TEXT,
  last_registered_at TEXT,
  updated_at TEXT NOT NULL, owner_github_user_id BIGINT,
  PRIMARY KEY (user_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS webhook_push_deliveries (
  delivery_id TEXT PRIMARY KEY,
  hook_id BIGINT,
  repo_id BIGINT,
  event TEXT NOT NULL,
  action TEXT,
  received_at TEXT NOT NULL,
  queued_task_id TEXT
, processing_state TEXT NOT NULL DEFAULT 'pending'
  CHECK (processing_state IN ('pending', 'processing', 'ignored', 'queued')), processing_started_at TEXT);

CREATE TABLE IF NOT EXISTS webhook_push_user_operation_leases (
  user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  task_id TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS llm_recovery_flags (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  llm_recovery_enabled BIGINT NOT NULL DEFAULT 0
    CHECK (llm_recovery_enabled IN (0, 1)),
  llm_recovery_rollout_percent BIGINT NOT NULL DEFAULT 0
    CHECK (llm_recovery_rollout_percent BETWEEN 0 AND 100),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS llm_model_health (
  model TEXT PRIMARY KEY,
  relevant_failure_count BIGINT NOT NULL DEFAULT 0
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

CREATE TABLE IF NOT EXISTS translation_attempt_events (
  id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY ,
  work_item_id TEXT NOT NULL,
  request_id TEXT,
  batch_id TEXT,
  scope_user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  variant TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  attempt_no BIGINT NOT NULL CHECK (attempt_no >= 1),
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
  retry_eligible BIGINT NOT NULL DEFAULT 0 CHECK (retry_eligible IN (0, 1)),
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

CREATE TABLE IF NOT EXISTS admin_collection_processing_coverage (
  record_kind TEXT NOT NULL,
  record_id TEXT NOT NULL,
  pipeline TEXT NOT NULL,
  status_origin TEXT NOT NULL CHECK (status_origin IN ('never_started', 'historical_unknown')),
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (record_kind, record_id, pipeline)
);

CREATE TABLE IF NOT EXISTS translation_attempt_llm_calls (
  id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY ,
  work_item_id TEXT NOT NULL,
  attempt_no BIGINT NOT NULL CHECK (attempt_no >= 1),
  llm_call_id TEXT NOT NULL,
  stage TEXT NOT NULL,
  ordinal BIGINT NOT NULL CHECK (ordinal >= 0),
  relation_role TEXT NOT NULL
    CHECK (relation_role IN ('primary', 'length_recovery', 'schema_repair', 'fallback', 'legacy_unverified')),
  evidence_availability TEXT NOT NULL DEFAULT 'available'
    CHECK (evidence_availability IN ('available', 'expired', 'not_captured')),
  created_at TEXT NOT NULL,
  UNIQUE(work_item_id, attempt_no, llm_call_id, stage, ordinal)
);

CREATE TABLE IF NOT EXISTS llm_diagnostic_access_audit (
  id TEXT PRIMARY KEY,
  call_id TEXT NOT NULL,
  actor_user_id TEXT,
  action TEXT NOT NULL
    CHECK (action IN ('reveal', 'copy')),
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS starred_repo_connection_memberships (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_connection_id TEXT NOT NULL,
  repo_id BIGINT NOT NULL,
  full_name TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  html_url TEXT NOT NULL,
  stargazed_at TEXT,
  is_private BIGINT NOT NULL DEFAULT 0,
  owner_avatar_url TEXT,
  open_graph_image_url TEXT,
  uses_custom_open_graph_image BIGINT NOT NULL DEFAULT 0,
  repo_stargazer_count BIGINT,
  last_seen_epoch_id TEXT,
  last_delta_seen_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, github_connection_id, repo_id),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY(github_connection_id) REFERENCES github_connections(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS star_sync_epochs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  github_connection_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  started_at TEXT NOT NULL,
  next_cursor TEXT,
  total_count_at_start BIGINT,
  processed_pages BIGINT NOT NULL DEFAULT 0,
  processed_items BIGINT NOT NULL DEFAULT 0,
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

CREATE TABLE IF NOT EXISTS content_processing_control (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  mode TEXT NOT NULL CHECK (mode IN ('legacy', 'rollback_freeze', 'global')),
  switch_token TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS content_work_items (
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
  priority BIGINT NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),
  cache_hit BIGINT NOT NULL DEFAULT 0 CHECK (cache_hit IN (0, 1)),
  token_estimate BIGINT NOT NULL DEFAULT 0 CHECK (token_estimate >= 0),
  batch_id TEXT,
  lease_owner TEXT,
  lease_expires_at TEXT,
  attempt_count BIGINT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
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
  UNIQUE (
    id, canonical_resource_type, canonical_resource_id, pipeline, variant,
    target_lang, source_hash, protocol_version
  ),
  FOREIGN KEY (supersedes_work_item_id) REFERENCES content_work_items(id)
);

CREATE TABLE IF NOT EXISTS content_batches (
  id TEXT PRIMARY KEY,
  partition_key TEXT NOT NULL,
  target_lang TEXT NOT NULL,
  protocol_version TEXT NOT NULL,
  model_profile TEXT NOT NULL,
  trigger_reason TEXT NOT NULL,
  worker_id TEXT,
  worker_kind TEXT NOT NULL DEFAULT 'general'
    CHECK (worker_kind = 'general'),
  request_count BIGINT NOT NULL DEFAULT 0 CHECK (request_count >= 0),
  item_count BIGINT NOT NULL DEFAULT 0 CHECK (item_count >= 0),
  estimated_input_tokens BIGINT NOT NULL DEFAULT 0 CHECK (estimated_input_tokens >= 0),
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed')),
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  started_at TEXT,
  finished_at TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS content_batch_items (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  item_index BIGINT NOT NULL CHECK (item_index >= 0),
  request_count BIGINT NOT NULL DEFAULT 0 CHECK (request_count >= 0),
  token_estimate BIGINT NOT NULL DEFAULT 0 CHECK (token_estimate >= 0),
  result_status TEXT,
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (batch_id, work_item_id),
  FOREIGN KEY (batch_id) REFERENCES content_batches(id) ON DELETE CASCADE,
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS content_result_projections (
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

CREATE TABLE IF NOT EXISTS content_request_links (
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

CREATE TABLE IF NOT EXISTS content_attempt_events (
  id TEXT PRIMARY KEY,
  work_item_id TEXT NOT NULL,
  attempt_no BIGINT NOT NULL CHECK (attempt_no >= 1),
  trigger TEXT NOT NULL
    CHECK (trigger IN ('initial', 'manual_retry', 'automatic_recovery', 'system_requeue')),
  event_type TEXT NOT NULL
    CHECK (event_type IN ('attempt_queued', 'attempt_started', 'attempt_completed', 'retry_scheduled')),
  result_status TEXT,
  error_code TEXT,
  error_summary TEXT,
  failure_class TEXT,
  retry_disposition TEXT,
  retry_eligible BIGINT NOT NULL DEFAULT 0 CHECK (retry_eligible IN (0, 1)),
  next_retry_at TEXT,
  duration_ms BIGINT CHECK (duration_ms IS NULL OR duration_ms >= 0),
  token_count BIGINT CHECK (token_count IS NULL OR token_count >= 0),
  cost_microunits BIGINT CHECK (cost_microunits IS NULL OR cost_microunits >= 0),
  created_at TEXT NOT NULL, configuration_snapshot_json TEXT, route_snapshot_json TEXT, configuration_fingerprint TEXT,
  UNIQUE (work_item_id, attempt_no, event_type),
  UNIQUE (id, work_item_id, attempt_no),
  FOREIGN KEY (work_item_id) REFERENCES content_work_items(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS content_attempt_llm_calls (
  id TEXT PRIMARY KEY,
  attempt_event_id TEXT NOT NULL,
  provider_call_id TEXT NOT NULL,
  model TEXT NOT NULL,
  status TEXT NOT NULL,
  duration_ms BIGINT CHECK (duration_ms IS NULL OR duration_ms >= 0),
  input_tokens BIGINT CHECK (input_tokens IS NULL OR input_tokens >= 0),
  output_tokens BIGINT CHECK (output_tokens IS NULL OR output_tokens >= 0),
  cost_microunits BIGINT CHECK (cost_microunits IS NULL OR cost_microunits >= 0),
  error_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  UNIQUE (attempt_event_id, provider_call_id),
  FOREIGN KEY (attempt_event_id) REFERENCES content_attempt_events(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS content_legacy_observations (
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

CREATE TABLE IF NOT EXISTS search_documents (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  resource_type TEXT NOT NULL CHECK (resource_type IN ('release', 'announcement', 'brief', 'notification', 'repository')),
  resource_id TEXT NOT NULL,
  repo_id BIGINT,
  repo_full_name TEXT,
  owner_login TEXT,
  title TEXT,
  body TEXT,
  translated_text TEXT,
  smart_text TEXT,
  source_time TEXT,
  unread BIGINT,
  target_path TEXT,
  target_url TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (user_id, resource_type, resource_id)
,
  search_vector TSVECTOR GENERATED ALWAYS AS (
    setweight(to_tsvector('simple', coalesce(title, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(body, '')), 'B') ||
    setweight(to_tsvector('simple', coalesce(repo_full_name, '')), 'A') ||
    setweight(to_tsvector('simple', coalesce(translated_text, '')), 'C') ||
    setweight(to_tsvector('simple', coalesce(smart_text, '')), 'C')
  ) STORED
);

CREATE TABLE IF NOT EXISTS search_document_user_lanes (
  document_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  translated_text TEXT,
  smart_text TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (document_id, user_id),
  FOREIGN KEY (document_id) REFERENCES search_documents(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS search_rate_limits (
  user_id TEXT PRIMARY KEY,
  window_started_at BIGINT NOT NULL,
  request_count BIGINT NOT NULL CHECK (request_count >= 0),
  updated_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS search_projection_backfill_state (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  phase TEXT NOT NULL,
  cursor BIGINT NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK (status IN ('pending', 'building', 'ready', 'paused_low_disk', 'failed')),
  last_error TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS webhook_push_reconcile_demands (
  user_id TEXT PRIMARY KEY,
  requested_generation BIGINT NOT NULL DEFAULT 0 CHECK (requested_generation >= 0),
  completed_generation BIGINT NOT NULL DEFAULT 0 CHECK (completed_generation >= 0),
  requested_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  CHECK (completed_generation <= requested_generation)
);

CREATE TABLE IF NOT EXISTS content_work_identities (
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

CREATE TABLE IF NOT EXISTS content_work_identity_members (
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

CREATE TABLE IF NOT EXISTS content_current_result_projections (
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

CREATE TABLE IF NOT EXISTS content_identity_upgrade_control (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  generation BIGINT NOT NULL CHECK (generation >= 1),
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

CREATE TABLE IF NOT EXISTS content_work_admission_events (
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

CREATE TABLE IF NOT EXISTS content_attempt_provider_admissions (
  id TEXT PRIMARY KEY,
  attempt_event_id TEXT NOT NULL,
  work_item_id TEXT NOT NULL,
  attempt_no BIGINT NOT NULL CHECK (attempt_no >= 1),
  call_ordinal BIGINT NOT NULL CHECK (call_ordinal >= 0),
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

CREATE TABLE IF NOT EXISTS public_metrics_hourly_snapshots (
  sampled_hour TEXT PRIMARY KEY,
  observed_at TEXT NOT NULL,
  deduplicated_repositories BIGINT NOT NULL CHECK (deduplicated_repositories >= 0),
  pressure DOUBLE PRECISION NOT NULL CHECK (pressure >= 0)
);

CREATE TABLE IF NOT EXISTS search_fts_document_rows (
  fts_rowid BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY ,
  document_id TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS search_fts_user_lane_rows (
  fts_rowid BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY ,
  document_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  UNIQUE (document_id, user_id)
);

CREATE TABLE IF NOT EXISTS search_metadata_backfill_queue (
  repo_id BIGINT PRIMARY KEY,
  updated_at TEXT NOT NULL,
  release_cursor BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS search_projection_outbox (
  projection_key TEXT PRIMARY KEY,
  operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
  resource_type TEXT NOT NULL CHECK (resource_type IN ('announcement', 'content', 'notification')),
  resource_id TEXT NOT NULL,
  canonical_resource_type TEXT,
  user_id TEXT,
  attempt_count BIGINT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  available_at TEXT NOT NULL,
  lease_owner TEXT,
  lease_expires_at TEXT,
  last_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
, previous_resource_id TEXT, fanout_cursor TEXT);

CREATE TABLE IF NOT EXISTS search_projection_outbox_failed (
  projection_key TEXT PRIMARY KEY,
  operation TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
  resource_type TEXT NOT NULL CHECK (resource_type IN ('announcement', 'content', 'notification')),
  resource_id TEXT NOT NULL,
  previous_resource_id TEXT,
  canonical_resource_type TEXT,
  user_id TEXT,
  attempt_count BIGINT NOT NULL CHECK (attempt_count >= 0),
  last_error TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  failed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS release_reading_backfill_runs (
  id TEXT PRIMARY KEY,
  started_at TEXT NOT NULL,
  window_start_utc TEXT NOT NULL,
  window_end_utc TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'waiting', 'paused', 'succeeded', 'failed')),
  cursor_published_at TEXT,
  cursor_release_id BIGINT,
  total BIGINT NOT NULL DEFAULT 0,
  succeeded BIGINT NOT NULL DEFAULT 0,
  failed BIGINT NOT NULL DEFAULT 0,
  waiting BIGINT NOT NULL DEFAULT 0,
  skipped BIGINT NOT NULL DEFAULT 0,
  lease_owner TEXT,
  lease_expires_at TEXT,
  error_text TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
, lease_task_id TEXT);

CREATE TABLE IF NOT EXISTS release_reading_backfill_items (
  run_id TEXT NOT NULL,
  release_id BIGINT NOT NULL,
  authorized_user_id TEXT,
  published_at TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'waiting', 'succeeded', 'failed', 'skipped')),
  attempt_count BIGINT NOT NULL DEFAULT 0,
  error_text TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (run_id, release_id),
  FOREIGN KEY(run_id) REFERENCES release_reading_backfill_runs(id) ON DELETE CASCADE,
  FOREIGN KEY(release_id) REFERENCES repo_releases(release_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS release_evidence_snapshots (
  id TEXT PRIMARY KEY,
  request_key TEXT NOT NULL UNIQUE,
  release_id BIGINT NOT NULL,
  repo_id BIGINT NOT NULL,
  repo_full_name TEXT NOT NULL,
  audience TEXT NOT NULL CHECK (audience IN ('public', 'authorized')),
  audience_user_id TEXT,
  source_revision TEXT NOT NULL,
  base_tag TEXT NOT NULL,
  head_tag TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'ready', 'failed')),
  snapshot_json TEXT,
  content_hash TEXT,
  attempt_count BIGINT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
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

CREATE TABLE IF NOT EXISTS release_evidence_index (
  id TEXT PRIMARY KEY,
  repo_id BIGINT NOT NULL,
  repo_full_name TEXT NOT NULL,
  audience TEXT NOT NULL CHECK (audience IN ('public', 'authorized')),
  audience_user_id TEXT,
  resource_kind TEXT NOT NULL CHECK (
    resource_kind IN ('compare', 'commit', 'tree', 'BYTEA', 'file')
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

CREATE TABLE IF NOT EXISTS release_evidence_snapshot_items (
  snapshot_id TEXT NOT NULL,
  index_id TEXT NOT NULL,
  ordinal BIGINT NOT NULL CHECK (ordinal >= 0),
  created_at TEXT NOT NULL,
  PRIMARY KEY (snapshot_id, index_id),
  FOREIGN KEY(snapshot_id) REFERENCES release_evidence_snapshots(id) ON DELETE CASCADE,
  FOREIGN KEY(index_id) REFERENCES release_evidence_index(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS tower_sessions.session (
  id TEXT PRIMARY KEY,
  data BYTEA NOT NULL,
  expiry_date TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS migration_control (
  id BIGINT PRIMARY KEY CHECK (id = 1),
  schema_version TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('ready', 'paused', 'failed')),
  owner_id TEXT,
  lease_expires_at TIMESTAMPTZ,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO migration_control (id, schema_version, state) VALUES (1, 'pg18-baseline-v1', 'ready') ON CONFLICT (id) DO NOTHING;
