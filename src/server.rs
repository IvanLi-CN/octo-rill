use std::{
    net::SocketAddr, path::Path, path::PathBuf, str::FromStr, sync::Arc,
    time::Duration as StdDuration,
};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, patch, post, put},
};
use serde_json::json;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use time::Duration;
use tokio::task::AbortHandle;
use tower::ServiceExt;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tower_sessions::cookie::SameSite;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;
use tracing::info;

use crate::runtime::SQLITE_BUSY_TIMEOUT;
use crate::state::AppState;
use crate::{
    admin_runtime, ai, api, auth, config::AppConfig, jobs, runtime, state, sync, translations,
    version,
};

const SQLITE_POOL_MAX_CONNECTIONS: u32 = 1;
const SESSION_COOKIE_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;
const STATIC_ASSET_EXTENSIONS: &[&str] = &[
    "avif",
    "bmp",
    "css",
    "eot",
    "gif",
    "html",
    "ico",
    "jpeg",
    "jpg",
    "js",
    "json",
    "map",
    "mjs",
    "otf",
    "pdf",
    "png",
    "svg",
    "ttf",
    "txt",
    "wasm",
    "webmanifest",
    "webp",
    "woff",
    "woff2",
    "xml",
];

pub async fn serve(config: AppConfig) -> Result<()> {
    ensure_sqlite_dir(&config.database_url)?;
    ensure_dir_exists(&config.task_log_dir)?;

    let connect_opts = build_sqlite_connect_options(&config.database_url)?;

    let pool = build_sqlite_pool_options()
        .connect_with(connect_opts)
        .await
        .context("failed to open sqlite database")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to apply database migrations")?;

    let runtime_settings = admin_runtime::load_or_seed_runtime_settings(&pool, &config)
        .await
        .context("failed to load admin runtime settings")?;

    let pragmas = read_sqlite_runtime_pragmas(&pool).await?;
    info!(
        journal_mode = %pragmas.journal_mode,
        busy_timeout_ms = pragmas.busy_timeout_ms,
        synchronous = pragmas.synchronous,
        pool_max_connections = SQLITE_POOL_MAX_CONNECTIONS,
        "sqlite runtime pragmas active"
    );

    let session_store = SqliteStore::new(pool.clone());
    session_store
        .migrate()
        .await
        .context("failed to migrate session store")?;
    let deletion_task = tokio::spawn(
        session_store
            .clone()
            .continuously_delete_expired(StdDuration::from_secs(60)),
    );
    let deletion_abort_handle = deletion_task.abort_handle();

    let github_oauth = state::build_oauth_client(&config)?;
    let linuxdo_oauth = state::build_linuxdo_oauth_client(&config)?;
    let http = reqwest::Client::builder()
        .user_agent("OctoRill")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build http client")?;

    let app_state = Arc::new(AppState {
        llm_scheduler: Arc::new(ai::LlmScheduler::new(runtime_settings.llm_max_concurrency)),
        translation_scheduler: Arc::new(translations::TranslationSchedulerController::new(
            translations::TranslationRuntimeConfig::new(
                runtime_settings.translation_general_worker_concurrency,
                runtime_settings.translation_dedicated_worker_concurrency,
            ),
        )),
        config: config.clone(),
        pool: pool.clone(),
        http,
        github_oauth,
        linuxdo_oauth,
        encryption_key: config.encryption_key.clone(),
        runtime_owner_id: crate::local_id::generate_local_id(),
    });

    let addr: SocketAddr = config.bind_addr;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind TCP listener")?;

    let is_secure_cookie = config.public_base_url.scheme() == "https";
    let session_cookie_name = build_session_cookie_name(&config);
    let session_layer = SessionManagerLayer::new(session_store)
        .with_name(session_cookie_name)
        .with_secure(is_secure_cookie)
        .with_same_site(SameSite::Lax)
        .with_expiry(session_inactivity_expiry());

    let api_router = Router::new()
        .route(
            "/health",
            get(api_health).layer(middleware::from_fn(version_no_store_cache)),
        )
        .route(
            "/version",
            get(api_version).layer(middleware::from_fn(version_no_store_cache)),
        )
        .route("/me", get(api::me))
        .route(
            "/me/profile",
            get(api::me_get_profile).patch(api::me_patch_profile),
        )
        .route("/tasks/{task_id}/events", get(api::task_events_sse))
        .route("/starred", get(api::list_starred))
        .route("/releases", get(api::list_releases))
        .route(
            "/releases/{release_id}/detail",
            get(api::get_release_detail),
        )
        .route("/notifications", get(api::list_notifications))
        .route("/feed", get(api::list_feed))
        .route("/admin/users", get(api::admin_list_users))
        .route("/admin/users/{user_id}", patch(api::admin_patch_user))
        .route(
            "/admin/users/{user_id}/profile",
            get(api::admin_get_user_profile).patch(api::admin_patch_user_profile),
        )
        .route("/admin/dashboard", get(api::admin_dashboard))
        .route("/admin/jobs/overview", get(api::admin_jobs_overview))
        .route("/admin/jobs/events", get(api::admin_jobs_events_sse))
        .route("/admin/jobs/realtime", get(api::admin_list_realtime_tasks))
        .route(
            "/admin/jobs/realtime/{task_id}",
            get(api::admin_get_realtime_task_detail),
        )
        .route(
            "/admin/jobs/realtime/{task_id}/log",
            get(api::admin_download_realtime_task_log),
        )
        .route(
            "/admin/jobs/realtime/{task_id}/retry",
            post(api::admin_retry_realtime_task),
        )
        .route(
            "/admin/jobs/realtime/{task_id}/cancel",
            post(api::admin_cancel_realtime_task),
        )
        .route(
            "/admin/jobs/scheduled",
            get(api::admin_list_scheduled_slots),
        )
        .route(
            "/admin/jobs/scheduled/{hour_utc}",
            patch(api::admin_patch_scheduled_slot),
        )
        .route(
            "/admin/jobs/llm/status",
            get(api::admin_get_llm_scheduler_status),
        )
        .route(
            "/admin/jobs/llm/runtime-config",
            patch(api::admin_patch_llm_runtime_config),
        )
        .route("/admin/jobs/llm/calls", get(api::admin_list_llm_calls))
        .route(
            "/admin/jobs/llm/calls/{call_id}",
            get(api::admin_get_llm_call_detail),
        )
        .route(
            "/admin/jobs/translations/status",
            get(translations::admin_get_translation_status),
        )
        .route(
            "/admin/jobs/translations/runtime-config",
            patch(translations::admin_patch_translation_runtime_config),
        )
        .route(
            "/admin/jobs/translations/requests",
            get(translations::admin_list_translation_requests),
        )
        .route(
            "/admin/jobs/translations/requests/{request_id}",
            get(translations::admin_get_translation_request_detail),
        )
        .route(
            "/admin/jobs/translations/batches",
            get(translations::admin_list_translation_batches),
        )
        .route(
            "/admin/jobs/translations/batches/{batch_id}",
            get(translations::admin_get_translation_batch_detail),
        )
        .route("/reaction-token/status", get(api::reaction_token_status))
        .route(
            "/me/linuxdo",
            get(api::me_get_linuxdo).delete(api::me_delete_linuxdo),
        )
        .route("/reaction-token/check", post(api::check_reaction_token))
        .route("/reaction-token", put(api::upsert_reaction_token))
        .route(
            "/release/reactions/toggle",
            post(api::toggle_release_reaction),
        )
        .route("/briefs", get(api::list_briefs))
        .route("/briefs/generate", post(api::generate_brief))
        .route(
            "/translate/requests",
            post(translations::submit_translation_request),
        )
        .route(
            "/translate/requests/{request_id}",
            get(translations::get_translation_request),
        )
        .route(
            "/translate/results",
            post(translations::resolve_translation_results),
        )
        .route(
            "/translate/requests/{request_id}/stream",
            get(translations::stream_translation_request),
        )
        .route(
            "/translate/releases/batch",
            post(api::translate_releases_batch),
        )
        .route(
            "/translate/releases/batch/stream",
            post(api::translate_releases_batch_stream),
        )
        .route("/translate/release", post(api::translate_release))
        .route(
            "/translate/release/detail/batch",
            post(api::translate_release_detail_batch),
        )
        .route(
            "/translate/release/detail",
            post(api::translate_release_detail),
        )
        .route(
            "/translate/notifications/batch",
            post(api::translate_notifications_batch),
        )
        .route("/translate/notification", post(api::translate_notification))
        .route("/sync/starred", post(api::sync_starred))
        .route("/sync/all", post(api::sync_all))
        .route("/sync/releases", post(api::sync_releases))
        .route("/sync/notifications", post(api::sync_notifications));

    let mut app = Router::new()
        .nest("/api", api_router)
        .route("/auth/github/login", get(auth::github_login))
        .route("/auth/github/callback", get(auth::github_callback))
        .route("/auth/linuxdo/login", get(auth::linuxdo_login))
        .route("/auth/linuxdo/callback", get(auth::linuxdo_callback))
        .route("/auth/logout", get(auth::logout))
        .with_state(app_state.clone())
        .layer(session_layer);

    if let Some(static_dir) = config.static_dir.clone() {
        app = attach_static_site_routes(app, static_dir);
    }

    let cors_origin = state::normalize_origin(&config.public_base_url)?;
    let cors_origin: HeaderValue = cors_origin
        .as_str()
        .parse()
        .context("invalid cors origin")?;

    let cors = CorsLayer::new()
        .allow_origin(cors_origin)
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH])
        .allow_headers([axum::http::header::CONTENT_TYPE]);

    let app = app.layer(cors).layer(TraceLayer::new_for_http());

    runtime::register_runtime_owner(app_state.as_ref()).await?;
    let runtime_owner_heartbeat = runtime::spawn_runtime_owner_heartbeat(app_state.clone());

    let serve_result = async {
        jobs::recover_runtime_state_on_startup(app_state.as_ref()).await?;
        sync::recover_repo_release_runtime_state_on_startup(app_state.as_ref()).await?;
        translations::recover_runtime_state_on_startup(app_state.as_ref()).await?;
        ai::recover_runtime_state_on_startup(app_state.as_ref()).await?;
        let backfilled_daily_brief_preferences =
            crate::briefs::backfill_legacy_daily_brief_preferences(app_state.as_ref()).await?;
        if backfilled_daily_brief_preferences > 0 {
            info!(
                backfilled_daily_brief_preferences,
                "backfilled legacy daily brief preferences on startup"
            );
        }

        jobs::spawn_task_workers(app_state.clone(), config.job_worker_concurrency);
        let task_recovery_abort_handle = jobs::spawn_task_recovery_worker(app_state.clone());
        sync::spawn_repo_release_workers(app_state.clone());
        let repo_release_recovery_abort_handle =
            sync::spawn_repo_release_recovery_worker(app_state.clone());
        jobs::spawn_hourly_scheduler(app_state.clone());
        jobs::spawn_subscription_scheduler(app_state.clone());
        jobs::spawn_admin_dashboard_rollup_scheduler(app_state.clone());
        if let Err(err) = jobs::enqueue_brief_history_recompute_if_needed(app_state.as_ref()).await
        {
            tracing::warn!(?err, "failed to enqueue brief history recompute bootstrap");
        }
        if let Err(err) = jobs::enqueue_brief_refresh_content_if_needed(app_state.as_ref()).await {
            tracing::warn!(?err, "failed to enqueue brief content refresh bootstrap");
        }
        let model_catalog_abort_handle = config
            .ai
            .as_ref()
            .map(|_| ai::spawn_model_catalog_sync_task(app_state.clone()));
        let llm_call_retention_abort_handle = ai::spawn_llm_call_retention_task(app_state.clone());
        let llm_call_recovery_abort_handle = ai::spawn_llm_call_recovery_task(app_state.clone());
        translations::spawn_translation_scheduler(app_state.clone()).await;
        let translation_recovery_abort_handle =
            translations::spawn_translation_recovery_task(app_state.clone());

        info!(%addr, "listening");

        let mut abort_handles = vec![
            deletion_abort_handle,
            llm_call_retention_abort_handle,
            llm_call_recovery_abort_handle,
            task_recovery_abort_handle,
            repo_release_recovery_abort_handle,
            translation_recovery_abort_handle,
        ];
        if let Some(handle) = model_catalog_abort_handle {
            abort_handles.push(handle);
        }

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(app_state.clone(), abort_handles))
            .await
            .context("http server exited")
    }
    .await;

    runtime_owner_heartbeat.stop().await;
    let unregister_result = runtime::unregister_runtime_owner(app_state.as_ref()).await;
    if let Err(err) = unregister_result {
        if serve_result.is_ok() {
            return Err(err);
        }
        tracing::warn!(?err, "failed to unregister runtime owner after server exit");
    }

    serve_result?;

    Ok(())
}

fn ensure_dir_exists(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))
}

fn ensure_sqlite_dir(database_url: &str) -> Result<()> {
    if database_url == "sqlite::memory:" {
        return Ok(());
    }

    let Some(path_part) = database_url.strip_prefix("sqlite:") else {
        return Ok(());
    };

    let path_part = path_part
        .trim_start_matches("//")
        .split('?')
        .next()
        .unwrap_or("");
    if path_part.is_empty() {
        return Ok(());
    }

    let path = Path::new(path_part);
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }

    std::fs::create_dir_all(parent).context("failed to create sqlite parent directory")?;
    Ok(())
}

fn build_sqlite_connect_options(database_url: &str) -> Result<SqliteConnectOptions> {
    let mut connect_opts = SqliteConnectOptions::from_str(database_url)
        .context("invalid DATABASE_URL for sqlite")?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
        .synchronous(SqliteSynchronous::Normal);

    if database_url != "sqlite::memory:" {
        connect_opts = connect_opts.journal_mode(SqliteJournalMode::Wal);
    }

    Ok(connect_opts)
}

fn build_sqlite_pool_options() -> SqlitePoolOptions {
    // OctoRill runs against a single local SQLite file. Multiple pool connections inside the same
    // process increase self-contention because SQLite still serializes writes. Keeping one shared
    // connection avoids reintroducing `database is locked` during background workers + lease
    // heartbeats, while retryable smart failures are now re-queued by the translation layer.
    SqlitePoolOptions::new()
        .max_connections(SQLITE_POOL_MAX_CONNECTIONS)
        .min_connections(1)
}

#[derive(Debug, PartialEq, Eq)]
struct SqliteRuntimePragmas {
    journal_mode: String,
    busy_timeout_ms: i64,
    synchronous: i64,
}

async fn read_sqlite_runtime_pragmas(pool: &SqlitePool) -> Result<SqliteRuntimePragmas> {
    let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
        .fetch_one(pool)
        .await
        .context("read sqlite journal_mode failed")?;
    let busy_timeout_ms = sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
        .fetch_one(pool)
        .await
        .context("read sqlite busy_timeout failed")?;
    let synchronous = sqlx::query_scalar::<_, i64>("PRAGMA synchronous")
        .fetch_one(pool)
        .await
        .context("read sqlite synchronous failed")?;

    Ok(SqliteRuntimePragmas {
        journal_mode,
        busy_timeout_ms,
        synchronous,
    })
}

async fn shutdown_signal(state: Arc<AppState>, abort_handles: Vec<AbortHandle>) {
    let abort_all = || {
        for handle in &abort_handles {
            handle.abort();
        }
    };

    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

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
        _ = ctrl_c => abort_all(),
        _ = terminate => abort_all(),
    }

    state.translation_scheduler.abort_all().await;
}

async fn api_health() -> axum::Json<serde_json::Value> {
    let info = version::resolve_effective_version();
    axum::Json(json!({
        "ok": true,
        "version": info.version,
    }))
}

async fn api_version() -> axum::Json<serde_json::Value> {
    let info = version::resolve_effective_version();
    axum::Json(json!({
        "ok": true,
        "version": info.version,
        "source": info.source,
    }))
}

fn apply_no_store_headers(headers: &mut axum::http::HeaderMap) {
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    headers.insert(
        axum::http::header::PRAGMA,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert(axum::http::header::EXPIRES, HeaderValue::from_static("0"));
}

async fn version_no_store_cache(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    apply_no_store_headers(response.headers_mut());
    response
}

fn attach_static_site_routes(app: Router, static_dir: PathBuf) -> Router {
    let index = static_dir.join("index.html");
    let spa_shell = ServeFile::new(index.clone());
    let spa_static = Arc::new(SpaStaticPaths {
        static_dir,
        index_path: index,
    });

    app.route_service("/", spa_shell.clone())
        .route_service("/admin", spa_shell.clone())
        .route_service("/admin/{*path}", spa_shell)
        .fallback({
            move |request: Request| {
                let spa_static = Arc::clone(&spa_static);
                async move { spa_document_fallback_handler(request, spa_static).await }
            }
        })
}

#[derive(Clone)]
struct SpaStaticPaths {
    static_dir: PathBuf,
    index_path: PathBuf,
}

async fn spa_document_fallback_handler(
    request: Request,
    spa_static: Arc<SpaStaticPaths>,
) -> Response {
    if looks_like_static_asset_path(request.uri().path()) {
        return match ServeDir::new(spa_static.static_dir.clone())
            .oneshot(request)
            .await
        {
            Ok(response) => into_axum_body(response),
            Err(err) => match err {},
        };
    }

    if should_serve_spa_shell(request.method(), request.uri(), request.headers()) {
        return match ServeFile::new(spa_static.index_path.clone())
            .oneshot(request)
            .await
        {
            Ok(response) => into_axum_body(response),
            Err(err) => match err {},
        };
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::empty())
        .expect("404 fallback response should build")
}

fn into_axum_body<B>(response: axum::http::Response<B>) -> Response
where
    B: axum::body::HttpBody<Data = axum::body::Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
    response.map(Body::new)
}

fn should_serve_spa_shell(method: &Method, uri: &Uri, headers: &HeaderMap) -> bool {
    matches!(*method, Method::GET | Method::HEAD)
        && !is_reserved_backend_path(uri.path())
        && !looks_like_static_asset_path(uri.path())
        && accepts_html_document(headers)
}

fn is_reserved_backend_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/") || path == "/auth" || path.starts_with("/auth/")
}

fn looks_like_static_asset_path(path: &str) -> bool {
    let Some(segment) = path.rsplit('/').next() else {
        return false;
    };
    if segment.is_empty() {
        return false;
    }

    let Some((_, extension)) = segment.rsplit_once('.') else {
        return false;
    };

    STATIC_ASSET_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn accepts_html_document(headers: &HeaderMap) -> bool {
    if headers
        .get("sec-fetch-dest")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("document"))
    {
        return true;
    }

    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html") || value.contains("application/xhtml+xml"))
}

fn build_session_cookie_name(config: &AppConfig) -> String {
    let Some(host) = config.public_base_url.host_str() else {
        return "octo_rill_sid".to_owned();
    };

    let scheme = config.public_base_url.scheme();
    let normalized_path = normalize_cookie_name_path(config.public_base_url.path());
    let effective_port = config
        .public_base_url
        .port_or_known_default()
        .unwrap_or(config.bind_addr.port());
    let is_loopback_host = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    let has_non_root_path = normalized_path != "/";
    let uses_non_default_port = config
        .public_base_url
        .port()
        .is_some_and(|port| !matches!((scheme, port), ("http", 80) | ("https", 443)));
    let is_fixed_https_root =
        scheme == "https" && !is_loopback_host && !has_non_root_path && !uses_non_default_port;

    if is_fixed_https_root {
        return "octo_rill_sid".to_owned();
    }

    let discriminator = crate::ai::sha256_hex(&format!(
        "{scheme}\n{host}\n{effective_port}\n{}",
        normalized_path
    ));
    format!("octo_rill_sid_{}", &discriminator[..16])
}

fn normalize_cookie_name_path(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() { "/" } else { trimmed }
}

fn session_inactivity_expiry() -> Expiry {
    Expiry::OnInactivity(Duration::seconds(SESSION_COOKIE_MAX_AGE_SECS))
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, SESSION_COOKIE_MAX_AGE_SECS, SQLITE_POOL_MAX_CONNECTIONS, SameSite,
        accepts_html_document, api_health, api_version, apply_no_store_headers,
        attach_static_site_routes, build_session_cookie_name, build_sqlite_connect_options,
        build_sqlite_pool_options, looks_like_static_asset_path, read_sqlite_runtime_pragmas,
        session_inactivity_expiry, should_serve_spa_shell,
    };
    use axum::{
        Router,
        body::Body,
        http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
        routing::get,
    };
    use std::{fs, time::SystemTime};
    use tower::ServiceExt;
    use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

    async fn create_test_session(session: Session) -> StatusCode {
        session
            .insert("user_id", "test-user")
            .await
            .expect("insert user id into session");
        StatusCode::NO_CONTENT
    }

    async fn read_test_session(session: Session) -> StatusCode {
        match session
            .get::<String>("user_id")
            .await
            .expect("read user id from session")
        {
            Some(_) => {
                session
                    .insert("activity_touched_at", chrono::Utc::now().timestamp())
                    .await
                    .expect("touch session activity");
                StatusCode::NO_CONTENT
            }
            None => StatusCode::UNAUTHORIZED,
        }
    }

    fn test_session_layer(cookie_name: &'static str) -> SessionManagerLayer<MemoryStore> {
        SessionManagerLayer::new(MemoryStore::default())
            .with_name(cookie_name.to_owned())
            .with_same_site(SameSite::Lax)
            .with_expiry(session_inactivity_expiry())
    }

    #[tokio::test]
    async fn api_version_reports_non_empty_version_and_source() {
        let payload = api_version().await.0;

        let version = payload
            .get("version")
            .and_then(serde_json::Value::as_str)
            .expect("version should be present");
        assert!(!version.trim().is_empty(), "version should never be blank");

        let source = payload
            .get("source")
            .and_then(serde_json::Value::as_str)
            .expect("source should be present");
        assert!(
            matches!(source, "APP_EFFECTIVE_VERSION" | "CARGO_PKG_VERSION"),
            "unexpected source: {source}"
        );
    }

    #[tokio::test]
    async fn api_health_and_api_version_share_the_same_version_value() {
        let health_payload = api_health().await.0;
        let version_payload = api_version().await.0;

        assert_eq!(health_payload.get("ok"), Some(&serde_json::json!(true)));
        assert_eq!(version_payload.get("ok"), Some(&serde_json::json!(true)));
        assert_eq!(
            health_payload.get("version"),
            version_payload.get("version"),
            "health/version endpoints should agree on effective version"
        );
    }

    #[test]
    fn version_endpoints_disable_cache_storage() {
        let mut headers = HeaderMap::new();
        apply_no_store_headers(&mut headers);

        assert_eq!(
            headers.get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static(
                "no-store, no-cache, must-revalidate"
            ))
        );
        assert_eq!(
            headers.get(header::PRAGMA),
            Some(&HeaderValue::from_static("no-cache"))
        );
        assert_eq!(
            headers.get(header::EXPIRES),
            Some(&HeaderValue::from_static("0"))
        );
    }

    #[test]
    fn session_cookie_name_is_fixed_for_root_public_origin() {
        let config = test_config("https://octo-rill.ivanli.cc");

        assert_eq!(build_session_cookie_name(&config), "octo_rill_sid");
    }

    #[test]
    fn session_cookie_name_keeps_fixed_name_for_explicit_default_https_port() {
        let config = test_config("https://example.com:443/");

        assert_eq!(build_session_cookie_name(&config), "octo_rill_sid");
    }

    #[test]
    fn session_cookie_name_derives_for_loopback_multi_instance_isolation() {
        let first = test_config("http://127.0.0.1:58090");
        let second = test_config("http://127.0.0.1:58091");
        let first_cookie_name = build_session_cookie_name(&first);
        let second_cookie_name = build_session_cookie_name(&second);

        assert_ne!(first_cookie_name, "octo_rill_sid");
        assert_ne!(first_cookie_name, second_cookie_name);
    }

    #[test]
    fn session_cookie_name_derives_for_non_root_public_path() {
        let config = test_config("https://example.com/octo-rill");

        assert_ne!(build_session_cookie_name(&config), "octo_rill_sid");
    }

    #[test]
    fn session_cookie_name_derives_for_root_http_origin() {
        let http_config = test_config("http://example.com");
        let https_config = test_config("https://example.com");

        assert_ne!(build_session_cookie_name(&http_config), "octo_rill_sid");
        assert_ne!(
            build_session_cookie_name(&http_config),
            build_session_cookie_name(&https_config)
        );
    }

    #[test]
    fn session_cookie_name_keeps_distinct_paths_isolated_without_lossy_collisions() {
        let dotted_path = test_config("https://example.com/foo.bar");
        let nested_path = test_config("https://example.com/foo/bar");

        assert_ne!(
            build_session_cookie_name(&dotted_path),
            build_session_cookie_name(&nested_path)
        );
    }

    #[test]
    fn session_cookie_name_treats_trailing_slash_variants_as_same_path() {
        let without_trailing_slash = test_config("https://example.com/octo-rill");
        let with_trailing_slash = test_config("https://example.com/octo-rill/");

        assert_eq!(
            build_session_cookie_name(&without_trailing_slash),
            build_session_cookie_name(&with_trailing_slash)
        );
    }

    fn test_config(public_base_url: &str) -> AppConfig {
        AppConfig {
            bind_addr: "127.0.0.1:58090".parse().expect("parse bind addr"),
            public_base_url: url::Url::parse(public_base_url).expect("parse public base url"),
            database_url: "sqlite::memory:".to_owned(),
            static_dir: None,
            task_log_dir: std::env::temp_dir().join("octo-rill-server-tests"),
            job_worker_concurrency: 1,
            encryption_key: crate::crypto::EncryptionKey::from_base64(
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            )
            .expect("build encryption key"),
            github: crate::config::GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: url::Url::parse("https://octo-rill.ivanli.cc/auth/github/callback")
                    .expect("parse redirect url"),
            },
            linuxdo: None,
            ai: None,
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: crate::briefs::DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned(),
        }
    }

    #[tokio::test]
    async fn session_layer_sets_persistent_cookie_and_refreshes_on_valid_request() {
        let app = Router::new()
            .route("/login", get(create_test_session))
            .route("/me", get(read_test_session))
            .layer(test_session_layer("octo_rill_sid_test"));

        let login_response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/login")
                    .body(Body::empty())
                    .expect("build login request"),
            )
            .await
            .expect("login response");
        let login_cookie = login_response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("login set-cookie header");

        assert!(login_cookie.contains("octo_rill_sid_test="));
        assert!(login_cookie.contains(&format!("Max-Age={SESSION_COOKIE_MAX_AGE_SECS}")));

        let cookie_header = login_cookie
            .split(';')
            .next()
            .expect("cookie pair")
            .to_owned();

        let me_response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/me")
                    .header(header::COOKIE, cookie_header)
                    .body(Body::empty())
                    .expect("build me request"),
            )
            .await
            .expect("me response");

        assert_eq!(me_response.status(), StatusCode::NO_CONTENT);

        let refresh_cookie = me_response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("refresh set-cookie header");
        assert!(refresh_cookie.contains("octo_rill_sid_test="));
        assert!(refresh_cookie.contains(&format!("Max-Age={SESSION_COOKIE_MAX_AGE_SECS}")));
    }

    #[tokio::test]
    async fn session_layer_clears_unknown_cookie_values() {
        let app = Router::new()
            .route("/me", get(read_test_session))
            .layer(test_session_layer("octo_rill_sid_test"));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/me")
                    .header(header::COOKIE, "octo_rill_sid_test=missing-session")
                    .body(Body::empty())
                    .expect("build me request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let clear_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("clear set-cookie header");
        assert!(clear_cookie.contains("octo_rill_sid_test="));
        assert!(clear_cookie.contains("Max-Age=0"));
    }

    #[tokio::test]
    async fn sqlite_runtime_pragmas_enable_wal_and_busy_timeout() {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-server-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let database_url = format!("sqlite:{}", database_path.display());
        let pool = build_sqlite_pool_options()
            .connect_with(
                build_sqlite_connect_options(&database_url).expect("build sqlite connect options"),
            )
            .await
            .expect("connect sqlite pool");

        let pragmas = read_sqlite_runtime_pragmas(&pool)
            .await
            .expect("read sqlite pragmas");

        assert_eq!(pragmas.journal_mode, "wal");
        assert_eq!(pragmas.busy_timeout_ms, 5000);
        assert_eq!(pragmas.synchronous, 1);
    }

    #[test]
    fn sqlite_pool_uses_single_connection_to_avoid_self_contention() {
        let _ = build_sqlite_pool_options();
        assert_eq!(SQLITE_POOL_MAX_CONNECTIONS, 1);
    }

    #[test]
    fn html_navigation_accept_header_is_detected() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml"),
        );

        assert!(accepts_html_document(&headers));
    }

    #[test]
    fn sec_fetch_document_header_is_detected() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-dest", HeaderValue::from_static("document"));

        assert!(accepts_html_document(&headers));
    }

    #[test]
    fn path_with_extension_is_treated_as_static_asset() {
        assert!(looks_like_static_asset_path("/assets/index-abc123.js"));
        assert!(looks_like_static_asset_path("/favicon.ico"));
        assert!(!looks_like_static_asset_path("/settings"));
        assert!(!looks_like_static_asset_path("/admin/jobs"));
        assert!(!looks_like_static_asset_path(
            "/admin/jobs/tasks/task-sync.subscriptions"
        ));
    }

    #[test]
    fn spa_shell_fallback_only_applies_to_html_navigation_paths() {
        let html_headers = HeaderMap::from_iter([(
            header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml"),
        )]);
        let json_headers =
            HeaderMap::from_iter([(header::ACCEPT, HeaderValue::from_static("application/json"))]);

        assert!(should_serve_spa_shell(
            &Method::GET,
            &Uri::from_static("/settings"),
            &html_headers,
        ));
        assert!(should_serve_spa_shell(
            &Method::GET,
            &Uri::from_static("/does-not-exist"),
            &html_headers,
        ));
        assert!(!should_serve_spa_shell(
            &Method::GET,
            &Uri::from_static("/assets/index.js"),
            &html_headers,
        ));
        assert!(!should_serve_spa_shell(
            &Method::GET,
            &Uri::from_static("/auth/missing"),
            &html_headers,
        ));
        assert!(!should_serve_spa_shell(
            &Method::GET,
            &Uri::from_static("/settings"),
            &json_headers,
        ));
    }

    #[tokio::test]
    async fn static_routes_fallback_to_spa_shell_without_masking_assets_or_api_404s() {
        let fixture_root = std::env::temp_dir().join(format!(
            "octo-rill-static-fixture-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("fixture time")
                .as_nanos()
        ));
        fs::create_dir_all(fixture_root.join("assets")).expect("create assets dir");
        fs::write(
            fixture_root.join("index.html"),
            "<!doctype html><html><body>spa-shell</body></html>",
        )
        .expect("write index.html");
        fs::write(
            fixture_root.join("assets/app.js"),
            "console.log('asset-ok');",
        )
        .expect("write asset file");

        let app = attach_static_site_routes(
            Router::new()
                .nest(
                    "/api",
                    Router::new().route("/health", get(|| async { "api-ok" })),
                )
                .route("/auth/logout", get(|| async { "bye" })),
            fixture_root.clone(),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("resolve test listener addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test router");
        });

        let client = reqwest::Client::new();

        let settings_response = client
            .get(format!("http://{addr}/settings?section=github-pat"))
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await
            .expect("request settings");
        assert_eq!(settings_response.status(), StatusCode::OK);
        assert!(
            settings_response
                .text()
                .await
                .expect("read settings body")
                .contains("spa-shell")
        );

        let unknown_app_route = client
            .get(format!("http://{addr}/does-not-exist"))
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await
            .expect("request unknown app route");
        assert_eq!(unknown_app_route.status(), StatusCode::OK);
        assert!(
            unknown_app_route
                .text()
                .await
                .expect("read unknown app route body")
                .contains("spa-shell")
        );

        let dotted_admin_route = client
            .get(format!(
                "http://{addr}/admin/jobs/tasks/task-sync.subscriptions"
            ))
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await
            .expect("request dotted admin route");
        assert_eq!(dotted_admin_route.status(), StatusCode::OK);
        assert!(
            dotted_admin_route
                .text()
                .await
                .expect("read dotted admin route body")
                .contains("spa-shell")
        );

        let asset_response = client
            .get(format!("http://{addr}/assets/app.js"))
            .send()
            .await
            .expect("request asset");
        assert_eq!(asset_response.status(), StatusCode::OK);
        assert_eq!(
            asset_response.text().await.expect("read asset body"),
            "console.log('asset-ok');"
        );

        let missing_asset_response = client
            .get(format!("http://{addr}/assets/missing.js"))
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await
            .expect("request missing asset");
        assert_eq!(missing_asset_response.status(), StatusCode::NOT_FOUND);

        let missing_api_response = client
            .get(format!("http://{addr}/api/missing"))
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await
            .expect("request missing api");
        assert_eq!(missing_api_response.status(), StatusCode::NOT_FOUND);

        let missing_auth_response = client
            .get(format!("http://{addr}/auth/missing"))
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await
            .expect("request missing auth");
        assert_eq!(missing_auth_response.status(), StatusCode::NOT_FOUND);

        let head_settings_response = client
            .head(format!("http://{addr}/settings"))
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await
            .expect("head settings");
        assert_eq!(head_settings_response.status(), StatusCode::OK);

        fs::remove_dir_all(fixture_root).expect("remove static fixture");
    }
}
