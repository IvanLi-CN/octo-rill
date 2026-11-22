use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    http::{HeaderValue, Method, StatusCode},
    middleware,
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
    routing::get,
};
use chrono::Utc;
use futures_util::stream;
use oauth2::{AuthorizationCode, CsrfToken, Scope, TokenResponse};
use serde::Deserialize;
use serde_json::json;
use sqlx::FromRow;
use std::convert::Infallible;
use tokio::sync::watch;
use tokio::time::{MissedTickBehavior, interval};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tower_sessions::{Expiry, Session, cookie::SameSite, session_store::ExpiredDeletion};
use tracing::{info, warn};

use crate::{
    config::AppConfig,
    local_id, postgres_jobs,
    postgres_repo_release_worker::RepoReleaseRefreshExecutor,
    postgres_runtime::PostgresRuntime,
    postgres_runtime_owner,
    postgres_tasks::{
        BriefExecutor, CompositeSyncExecutor, NotificationsExecutor, ReleasesExecutor,
        StarredExecutor, TranslationExecutor, WebhookExecutor,
    },
    postgres_worker::TaskExecutorRegistry,
    state::GitHubOAuthClient,
    translations::{TranslationResolveRequest, TranslationSubmitRequest},
};

#[derive(Clone)]
pub(super) struct PostgresServerState {
    pub(super) runtime: PostgresRuntime,
    pub(super) owner_id: Arc<str>,
    pub(super) linuxdo_available: bool,
    pub(super) github_oauth: GitHubOAuthClient,
    pub(super) auth_ext: crate::postgres_auth_ext::AuthExtState,
    pub(super) http: reqwest::Client,
    pub(super) encryption_key: crate::crypto::EncryptionKey,
    pub(super) task_registry: TaskExecutorRegistry,
    pub(super) ai_model: Option<String>,
    pub(super) ai: Option<crate::config::AiConfig>,
}

/// PostgreSQL production application entry.
///
/// This entry point owns the PG pool and owner lease while preserving the
/// complete application boundary: route inventory, sessions, static assets,
/// CORS, observability, workers, and graceful shutdown. A route without a
/// native PG implementation remains registered and fails closed explicitly.
pub async fn serve_postgres(config: AppConfig) -> Result<()> {
    let runtime = crate::postgres_runtime::connect_from_config(&config).await?;
    crate::postgres_runtime::verify_application_schema(&runtime.pool).await?;
    let owner_id: Arc<str> = local_id::generate_local_id().into();
    postgres_runtime_owner::register(&runtime.pool, &owner_id, Utc::now()).await?;

    // GitHub answers 301 for renamed repositories and organisations. The
    // legacy client followed those redirects; refusing them turned every
    // renamed repository into a permanent `github_http_error`.
    let http = reqwest::Client::builder()
        .user_agent("OctoRill")
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .context("build PostgreSQL service HTTP client")?;
    let task_registry = TaskExecutorRegistry::builder()
        .register(
            "sync.starred",
            StarredExecutor::new(
                runtime.pool.clone(),
                http.clone(),
                config.encryption_key.clone(),
            ),
        )
        .register(
            "sync.releases",
            ReleasesExecutor::new(
                runtime.pool.clone(),
                http.clone(),
                config.encryption_key.clone(),
            ),
        )
        .register(
            "sync.repo_release",
            RepoReleaseRefreshExecutor::new(
                runtime.pool.clone(),
                http.clone(),
                config.encryption_key.clone(),
            ),
        )
        .register(
            "sync.notifications",
            NotificationsExecutor::new(
                runtime.pool.clone(),
                http.clone(),
                config.encryption_key.clone(),
            ),
        )
        .register(
            "sync.all",
            CompositeSyncExecutor::new(
                runtime.pool.clone(),
                http.clone(),
                config.encryption_key.clone(),
            ),
        )
        .register(
            "sync.access_refresh",
            CompositeSyncExecutor::new(
                runtime.pool.clone(),
                http.clone(),
                config.encryption_key.clone(),
            ),
        )
        .register(
            "webhook.push.manage",
            WebhookExecutor::new(
                runtime.pool.clone(),
                http.clone(),
                config.encryption_key.clone(),
                config.public_base_url.to_string(),
            ),
        )
        .register(
            "webhook.push.reconcile",
            WebhookExecutor::new(
                runtime.pool.clone(),
                http.clone(),
                config.encryption_key.clone(),
                config.public_base_url.to_string(),
            ),
        )
        .register("brief.generate", BriefExecutor::new(runtime.pool.clone()))
        .register("brief.daily_slot", BriefExecutor::new(runtime.pool.clone()))
        .register(
            "translation.generate",
            TranslationExecutor::new(runtime.pool.clone(), http.clone(), config.ai.clone()),
        )
        .build();
    let state = PostgresServerState {
        runtime: runtime.clone(),
        owner_id: owner_id.clone(),
        linuxdo_available: config.linuxdo.is_some(),
        github_oauth: crate::state::build_oauth_client(&config)?,
        auth_ext: crate::postgres_auth_ext::AuthExtState::new(&config)?,
        http,
        encryption_key: config.encryption_key.clone(),
        task_registry,
        ai_model: config.ai.as_ref().map(|ai| ai.model.clone()),
        ai: config.ai.clone(),
    };
    info!(
        task_types = ?state.task_registry.registered_task_types(),
        "PostgreSQL task executors registered"
    );
    let (worker_shutdown, worker_shutdown_rx) = watch::channel(false);
    // Periodic lanes only enqueue work; the worker performs it. They share the
    // same shutdown signal so a drain stops new dispatches immediately.
    crate::postgres_scheduler::spawn(state.clone(), worker_shutdown.subscribe());
    let worker_registry = state.task_registry.clone();
    let app = Router::new()
        .route("/health", get(health))
        .route("/version", get(version))
        .route("/api/health", get(health))
        .route("/api/version", get(version))
        .route("/api/me", get(me))
        .route("/api/me/resume", axum::routing::post(me_resume))
        .route("/api/me/personal-repos", get(personal_repos))
        .route("/api/me/linuxdo", get(me_linuxdo))
        .route("/api/me/passkeys", get(me_passkeys))
        .route(
            "/api/me/passkeys/{passkey_id}",
            axum::routing::delete(delete_passkey),
        )
        .route(
            "/api/me/webhook-push",
            get(webhook_push_settings).patch(patch_webhook_push_settings),
        )
        .route(
            "/api/me/webhook-push/reconcile",
            axum::routing::post(reconcile_webhook_push),
        )
        .route("/api/me/github-connections", get(me_github_connections))
        .route(
            "/api/me/github-connections/{connection_id}",
            axum::routing::delete(delete_github_connection),
        )
        .route("/api/me/linuxdo", axum::routing::delete(delete_linuxdo))
        .route("/api/me/profile", get(me_profile).patch(update_me_profile))
        .route("/api/me/api-keys", get(list_api_keys).post(create_api_key))
        .route(
            "/api/me/api-keys/{api_key_id}",
            axum::routing::delete(delete_api_key),
        )
        .route("/api/auth/bind-context", get(auth_bind_context))
        .route("/api/search", get(search))
        .route("/api/starred", get(list_starred))
        .route("/api/releases", get(list_releases))
        .route("/api/releases/{release_id}/detail", get(release_detail))
        .route("/api/notifications", get(list_notifications))
        .route("/api/repos/following", get(list_following_repos))
        .route(
            "/api/repos/{owner}/{repo}/following",
            axum::routing::put(follow_repo).delete(unfollow_repo),
        )
        .route(
            "/api/repos/{owner}/{repo}/releases/tag/{tag}/detail",
            get(crate::postgres_repo_releases::get_release_detail_by_repo_tag),
        )
        .route(
            "/api/repos/{owner}/{repo}/discussions/{number}/detail",
            get(crate::postgres_repo_releases::get_announcement_detail),
        )
        .route(
            "/api/repos/{owner}/{repo}/public-release",
            get(crate::postgres_repo_releases::get_repo_public_release_status)
                .post(crate::postgres_repo_releases::publish_repo_public_release)
                .delete(crate::postgres_repo_releases::unpublish_repo_public_release),
        )
        .route(
            "/api/public/repos/{owner}/{repo}/releases",
            get(crate::postgres_repo_releases::public_list_repo_releases_http),
        )
        .route(
            "/api/public/repos/{owner}/{repo}/releases/tag/{tag}",
            get(crate::postgres_repo_releases::public_get_repo_release_detail),
        )
        .route(
            "/api/public/repos/{owner}/{repo}/releases/content",
            get(crate::postgres_repo_releases::public_get_repo_release_content),
        )
        .route(
            "/api/webhooks/github/releases",
            axum::routing::post(crate::postgres_webhook::receive),
        )
        .route(
            "/api/dashboard/updates",
            get(crate::postgres_dashboard::dashboard_updates),
        )
        .route(
            "/api/dashboard/feed/sections/{section_id}/items",
            get(crate::postgres_dashboard::dashboard_readable_section_items),
        )
        .route("/api/tasks/{task_id}/events", get(task_events))
        .route("/api/tasks/{task_id}", get(task_status))
        .route(
            "/api/tasks/{task_id}/cancel",
            axum::routing::post(cancel_task),
        )
        .route(
            "/api/admin/jobs/overview",
            get(crate::postgres_admin_jobs::overview),
        )
        .route(
            "/api/admin/dashboard",
            get(crate::postgres_admin_dashboard::admin_dashboard),
        )
        .route(
            "/api/admin/jobs/events",
            get(crate::postgres_admin_jobs::events_stream),
        )
        .route("/api/admin/users", get(admin_users))
        .route(
            "/api/admin/repos/overview",
            get(admin_repo_governance_overview),
        )
        .route("/api/admin/repos", get(admin_repo_governance))
        .route(
            "/api/admin/public-release-repos",
            get(admin_public_release_repos),
        )
        .route(
            "/api/admin/public-release-repos/{usage_id}",
            axum::routing::delete(crate::postgres_admin_actions::admin_delete_public_release_repo),
        )
        .route(
            "/api/admin/jobs/release-reading/backfill",
            axum::routing::post(
                crate::postgres_admin_actions::admin_enqueue_release_reading_backfill,
            ),
        )
        .route(
            "/api/admin/jobs/webhook-push/runtime-config",
            get(admin_webhook_runtime_config),
        )
        .route(
            "/api/admin/jobs/realtime/{task_id}/cancel",
            axum::routing::post(admin_cancel_task),
        )
        .route(
            "/api/admin/jobs/realtime/{task_id}",
            get(crate::postgres_admin_jobs::task_detail),
        )
        .route(
            "/api/admin/jobs/realtime/{task_id}/retry",
            axum::routing::post(admin_retry_task),
        )
        .route(
            "/api/admin/jobs/realtime/{task_id}/release-freshness",
            get(admin_release_freshness),
        )
        .route(
            "/api/admin/jobs/realtime/{task_id}/log",
            get(admin_task_log),
        )
        .route("/api/admin/jobs/scheduled", get(admin_scheduled_slots))
        .route(
            "/api/admin/jobs/scheduled/{hour_utc}",
            axum::routing::patch(admin_patch_scheduled_slot),
        )
        .route(
            "/api/admin/jobs/sync/runtime-config",
            get(admin_sync_runtime_config),
        )
        .route(
            "/api/admin/jobs/llm/status",
            get(crate::postgres_admin_llm::status),
        )
        .route(
            "/api/admin/jobs/llm/activity",
            get(crate::postgres_admin_llm::activity),
        )
        .route(
            "/api/admin/jobs/llm/runtime-config",
            axum::routing::patch(crate::postgres_admin_actions::admin_patch_llm_runtime_config),
        )
        .route(
            "/api/admin/jobs/llm/calls",
            get(crate::postgres_admin_llm::calls),
        )
        .route(
            "/api/admin/jobs/llm/calls/{call_id}",
            get(crate::postgres_admin_llm::detail),
        )
        .route(
            "/api/admin/jobs/llm/calls/{call_id}/diagnostic-access",
            axum::routing::post(crate::postgres_admin_actions::admin_audit_llm_diagnostic_access),
        )
        .route(
            "/api/admin/jobs/ai-records/{record_kind}",
            get(crate::postgres_admin_ai_records::admin_list_collection_records),
        )
        .route(
            "/api/admin/jobs/ai-records/{record_kind}/activity",
            get(crate::postgres_admin_ai_records::admin_get_collection_activity),
        )
        .route(
            "/api/admin/jobs/ai-records/{record_kind}/{record_id}",
            get(crate::postgres_admin_ai_records::admin_get_collection_record_detail),
        )
        .route("/api/admin/jobs/migrations", get(admin_migrations))
        .route(
            "/api/admin/jobs/migrations/{migration_id}/pause",
            axum::routing::post(crate::postgres_admin_actions::admin_pause_online_migration),
        )
        .route(
            "/api/admin/jobs/migrations/{migration_id}/resume",
            axum::routing::post(crate::postgres_admin_actions::admin_resume_online_migration),
        )
        .route(
            "/api/admin/jobs/content-processing/cutover",
            axum::routing::post(crate::postgres_admin_actions::admin_content_processing_cutover),
        )
        .route(
            "/api/admin/jobs/content-processing/freeze",
            axum::routing::post(crate::postgres_admin_actions::admin_content_processing_freeze),
        )
        .route(
            "/api/admin/jobs/content-processing/identity-upgrade",
            get(admin_identity_upgrade_status)
                .post(crate::postgres_admin_actions::admin_content_identity_upgrade_control),
        )
        .route(
            "/api/admin/jobs/search-index",
            get(admin_search_index_status),
        )
        .route(
            "/api/admin/jobs/search-index/resume",
            axum::routing::post(crate::postgres_admin_actions::admin_resume_search_index),
        )
        .route(
            "/api/admin/users/{user_id}",
            get(admin_user_detail).patch(admin_patch_user),
        )
        .route(
            "/api/admin/users/{user_id}/profile",
            get(admin_user_profile).patch(crate::postgres_admin_actions::admin_patch_user_profile),
        )
        .route(
            "/api/admin/jobs/realtime",
            get(crate::postgres_admin_jobs::realtime_tasks),
        )
        .route("/api/public/metrics/v1/octo-rill", get(public_metrics))
        .route("/api/reaction-token/status", get(reaction_token_status))
        .route(
            "/api/reaction-token/check",
            axum::routing::post(crate::postgres_reactions::check_reaction_token),
        )
        .route(
            "/api/reaction-token",
            axum::routing::put(crate::postgres_reactions::upsert_reaction_token),
        )
        .route(
            "/api/release/reactions/toggle",
            axum::routing::post(crate::postgres_reactions::toggle_release_reaction),
        )
        .route(
            "/api/auth/passkeys/register/options",
            axum::routing::post(crate::postgres_auth_ext::passkey_register_options),
        )
        .route(
            "/api/auth/passkeys/register/verify",
            axum::routing::post(crate::postgres_auth_ext::passkey_register_verify),
        )
        .route(
            "/api/auth/passkeys/authenticate/options",
            axum::routing::post(crate::postgres_auth_ext::passkey_authenticate_options),
        )
        .route(
            "/api/auth/passkeys/authenticate/verify",
            axum::routing::post(crate::postgres_auth_ext::passkey_authenticate_verify),
        )
        .route(
            "/translate/requests",
            axum::routing::post(submit_translation_request),
        )
        .route(
            "/translate/requests/{request_id}",
            get(get_translation_request),
        )
        .route(
            "/translate/requests/{request_id}/retry",
            axum::routing::post(retry_translation_request),
        )
        .route(
            "/translate/results",
            axum::routing::post(resolve_translation_results),
        )
        .route(
            "/translate/requests/{request_id}/stream",
            get(stream_translation_request),
        )
        .route(
            "/api/translate/requests",
            axum::routing::post(submit_translation_request),
        )
        .route(
            "/api/translate/requests/{request_id}",
            get(get_translation_request),
        )
        .route(
            "/api/translate/requests/{request_id}/retry",
            axum::routing::post(retry_translation_request),
        )
        .route(
            "/api/translate/results",
            axum::routing::post(resolve_translation_results),
        )
        .route(
            "/api/translate/requests/{request_id}/stream",
            get(stream_translation_request),
        )
        .route(
            "/translate/releases/batch",
            axum::routing::post(crate::postgres_translate::translate_releases_batch),
        )
        .route(
            "/translate/releases/batch/stream",
            axum::routing::post(crate::postgres_translate::translate_releases_batch_stream),
        )
        .route(
            "/translate/release",
            axum::routing::post(crate::postgres_translate::translate_release),
        )
        .route(
            "/translate/release/stream",
            axum::routing::post(crate::postgres_translate::translate_release_stream),
        )
        .route(
            "/admin/jobs/translations/status",
            get(admin_translation_status),
        )
        .route(
            "/admin/jobs/translations/runtime-config",
            axum::routing::patch(
                crate::postgres_admin_actions::admin_patch_translation_runtime_config,
            ),
        )
        .route(
            "/admin/jobs/translations/requests",
            get(admin_translation_requests),
        )
        .route(
            "/admin/jobs/translations/requests/{request_id}",
            get(admin_translation_request_detail),
        )
        .route(
            "/admin/jobs/translations/attempt-events",
            get(admin_translation_attempt_events),
        )
        .route(
            "/admin/jobs/translations/batches",
            get(admin_translation_batches),
        )
        .route(
            "/admin/jobs/translations/batches/{batch_id}",
            get(admin_translation_batch_detail),
        )
        .route(
            "/api/translate/releases/batch",
            axum::routing::post(crate::postgres_translate::translate_releases_batch),
        )
        .route(
            "/api/translate/releases/batch/stream",
            axum::routing::post(crate::postgres_translate::translate_releases_batch_stream),
        )
        .route(
            "/api/translate/release",
            axum::routing::post(crate::postgres_translate::translate_release),
        )
        .route(
            "/api/translate/release/detail/batch",
            axum::routing::post(crate::postgres_translate::translate_release_detail_batch),
        )
        .route(
            "/api/translate/release/detail",
            axum::routing::post(crate::postgres_translate::translate_release_detail),
        )
        .route(
            "/api/translate/notifications/batch",
            axum::routing::post(crate::postgres_translate::translate_notifications_batch),
        )
        .route(
            "/api/translate/notification",
            axum::routing::post(crate::postgres_translate::translate_notification),
        )
        .route(
            "/api/admin/jobs/translations/status",
            get(admin_translation_status),
        )
        .route(
            "/api/admin/jobs/translations/runtime-config",
            axum::routing::patch(
                crate::postgres_admin_actions::admin_patch_translation_runtime_config,
            ),
        )
        .route(
            "/api/admin/jobs/translations/requests",
            get(admin_translation_requests),
        )
        .route(
            "/api/admin/jobs/translations/requests/{request_id}",
            get(admin_translation_request_detail),
        )
        .route(
            "/api/admin/jobs/translations/attempt-events",
            get(admin_translation_attempt_events),
        )
        .route(
            "/api/admin/jobs/translations/batches",
            get(admin_translation_batches),
        )
        .route(
            "/api/admin/jobs/translations/batches/{batch_id}",
            get(admin_translation_batch_detail),
        )
        .route(
            "/api/feed",
            get(crate::postgres_dashboard::feed).head(crate::postgres_dashboard::head_feed),
        )
        .route(
            "/api/feed/reactions/refresh",
            axum::routing::post(crate::postgres_reactions::refresh_feed_reactions),
        )
        .route(
            "/api/dashboard/feed",
            get(crate::postgres_dashboard::dashboard_feed),
        )
        .route("/api/briefs", get(list_briefs))
        .route("/api/briefs/{brief_id}", get(get_brief))
        .route("/api/briefs/generate", axum::routing::post(generate_brief))
        .route("/api/sync/starred", axum::routing::post(sync_starred))
        .route("/api/sync/all", axum::routing::post(sync_all))
        .route("/api/sync/releases", axum::routing::post(sync_releases))
        .route(
            "/api/sync/notifications",
            axum::routing::post(sync_notifications),
        )
        .route("/auth/github/login", get(github_login))
        .route(
            "/auth/github/connect",
            get(crate::postgres_auth_ext::github_connect),
        )
        .route("/auth/github/callback", get(github_callback))
        .route(
            "/auth/linuxdo/login",
            get(crate::postgres_auth_ext::linuxdo_login),
        )
        .route(
            "/auth/linuxdo/callback",
            get(crate::postgres_auth_ext::linuxdo_callback),
        )
        .route("/auth/logout", get(logout))
        .with_state(state);
    let session_layer =
        crate::session_store::CoordinatedSessionLayer::new(runtime.sessions.clone())
            .with_name("octo_rill_sid")
            .with_secure(config.public_base_url.scheme() == "https")
            .with_same_site(SameSite::Lax)
            .with_expiry(Expiry::OnInactivity(time::Duration::days(30)));
    let mut app = app.layer(session_layer);
    if let Some(static_dir) = config.static_dir.clone() {
        app = crate::server::attach_static_site_routes(app, static_dir);
    }

    let cors_origin = crate::state::normalize_origin(&config.public_base_url)?;
    let cors_origin: HeaderValue = cors_origin
        .as_str()
        .parse()
        .context("invalid cors origin")?;
    let cors = CorsLayer::new()
        .allow_origin(cors_origin)
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ]);
    let (set_request_id, propagate_request_id) = crate::observability::request_id_layers();
    let access_log = middleware::from_fn(crate::observability::access_log_middleware);
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::extract::Request| {
            crate::observability::request_trace_span(request)
        })
        .on_request(())
        .on_response(())
        .on_failure(());
    let app = app.layer(cors).layer(
        ServiceBuilder::new()
            .layer(set_request_id)
            .layer(trace_layer)
            .layer(access_log)
            .layer(propagate_request_id),
    );

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("bind PostgreSQL service listener at {}", config.bind_addr))?;
    let addr = listener
        .local_addr()
        .context("read PostgreSQL service listener address")?;
    info!(%addr, owner_id = %owner_id, "PostgreSQL service boundary listening");

    let heartbeat_pool = runtime.pool.clone();
    let heartbeat_owner = owner_id.clone();
    let heartbeat = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(15));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) =
                postgres_runtime_owner::heartbeat(&heartbeat_pool, &heartbeat_owner, Utc::now())
                    .await
            {
                warn!(%error, owner_id = %heartbeat_owner, "PostgreSQL runtime owner heartbeat failed");
            }
        }
    });
    let session_cleanup = tokio::spawn(
        runtime
            .sessions
            .clone()
            .continuously_delete_expired(Duration::from_secs(60)),
    );
    let worker_pool = runtime.pool.clone();
    let worker_owner = owner_id.clone();
    let worker = tokio::spawn(async move {
        if postgres_worker_enabled() {
            crate::postgres_worker::run_registered_until_shutdown(
                &worker_pool,
                &worker_owner,
                &worker_registry,
                worker_shutdown_rx,
                Duration::from_millis(250),
            )
            .await
        } else {
            info!("PostgreSQL task worker disabled by OCTORILL_PG_WORKER_ENABLED");
            Ok(())
        }
    });

    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("PostgreSQL service exited");
    let _ = worker_shutdown.send(true);
    heartbeat.abort();
    session_cleanup.abort();
    if let Ok(Err(error)) = worker.await {
        warn!(%error, "PostgreSQL worker stopped with an error");
    }
    postgres_runtime_owner::unregister(&runtime.pool, &owner_id).await?;
    runtime.pool.close().await;
    serve_result
}

fn postgres_worker_enabled() -> bool {
    std::env::var("OCTORILL_PG_WORKER_ENABLED").as_deref() != Ok("0")
}

async fn logout(session: Session) -> impl IntoResponse {
    match session.delete().await {
        Ok(()) => (StatusCode::NO_CONTENT, axum::Json(json!({}))),
        Err(error) => {
            warn!(%error, "PostgreSQL session logout failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"ok": false, "error": "session_delete_failed"})),
            )
        }
    }
}

const SESSION_KEY_OAUTH_STATE: &str = "oauth_state";
const SESSION_KEY_USER_ID: &str = "user_id";

async fn github_login(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let (auth_url, csrf_token) = state
        .github_oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("read:user".to_owned()))
        .add_scope(Scope::new("user:email".to_owned()))
        .add_scope(Scope::new("notifications".to_owned()))
        .add_scope(Scope::new("public_repo".to_owned()))
        .url();
    if session
        .insert(SESSION_KEY_OAUTH_STATE, csrf_token.secret())
        .await
        .is_err()
        || session
            .insert(
                crate::postgres_auth_ext::SESSION_KEY_GITHUB_OAUTH_MODE,
                crate::postgres_auth_ext::GITHUB_OAUTH_MODE_LOGIN,
            )
            .await
            .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::response::Redirect::to("/").into_response(),
        )
            .into_response();
    }
    axum::response::Redirect::to(auth_url.as_str()).into_response()
}

#[derive(Debug, Deserialize)]
struct GitHubCallbackQuery {
    code: String,
    state: String,
}

async fn github_callback(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<GitHubCallbackQuery>,
) -> axum::response::Response {
    match github_callback_inner(state, session, query).await {
        Ok(redirect) => redirect.into_response(),
        Err(error) => error.into_response(),
    }
}

async fn github_callback_inner(
    state: PostgresServerState,
    session: Session,
    query: GitHubCallbackQuery,
) -> Result<axum::response::Redirect, crate::error::ApiError> {
    let expected = session
        .get::<String>(SESSION_KEY_OAUTH_STATE)
        .await
        .map_err(crate::error::ApiError::internal)?;
    if expected.as_deref() != Some(query.state.as_str()) {
        return Err(crate::error::ApiError::bad_request("invalid oauth state"));
    }
    let requested_mode = session
        .get::<String>(crate::postgres_auth_ext::SESSION_KEY_GITHUB_OAUTH_MODE)
        .await
        .map_err(crate::error::ApiError::internal)?;
    session
        .remove::<String>(SESSION_KEY_OAUTH_STATE)
        .await
        .map_err(crate::error::ApiError::internal)?;
    let _ = session
        .remove::<String>(crate::postgres_auth_ext::SESSION_KEY_GITHUB_OAUTH_MODE)
        .await;

    let token = state
        .github_oauth
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(&state.http)
        .await
        .context("github oauth token exchange failed")
        .map_err(crate::error::ApiError::internal)?;
    let access_token = token.access_token().secret().to_owned();
    let scopes = token
        .scopes()
        .map(|scopes| {
            scopes
                .iter()
                .map(|scope| scope.as_ref())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let user = crate::github::fetch_user(&state.http, &access_token)
        .await
        .context("failed to fetch github user")
        .map_err(crate::error::ApiError::internal)?;
    let email = if user.email.is_some() {
        user.email.clone()
    } else {
        crate::github::fetch_primary_email(&state.http, &access_token)
            .await
            .ok()
            .flatten()
    };
    crate::postgres_auth_ext::finalize_github_auth(
        &state,
        &session,
        &user,
        email.as_deref(),
        access_token.as_str(),
        scopes.as_str(),
        requested_mode,
    )
    .await
}

#[derive(sqlx::FromRow)]
struct MeRow {
    id: String,
    is_admin: i64,
    is_disabled: i64,
    paused_at: Option<String>,
    include_own_releases: i64,
    github_user_id: Option<i64>,
    login: Option<String>,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
}

#[derive(sqlx::FromRow)]
struct GithubConnectionRow {
    id: String,
    github_user_id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
    scopes: String,
    linked_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct LinuxDoConnectionRow {
    linuxdo_user_id: i64,
    username: String,
    name: Option<String>,
    avatar_url: Option<String>,
    trust_level: i64,
    active: i64,
    silenced: i64,
    linked_at: String,
    updated_at: String,
}

async fn me_linuxdo(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let rows = sqlx::query_as::<_, LinuxDoConnectionRow>(
        "SELECT linuxdo_user_id, username, name, avatar_url, trust_level, active, silenced, linked_at, updated_at \
         FROM public.linuxdo_connections WHERE user_id = $1 LIMIT 1",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await;
    match rows {
        Ok(connection) => (
            StatusCode::OK,
            axum::Json(json!({
                "available": state.linuxdo_available,
                "connection": connection.map(|row| json!({
                    "linuxdo_user_id": row.linuxdo_user_id,
                    "username": row.username,
                    "name": row.name,
                    "avatar_url": row.avatar_url,
                    "trust_level": row.trust_level,
                    "active": row.active != 0,
                    "silenced": row.silenced != 0,
                    "linked_at": row.linked_at,
                    "updated_at": row.updated_at,
                })),
            })),
        ),
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL LinuxDo connection lookup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
        }
    }
}

async fn delete_linuxdo(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    match sqlx::query("DELETE FROM public.linuxdo_connections WHERE user_id = $1")
        .bind(&user_id)
        .execute(&state.runtime.pool)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            axum::Json(json!({
                "available": state.linuxdo_available,
                "connection": null,
            })),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL LinuxDo connection deletion failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn me_passkeys(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    match sqlx::query(
        "SELECT id, label, created_at, last_used_at FROM public.user_passkeys \
         WHERE user_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(&user_id)
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(rows) => {
            use sqlx::Row;
            let items = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "id": row.get::<String, _>("id"),
                        "label": row.get::<String, _>("label"),
                        "created_at": row.get::<String, _>("created_at"),
                        "last_used_at": row.get::<Option<String>, _>("last_used_at"),
                    })
                })
                .collect::<Vec<_>>();
            (StatusCode::OK, axum::Json(json!({"items": items})))
        }
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL passkey lookup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
        }
    }
}

async fn delete_passkey(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(passkey_id): Path<String>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    let deleted = async {
        let mut tx = state.runtime.pool.begin().await?;
        let deleted = sqlx::query(
            "DELETE FROM public.user_passkeys WHERE id = $1 AND user_id = $2 RETURNING id",
        )
        .bind(&passkey_id)
        .bind(&user_id)
        .fetch_optional(&mut *tx)
        .await?;
        if deleted.is_some() {
            let remaining: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM public.user_passkeys WHERE user_id = $1",
            )
            .bind(&user_id)
            .fetch_one(&mut *tx)
            .await?;
            if remaining == 0 {
                sqlx::query(
                    "UPDATE public.users SET passkey_user_handle_uuid = NULL, updated_at = $1 WHERE id = $2",
                )
                .bind(Utc::now().to_rfc3339())
                .bind(&user_id)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok::<_, sqlx::Error>(deleted)
    }
    .await;
    match deleted {
        Ok(Some(_)) => match sqlx::query(
            "SELECT id, label, created_at, last_used_at FROM public.user_passkeys WHERE user_id = $1 ORDER BY created_at ASC, id ASC",
        )
        .bind(&user_id)
        .fetch_all(&state.runtime.pool)
        .await
        {
            Ok(rows) => {
                use sqlx::Row;
                let items = rows
                    .into_iter()
                    .map(|row| {
                        json!({
                            "id": row.get::<String, _>("id"),
                            "label": row.get::<String, _>("label"),
                            "created_at": row.get::<String, _>("created_at"),
                            "last_used_at": row.get::<Option<String>, _>("last_used_at"),
                        })
                    })
                    .collect::<Vec<_>>();
                (StatusCode::OK, axum::Json(json!({"items": items}))).into_response()
            }
            Err(error) => {
                warn!(%error, user_id = %user_id, "PostgreSQL passkey reload failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"code": "database_error"})),
                )
                    .into_response()
            }
        },
        Ok(None) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "passkey_not_found", "message": "passkey not found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, user_id = %user_id, passkey_id = %passkey_id, "PostgreSQL passkey deletion failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct WebhookPushPatch {
    desired_state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebhookReconcileRequest {
    repo_id: Option<i64>,
}

async fn reconcile_webhook_push(
    State(state): State<PostgresServerState>,
    session: Session,
    request: Option<axum::Json<WebhookReconcileRequest>>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    let config = sqlx::query(
        "SELECT include_own_releases, webhook_push_desired_state FROM public.users WHERE id = $1 AND is_disabled = 0",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await;
    let Some(config) = (match config {
        Ok(config) => config,
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL webhook reconcile config lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response();
        }
    }) else {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "user_not_found"})),
        )
            .into_response();
    };
    use sqlx::Row;
    let include_own_releases = config.get::<i64, _>("include_own_releases") != 0;
    let desired_state = config.get::<String, _>("webhook_push_desired_state");
    if !include_own_releases && desired_state == "enabled" {
        return (
            StatusCode::CONFLICT,
            axum::Json(json!({
                "code": "my_releases_disabled",
                "message": "请先开启“我的发布”。"
            })),
        )
            .into_response();
    }
    let task_id = local_id::generate_local_id();
    let now = Utc::now();
    let repo_id = request.and_then(|body| body.repo_id);
    let payload = json!({
        "user_id": user_id,
        "operation": "reconcile",
        "repo_id": repo_id,
        "trigger": "manual"
    });
    match sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at) VALUES ($1, 'webhook.push.manage', 'queued', 'webhook_push', $2, $3, $4, $4)",
    )
    .bind(&task_id)
    .bind(&user_id)
    .bind(payload.to_string())
    .bind(now)
    .execute(&state.runtime.pool)
    .await
    {
        Ok(_) => (
            StatusCode::ACCEPTED,
            axum::Json(json!({
                "mode": "task_id",
                "task_id": task_id,
                "task_type": "webhook.push.manage",
                "status": "queued"
            })),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL webhook reconcile enqueue failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn webhook_push_settings(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    match sqlx::query(
        "SELECT include_own_releases, webhook_push_enabled, webhook_push_desired_state, webhook_push_last_completed_check_at \
         FROM public.users WHERE id = $1 AND is_disabled = 0",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    {
        Ok(Some(row)) => {
            use sqlx::Row;
            (
                StatusCode::OK,
                axum::Json(json!({
                    "desired_state": row.get::<String, _>("webhook_push_desired_state"),
                    "enabled": row.get::<i64, _>("webhook_push_enabled") != 0,
                    "include_own_releases": row.get::<i64, _>("include_own_releases") != 0,
                    "callback_ready": false,
                    "last_completed_check_at": row.get::<Option<String>, _>("webhook_push_last_completed_check_at"),
                    "summary": {"total": 0, "registered": 0, "missing": 0, "errors": 0, "pending": 0},
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "user_not_found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL webhook settings query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn patch_webhook_push_settings(
    State(state): State<PostgresServerState>,
    session: Session,
    axum::Json(patch): axum::Json<WebhookPushPatch>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    let Some(desired_state) = patch
        .desired_state
        .filter(|value| matches!(value.as_str(), "enabled" | "paused" | "deleted"))
    else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"code": "invalid_desired_state"})),
        )
            .into_response();
    };
    let now = Utc::now();
    let task_id = local_id::generate_local_id();
    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            warn!(%error, "PostgreSQL webhook settings transaction failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response();
        }
    };
    let updated = sqlx::query(
        "UPDATE public.users SET webhook_push_desired_state = $1, webhook_push_enabled = $2, updated_at = $3 WHERE id = $4 AND is_disabled = 0 AND paused_at IS NULL",
    )
    .bind(&desired_state)
    .bind(if desired_state == "enabled" { 1_i64 } else { 0_i64 })
    .bind(now.to_rfc3339())
    .bind(&user_id)
    .execute(&mut *tx)
    .await;
    match updated {
        Ok(result) if result.rows_affected() == 1 => {
            let payload = json!({"user_id": user_id, "desired_state": desired_state});
            let queued = sqlx::query(
                "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at) VALUES ($1, 'webhook.push.reconcile', 'queued', 'api.webhook_push', $2, $3, $4, $4)",
            )
            .bind(&task_id)
            .bind(&user_id)
            .bind(payload.to_string())
            .bind(now)
            .execute(&mut *tx)
            .await;
            match queued {
                Ok(_) => match tx.commit().await {
                    Ok(()) => (
                        StatusCode::ACCEPTED,
                        axum::Json(json!({"desired_state": desired_state, "task_id": task_id, "status": "queued"})),
                    )
                        .into_response(),
                    Err(error) => {
                        warn!(%error, "PostgreSQL webhook settings commit failed");
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(json!({"code": "database_error"})),
                        )
                            .into_response()
                    }
                },
                Err(error) => {
                    let _ = tx.rollback().await;
                    warn!(%error, "PostgreSQL webhook reconcile enqueue failed");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(json!({"code": "database_error"})),
                    )
                        .into_response()
                }
            }
        }
        Ok(_) => {
            let _ = tx.rollback().await;
            (
                StatusCode::NOT_FOUND,
                axum::Json(json!({"code": "user_not_found_or_inactive"})),
            )
                .into_response()
        }
        Err(error) => {
            let _ = tx.rollback().await;
            warn!(%error, "PostgreSQL webhook settings update failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn me_github_connections(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let active = sqlx::query_scalar::<_, bool>(
        "SELECT (is_disabled = 0 AND paused_at IS NULL) FROM public.users WHERE id = $1",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await;
    match active {
        Ok(Some(true)) => {}
        Ok(Some(false)) => {
            return (
                StatusCode::FORBIDDEN,
                axum::Json(json!({"code": "account_inactive"})),
            );
        }
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"code": "unauthorized", "message": "session user not found"})),
            );
        }
        Err(error) => {
            warn!(%error, "PostgreSQL GitHub connection access check failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            );
        }
    }
    let rows = sqlx::query_as::<_, GithubConnectionRow>(
        "SELECT id, github_user_id, login, name, avatar_url, email, scopes, linked_at, updated_at \
         FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC",
    )
    .bind(&user_id)
    .fetch_all(&state.runtime.pool)
    .await;
    match rows {
        Ok(rows) => (
            StatusCode::OK,
            axum::Json(json!({"items": rows.into_iter().map(|row| json!({
                "id": row.id,
                "github_user_id": row.github_user_id,
                "login": row.login,
                "name": row.name,
                "avatar_url": row.avatar_url,
                "email": row.email,
                "scopes": row.scopes,
                "linked_at": row.linked_at,
                "updated_at": row.updated_at,
            })).collect::<Vec<_>>() })),
        ),
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL GitHub connection lookup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
        }
    }
}

async fn delete_github_connection(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(connection_id): Path<String>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            warn!(%error, "PostgreSQL GitHub connection delete transaction failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            );
        }
    };
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.github_connections WHERE user_id = $1",
    )
    .bind(&user_id)
    .fetch_one(&mut *tx)
    .await;
    let count = match count {
        Ok(count) => count,
        Err(error) => {
            warn!(%error, "PostgreSQL GitHub connection count failed");
            let _ = tx.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            );
        }
    };
    if count <= 1 {
        let _ = tx.rollback().await;
        return (
            StatusCode::CONFLICT,
            axum::Json(json!({"code": "last_github_connection_guard"})),
        );
    }
    let deleted =
        sqlx::query("DELETE FROM public.github_connections WHERE id = $1 AND user_id = $2")
            .bind(&connection_id)
            .bind(&user_id)
            .execute(&mut *tx)
            .await;
    match deleted {
        Ok(result) if result.rows_affected() == 1 => match tx.commit().await {
            Ok(()) => (StatusCode::NO_CONTENT, axum::Json(json!({}))),
            Err(error) => {
                warn!(%error, "PostgreSQL GitHub connection delete commit failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"code": "database_error"})),
                )
            }
        },
        Ok(_) => {
            let _ = tx.rollback().await;
            (
                StatusCode::NOT_FOUND,
                axum::Json(json!({"code": "github_connection_not_found"})),
            )
        }
        Err(error) => {
            warn!(%error, "PostgreSQL GitHub connection delete failed");
            let _ = tx.rollback().await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
        }
    }
}

async fn me(State(state): State<PostgresServerState>, session: Session) -> impl IntoResponse {
    let user_id = match session.get::<String>("user_id").await {
        Ok(Some(user_id)) => user_id,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"code": "unauthorized", "message": "login required"})),
            );
        }
        Err(error) => {
            warn!(%error, "PostgreSQL session read failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "session_read_failed"})),
            );
        }
    };

    let row = sqlx::query_as::<_, MeRow>(
        "SELECT u.id, u.is_admin, u.is_disabled, u.paused_at, u.include_own_releases, \
                c.github_user_id, c.login, c.name, c.avatar_url, c.email \
         FROM public.users u \
         LEFT JOIN LATERAL (\
             SELECT github_user_id, login, name, avatar_url, email \
             FROM public.github_connections \
             WHERE user_id = u.id ORDER BY linked_at ASC, id ASC LIMIT 1\
         ) c ON TRUE \
         WHERE u.id = $1",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await;

    let Some(row) = (match row {
        Ok(row) => row,
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL user lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            );
        }
    }) else {
        let _ = session.clear().await;
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "session user not found"})),
        );
    };
    if row.is_disabled != 0 {
        let _ = session.clear().await;
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({"code": "account_disabled"})),
        );
    }
    let (Some(github_user_id), Some(login)) = (row.github_user_id, row.login) else {
        let _ = session.clear().await;
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(
                json!({"code": "unauthorized", "message": "session user has no github connection"}),
            ),
        );
    };
    let account_status = if row.paused_at.is_some() {
        "paused"
    } else {
        "active"
    };
    (
        StatusCode::OK,
        axum::Json(json!({
            "user": {
                "id": row.id,
                "github_user_id": github_user_id,
                "login": login,
                "name": row.name,
                "avatar_url": row.avatar_url,
                "email": row.email,
                "is_admin": row.is_admin != 0,
                "account_status": account_status,
                "paused_at": row.paused_at,
            },
            "access_sync": {"task_id": null, "task_type": null, "event_path": null, "reason": "none"},
            "dashboard": {"daily_boundary_local": "00:00", "daily_boundary_time_zone": null, "daily_boundary_utc_offset_minutes": 0, "include_own_releases": row.include_own_releases != 0}
        })),
    )
}

async fn me_resume(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let now = Utc::now();
    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            warn!(%error, "PostgreSQL account resume transaction failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            );
        }
    };
    let disabled = sqlx::query_scalar::<_, i64>(
        "SELECT is_disabled FROM public.users WHERE id = $1 FOR UPDATE",
    )
    .bind(&user_id)
    .fetch_optional(&mut *tx)
    .await;
    let Some(disabled) = (match disabled {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "PostgreSQL account resume lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            );
        }
    }) else {
        let _ = tx.rollback().await;
        let _ = session.clear().await;
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "session user not found"})),
        );
    };
    if disabled != 0 {
        let _ = tx.rollback().await;
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({"code": "account_disabled"})),
        );
    }
    if let Err(error) = sqlx::query(
        "UPDATE public.users SET paused_at = NULL, last_active_at = $1, updated_at = $1 WHERE id = $2",
    )
    .bind(now.to_rfc3339())
    .bind(&user_id)
    .execute(&mut *tx)
    .await
    {
        warn!(%error, "PostgreSQL account resume update failed");
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "database_error"})),
        );
    }
    let task_id = local_id::generate_local_id();
    let payload = json!({"user_id": user_id});
    if let Err(error) = sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at) VALUES ($1, 'sync.access_refresh', 'queued', 'api.me_resume', $2, $3, $4, $4)",
    )
    .bind(&task_id)
    .bind(&user_id)
    .bind(payload.to_string())
    .bind(now)
    .execute(&mut *tx)
    .await
    {
        warn!(%error, "PostgreSQL account resume task enqueue failed");
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "database_error"})),
        );
    }
    if let Err(error) = tx.commit().await {
        warn!(%error, "PostgreSQL account resume transaction commit failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "database_error"})),
        );
    }
    (
        StatusCode::OK,
        axum::Json(json!({
            "status": "enabled",
            "access_sync": {"task_id": task_id, "task_type": "sync.access_refresh", "event_path": null, "reason": "account_resumed"},
            "sync_enqueue_error": null
        })),
    )
}

#[derive(Debug, FromRow)]
struct PgProfileRow {
    daily_brief_schedule_local_time: String,
    daily_brief_time_zone: Option<String>,
    include_own_releases: i64,
    last_active_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PgProfilePatch {
    daily_brief_time_zone: String,
    include_own_releases: Option<bool>,
}

async fn me_profile(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    match sqlx::query_as::<_, PgProfileRow>(
        "SELECT COALESCE((SELECT daily_brief_schedule_local_time FROM public.admin_runtime_settings WHERE id=1), '06:00') AS daily_brief_schedule_local_time, daily_brief_time_zone, include_own_releases, last_active_at FROM public.users WHERE id = $1 AND is_disabled = 0 AND paused_at IS NULL",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    {
        Ok(Some(row)) => (
            StatusCode::OK,
            axum::Json(json!({"user_id": user_id, "daily_brief_schedule_local_time": row.daily_brief_schedule_local_time, "daily_brief_time_zone": row.daily_brief_time_zone.unwrap_or_else(|| "UTC".to_owned()), "include_own_releases": row.include_own_releases != 0, "last_active_at": row.last_active_at})),
        ).into_response(),
        Ok(None) => (StatusCode::UNAUTHORIZED, axum::Json(json!({"code": "unauthorized"}))).into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL profile query failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"}))).into_response()
        }
    }
}

async fn update_me_profile(
    State(state): State<PostgresServerState>,
    session: Session,
    axum::Json(request): axum::Json<PgProfilePatch>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE public.users SET daily_brief_time_zone = $1, include_own_releases = COALESCE($2, include_own_releases), updated_at = $3 WHERE id = $4 AND is_disabled = 0 AND paused_at IS NULL",
    )
    .bind(request.daily_brief_time_zone.trim())
    .bind(request.include_own_releases.map(|value| if value { 1_i64 } else { 0_i64 }))
    .bind(&now)
    .bind(&user_id)
    .execute(&state.runtime.pool)
    .await;
    match result {
        Ok(result) if result.rows_affected() == 1 => {
            me_profile(State(state), session).await.into_response()
        }
        Ok(_) => (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL profile update failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct PgCreateApiKeyRequest {
    name: Option<String>,
}

async fn list_api_keys(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    match sqlx::query(
        "SELECT id, name, masked_key, created_at, last_used_at FROM public.user_api_keys WHERE user_id = $1 AND revoked_at IS NULL ORDER BY created_at DESC, id DESC",
    )
    .bind(&user_id)
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(rows) => {
            use sqlx::Row;
            let items = rows
                .into_iter()
                .map(|row| json!({"id": row.get::<String, _>("id"), "name": row.get::<String, _>("name"), "masked_key": row.get::<String, _>("masked_key"), "created_at": row.get::<String, _>("created_at"), "last_used_at": row.get::<Option<String>, _>("last_used_at")}))
                .collect::<Vec<_>>();
            (StatusCode::OK, axum::Json(json!({"items": items})))
        }
        Err(error) => {
            warn!(%error, "PostgreSQL API key listing failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"})))
        }
    }
}

async fn create_api_key(
    State(state): State<PostgresServerState>,
    session: Session,
    axum::Json(request): axum::Json<PgCreateApiKeyRequest>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    let name = match crate::api_keys::normalize_name(request.name.as_deref()) {
        Ok(name) => name,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"code": "invalid_name", "message": error.to_string()})),
            )
                .into_response();
        }
    };
    let plaintext = crate::api_keys::generate_api_key_plaintext();
    let encrypted = match state.encryption_key.encrypt_str(&plaintext) {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "PostgreSQL API key encryption failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "encryption_failed"})),
            )
                .into_response();
        }
    };
    let id = local_id::generate_local_id();
    let now = Utc::now().to_rfc3339();
    let hash = crate::api_keys::hash_api_key(&plaintext);
    let prefix = crate::api_keys::key_prefix(&plaintext);
    let masked = crate::api_keys::mask_api_key(&plaintext);
    match sqlx::query(
        "INSERT INTO public.user_api_keys (id, user_id, name, key_hash, key_ciphertext, key_nonce, key_prefix, masked_key, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(&id)
    .bind(&user_id)
    .bind(&name)
    .bind(&hash)
    .bind(encrypted.ciphertext)
    .bind(encrypted.nonce)
    .bind(&prefix)
    .bind(&masked)
    .bind(&now)
    .execute(&state.runtime.pool)
    .await
    {
        Ok(_) => (StatusCode::CREATED, axum::Json(json!({"item": {"id": id, "name": name, "masked_key": masked, "created_at": now, "last_used_at": null}, "api_key": plaintext}))).into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL API key insert failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"}))).into_response()
        }
    }
}

async fn delete_api_key(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(api_key_id): Path<String>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    match sqlx::query("UPDATE public.user_api_keys SET revoked_at = $1 WHERE id = $2 AND user_id = $3 AND revoked_at IS NULL")
        .bind(Utc::now().to_rfc3339())
        .bind(&api_key_id)
        .bind(&user_id)
        .execute(&state.runtime.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 1 => (StatusCode::NO_CONTENT, axum::Json(json!({}))).into_response(),
        Ok(_) => (StatusCode::NOT_FOUND, axum::Json(json!({"code": "not_found"}))).into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL API key revoke failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"}))).into_response()
        }
    }
}

async fn auth_bind_context(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let pending_linuxdo = session
        .get::<serde_json::Value>("pending_linuxdo")
        .await
        .ok()
        .flatten();
    let pending_passkey = session
        .get::<serde_json::Value>("pending_passkey_credential")
        .await
        .ok()
        .flatten();
    (
        StatusCode::OK,
        axum::Json(json!({
            "linuxdo_available": state.linuxdo_available,
            "pending_linuxdo": pending_linuxdo,
            "pending_passkey": pending_passkey
        })),
    )
}

#[derive(Debug, FromRow, serde::Serialize)]
struct PgStarredRepoItem {
    repo_id: i64,
    full_name: String,
    description: Option<String>,
    html_url: String,
    stargazed_at: Option<String>,
    is_private: i64,
}

async fn list_starred(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    match sqlx::query_as::<_, PgStarredRepoItem>(
        "SELECT repo_id, full_name, description, html_url, stargazed_at, is_private FROM public.starred_repos WHERE user_id = $1 ORDER BY stargazed_at DESC LIMIT 2000",
    )
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(items) => (StatusCode::OK, axum::Json(json!(items))),
        Err(error) => {
            warn!(%error, "PostgreSQL starred repositories query failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"})))
        }
    }
}

#[derive(Debug, FromRow, serde::Serialize)]
struct PgReleaseItem {
    full_name: String,
    tag_name: String,
    name: Option<String>,
    published_at: Option<String>,
    html_url: String,
    is_prerelease: i64,
    is_draft: i64,
}

async fn list_releases(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    match sqlx::query_as::<_, PgReleaseItem>(
        "SELECT sr.full_name, r.tag_name, r.name, r.published_at, r.html_url, r.is_prerelease, r.is_draft FROM public.repo_releases r JOIN public.user_release_visible_repos sr ON sr.user_id = $1 AND sr.repo_id = r.repo_id ORDER BY COALESCE(r.published_at, r.created_at) DESC LIMIT 200",
    )
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(items) => (StatusCode::OK, axum::Json(json!(items))),
        Err(error) => {
            warn!(%error, "PostgreSQL releases query failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"})))
        }
    }
}

/// Translation payloads for the release-detail endpoint.
///
/// Global content processing is authoritative when enabled; otherwise the
/// legacy per-user `ai_translations` cache is served. Both modes return the
/// same item shape so clients keep one contract.
#[allow(clippy::too_many_arguments)]
async fn release_detail_processing_items(
    state: &PostgresServerState,
    user_id: &str,
    release_id: i64,
    repo_id: i64,
    html_url: &str,
    tag_name: &str,
    name: Option<&str>,
    body: Option<&str>,
) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    let overlay = crate::postgres_content_overlay::OverlayState::load(&state.runtime.pool).await;
    if overlay.is_global() {
        // Projection identities are keyed by the writer's repository identity,
        // which is derived from the release URL (and falls back to
        // `unknown/<repo_id>` for tags containing `/`). The visible-repository
        // name is not that identity, so it cannot be used here.
        let repo_identity =
            crate::postgres_content_overlay::release_repo_identity(html_url, repo_id);
        let source = crate::postgres_content_overlay::ReleaseSource {
            release_id,
            repo_full_name: repo_identity.as_str(),
            repo_identity: repo_identity.as_str(),
            tag_name,
            name,
            body,
            previous_tag_name: None,
        };
        let audience = crate::postgres_content_overlay::release_audience(
            &state.runtime.pool,
            user_id,
            repo_id,
        )
        .await
        .unwrap_or(crate::postgres_content_overlay::ReleaseAudience::Authorized);
        let release_key = release_id.to_string();
        let translation_request_id =
            crate::postgres_content_overlay::latest_translation_request_id(
                &state.runtime.pool,
                release_key.as_str(),
                &crate::postgres_content_overlay::RELEASE_TRANSLATION_REQUEST_KINDS,
                None,
            )
            .await
            .unwrap_or_default();
        let smart_request_id = crate::postgres_content_overlay::latest_translation_request_id(
            &state.runtime.pool,
            release_key.as_str(),
            &crate::postgres_content_overlay::RELEASE_SMART_REQUEST_KINDS,
            None,
        )
        .await
        .unwrap_or_default();
        return match crate::postgres_content_overlay::release_detail_processing(
            &state.runtime.pool,
            overlay,
            &source,
            audience,
        )
        .await
        {
            Ok(processing) => (
                processing.translation.map(|projection| {
                    translation_projection_item(&projection, false, translation_request_id)
                }),
                processing.smart.map(|projection| {
                    translation_projection_item(&projection, true, smart_request_id)
                }),
            ),
            Err(error) => {
                warn!(%error, release_id, "PostgreSQL global release overlay failed");
                (None, None)
            }
        };
    }
    detail_translation_items(&state.runtime.pool, user_id, release_id).await
}

fn translation_projection_item(
    projection: &crate::postgres_content_overlay::ContentProjection,
    smart: bool,
    request_id: Option<String>,
) -> serde_json::Value {
    let title = projection.title();
    let summary = if smart {
        projection.smart_summary()
    } else {
        projection.translation_summary()
    };
    let error = if projection.status == "ready" {
        None
    } else {
        projection
            .payload
            .get("error_text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let mut item = json!({
        "lang": "zh-CN",
        "status": projection.status,
        "title": title,
        "summary": summary,
        "error_code": error.as_deref(),
        "error_summary": error.as_deref(),
        "error_detail": null
    });
    crate::postgres_repo_releases::apply_translation_request_metadata(
        &mut item,
        request_id,
        crate::postgres_content_overlay::is_active_work_status(projection.status.as_str()),
    );
    item
}

/// Legacy-mode translation cache for one release.
async fn detail_translation_items(
    pool: &sqlx::PgPool,
    user_id: &str,
    release_id: i64,
) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    #[derive(sqlx::FromRow)]
    struct Row {
        entity_type: String,
        lang: String,
        status: String,
        title: Option<String>,
        summary: Option<String>,
        error_text: Option<String>,
        work_status: Option<String>,
    }
    let rows = sqlx::query_as::<_, Row>(
        "SELECT t.entity_type, t.lang, t.status, t.title, t.summary, t.error_text, \
                w.status AS work_status \
         FROM public.ai_translations t \
         LEFT JOIN public.translation_work_items w ON w.id = t.active_work_item_id \
         WHERE t.user_id = $1 AND t.entity_id = $2 \
           AND t.entity_type IN ('release_detail', 'release_smart')",
    )
    .bind(user_id)
    .bind(release_id.to_string())
    .fetch_all(pool)
    .await;
    let Ok(rows) = rows else {
        return (None, None);
    };
    let mut translated = None;
    let mut smart = None;
    let mut translated_in_flight = false;
    let mut smart_in_flight = false;
    for row in rows {
        let in_flight = matches!(row.status.as_str(), "queued" | "running")
            || matches!(
                row.work_status.as_deref(),
                Some("queued" | "batched" | "running")
            );
        let mut item = if row.status == "ready" {
            crate::postgres_repo_releases::translated_item_json(
                "ready",
                row.title,
                row.summary,
                None,
                None,
                None,
            )
        } else if in_flight {
            crate::postgres_repo_releases::translated_item_json(
                "missing",
                None,
                None,
                None,
                row.error_text.as_deref(),
                None,
            )
        } else {
            continue;
        };
        item["lang"] = serde_json::json!(row.lang);
        match row.entity_type.as_str() {
            "release_detail" => {
                translated_in_flight |= in_flight;
                translated = Some(item);
            }
            "release_smart" => {
                smart_in_flight |= in_flight;
                smart = Some(item);
            }
            _ => {}
        }
    }
    let release_key = release_id.to_string();
    let translation_request_id = crate::postgres_content_overlay::latest_translation_request_id(
        pool,
        release_key.as_str(),
        &crate::postgres_content_overlay::RELEASE_TRANSLATION_REQUEST_KINDS,
        Some(user_id),
    )
    .await
    .unwrap_or_default();
    let smart_request_id = crate::postgres_content_overlay::latest_translation_request_id(
        pool,
        release_key.as_str(),
        &crate::postgres_content_overlay::RELEASE_SMART_REQUEST_KINDS,
        Some(user_id),
    )
    .await
    .unwrap_or_default();
    if let Some(item) = translated.as_mut() {
        crate::postgres_repo_releases::apply_translation_request_metadata(
            item,
            translation_request_id,
            translated_in_flight,
        );
    }
    if let Some(item) = smart.as_mut() {
        crate::postgres_repo_releases::apply_translation_request_metadata(
            item,
            smart_request_id,
            smart_in_flight,
        );
    }
    (translated, smart)
}

async fn release_detail(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(release_id): Path<i64>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    let row = sqlx::query(
        "SELECT r.release_id, r.repo_id, r.tag_name, r.name, r.body, r.html_url, r.published_at, r.is_prerelease, r.is_draft, r.created_at, r.updated_at, sr.full_name FROM public.repo_releases r JOIN public.user_release_visible_repos sr ON sr.user_id = $1 AND sr.repo_id = r.repo_id WHERE r.release_id = $2 LIMIT 1",
    )
    .bind(&user_id)
    .bind(release_id)
    .fetch_optional(&state.runtime.pool)
    .await;
    match row {
        Ok(Some(row)) => {
            use sqlx::Row;
            let release_id_value = row.get::<i64, _>("release_id");
            let repo_id = row.get::<i64, _>("repo_id");
            let full_name = row.get::<String, _>("full_name");
            let tag_name = row.get::<String, _>("tag_name");
            let name = row.get::<Option<String>, _>("name");
            let body = row.get::<Option<String>, _>("body");
            let (translated, smart) = release_detail_processing_items(
                &state,
                &user_id,
                release_id_value,
                repo_id,
                row.get::<String, _>("html_url").as_str(),
                tag_name.as_str(),
                name.as_deref(),
                body.as_deref(),
            )
            .await;
            (
                StatusCode::OK,
                axum::Json(json!({
                    "release_id": release_id_value.to_string(),
                    "repo_full_name": full_name,
                    "tag_name": tag_name,
                    "name": name,
                    "body": body,
                    "html_url": row.get::<String, _>("html_url"),
                    "published_at": row.get::<Option<String>, _>("published_at"),
                    "is_prerelease": row.get::<i64, _>("is_prerelease") != 0,
                    "is_draft": row.get::<i64, _>("is_draft") != 0,
                    "translated": translated,
                    "smart": smart
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "not_found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL release detail query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, FromRow, serde::Serialize)]
struct PgNotificationItem {
    thread_id: String,
    repo_full_name: Option<String>,
    subject_title: Option<String>,
    subject_type: Option<String>,
    reason: Option<String>,
    updated_at: Option<String>,
    unread: i64,
    html_url: Option<String>,
}

async fn list_notifications(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    match sqlx::query_as::<_, PgNotificationItem>(
        "SELECT thread_id, repo_full_name, subject_title, subject_type, reason, updated_at, unread, html_url FROM public.notifications WHERE user_id = $1 ORDER BY updated_at DESC LIMIT 200",
    )
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(items) => (StatusCode::OK, axum::Json(json!(items))),
        Err(error) => {
            warn!(%error, "PostgreSQL notifications query failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"})))
        }
    }
}

#[derive(Debug, FromRow)]
struct PgPersonalRepoRow {
    repo_id: i64,
    full_name: String,
    owner_login: String,
    name: String,
    html_url: String,
    updated_at: Option<String>,
    release_count: i64,
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: Option<i64>,
}

async fn personal_repos(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let owner_login = match sqlx::query_scalar::<_, String>(
        "SELECT login FROM public.users WHERE id = $1 AND is_disabled = 0",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    {
        Ok(Some(login)) => login,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"code": "unauthorized"})),
            );
        }
        Err(error) => {
            warn!(%error, "PostgreSQL personal repository owner lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            );
        }
    };
    let rows = match sqlx::query_as::<_, PgPersonalRepoRow>(
        "SELECT ob.repo_id, ob.repo_full_name AS full_name, split_part(ob.repo_full_name, '/', 1) AS owner_login, split_part(ob.repo_full_name, '/', 2) AS name, 'https://github.com/' || ob.repo_full_name AS html_url, ob.updated_at, COALESCE(rc.release_count, 0) AS release_count, ob.owner_avatar_url, ob.open_graph_image_url, ob.uses_custom_open_graph_image FROM public.owned_repo_star_baselines ob LEFT JOIN (SELECT repo_id, COUNT(*) AS release_count FROM public.repo_releases WHERE COALESCE(is_draft, 0)=0 GROUP BY repo_id) rc ON rc.repo_id=ob.repo_id WHERE ob.user_id = $1 AND position('/' in ob.repo_full_name) > 1 AND lower(split_part(ob.repo_full_name, '/', 1)) = lower($2) ORDER BY lower(ob.repo_full_name) ASC",
    )
    .bind(&user_id)
    .bind(&owner_login)
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            warn!(%error, "PostgreSQL personal repositories query failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"})));
        }
    };
    let repos = rows
        .into_iter()
        .map(|row| {
            json!({
                "repo_id": row.repo_id,
                "full_name": row.full_name,
                "owner_login": row.owner_login,
                "name": row.name,
                "html_url": row.html_url,
                "updated_at": row.updated_at,
                "release_count": row.release_count,
                "repo_visual": {
                    "owner_avatar_url": row.owner_avatar_url,
                    "open_graph_image_url": row.open_graph_image_url,
                    "uses_custom_open_graph_image": row.uses_custom_open_graph_image.unwrap_or_default() != 0
                }
            })
        })
        .collect::<Vec<_>>();
    (
        StatusCode::OK,
        axum::Json(json!({"owner_login": owner_login, "total_count": repos.len(), "repos": repos})),
    )
}

#[derive(Debug, FromRow)]
struct PgFollowingRepoRow {
    repo_id: i64,
    full_name: String,
    owner_login: String,
    name: String,
    html_url: String,
    description: Option<String>,
    is_private: Option<i64>,
    first_source: Option<String>,
    first_associated_at: Option<String>,
    last_seen_at: Option<String>,
    is_following: i64,
    follow_state_source: Option<String>,
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: Option<i64>,
    has_personal_owned_source: i64,
    has_github_star_source: i64,
    has_manual_feed_source: i64,
}

async fn list_following_repos(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let rows = match sqlx::query_as::<_, PgFollowingRepoRow>(
        "SELECT repo_id, repo_full_name AS full_name, owner_login, repo_name AS name, COALESCE(html_url, 'https://github.com/' || repo_full_name) AS html_url, description, is_private, first_source, first_associated_at, last_seen_at, is_following, follow_state_source, owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image, has_personal_owned_source, has_github_star_source, has_manual_feed_source FROM public.user_repo_associations WHERE user_id = $1 ORDER BY is_following DESC, lower(repo_full_name) ASC",
    )
    .bind(&user_id)
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            warn!(%error, "PostgreSQL following repositories query failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"})));
        }
    };
    let associated_items = rows
        .into_iter()
        .map(|row| {
            json!({
                "repo_id": row.repo_id,
                "full_name": row.full_name,
                "owner_login": row.owner_login,
                "name": row.name,
                "html_url": row.html_url,
                "description": row.description,
                "is_private": row.is_private.map(|v| v != 0),
                "first_source": row.first_source,
                "first_associated_at": row.first_associated_at,
                "last_seen_at": row.last_seen_at,
                "is_following": row.is_following != 0,
                "follow_state_source": row.follow_state_source,
                "repo_visual": {"owner_avatar_url": row.owner_avatar_url, "open_graph_image_url": row.open_graph_image_url, "uses_custom_open_graph_image": row.uses_custom_open_graph_image.unwrap_or_default() != 0},
                "sources": {"personal_owned": row.has_personal_owned_source != 0, "github_star": row.has_github_star_source != 0, "manual_feed": row.has_manual_feed_source != 0}
            })
        })
        .collect::<Vec<_>>();
    let items = associated_items
        .iter()
        .filter(|item| {
            item.get("is_following")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    (
        StatusCode::OK,
        axum::Json(
            json!({"following_count": items.len(), "associated_count": associated_items.len(), "items": items, "associated_items": associated_items}),
        ),
    )
}

async fn public_metrics(State(state): State<PostgresServerState>) -> impl IntoResponse {
    let repositories = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(DISTINCT repo_full_name) FROM public.search_documents WHERE repo_full_name IS NOT NULL",
    )
    .fetch_one(&state.runtime.pool)
    .await;
    match repositories {
        Ok(value) => (
            StatusCode::OK,
            axum::Json(json!({
                "deduplicatedRepositories": {"value": value, "trend": []},
                "pressure": {"value": 0.0, "trend": []},
                "freshness": []
            })),
        ),
        Err(error) => {
            warn!(%error, "PostgreSQL public metrics query failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(json!({"error": "metrics_unavailable"})),
            )
        }
    }
}

#[derive(Debug, FromRow)]
struct PgBriefRow {
    id: String,
    date: String,
    window_start_utc: Option<String>,
    window_end_utc: Option<String>,
    effective_time_zone: Option<String>,
    effective_local_boundary: Option<String>,
    content_markdown: String,
    created_at: String,
    updated_at: String,
}

async fn brief_json(
    state: &PostgresServerState,
    row: PgBriefRow,
) -> Result<serde_json::Value, sqlx::Error> {
    let release_ids = sqlx::query_scalar::<_, i64>(
        "SELECT release_id FROM public.brief_release_memberships WHERE brief_id = $1 ORDER BY ordinal ASC",
    )
    .bind(&row.id)
    .fetch_all(&state.runtime.pool)
    .await?
    .into_iter()
    .map(|id| id.to_string())
    .collect::<Vec<_>>();
    let release_count = release_ids.len();
    let preview: String = row.content_markdown.chars().take(600).collect();
    Ok(json!({
        "id": row.id,
        "date": row.date,
        "window_start": row.window_start_utc,
        "window_end": row.window_end_utc,
        "effective_time_zone": row.effective_time_zone,
        "effective_local_boundary": row.effective_local_boundary,
        "release_count": release_count,
        "release_ids": release_ids,
        "preview_markdown": preview,
        "covers_repo_stars": row.content_markdown.contains("### 获星"),
        "covers_followers": row.content_markdown.contains("### 关注"),
        "content_markdown": row.content_markdown,
        "created_at": row.created_at,
        "updated_at": row.updated_at
    }))
}

async fn list_briefs(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let rows = sqlx::query_as::<_, PgBriefRow>(
        "SELECT id, date, window_start_utc, window_end_utc, effective_time_zone, effective_local_boundary, content_markdown, created_at, updated_at FROM public.briefs WHERE user_id = $1 ORDER BY COALESCE(window_end_utc::text, created_at::text) DESC, created_at DESC, id DESC",
    )
    .bind(&user_id)
    .fetch_all(&state.runtime.pool)
    .await;
    match rows {
        Ok(rows) => {
            let mut items = Vec::with_capacity(rows.len());
            for row in rows {
                match brief_json(&state, row).await {
                    Ok(item) => items.push(item),
                    Err(error) => {
                        warn!(%error, "PostgreSQL brief membership query failed");
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(json!({"code": "database_error"})),
                        );
                    }
                }
            }
            (StatusCode::OK, axum::Json(json!(items)))
        }
        Err(error) => {
            warn!(%error, "PostgreSQL briefs query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
        }
    }
}

async fn get_brief(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(brief_id): Path<String>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    match sqlx::query_as::<_, PgBriefRow>(
        "SELECT id, date, window_start_utc, window_end_utc, effective_time_zone, effective_local_boundary, content_markdown, created_at, updated_at FROM public.briefs WHERE user_id = $1 AND id = $2",
    )
    .bind(&user_id)
    .bind(&brief_id)
    .fetch_optional(&state.runtime.pool)
    .await
    {
        Ok(Some(row)) => match brief_json(&state, row).await {
            Ok(item) => (StatusCode::OK, axum::Json(item)).into_response(),
            Err(error) => {
                warn!(%error, "PostgreSQL brief membership query failed");
                (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"}))).into_response()
            }
        },
        Ok(None) => (StatusCode::NOT_FOUND, axum::Json(json!({"code": "not_found"}))).into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL brief query failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"}))).into_response()
        }
    }
}

async fn generate_brief(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let task_id = local_id::generate_local_id();
    let now = Utc::now();
    let payload = json!({"user_id": user_id});
    match sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at) VALUES ($1, 'brief.generate', 'queued', 'api.briefs.generate', $2, $3, $4, $4)",
    )
    .bind(&task_id)
    .bind(&user_id)
    .bind(payload.to_string())
    .bind(now)
    .execute(&state.runtime.pool)
    .await
    {
        Ok(_) => (
            StatusCode::ACCEPTED,
            axum::Json(json!({"mode": "task_id", "task_id": task_id, "task_type": "brief.generate", "status": "queued"})),
        ),
        Err(error) => {
            warn!(%error, "PostgreSQL brief task enqueue failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"})))
        }
    }
}

async fn enqueue_sync_task(
    state: &PostgresServerState,
    session: &Session,
    task_type: &str,
    source: &str,
) -> axum::response::Response {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    let task_id = local_id::generate_local_id();
    let now = Utc::now();
    let payload = json!({"user_id": user_id});
    match sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at) VALUES ($1, $2, 'queued', $3, $4, $5, $6, $6)",
    )
    .bind(&task_id)
    .bind(task_type)
    .bind(source)
    .bind(&user_id)
    .bind(payload.to_string())
    .bind(now)
    .execute(&state.runtime.pool)
    .await
    {
        Ok(_) => (
            StatusCode::ACCEPTED,
            axum::Json(json!({"mode": "task_id", "task_id": task_id, "task_type": task_type, "status": "queued"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, task_type, "PostgreSQL sync task enqueue failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, FromRow)]
struct PgTaskStatusRow {
    id: String,
    task_type: String,
    status: String,
    source: String,
    requested_by: Option<String>,
    payload_json: String,
    result_json: Option<String>,
    error_message: Option<String>,
    cancel_requested: i64,
    created_at: chrono::DateTime<Utc>,
    started_at: Option<chrono::DateTime<Utc>>,
    finished_at: Option<chrono::DateTime<Utc>>,
}

async fn task_status(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    match sqlx::query_as::<_, PgTaskStatusRow>(
        "SELECT id, task_type, status, source, requested_by, payload_json, result_json, error_message, cancel_requested, created_at, started_at, finished_at FROM public.job_tasks WHERE id = $1 AND requested_by = $2",
    )
    .bind(&task_id)
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    {
        Ok(Some(row)) => (
            StatusCode::OK,
            axum::Json(json!({"id": row.id, "task_id": row.id, "task_type": row.task_type, "status": row.status, "source": row.source, "requested_by": row.requested_by, "payload": serde_json::from_str::<serde_json::Value>(&row.payload_json).unwrap_or(json!({})), "result": row.result_json.and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok()), "error": row.error_message, "cancel_requested": row.cancel_requested != 0, "created_at": row.created_at, "started_at": row.started_at, "finished_at": row.finished_at})),
        ).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, axum::Json(json!({"code": "not_found"}))).into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL task status query failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"}))).into_response()
        }
    }
}

async fn cancel_task(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    match postgres_jobs::request_cancel_task(
        &state.runtime.pool,
        &task_id,
        Some(&user_id),
        Utc::now(),
    )
    .await
    {
        Ok(true) => (
            StatusCode::OK,
            axum::Json(json!({"task_id": task_id, "cancel_requested": true})),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "not_found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL task cancellation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

pub(super) async fn require_admin(
    state: &PostgresServerState,
    session: &Session,
) -> Result<String, StatusCode> {
    let user_id = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let is_admin = sqlx::query_scalar::<_, i64>(
        "SELECT is_admin FROM public.users WHERE id = $1 AND is_disabled = 0",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;
    if is_admin == 0 {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(user_id)
}

#[derive(Debug, Deserialize)]
struct AdminUsersQuery {
    page: Option<u32>,
    page_size: Option<u32>,
}

async fn admin_users(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<AdminUsersQuery>,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    let page_size = query.page_size.unwrap_or(50).clamp(1, 100) as i64;
    let page = query.page.unwrap_or(1).max(1) as i64;
    let offset = (page - 1).saturating_mul(page_size);
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM public.users")
        .fetch_one(&state.runtime.pool)
        .await;
    let rows = sqlx::query(
        "SELECT id, github_user_id, login, name, email, is_admin, is_disabled, paused_at, created_at, updated_at \
         FROM public.users ORDER BY created_at DESC, id DESC LIMIT $1 OFFSET $2",
    )
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.runtime.pool)
    .await;
    match (total, rows) {
        (Ok(total), Ok(rows)) => {
            use sqlx::Row;
            let items = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "id": row.get::<String, _>("id"),
                        "github_user_id": row.get::<Option<i64>, _>("github_user_id"),
                        "login": row.get::<Option<String>, _>("login"),
                        "name": row.get::<Option<String>, _>("name"),
                        "email": row.get::<Option<String>, _>("email"),
                        "is_admin": row.get::<i64, _>("is_admin") != 0,
                        "is_disabled": row.get::<i64, _>("is_disabled") != 0,
                        "paused_at": row.get::<Option<String>, _>("paused_at"),
                        "created_at": row.get::<String, _>("created_at"),
                        "updated_at": row.get::<String, _>("updated_at"),
                    })
                })
                .collect::<Vec<_>>();
            (
                StatusCode::OK,
                axum::Json(
                    json!({"items": items, "page": page, "page_size": page_size, "total": total}),
                ),
            )
                .into_response()
        }
        (Err(error), _) | (_, Err(error)) => {
            warn!(%error, "PostgreSQL admin users query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_user_detail(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    match sqlx::query(
        "SELECT id, github_user_id, login, name, email, is_admin, is_disabled, paused_at, created_at, updated_at, last_active_at, include_own_releases \
         FROM public.users WHERE id = $1",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    {
        Ok(Some(row)) => {
            use sqlx::Row;
            (
                StatusCode::OK,
                axum::Json(json!({
                    "id": row.get::<String, _>("id"),
                    "github_user_id": row.get::<Option<i64>, _>("github_user_id"),
                    "login": row.get::<Option<String>, _>("login"),
                    "name": row.get::<Option<String>, _>("name"),
                    "email": row.get::<Option<String>, _>("email"),
                    "is_admin": row.get::<i64, _>("is_admin") != 0,
                    "is_disabled": row.get::<i64, _>("is_disabled") != 0,
                    "paused_at": row.get::<Option<String>, _>("paused_at"),
                    "created_at": row.get::<String, _>("created_at"),
                    "updated_at": row.get::<String, _>("updated_at"),
                    "last_active_at": row.get::<Option<String>, _>("last_active_at"),
                    "include_own_releases": row.get::<i64, _>("include_own_releases") != 0,
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "user_not_found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL admin user detail query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdminUserPatch {
    is_admin: Option<bool>,
    is_disabled: Option<bool>,
}

async fn admin_patch_user(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(target_user_id): Path<String>,
    axum::Json(patch): axum::Json<AdminUserPatch>,
) -> impl IntoResponse {
    let Ok(acting_user_id) = require_admin(&state, &session).await else {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({"code": "admin_required"})),
        )
            .into_response();
    };
    if patch.is_admin.is_none() && patch.is_disabled.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"code": "empty_patch"})),
        )
            .into_response();
    }
    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            warn!(%error, "PostgreSQL admin user patch transaction failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response();
        }
    };
    let target = sqlx::query(
        "SELECT is_admin, is_disabled, paused_at FROM public.users WHERE id = $1 FOR UPDATE",
    )
    .bind(&target_user_id)
    .fetch_optional(&mut *tx)
    .await;
    let Some(target) = (match target {
        Ok(row) => row,
        Err(error) => {
            let _ = tx.rollback().await;
            warn!(%error, "PostgreSQL admin user lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response();
        }
    }) else {
        let _ = tx.rollback().await;
        return (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "user_not_found"})),
        )
            .into_response();
    };
    use sqlx::Row;
    let current_admin = target.get::<i64, _>("is_admin") != 0;
    let current_disabled = target.get::<i64, _>("is_disabled") != 0;
    let paused = target.get::<Option<String>, _>("paused_at").is_some();
    let next_admin = patch.is_admin.unwrap_or(current_admin);
    let next_disabled = patch.is_disabled.unwrap_or(current_disabled);
    let admin_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM public.users WHERE is_admin = 1")
            .fetch_one(&mut *tx)
            .await;
    let active_admin_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.users WHERE is_admin = 1 AND is_disabled = 0 AND paused_at IS NULL",
    )
    .fetch_one(&mut *tx)
    .await;
    let (Ok(admin_count), Ok(active_admin_count)) = (admin_count, active_admin_count) else {
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "database_error"})),
        )
            .into_response();
    };
    if acting_user_id == target_user_id && next_disabled {
        let _ = tx.rollback().await;
        return (
            StatusCode::CONFLICT,
            axum::Json(json!({"code": "cannot_disable_self"})),
        )
            .into_response();
    }
    if current_admin && !next_admin && admin_count <= 1
        || current_admin
            && !current_disabled
            && !paused
            && (!next_admin || next_disabled)
            && active_admin_count <= 1
    {
        let _ = tx.rollback().await;
        return (
            StatusCode::CONFLICT,
            axum::Json(json!({"code": "last_admin_guard"})),
        )
            .into_response();
    }
    let now = Utc::now();
    match sqlx::query(
        "UPDATE public.users SET is_admin = $1, is_disabled = $2, updated_at = $3 WHERE id = $4",
    )
    .bind(if next_admin { 1_i64 } else { 0 })
    .bind(if next_disabled { 1_i64 } else { 0 })
    .bind(now.to_rfc3339())
    .bind(&target_user_id)
    .execute(&mut *tx)
    .await
    {
        Ok(_) => match tx.commit().await {
            Ok(()) => (
                StatusCode::OK,
                axum::Json(json!({"id": target_user_id, "is_admin": next_admin, "is_disabled": next_disabled})),
            )
                .into_response(),
            Err(error) => {
                warn!(%error, "PostgreSQL admin user patch commit failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"code": "database_error"})),
                )
                    .into_response()
            }
        },
        Err(error) => {
            let _ = tx.rollback().await;
            warn!(%error, "PostgreSQL admin user update failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

pub(crate) async fn admin_user_profile(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    match sqlx::query(
        "SELECT id, COALESCE((SELECT daily_brief_schedule_local_time FROM public.admin_runtime_settings WHERE id=1), '06:00') AS daily_brief_schedule_local_time, daily_brief_time_zone, include_own_releases, last_active_at, webhook_push_enabled, webhook_push_desired_state \
         FROM public.users WHERE id = $1",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    {
        Ok(Some(row)) => {
            use sqlx::Row;
            (
                StatusCode::OK,
                axum::Json(json!({
                    "user_id": row.get::<String, _>("id"),
                    "daily_brief_schedule_local_time": row.get::<String, _>("daily_brief_schedule_local_time"),
                    "daily_brief_time_zone": row.get::<Option<String>, _>("daily_brief_time_zone"),
                    "include_own_releases": row.get::<i64, _>("include_own_releases") != 0,
                    "last_active_at": row.get::<Option<String>, _>("last_active_at"),
                    "webhook_push_enabled": row.get::<i64, _>("webhook_push_enabled") != 0,
                    "webhook_push_desired_state": row.get::<String, _>("webhook_push_desired_state"),
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "user_not_found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, user_id = %user_id, "PostgreSQL admin user profile query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_cancel_task(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    match postgres_jobs::request_cancel_task(&state.runtime.pool, &task_id, None, Utc::now()).await
    {
        Ok(true) => (
            StatusCode::OK,
            axum::Json(json!({"task_id": task_id, "cancel_requested": true})),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "task_not_found_or_terminal"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, task_id = %task_id, "PostgreSQL admin task cancellation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdminScheduledSlotPatch {
    enabled: bool,
}

async fn admin_scheduled_slots(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    match sqlx::query(
        "SELECT hour_utc, enabled, last_dispatch_at, updated_at FROM public.daily_brief_hour_slots ORDER BY hour_utc ASC",
    )
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(rows) => {
            use sqlx::Row;
            let items = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "hour_utc": row.get::<i64, _>("hour_utc"),
                        "enabled": row.get::<i64, _>("enabled") != 0,
                        "last_dispatch_at": row.get::<Option<String>, _>("last_dispatch_at"),
                        "updated_at": row.get::<String, _>("updated_at"),
                    })
                })
                .collect::<Vec<_>>();
            (StatusCode::OK, axum::Json(json!({"items": items}))).into_response()
        }
        Err(error) => {
            warn!(%error, "PostgreSQL scheduled slot query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_patch_scheduled_slot(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(hour_utc): Path<i64>,
    axum::Json(patch): axum::Json<AdminScheduledSlotPatch>,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    if !(0..=23).contains(&hour_utc) {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"code": "invalid_hour_utc", "message": "hour_utc must be 0..23"})),
        )
            .into_response();
    }
    let updated_at = Utc::now().to_rfc3339();
    let updated = sqlx::query(
        "UPDATE public.daily_brief_hour_slots SET enabled = $1, updated_at = $2 WHERE hour_utc = $3",
    )
    .bind(if patch.enabled { 1_i64 } else { 0_i64 })
    .bind(&updated_at)
    .bind(hour_utc)
    .execute(&state.runtime.pool)
    .await;
    match updated {
        Ok(result) if result.rows_affected() == 1 => match sqlx::query(
            "SELECT hour_utc, enabled, last_dispatch_at, updated_at FROM public.daily_brief_hour_slots WHERE hour_utc = $1",
        )
        .bind(hour_utc)
        .fetch_one(&state.runtime.pool)
        .await
        {
            Ok(row) => {
                use sqlx::Row;
                (
                    StatusCode::OK,
                    axum::Json(json!({
                        "hour_utc": row.get::<i64, _>("hour_utc"),
                        "enabled": row.get::<i64, _>("enabled") != 0,
                        "last_dispatch_at": row.get::<Option<String>, _>("last_dispatch_at"),
                        "updated_at": row.get::<String, _>("updated_at"),
                    })),
                )
                    .into_response()
            }
            Err(error) => {
                warn!(%error, hour_utc, "PostgreSQL scheduled slot reload failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"code": "database_error"})),
                )
                    .into_response()
            }
        },
        Ok(_) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "scheduled_slot_not_found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, hour_utc, "PostgreSQL scheduled slot update failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_retry_task(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    match postgres_jobs::retry_task(&state.runtime.pool, &task_id, Utc::now()).await {
        Ok(true) => (
            StatusCode::ACCEPTED,
            axum::Json(json!({"task_id": task_id, "status": "queued"})),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::CONFLICT,
            axum::Json(json!({"code": "task_not_retryable"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, task_id = %task_id, "PostgreSQL admin task retry update failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn sync_starred(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    enqueue_sync_task(&state, &session, "sync.starred", "api.sync.starred").await
}

async fn sync_all(State(state): State<PostgresServerState>, session: Session) -> impl IntoResponse {
    enqueue_sync_task(&state, &session, "sync.all", "api.sync.all").await
}

async fn sync_releases(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    enqueue_sync_task(&state, &session, "sync.releases", "api.sync.releases").await
}

async fn sync_notifications(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    enqueue_sync_task(
        &state,
        &session,
        "sync.notifications",
        "api.sync.notifications",
    )
    .await
}

async fn follow_repo(
    State(state): State<PostgresServerState>,
    session: Session,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    set_following(&state, &session, &owner, &repo, true).await
}

async fn unfollow_repo(
    State(state): State<PostgresServerState>,
    session: Session,
    Path((owner, repo)): Path<(String, String)>,
) -> impl IntoResponse {
    set_following(&state, &session, &owner, &repo, false).await
}

async fn set_following(
    state: &PostgresServerState,
    session: &Session,
    owner: &str,
    repo: &str,
    following: bool,
) -> axum::response::Response {
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        )
            .into_response();
    };
    let full_name = format!("{owner}/{repo}");
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE public.user_repo_associations SET is_following = $1, follow_state_source = 'user_explicit', updated_at = $2 WHERE user_id = $3 AND repo_full_name_lower = lower($4)",
    )
    .bind(if following { 1_i64 } else { 0_i64 })
    .bind(&now)
    .bind(&user_id)
    .bind(&full_name)
    .execute(&state.runtime.pool)
    .await;
    match result {
        Ok(result) if result.rows_affected() == 0 => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "repository_not_found"})),
        )
            .into_response(),
        Ok(_) => match sqlx::query_as::<_, PgFollowingRepoRow>(
            "SELECT repo_id, repo_full_name AS full_name, owner_login, repo_name AS name, COALESCE(html_url, 'https://github.com/' || repo_full_name) AS html_url, description, is_private, first_source, first_associated_at, last_seen_at, is_following, follow_state_source, owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image, has_personal_owned_source, has_github_star_source, has_manual_feed_source FROM public.user_repo_associations WHERE user_id = $1 AND repo_full_name_lower = lower($2)",
        )
        .bind(&user_id)
        .bind(&full_name)
        .fetch_one(&state.runtime.pool)
        .await
        {
            Ok(row) => (
                StatusCode::OK,
                axum::Json(json!({
                    "repo_id": row.repo_id,
                    "full_name": row.full_name,
                    "owner_login": row.owner_login,
                    "name": row.name,
                    "html_url": row.html_url,
                    "description": row.description,
                    "is_private": row.is_private.map(|v| v != 0),
                    "is_following": row.is_following != 0,
                    "follow_state_source": row.follow_state_source,
                    "repo_visual": {"owner_avatar_url": row.owner_avatar_url, "open_graph_image_url": row.open_graph_image_url, "uses_custom_open_graph_image": row.uses_custom_open_graph_image.unwrap_or_default() != 0}
                })),
            )
                .into_response(),
            Err(error) => {
                warn!(%error, "PostgreSQL following repository readback failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"code": "database_error"})),
                )
                    .into_response()
            }
        },
        Err(error) => {
            warn!(%error, "PostgreSQL following repository update failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

async fn search(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(request): Query<SearchQuery>,
) -> impl IntoResponse {
    let Some(user_id) = session.get::<String>("user_id").await.ok().flatten() else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized", "message": "login required"})),
        );
    };
    let raw_query = request.q.unwrap_or_default();
    let parsed = match crate::search::parse_query(&raw_query) {
        Ok(parsed) => parsed,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"code": error.code()})),
            );
        }
    };
    let rows = match crate::search::query_postgres(&state.runtime.pool, &user_id, &parsed).await {
        Ok(rows) => rows,
        Err(error) => {
            warn!(%error, "PostgreSQL search query failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "search_failed"})),
            );
        }
    };
    (
        StatusCode::OK,
        axum::Json(json!({
            "items": rows,
            "remaining_requests": crate::search::SEARCH_RATE_LIMIT - 1,
            "reset_at": (chrono::Utc::now() + chrono::Duration::seconds(crate::search::SEARCH_RATE_WINDOW_SECONDS)).to_rfc3339(),
            "index_status": "ready"
        })),
    )
}

#[derive(Debug, FromRow)]
struct PgTaskEventRow {
    id: String,
    event_type: String,
    payload_json: String,
    created_at: chrono::DateTime<Utc>,
}

async fn task_events(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(task_id): Path<String>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let Some(user_id) = session
        .get::<String>("user_id")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let owned = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.job_tasks WHERE id = $1 AND requested_by = $2",
    )
    .bind(&task_id)
    .bind(&user_id)
    .fetch_one(&state.runtime.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if owned == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    let rows = sqlx::query_as::<_, PgTaskEventRow>(
        "SELECT id, event_type, payload_json, created_at \
         FROM public.job_task_events WHERE task_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(&task_id)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let events = rows.into_iter().map(|row| {
        Ok(Event::default()
            .id(row.id)
            .event(row.event_type)
            .data(row.payload_json)
            .comment(row.created_at.to_rfc3339()))
    });
    Ok(Sse::new(stream::iter(events)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

async fn health(State(state): State<PostgresServerState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.runtime.pool)
        .await
    {
        Ok(_) => {
            // The release smoke asserts the effective version here, matching
            // the legacy `/api/health` payload.
            let info = crate::version::resolve_effective_version();
            (
                StatusCode::OK,
                axum::Json(json!({"ok": true, "version": info.version})),
            )
        }
        Err(error) => {
            warn!(%error, owner_id = %state.owner_id, "PostgreSQL health probe failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(json!({"ok": false, "error": "postgres_unavailable"})),
            )
        }
    }
}

async fn version() -> axum::Json<serde_json::Value> {
    let info = crate::version::resolve_effective_version();
    axum::Json(json!({
        "ok": true,
        "version": info.version,
        "source": info.source,
    }))
}

async fn reaction_token_status(
    State(state): State<PostgresServerState>,
    session: Session,
) -> axum::response::Response {
    use sqlx::Row;
    let Some(user_id) = session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code":"unauthorized"})),
        )
            .into_response();
    };
    let active = sqlx::query_scalar::<_, i64>("SELECT is_disabled FROM public.users WHERE id=$1")
        .bind(&user_id)
        .fetch_optional(&state.runtime.pool)
        .await;
    match active {
        Ok(Some(0)) => {}
        Ok(Some(_)) => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"code":"account_disabled"})),
            )
                .into_response();
        }
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"code":"unauthorized"})),
            )
                .into_response();
        }
        Err(error) => {
            warn!(%error, "PostgreSQL reaction token user check failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response();
        }
    }
    match sqlx::query("SELECT masked_token,last_check_state,last_check_message,last_checked_at,owner_github_connection_id,owner_github_user_id,owner_login FROM public.reaction_pat_tokens WHERE user_id=$1")
        .bind(&user_id).fetch_optional(&state.runtime.pool).await {
        Ok(None) => Json(json!({"configured":false,"masked_token":null,"check":{"state":"idle","message":null,"checked_at":null},"owner":null})).into_response(),
        Ok(Some(row)) => {
            let status: String = row.get("last_check_state");
            let status = if matches!(status.as_str(), "valid"|"invalid"|"error") {status.as_str()} else {"idle"};
            let owner = match (row.get::<Option<String>,_>("owner_github_connection_id"),row.get::<Option<i64>,_>("owner_github_user_id"),row.get::<Option<String>,_>("owner_login")) {
                (Some(connection),Some(id),Some(login)) => json!({"github_connection_id":connection,"github_user_id":id,"login":login}),
                _ => serde_json::Value::Null,
            };
            Json(json!({"configured":true,"masked_token":row.get::<String,_>("masked_token"),"check":{"state":status,"message":row.get::<Option<String>,_>("last_check_message"),"checked_at":row.get::<Option<String>,_>("last_checked_at")},"owner":owner})).into_response()
        },
        Err(error) => {
            warn!(%error, "PostgreSQL reaction token status query failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"code":"database_error"}))).into_response()
        }
    }
}

fn postgres_translation_request_select() -> String {
    let mut base = crate::translations::request_row_select_sql().to_owned();
    for field in ["created_at", "started_at", "finished_at", "updated_at"] {
        base = base.replace(
            &format!("r.{field},"),
            &format!("r.{field}::text AS {field},"),
        );
    }
    base = base.replace("r.max_wait_ms,", "r.max_wait_ms::bigint AS max_wait_ms,");
    base = base.replace("SELECT MAX(created_at)", "SELECT MAX(created_at)::text");
    for table in [
        "translation_work_items",
        "translation_requests",
        "translation_attempt_events",
    ] {
        base = base.replace(&format!("FROM {table} "), &format!("FROM public.{table} "));
    }
    base
}

async fn admin_translation_requests(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<crate::translations::AdminTranslationListQuery>,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let status = query.status.unwrap_or_else(|| "all".to_owned());
    let group = query.task_group.unwrap_or_else(|| "all".to_owned());
    if !matches!(group.as_str(), "all" | "translation" | "polish") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"code":"bad_request","message":"invalid task_group filter"})),
        )
            .into_response();
    }
    let parse_time =
        |value: Option<String>| -> Result<Option<chrono::DateTime<Utc>>, chrono::ParseError> {
            value
                .filter(|v| !v.trim().is_empty())
                .map(|v| {
                    chrono::DateTime::parse_from_rfc3339(v.trim()).map(|t| t.with_timezone(&Utc))
                })
                .transpose()
        };
    let (from, before) = match (
        parse_time(query.discovered_from),
        parse_time(query.discovered_before),
    ) {
        (Ok(from), Ok(before)) => (from, before),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"code":"bad_request","message":"invalid timestamp filter"})),
            )
                .into_response();
        }
    };
    let base = postgres_translation_request_select();
    let filter = " WHERE ($1='all' OR CASE WHEN status='queued' AND work_item_status IN ('running','batched') THEN 'running' ELSE status END=$1) AND ($2='all' OR ($2='translation' AND kind!='release_smart') OR ($2='polish' AND kind='release_smart')) AND ($3::timestamptz IS NULL OR created_at::timestamptz >= $3) AND ($4::timestamptz IS NULL OR created_at::timestamptz < $4)";
    let count_sql = format!("SELECT COUNT(*) FROM ({base}) request_rows {filter}");
    let items_sql = format!(
        "SELECT * FROM ({base}) request_rows {filter} ORDER BY CASE WHEN status IN ('queued','running') THEN 0 ELSE 1 END, created_at DESC,id DESC LIMIT $5 OFFSET $6"
    );
    let result = async {
        let total = sqlx::query_scalar::<_, i64>(&count_sql)
            .bind(&status)
            .bind(&group)
            .bind(from)
            .bind(before)
            .fetch_one(&state.runtime.pool)
            .await?;
        let rows = sqlx::query_as::<_, crate::translations::RequestRow>(&items_sql)
            .bind(&status)
            .bind(&group)
            .bind(from)
            .bind(before)
            .bind(page_size)
            .bind(page.saturating_sub(1).saturating_mul(page_size))
            .fetch_all(&state.runtime.pool)
            .await?;
        Ok::<_, sqlx::Error>(crate::translations::AdminTranslationRequestsResponse {
            items: rows
                .iter()
                .map(|r| r.to_admin_request_list_item())
                .collect(),
            page,
            page_size,
            total,
        })
    }
    .await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL admin translation requests failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_translation_batches(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<crate::translations::AdminTranslationListQuery>,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let status = query.status.unwrap_or_else(|| "all".to_owned());
    let result = async {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM public.translation_batches WHERE ($1='all' OR status=$1)",
        ).bind(&status).fetch_one(&state.runtime.pool).await?;
        let rows = sqlx::query_as::<_, crate::translations::AdminTranslationBatchRow>(
            "SELECT id,status,trigger_reason,worker_slot::bigint AS worker_slot,request_count::bigint AS request_count,item_count::bigint AS item_count,estimated_input_tokens::bigint AS estimated_input_tokens,error_text,created_at::text AS created_at,started_at::text AS started_at,finished_at::text AS finished_at,updated_at::text AS updated_at FROM public.translation_batches WHERE ($1='all' OR status=$1) ORDER BY created_at DESC,id DESC LIMIT $2 OFFSET $3",
        ).bind(&status).bind(page_size).bind(page.saturating_sub(1).saturating_mul(page_size))
            .fetch_all(&state.runtime.pool).await?;
        let batch_ids = rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        let summaries = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT bi.batch_id, CASE WHEN bi.result_status IS NOT NULL AND bi.result_status!='' THEN bi.result_status WHEN w.result_status IS NOT NULL AND w.result_status!='' THEN w.result_status WHEN w.status='queued' OR w.status IS NULL THEN 'queued' ELSE 'running' END AS result_status,COUNT(*) FROM public.translation_batch_items bi JOIN public.translation_work_items w ON w.id=bi.work_item_id WHERE bi.batch_id=ANY($1) GROUP BY bi.batch_id,2",
        ).bind(&batch_ids).fetch_all(&state.runtime.pool).await?;
        let mut by_id = std::collections::HashMap::<String, crate::translations::AdminTranslationBatchResultSummary>::new();
        for (id, status, count) in summaries {
            let entry = by_id.entry(id).or_default();
            match status.as_str() {
                "ready" => entry.ready=count,
                "error" => entry.error=count,
                "missing" => entry.missing=count,
                "disabled" => entry.disabled=count,
                "queued" => entry.queued=count,
                "running" => entry.running=count,
                _ => {}
            }
        }
        Ok::<_, sqlx::Error>((total, rows, by_id))
    }.await;
    match result {
        Ok((total, rows, mut summaries)) => {
            let items = rows
                .into_iter()
                .map(|row| {
                    let summary = summaries.remove(&row.id).unwrap_or_default();
                    row.into_list_item(summary)
                })
                .collect::<Vec<_>>();
            Json(crate::translations::AdminTranslationBatchesResponse {
                items,
                page,
                page_size,
                total,
            })
            .into_response()
        }
        Err(error) => {
            warn!(%error,"PostgreSQL admin translation batches failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn load_pg_translation_attempts(
    state: &PostgresServerState,
    work_item_id: &str,
) -> Result<Vec<crate::translations::AdminTranslationAttemptEvent>, sqlx::Error> {
    use sqlx::Row;
    let rows = sqlx::query("SELECT to_jsonb(e) AS data FROM public.translation_attempt_events e WHERE work_item_id=$1 ORDER BY created_at,id")
        .bind(work_item_id).fetch_all(&state.runtime.pool).await?;
    let mut result = Vec::new();
    for row in rows {
        let mut data: serde_json::Value = row.try_get("data")?;
        let text = |key: &str| {
            data.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        let ids =
            serde_json::from_str::<Vec<String>>(&text("llm_call_ids_json")).unwrap_or_default();
        let attempt = data.get("attempt_no").and_then(|v| v.as_i64()).unwrap_or(0);
        let links = sqlx::query("SELECT llm_call_id,stage,relation_role,evidence_availability FROM public.translation_attempt_llm_calls WHERE work_item_id=$1 AND attempt_no=$2")
            .bind(work_item_id).bind(attempt).fetch_all(&state.runtime.pool).await?;
        let calls = sqlx::query_as::<_,crate::translations::AdminTranslationLinkedLlmCall>("SELECT id,status,source,model,scheduler_wait_ms,duration_ms,created_at::text AS created_at FROM public.llm_calls WHERE id=ANY($1)")
            .bind(&ids).fetch_all(&state.runtime.pool).await?;
        let mut hydrated = Vec::new();
        for id in &ids {
            let link = links
                .iter()
                .find(|l| l.get::<String, _>("llm_call_id") == *id);
            let call = calls.iter().find(|c| c.id == *id).cloned().or_else(|| {
                link.map(|l| crate::translations::AdminTranslationLinkedLlmCall {
                    id: id.clone(),
                    status: "expired".to_owned(),
                    source: "诊断载荷已过期".to_owned(),
                    model: "unknown".to_owned(),
                    scheduler_wait_ms: 0,
                    duration_ms: None,
                    stage: Some(l.get("stage")),
                    relation_role: Some(l.get("relation_role")),
                    evidence_availability: Some(l.get("evidence_availability")),
                    created_at: String::new(),
                })
            });
            if let Some(mut call) = call {
                if let Some(link) = link {
                    call.stage = Some(link.get("stage"));
                    call.relation_role = Some(link.get("relation_role"));
                    call.evidence_availability = Some(link.get("evidence_availability"));
                } else {
                    call.relation_role = Some("legacy_unverified".to_owned());
                    call.evidence_availability = Some("not_captured".to_owned());
                }
                hydrated.push(call);
            }
        }
        if let Some(id) = data
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|v| v.parse::<i64>().ok())
        {
            data["id"] = json!(id);
        }
        if let Some(value) = data.get("retry_eligible").and_then(|v| v.as_i64()) {
            data["retry_eligible"] = json!(value != 0);
        }
        data["llm_call_ids"] = json!(ids);
        data["llm_calls"] = json!(hydrated);
        let event = serde_json::from_value(data).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        result.push(event);
    }
    Ok(result)
}

async fn admin_translation_request_detail(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(request_id): Path<String>,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let query = format!(
        "{} WHERE r.id=$1 LIMIT 1",
        postgres_translation_request_select()
    );
    let result = async {
        let row = sqlx::query_as::<_, crate::translations::RequestRow>(&query)
            .bind(&request_id)
            .fetch_optional(&state.runtime.pool)
            .await?;
        let Some(row) = row else {
            return Ok::<_, sqlx::Error>(None);
        };
        let item = row.to_result();
        let attempts = if let Some(id) = item.work_item_id.as_deref() {
            load_pg_translation_attempts(&state, id).await?
        } else {
            Vec::new()
        };
        Ok(Some(
            crate::translations::AdminTranslationRequestDetailResponse {
                request: row.to_admin_request_list_item(),
                result: crate::translations::admin_translation_result_item(item),
                attempts,
            },
        ))
    }
    .await;
    match result {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"code":"not_found"}))).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL translation request detail failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_translation_attempt_events(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<crate::translations::AdminTranslationAttemptListQuery>,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let entity = query.entity_id.unwrap_or_default().trim().to_owned();
    let request = query.request_id.unwrap_or_default().trim().to_owned();
    let work = query.work_item_id.unwrap_or_default().trim().to_owned();
    let kind = query.kind.unwrap_or_default().trim().to_owned();
    let variant = query.variant.unwrap_or_default().trim().to_owned();
    if entity.is_empty() && request.is_empty() && work.is_empty() {
        return (StatusCode::BAD_REQUEST,Json(json!({"code":"bad_request","message":"entity_id, request_id, or work_item_id is required"}))).into_response();
    }
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(50).clamp(1, 200);
    let result=async {
        let filter=" WHERE ($1='' OR entity_id=$1) AND ($2='' OR request_id=$2) AND ($3='' OR work_item_id=$3) AND ($4='' OR kind=$4) AND ($5='' OR variant=$5)";
        let total=sqlx::query_scalar::<_,i64>(&format!("SELECT COUNT(*) FROM public.translation_attempt_events {filter}")).bind(&entity).bind(&request).bind(&work).bind(&kind).bind(&variant).fetch_one(&state.runtime.pool).await?;
        let query=format!("SELECT id::text,work_item_id FROM public.translation_attempt_events {filter} ORDER BY created_at,id LIMIT $6 OFFSET $7");
        let rows=sqlx::query_as::<_,(String,String)>(&query).bind(&entity).bind(&request).bind(&work).bind(&kind).bind(&variant).bind(page_size).bind(page.saturating_sub(1).saturating_mul(page_size)).fetch_all(&state.runtime.pool).await?;
        let ids=rows.iter().map(|r|r.0.clone()).collect::<std::collections::HashSet<_>>();
        let works=rows.into_iter().map(|r|r.1).collect::<std::collections::HashSet<_>>();
        let mut items=Vec::new();
        for work in works {items.extend(load_pg_translation_attempts(&state,&work).await?.into_iter().filter(|e|ids.contains(&e.id.to_string())));}
        items.sort_by(|a,b|a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok::<_,sqlx::Error>(crate::translations::AdminTranslationAttemptEventsResponse{items,page,page_size,total})
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL translation attempt list failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

pub(crate) async fn admin_translation_status(
    State(state): State<PostgresServerState>,
    session: Session,
) -> axum::response::Response {
    use sqlx::Row;
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async {
        let counts=sqlx::query("SELECT (SELECT COUNT(*) FROM public.translation_requests WHERE status='queued') AS queued_requests,(SELECT COUNT(*) FROM public.translation_work_items WHERE status='queued') AS queued_work_items,(SELECT COUNT(*) FROM public.translation_batches WHERE status='running') AS running_batches,(SELECT COUNT(*) FROM public.translation_requests WHERE created_at::timestamptz BETWEEN now()-interval '24 hours' AND now()) AS requests_24h,(SELECT MAX(finished_at)::text FROM public.translation_batches) AS last_batch_finished_at").fetch_one(&state.runtime.pool).await?;
        let batches=sqlx::query("SELECT id,status,item_count,error_text FROM public.translation_batches WHERE created_at::timestamptz BETWEEN now()-interval '24 hours' AND now()").fetch_all(&state.runtime.pool).await?;
        let mut completed=0_i64;let mut clean=0_i64;let mut issues=0_i64;let mut failed=0_i64;
        let mut errors=0_i64;let mut missing=0_i64;
        for batch in batches {
            let id:String=batch.try_get("id")?;
            let rows=sqlx::query_as::<_,(String,i64)>("SELECT CASE WHEN bi.result_status IS NOT NULL AND bi.result_status!='' THEN bi.result_status WHEN w.result_status IS NOT NULL AND w.result_status!='' THEN w.result_status WHEN w.status='queued' OR w.status IS NULL THEN 'queued' ELSE 'running' END AS result_status,COUNT(*) FROM public.translation_batch_items bi JOIN public.translation_work_items w ON w.id=bi.work_item_id WHERE bi.batch_id=$1 GROUP BY 1").bind(id).fetch_all(&state.runtime.pool).await?;
            let mut summary=crate::translations::AdminTranslationBatchResultSummary::default();
            for (status,n) in rows {match status.as_str(){"ready"=>summary.ready=n,"error"=>summary.error=n,"missing"=>summary.missing=n,"disabled"=>summary.disabled=n,"queued"=>summary.queued=n,"running"=>summary.running=n,_=>{}}}
            errors+=summary.error;missing+=summary.missing;
            let status:String=batch.try_get("status")?;
            if status=="completed" {completed+=1;let outcome=crate::translations::translation_batch_business_outcome(&status,&summary,batch.try_get("item_count")?,batch.try_get::<Option<String>,_>("error_text")?.as_deref());if matches!(outcome.code.as_str(),"ok"|"disabled"){clean+=1}else{issues+=1}}
            if status=="failed" {failed+=1}
        }
        let avg=sqlx::query_scalar::<_,Option<f64>>("SELECT AVG(EXTRACT(EPOCH FROM (deadline_at::timestamptz-created_at::timestamptz))*1000)::float8 FROM public.translation_work_items WHERE finished_at IS NOT NULL AND created_at::timestamptz BETWEEN now()-interval '24 hours' AND now()").fetch_one(&state.runtime.pool).await?;
        let context=sqlx::query_scalar::<_,Option<i64>>("SELECT ai_model_context_limit FROM public.admin_runtime_settings WHERE id=1").fetch_optional(&state.runtime.pool).await?.flatten();
        let (limit,source)=context.map(|v|(v.max(1) as u32,"admin_override")).unwrap_or_else(||crate::ai::resolve_model_input_limit_for_model(state.ai_model.as_deref().unwrap_or_default()));
        // The PG registry is a single sequential worker; it has no separate
        // batching scheduler or dedicated workers. Report that actual state.
        let enabled=postgres_worker_enabled();
        let running=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM public.job_tasks WHERE task_type='translation.generate' AND status='running' AND runtime_owner_id=$1").bind(state.owner_id.as_ref()).fetch_one(&state.runtime.pool).await?;
        let workers=if enabled {vec![crate::translations::AdminTranslationWorkerStatus{worker_id:state.owner_id.to_string(),worker_slot:1,worker_kind:"general".to_owned(),status:if running>0{"running"}else{"idle"}.to_owned(),current_batch_id:None,request_count:running,work_item_count:running,trigger_reason:None,updated_at:Utc::now().to_rfc3339(),error_text:None,error_code:None,error_summary:None,error_detail:None}]}else{Vec::new()};
        let concurrency=i64::from(enabled);
        Ok::<_,sqlx::Error>(crate::translations::AdminTranslationStatusResponse{scheduler_enabled:enabled,llm_enabled:state.ai_model.is_some(),scan_interval_ms:250,batch_token_threshold:i64::from(limit.saturating_sub(1800).saturating_sub(((f64::from(limit)*0.05).ceil() as u32).max(512)).clamp(1,1800)),ai_model_context_limit:context,effective_model_input_limit:i64::from(limit),effective_model_input_limit_source:source.to_owned(),general_worker_concurrency:concurrency,dedicated_worker_concurrency:0,worker_concurrency:concurrency,target_general_worker_concurrency:concurrency,target_dedicated_worker_concurrency:0,target_worker_concurrency:concurrency,idle_workers:if enabled&&running==0{1}else{0},busy_workers:running.min(concurrency),workers,queued_requests:counts.try_get("queued_requests")?,queued_work_items:counts.try_get("queued_work_items")?,running_batches:counts.try_get("running_batches")?,requests_24h:counts.try_get("requests_24h")?,completed_batches_24h:completed,clean_completed_batches_24h:clean,completed_with_issues_batches_24h:issues,failed_batches_24h:failed,error_work_items_24h:errors,missing_work_items_24h:missing,avg_wait_ms_24h:avg.map(|v|v.round() as i64),last_batch_finished_at:counts.try_get("last_batch_finished_at")?})
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL translation status failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

pub(crate) async fn admin_migrations(
    State(state): State<PostgresServerState>,
    session: Session,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async {
        let run=sqlx::query_scalar::<_,serde_json::Value>("SELECT to_jsonb(r)-'last_error' || jsonb_build_object('last_error',CASE WHEN last_error IS NULL THEN NULL ELSE 'migration failed; consult protected service logs' END,'pause_requested',pause_requested<>0) FROM public.online_migration_runs r WHERE migration_id='content-processing-online-v2'").fetch_optional(&state.runtime.pool).await?;
        let operations=sqlx::query_scalar::<_,serde_json::Value>("SELECT to_jsonb(o)-'last_error' || jsonb_build_object('last_error',CASE WHEN last_error IS NULL THEN NULL ELSE 'migration failed; consult protected service logs' END) FROM public.online_migration_operations o WHERE migration_id='content-processing-online-v2' ORDER BY operation_order").fetch_all(&state.runtime.pool).await?;
        Ok::<_,sqlx::Error>(json!({"run":run,"operations":operations}))
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL online migration read failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_webhook_runtime_config(
    State(state): State<PostgresServerState>,
    session: Session,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    match sqlx::query_as::<_,(i64,Option<String>)>("SELECT webhook_push_audit_interval_days,webhook_push_audit_last_started_at FROM public.admin_runtime_settings WHERE id=1").fetch_one(&state.runtime.pool).await {
        Ok((days,last))=>{
            let next=last.as_deref().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok()).map(|t|(t+chrono::Duration::days(days)).to_rfc3339());
            Json(json!({"audit_interval_days":days,"last_started_at":last,"next_started_at":next})).into_response()
        },
        Err(error)=>{warn!(%error,"PostgreSQL webhook runtime config read failed");(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"code":"database_error"}))).into_response()}
    }
}

#[derive(Deserialize)]
struct PgAdminPublicRepoQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    query: Option<String>,
}

async fn admin_public_release_repos(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<PgAdminPublicRepoQuery>,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let page = query.page.unwrap_or(1);
    if page < 1 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"code":"bad_request","message":"page must be >= 1"})),
        )
            .into_response();
    }
    let size = query.page_size.unwrap_or(20).clamp(1, 100);
    let text = query.query.unwrap_or_default().trim().to_lowercase();
    match public_release_repos_body(&state, page, size, &text).await {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL admin public repositories failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

/// Load one public-repo governance page. Shared by the list handler and the
/// admin deletion handler, which returns the refreshed page with its cleanup
/// summary attached.
pub(crate) async fn public_release_repos_body(
    state: &PostgresServerState,
    page: i64,
    size: i64,
    text: &str,
) -> Result<serde_json::Value, sqlx::Error> {
    let pattern = format!("%{text}%");
    async {
        let total=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM public.public_repo_release_usage WHERE $1='' OR full_name_lower LIKE $2").bind(text).bind(&pattern).fetch_one(&state.runtime.pool).await?;
        let rows=sqlx::query_scalar::<_,serde_json::Value>(r#"SELECT to_jsonb(item) FROM (
            SELECT pu.id,pu.repo_id,pu.full_name,pu.first_registered_at,pu.last_requested_at,pu.last_list_requested_at,pu.last_detail_requested_at,pu.api_list_requests,pu.api_detail_requests,pu.page_list_requests,pu.page_detail_requests,pu.last_sync_status,pu.last_sync_error,COUNT(DISTINCT r.release_id) AS release_count,
            COUNT(DISTINCT CASE WHEN td.status='ready' THEN r.release_id END) AS translated_ready_count,
            COUNT(DISTINCT CASE WHEN COALESCE(td.status,'missing')!='ready' THEN r.release_id END) AS translated_missing_count,
            COUNT(DISTINCT CASE WHEN sm.status='ready' THEN r.release_id END) AS polished_ready_count,
            COUNT(DISTINCT CASE WHEN COALESCE(sm.status,'missing')!='ready' THEN r.release_id END) AS polished_missing_count,pu.created_at,pu.updated_at
            FROM (SELECT * FROM public.public_repo_release_usage WHERE $1='' OR full_name_lower LIKE $2 ORDER BY last_requested_at DESC,full_name LIMIT $3 OFFSET $4) pu
            LEFT JOIN public.repo_releases r ON r.repo_id=pu.repo_id
            LEFT JOIN LATERAL(SELECT status FROM public.ai_translations WHERE entity_type='release_detail' AND entity_id=r.release_id::text AND lang='zh-CN' ORDER BY CASE WHEN status='ready' THEN 0 ELSE 1 END,updated_at DESC LIMIT 1) td ON true
            LEFT JOIN LATERAL(SELECT status FROM public.ai_translations WHERE entity_type='release_smart' AND entity_id=r.release_id::text AND lang='zh-CN' ORDER BY CASE WHEN status='ready' THEN 0 ELSE 1 END,updated_at DESC LIMIT 1) sm ON true
            GROUP BY pu.id,pu.repo_id,pu.full_name,pu.first_registered_at,pu.last_requested_at,pu.last_list_requested_at,pu.last_detail_requested_at,pu.api_list_requests,pu.api_detail_requests,pu.page_list_requests,pu.page_detail_requests,pu.last_sync_status,pu.last_sync_error,pu.created_at,pu.updated_at
            ORDER BY pu.last_requested_at DESC,pu.full_name) item"#).bind(text).bind(&pattern).bind(size).bind(page.saturating_sub(1).saturating_mul(size)).fetch_all(&state.runtime.pool).await?;
        Ok::<_,sqlx::Error>(json!({"items":rows,"page":page,"page_size":size,"total":total}))
    }.await
}

pub(crate) async fn admin_search_index_status(
    State(state): State<PostgresServerState>,
    session: Session,
) -> axum::response::Response {
    use sqlx::Row;
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async {
        let row=sqlx::query("SELECT phase,cursor,status,last_error,updated_at FROM public.search_projection_backfill_state WHERE id=1").fetch_one(&state.runtime.pool).await?;
        let metadata=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM public.search_metadata_backfill_queue)").fetch_one(&state.runtime.pool).await?;
        let pending=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM public.search_projection_outbox)").fetch_one(&state.runtime.pool).await?;
        let failure=sqlx::query_scalar::<_,Option<String>>("SELECT last_error FROM public.search_projection_outbox_failed ORDER BY failed_at DESC,projection_key DESC LIMIT 1").fetch_optional(&state.runtime.pool).await?.flatten();
        let persisted:String=row.try_get("status")?;
        let status=if failure.is_some(){"failed"}else if persisted=="ready"&&(metadata||pending){"building"}else{persisted.as_str()};
        let error=row.try_get::<Option<String>,_>("last_error")?.or(failure);
        let mut body=json!({"phase":row.try_get::<String,_>("phase")?,"cursor":row.try_get::<i64,_>("cursor")?,"status":status,"updated_at":row.try_get::<String,_>("updated_at")?,"metadata_pending":metadata});
        if error.is_some(){body["last_error"]=json!("search projection failed; consult protected service logs");}
        Ok::<_,sqlx::Error>(body)
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL search index status failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

pub(crate) async fn admin_identity_upgrade_status(
    State(state): State<PostgresServerState>,
    session: Session,
) -> axum::response::Response {
    use sqlx::Row;
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async {
        let row=sqlx::query("SELECT generation,status,phase,cursor,last_error_code,started_at,completed_at,updated_at FROM public.content_identity_upgrade_control WHERE id=1").fetch_one(&state.runtime.pool).await?;
        let raw:Option<String>=row.try_get("cursor")?;
        let cursor:serde_json::Value=raw.map(|v|serde_json::from_str(&v)).transpose().map_err(|e|sqlx::Error::Decode(Box::new(e)))?.unwrap_or(json!({}));
        let progress=|name:&str| {let p=&cursor[name];json!({"processed":p["processed"].as_i64().unwrap_or(0),"total":p.get("total"),"completed_at":p.get("completed_at"),"requeued":p["requeued"].as_i64().unwrap_or(0)})};
        let waiting=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM (SELECT canonical_resource_type,canonical_resource_id,pipeline,variant,target_lang,source_hash,protocol_version FROM public.content_work_items WHERE status='blocked_config' GROUP BY canonical_resource_type,canonical_resource_id,pipeline,variant,target_lang,source_hash,protocol_version) blocked WHERE NOT EXISTS(SELECT 1 FROM public.content_work_identities i JOIN public.content_current_result_projections p ON p.identity_id=i.id WHERE i.canonical_resource_type=blocked.canonical_resource_type AND i.canonical_resource_id=blocked.canonical_resource_id AND i.pipeline=blocked.pipeline AND i.variant=blocked.variant AND i.target_lang=blocked.target_lang AND i.source_hash=blocked.source_hash AND i.protocol_version=blocked.protocol_version)").fetch_one(&state.runtime.pool).await?;
        Ok::<_,sqlx::Error>(json!({"generation":row.try_get::<i64,_>("generation")?,"status":row.try_get::<String,_>("status")?,"phase":row.try_get::<String,_>("phase")?,"last_error_code":row.try_get::<Option<String>,_>("last_error_code")?,"started_at":row.try_get::<Option<String>,_>("started_at")?,"completed_at":row.try_get::<Option<String>,_>("completed_at")?,"updated_at":row.try_get::<String,_>("updated_at")?,"search_index_refresh_pending":cursor["search_index_refresh_pending"].as_bool().unwrap_or(false),"work_identity_backfill":progress("work_identity_backfill"),"projection_backfill":progress("projection_backfill"),"blocked_config_recovery":progress("blocked_config_recovery"),"waiting_blocked_config_identities":waiting}))
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL identity upgrade status failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn pg_sync_runtime_config(
    state: &PostgresServerState,
) -> Result<serde_json::Value, sqlx::Error> {
    use sqlx::Row;
    let settings=sqlx::query_scalar::<_,serde_json::Value>("SELECT jsonb_build_object('sync_auto_fetch_interval_minutes',sync_auto_fetch_interval_minutes,'sync_auto_fetch_effective_at',sync_auto_fetch_effective_at,'star_sync_delta_interval_minutes',star_sync_delta_interval_minutes,'star_sync_full_sweep_interval_minutes',star_sync_full_sweep_interval_minutes,'retry_recent_failures_interval_minutes',retry_recent_failures_interval_minutes,'repo_release_worker_concurrency',repo_release_worker_concurrency,'repo_refresh_system_budget_per_window',repo_refresh_system_budget_per_window,'dashboard_release_freshness_profile',dashboard_release_freshness_profile,'daily_brief_schedule_local_time',daily_brief_schedule_local_time) FROM public.admin_runtime_settings WHERE id=1").fetch_one(&state.runtime.pool).await?;
    let delta=sqlx::query_scalar::<_,Option<String>>("SELECT MAX(finished_at)::text FROM public.job_tasks WHERE task_type='sync.starred.delta' AND status='succeeded'").fetch_one(&state.runtime.pool).await?;
    let full = sqlx::query_scalar::<_, Option<String>>(
        "SELECT MAX(completed_at)::text FROM public.star_sync_epochs WHERE status='succeeded'",
    )
    .fetch_one(&state.runtime.pool)
    .await?;
    let epochs=sqlx::query("SELECT github_connection_id,processed_pages,processed_items,total_count_at_start,next_slice_not_before FROM public.star_sync_epochs WHERE status IN ('queued','running') ORDER BY created_at,id").fetch_all(&state.runtime.pool).await?;
    let epochs=epochs.into_iter().map(|r|{let items=r.get::<i64,_>("processed_items").max(0);let total=r.get::<Option<i64>,_>("total_count_at_start");json!({"github_connection_id":r.get::<String,_>("github_connection_id"),"processed_pages":r.get::<i64,_>("processed_pages").max(0),"processed_items":items,"total_count_at_start":total,"completion_percent":total.filter(|v|*v>0).map(|v|(items as f64/v as f64*100.0).min(100.0)),"next_slice_not_before":r.get::<Option<String>,_>("next_slice_not_before")})}).collect::<Vec<_>>();
    let tasks=sqlx::query_scalar::<_,serde_json::Value>(r#"WITH roots AS(SELECT id,status,source,created_at,started_at,finished_at FROM public.job_tasks WHERE task_type='sync.subscriptions' AND CASE WHEN pg_input_is_valid(result_json,'jsonb') THEN COALESCE((result_json::jsonb->>'skipped'),'false') NOT IN ('true','1') ELSE true END ORDER BY created_at DESC,id DESC LIMIT 3),chain AS(SELECT id AS root_id,finished_at FROM roots UNION ALL SELECT t.parent_task_id,t.finished_at FROM public.job_tasks t JOIN roots r ON r.id=t.parent_task_id WHERE t.task_type IN ('translate.release.batch','summarize.release.smart.batch')),rollup AS(SELECT root_id,MAX(finished_at) AS finished_at,COUNT(*) FILTER(WHERE finished_at IS NULL) AS unfinished FROM chain GROUP BY root_id) SELECT jsonb_build_object('id',r.id,'status',r.status,'source',r.source,'skipped',false,'duration_ms',CASE WHEN c.unfinished=0 THEN (EXTRACT(EPOCH FROM(c.finished_at-r.created_at))*1000)::bigint ELSE NULL END,'created_at',r.created_at::text,'started_at',r.started_at::text,'finished_at',CASE WHEN c.unfinished=0 THEN c.finished_at::text ELSE NULL END) FROM roots r JOIN rollup c ON c.root_id=r.id ORDER BY r.created_at DESC,r.id DESC"#).fetch_all(&state.runtime.pool).await?;
    let mut body = settings;
    body["star_sync"] = json!({"last_delta_completed_at":delta,"last_full_sweep_completed_at":full,"active_epochs":epochs});
    body["recent_sync_tasks"] = json!(tasks);
    Ok(body)
}

async fn admin_sync_runtime_config(
    State(state): State<PostgresServerState>,
    session: Session,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    match pg_sync_runtime_config(&state).await {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL sync runtime read failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct PgGovernanceQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    query: Option<String>,
    aging: Option<String>,
    target_windows: Option<String>,
    urgency_min: Option<f64>,
    urgency_max: Option<f64>,
}
async fn admin_repo_governance(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<PgGovernanceQuery>,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let page = query.page.unwrap_or(1).max(1);
    let size = query.page_size.unwrap_or(100).clamp(1, 500);
    let text = query.query.unwrap_or_default().trim().to_lowercase();
    let aging = query.aging.unwrap_or_else(|| "all".to_owned());
    let windows = query
        .target_windows
        .unwrap_or_default()
        .split(',')
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().parse::<i64>())
        .collect::<Result<Vec<_>, _>>();
    let valid = query.urgency_min.is_none_or(|v| v.is_finite() && v >= 0.0)
        && query.urgency_max.is_none_or(|v| v.is_finite() && v >= 0.0)
        && !matches!((query.urgency_min,query.urgency_max),(Some(a),Some(b)) if a>b);
    let windows = match windows {
        Ok(w)
            if w.iter().all(|v| *v > 0)
                && valid
                && matches!(aging.as_str(), "all" | "stale" | "missing") =>
        {
            w
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"code":"bad_request","message":"invalid governance filter"})),
            )
                .into_response();
        }
    };
    let result=async{
        let filter=" WHERE ($1='' OR lower(repo_full_name) LIKE $2) AND ($3='all' OR ($3='stale' AND actual_last_success_at::timestamptz <= now()-interval '1 day') OR ($3='missing' AND actual_last_success_at IS NULL)) AND (cardinality($4::bigint[])=0 OR target_window=ANY($4)) AND ($5::float8 IS NULL OR urgency_score >= $5) AND ($6::float8 IS NULL OR urgency_score <= $6)";
        let count=format!("SELECT COUNT(*) FROM public.repo_refresh_governance_snapshots {filter}");let pattern=format!("%{text}%");
        let total=sqlx::query_scalar::<_,i64>(&count).bind(&text).bind(&pattern).bind(&aging).bind(&windows).bind(query.urgency_min).bind(query.urgency_max).fetch_one(&state.runtime.pool).await?;
        let sql=format!("SELECT to_jsonb(item) FROM (SELECT repo_id,repo_full_name,watcher_user_count,watcher_repo_total_sum,cached_stargazer_count,priority_rank,target_window,target_interval_minutes,urgency_score,urgency_bucket,system_last_selected_at,system_last_success_at,system_last_attempt_at,system_last_attempt_status,system_last_attempt_error,actual_last_success_at,actual_last_success_source FROM public.repo_refresh_governance_snapshots {filter} ORDER BY urgency_score DESC,priority_rank,repo_id LIMIT $7 OFFSET $8) item");
        let items=sqlx::query_scalar::<_,serde_json::Value>(&sql).bind(&text).bind(&pattern).bind(&aging).bind(&windows).bind(query.urgency_min).bind(query.urgency_max).bind(size).bind(page.saturating_sub(1).saturating_mul(size)).fetch_all(&state.runtime.pool).await?;
        let options=sqlx::query_scalar::<_,serde_json::Value>("SELECT jsonb_build_object('target_window',target_window,'repo_count',COUNT(*)) FROM public.repo_refresh_governance_snapshots GROUP BY target_window ORDER BY target_window").fetch_all(&state.runtime.pool).await?;
        Ok::<_,sqlx::Error>(json!({"items":items,"page":page,"page_size":size,"total":total,"target_window_options":options}))
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL governance list failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_repo_governance_overview(
    State(state): State<PostgresServerState>,
    session: Session,
) -> axum::response::Response {
    use sqlx::Row;
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async{
        let settings=pg_sync_runtime_config(&state).await?;
        let budget=settings["repo_refresh_system_budget_per_window"].as_i64().unwrap_or(1).max(1);
        let count=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM public.repo_refresh_governance_snapshots").fetch_one(&state.runtime.pool).await?;
        let pressure=sqlx::query_scalar::<_,f64>("SELECT COALESCE(SUM(GREATEST(0.0,LEAST(COALESCE(urgency_score,1),4.0)-1.0))/$1::float8,0) FROM public.repo_refresh_governance_snapshots").bind(budget as f64).fetch_one(&state.runtime.pool).await?;
        let last=sqlx::query_scalar::<_,Option<String>>("SELECT completed_at FROM public.repo_refresh_governance_cycles WHERE status='completed' ORDER BY completed_at DESC,updated_at DESC LIMIT 1").fetch_optional(&state.runtime.pool).await?.flatten();
        let cycle=sqlx::query_scalar::<_,serde_json::Value>("SELECT jsonb_build_object('active_cycle_id',id,'active_cycle_started_at',started_at,'active_cycle_repo_count',frozen_repo_count,'active_cycle_completed_count',completed_repo_count,'active_cycle_window_minutes',window_minutes,'active_cycle_window_budget',window_budget,'active_cycle_last_selection_window_index',last_selection_window_index) FROM public.repo_refresh_governance_cycles WHERE status='active' ORDER BY started_at DESC LIMIT 1").fetch_optional(&state.runtime.pool).await?.unwrap_or(json!({"active_cycle_id":null,"active_cycle_started_at":null,"active_cycle_repo_count":0,"active_cycle_completed_count":0,"active_cycle_window_minutes":null,"active_cycle_window_budget":null,"active_cycle_last_selection_window_index":null}));
        let rows=sqlx::query("SELECT repo_id,actual_last_success_at,target_window,target_interval_minutes,urgency_score,system_last_attempt_status FROM public.repo_refresh_governance_snapshots ORDER BY priority_rank,repo_id").fetch_all(&state.runtime.pool).await?;
        let cells=rows.into_iter().map(|r|{
            let last:Option<String>=r.get("actual_last_success_at");let age=last.as_deref().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok()).map(|t|Utc::now().signed_duration_since(t.with_timezone(&Utc)).num_minutes());
            let bucket=match age{Some(v) if v<=30=>"fresh",Some(v) if v<=360=>"warm",Some(v) if v<=1440=>"aging",Some(_)=>"stale",None=>"unknown"};
            json!({"repo_id":r.get::<i64,_>("repo_id"),"age_bucket":bucket,"band_label":format!("W{} · {} 分钟",r.get::<i64,_>("target_window"),r.get::<i64,_>("target_interval_minutes")),"urgency_score":r.get::<f64,_>("urgency_score"),"system_attempt_status":r.get::<Option<String>,_>("system_last_attempt_status")})
        }).collect::<Vec<_>>();
        Ok::<_,sqlx::Error>(json!({"summary":{"dedup_repo_count":count,"pressure_windows":pressure,"last_full_cycle_completed_at":last},"cycle":cycle,"settings":settings,"grid_cells":cells}))
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL governance overview failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_translation_batch_detail(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(batch_id): Path<String>,
) -> axum::response::Response {
    use sqlx::Row;
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result=async{
        let batch=sqlx::query_as::<_,crate::translations::AdminTranslationBatchRow>("SELECT id,status,trigger_reason,worker_slot::bigint AS worker_slot,request_count::bigint AS request_count,item_count::bigint AS item_count,estimated_input_tokens::bigint AS estimated_input_tokens,error_text,created_at::text AS created_at,started_at::text AS started_at,finished_at::text AS finished_at,updated_at::text AS updated_at FROM public.translation_batches WHERE id=$1").bind(&batch_id).fetch_optional(&state.runtime.pool).await?;
        let Some(batch)=batch else{return Ok::<_,sqlx::Error>(None)};
        let rows=sqlx::query("SELECT w.id AS work_item_id,b.entity_id,b.kind,b.variant,CASE WHEN b.result_status IS NOT NULL AND b.result_status!='' THEN b.result_status WHEN w.result_status IS NOT NULL AND w.result_status!='' THEN w.result_status WHEN w.status='queued' OR w.status IS NULL THEN 'queued' ELSE 'running' END AS result_status,w.title_zh,w.summary_md,w.body_md,COALESCE(b.error_text,w.error_text) AS error_text,(SELECT MIN(r.producer_ref) FROM public.translation_requests r WHERE r.work_item_id=w.id) AS producer_ref FROM public.translation_batch_items b JOIN public.translation_work_items w ON w.id=b.work_item_id WHERE b.batch_id=$1 ORDER BY b.item_index,w.id").bind(&batch_id).fetch_all(&state.runtime.pool).await?;
        let mut summary=crate::translations::AdminTranslationBatchResultSummary::default();
        let mut items=Vec::new();
        for row in rows{
            let status:String=row.try_get("result_status")?;match status.as_str(){"ready"=>summary.ready+=1,"error"=>summary.error+=1,"missing"=>summary.missing+=1,"disabled"=>summary.disabled+=1,"queued"=>summary.queued+=1,"running"=>summary.running+=1,_=>{}}
            let item=crate::translations::TranslationResultItem{producer_ref:row.try_get::<Option<String>,_>("producer_ref")?.unwrap_or_else(||"(shared)".to_owned()),entity_id:row.try_get("entity_id")?,kind:row.try_get("kind")?,variant:row.try_get("variant")?,status,title_zh:row.try_get("title_zh")?,summary_md:row.try_get("summary_md")?,body_md:row.try_get("body_md")?,reading:None,error:row.try_get("error_text")?,work_item_id:Some(row.try_get("work_item_id")?),batch_id:Some(batch_id.clone())};
            items.push(crate::translations::admin_translation_result_item(item));
        }
        let calls=sqlx::query_as::<_,crate::translations::AdminTranslationLinkedLlmCall>("SELECT id,status,source,model,scheduler_wait_ms,duration_ms,created_at::text AS created_at FROM public.llm_calls WHERE parent_translation_batch_id=$1 ORDER BY created_at DESC,id DESC").bind(&batch_id).fetch_all(&state.runtime.pool).await?;
        Ok(Some(json!({"batch":batch.into_list_item(summary),"items":items,"llm_calls":calls})))
    }.await;
    match result {
        Ok(Some(body)) => Json(body).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"code":"not_found"}))).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL translation batch detail failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_task_log(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(task_id): Path<String>,
) -> axum::response::Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let result = async {
        let path = sqlx::query_scalar::<_, Option<String>>(
            "SELECT log_file_path FROM public.job_tasks WHERE id=$1",
        )
        .bind(&task_id)
        .fetch_optional(&state.runtime.pool)
        .await?
        .flatten();
        Ok::<_, sqlx::Error>(path)
    }
    .await;
    match result {
        Ok(Some(path)) => match tokio::fs::File::open(path).await {
            Ok(mut file) => {
                use tokio::io::AsyncReadExt;
                let stream = async_stream::stream! {let mut buffer=[0_u8;16384];loop{match file.read(&mut buffer).await{Ok(0)=>break,Ok(n)=>yield Ok::<bytes::Bytes,std::io::Error>(bytes::Bytes::copy_from_slice(&buffer[..n])),Err(e)=>{yield Err(e);break}}}};
                let mut response =
                    axum::response::Response::new(axum::body::Body::from_stream(stream));
                response.headers_mut().insert(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/x-ndjson"),
                );
                if task_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && let Ok(value) =
                        HeaderValue::from_str(&format!("attachment; filename=\"{task_id}.ndjson\""))
                {
                    response
                        .headers_mut()
                        .insert(axum::http::header::CONTENT_DISPOSITION, value);
                }
                response
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
                StatusCode::NOT_FOUND,
                Json(json!({"code":"not_found","message":"task log not found"})),
            )
                .into_response(),
            Err(error) => {
                warn!(%error,"PostgreSQL task log open failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"code":"log_read_failed"})),
                )
                    .into_response()
            }
        },
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"code":"not_found","message":"task log not found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL task log lookup failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct PgFreshnessQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    decision: Option<String>,
    reason: Option<String>,
}
async fn admin_release_freshness(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(task_id): Path<String>,
    Query(query): Query<PgFreshnessQuery>,
) -> axum::response::Response {
    use sqlx::Row;
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(json!({"code":"admin_required"}))).into_response();
    }
    let kind =
        sqlx::query_scalar::<_, String>("SELECT task_type FROM public.job_tasks WHERE id=$1")
            .bind(&task_id)
            .fetch_optional(&state.runtime.pool)
            .await;
    match kind{Ok(Some(kind)) if kind=="sync.access_refresh"=>{},Ok(Some(_))=>return(StatusCode::BAD_REQUEST,Json(json!({"code":"bad_request","message":"release freshness audit is only available for sync.access_refresh tasks"}))).into_response(),Ok(None)=>return(StatusCode::NOT_FOUND,Json(json!({"code":"not_found"}))).into_response(),Err(error)=>{warn!(%error,"PostgreSQL freshness task lookup failed");return(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"code":"database_error"}))).into_response();}}
    let decision = query.decision.unwrap_or_default().trim().to_owned();
    let reason = query.reason.unwrap_or_default().trim().to_owned();
    if !matches!(
        decision.as_str(),
        "" | "fetch" | "reused_fresh" | "reused_running"
    ) || reason.len() > 64
        || !reason
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return (StatusCode::BAD_REQUEST, Json(json!({"code":"bad_request"}))).into_response();
    }
    let page = query.page.unwrap_or(1).max(1);
    let size = query.page_size.unwrap_or(50).clamp(1, 100);
    let result=async{
        let filter=" WHERE rw.task_id=$1 AND ($2='' OR rw.freshness_decision=$2) AND ($3='' OR CASE WHEN pg_input_is_valid(rw.freshness_assessment_json,'jsonb') THEN (rw.freshness_assessment_json::jsonb->'reason_codes') ? $3 ELSE false END)";
        let total=sqlx::query_scalar::<_,i64>(&format!("SELECT COUNT(*) FROM public.repo_release_watchers rw {filter}")).bind(&task_id).bind(&decision).bind(&reason).fetch_one(&state.runtime.pool).await?;
        let sql=format!("SELECT wi.repo_id,wi.repo_full_name,rw.status,rw.reused_fresh,rw.freshness_window_minutes,rw.freshness_decision,rw.freshness_assessment_json,wi.last_success_at,rw.error_text FROM public.repo_release_watchers rw JOIN public.repo_release_work_items wi ON wi.id=rw.work_item_id {filter} ORDER BY wi.repo_id,rw.id LIMIT $4 OFFSET $5");
        let rows=sqlx::query(&sql).bind(&task_id).bind(&decision).bind(&reason).bind(size).bind(page.saturating_sub(1).saturating_mul(size)).fetch_all(&state.runtime.pool).await?;
        let items=rows.into_iter().map(|r|{let raw:Option<String>=r.get("freshness_assessment_json");let assessment=raw.and_then(|s|serde_json::from_str::<serde_json::Value>(&s).ok());let last=assessment.as_ref().and_then(|a|a["last_success_at"].as_str()).map(str::to_owned).or(r.get::<Option<String>,_>("last_success_at"));json!({"repo_id":r.get::<i64,_>("repo_id"),"repo_full_name":r.get::<String,_>("repo_full_name"),"status":r.get::<String,_>("status"),"reused_fresh":r.get::<i64,_>("reused_fresh")!=0,"freshness_window_minutes":r.get::<Option<i64>,_>("freshness_window_minutes"),"freshness_decision":r.get::<Option<String>,_>("freshness_decision"),"assessment":assessment,"last_success_at":last,"error_text":r.get::<Option<String>,_>("error_text")})}).collect::<Vec<_>>();
        Ok::<_,sqlx::Error>(json!({"items":items,"page":page,"page_size":size,"total":total}))
    }.await;
    match result {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error,"PostgreSQL freshness audit failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code":"database_error"})),
            )
                .into_response()
        }
    }
}

async fn submit_translation_request(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<TranslationSubmitRequest>,
) -> impl IntoResponse {
    let Some(user_id) = session.get::<String>("user_id").await.ok().flatten() else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized"})),
        );
    };
    let Some(item) = request
        .item
        .or_else(|| request.items.and_then(|mut items| items.pop()))
    else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"code": "translation_item_required"})),
        );
    };
    let now = Utc::now();
    let work_item_id = local_id::generate_local_id();
    let request_id = local_id::generate_local_id();
    let source_blocks_json = match serde_json::to_string(&item.source_blocks) {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"code": "invalid_source_blocks"})),
            );
        }
    };
    let target_slots_json = match serde_json::to_string(&item.target_slots) {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"code": "invalid_target_slots"})),
            );
        }
    };
    let source_hash = crate::ai::sha256_hex(&format!("{source_blocks_json}\n{target_slots_json}"));
    let dedupe_key = format!(
        "{}:{}:{}:{}:{}:{}",
        user_id, item.kind, item.entity_id, item.variant, item.target_lang, source_hash
    );
    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(json!({"code": "postgres_unavailable"})),
            );
        }
    };
    let work_result = sqlx::query_as::<_, (String,)>(
        "INSERT INTO public.translation_work_items (id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version, model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate, deadline_at, status, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,'translation-request.v1','postgres-default',$8,$9,$10,0,$11,'queued',$12,$12) ON CONFLICT (dedupe_key) DO UPDATE SET updated_at = EXCLUDED.updated_at RETURNING id",
    )
    .bind(&work_item_id)
    .bind(&dedupe_key)
    .bind(&user_id)
    .bind(&item.kind)
    .bind(&item.variant)
    .bind(&item.entity_id)
    .bind(&item.target_lang)
    .bind(&source_hash)
    .bind(&source_blocks_json)
    .bind(&target_slots_json)
    .bind(now + chrono::Duration::milliseconds(item.max_wait_ms.max(0)))
    .bind(now)
    .fetch_one(&mut *tx)
    .await;
    let Ok((work_item_id,)) = work_result else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "translation_enqueue_failed"})),
        );
    };
    let request_result = sqlx::query(
        "INSERT INTO public.translation_requests (id, mode, source, request_origin, requested_by, scope_user_id, producer_ref, kind, variant, entity_id, target_lang, max_wait_ms, source_hash, source_blocks_json, target_slots_json, work_item_id, status, created_at, updated_at) VALUES ($1,$2,'postgres','user',$3,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'queued',$14,$14)",
    )
    .bind(&request_id)
    .bind(&request.mode)
    .bind(&user_id)
    .bind(&item.producer_ref)
    .bind(&item.kind)
    .bind(&item.variant)
    .bind(&item.entity_id)
    .bind(&item.target_lang)
    .bind(item.max_wait_ms.max(0))
    .bind(&source_hash)
    .bind(&source_blocks_json)
    .bind(&target_slots_json)
    .bind(&work_item_id)
    .bind(now)
    .execute(&mut *tx)
    .await;
    if request_result.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "translation_enqueue_failed"})),
        );
    }
    let task_id = local_id::generate_local_id();
    let task_payload = json!({"work_item_id": work_item_id});
    let task_result = sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at) VALUES ($1, 'translation.generate', 'queued', 'api.translate.requests', $2, $3, $4, $4)",
    )
    .bind(&task_id)
    .bind(&user_id)
    .bind(task_payload.to_string())
    .bind(now)
    .execute(&mut *tx)
    .await;
    if task_result.is_err() || tx.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "translation_enqueue_failed"})),
        );
    }
    (
        StatusCode::ACCEPTED,
        axum::Json(
            json!({"mode": "task_id", "task_id": task_id, "request_id": request_id, "work_item_id": work_item_id, "status": "queued"}),
        ),
    )
}

async fn get_translation_request(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    let Some(user_id) = session.get::<String>("user_id").await.ok().flatten() else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized"})),
        );
    };
    let row = sqlx::query_as::<_, (String, String, String, String, String, String, Option<String>, Option<String>, Option<String>)>(
        "SELECT id, mode, status, kind, variant, entity_id, title_zh, summary_md, body_md FROM public.translation_requests WHERE id = $1 AND scope_user_id = $2",
    )
    .bind(&request_id)
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await;
    match row {
        Ok(Some((id, mode, status, kind, variant, entity_id, title_zh, summary_md, body_md))) => (
            StatusCode::OK,
            axum::Json(
                json!({"id": id, "request_id": id, "mode": mode, "status": status, "kind": kind, "variant": variant, "entity_id": entity_id, "result": {"title_zh": title_zh, "summary_md": summary_md, "body_md": body_md}}),
            ),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "translation_request_not_found"})),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "translation_request_failed"})),
        ),
    }
}

async fn retry_translation_request(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    let Some(user_id) = session.get::<String>("user_id").await.ok().flatten() else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized"})),
        );
    };
    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(json!({"code": "postgres_unavailable"})),
            );
        }
    };
    let work_item = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT work_item_id FROM public.translation_requests WHERE id = $1 AND scope_user_id = $2 AND status IN ('failed', 'completed')",
    )
    .bind(&request_id)
    .bind(&user_id)
    .fetch_optional(&mut *tx)
    .await;
    let Some((Some(work_item_id),)) = (match work_item {
        Ok(value) => value,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "translation_retry_failed"})),
            );
        }
    }) else {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "translation_request_not_retryable"})),
        );
    };
    let now = Utc::now();
    let updated = sqlx::query(
        "UPDATE public.translation_requests SET status = 'queued', result_status = NULL, error_text = NULL, updated_at = $1 WHERE id = $2 AND scope_user_id = $3 AND status IN ('failed', 'completed')",
    )
    .bind(now)
    .bind(&request_id)
    .bind(&user_id)
    .execute(&mut *tx)
    .await;
    let work_updated = sqlx::query(
        "UPDATE public.translation_work_items SET status = 'queued', result_status = NULL, error_text = NULL, updated_at = $1 WHERE id = $2",
    )
    .bind(now)
    .bind(&work_item_id)
    .execute(&mut *tx)
    .await;
    let task_id = local_id::generate_local_id();
    let task_result = sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at) VALUES ($1, 'translation.generate', 'queued', 'api.translate.retry', $2, $3, $4, $4)",
    )
    .bind(&task_id)
    .bind(&user_id)
    .bind(json!({"work_item_id": work_item_id}).to_string())
    .bind(now)
    .execute(&mut *tx)
    .await;
    if updated.is_err()
        || work_updated.is_err()
        || task_result.is_err()
        || tx.commit().await.is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "translation_retry_failed"})),
        );
    }
    (
        StatusCode::ACCEPTED,
        axum::Json(json!({"request_id": request_id, "task_id": task_id, "status": "queued"})),
    )
}

async fn resolve_translation_results(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<TranslationResolveRequest>,
) -> impl IntoResponse {
    let Some(user_id) = session.get::<String>("user_id").await.ok().flatten() else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"code": "unauthorized"})),
        );
    };
    let mut resolved = 0usize;
    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(json!({"code": "postgres_unavailable"})),
            );
        }
    };
    for (request_id, _) in request.request_ids {
        if let Ok(result) = sqlx::query(
            "UPDATE public.translation_requests SET status = 'completed', result_status = 'ready', error_text = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND scope_user_id = $2 AND status IN ('queued', 'running', 'failed')",
        )
        .bind(request_id)
        .bind(&user_id)
        .execute(&mut *tx)
        .await
        {
            resolved += result.rows_affected() as usize;
        }
    }
    if tx.commit().await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"code": "translation_resolve_failed"})),
        );
    }
    (
        StatusCode::OK,
        axum::Json(json!({"resolved": resolved, "status": "completed"})),
    )
}

async fn stream_translation_request(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(request_id): Path<String>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let Some(user_id) = session
        .get::<String>("user_id")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let row = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, Option<String>)>(
        "SELECT id, status, result_status, title_zh, summary_md, body_md FROM public.translation_requests WHERE id = $1 AND scope_user_id = $2",
    )
    .bind(&request_id)
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    let payload = json!({
        "request_id": row.0,
        "status": row.1,
        "result_status": row.2,
        "title_zh": row.3,
        "summary_md": row.4,
        "body_md": row.5,
    });
    let event = Event::default()
        .event("translation.status")
        .data(payload.to_string());
    Ok(Sse::new(stream::iter([Ok(event)])).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

async fn shutdown_signal() {
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate => {}
    }
}
