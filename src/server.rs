use std::{net::SocketAddr, path::Path, str::FromStr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    http::{HeaderValue, Method},
    routing::{get, patch, post, put},
};
use serde_json::json;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tokio::task::AbortHandle;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tower_sessions::SessionManagerLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions_sqlx_store::SqliteStore;
use tracing::info;

use crate::config::AppConfig;
use crate::runtime::SQLITE_BUSY_TIMEOUT;
use crate::state::AppState;
use crate::{ai, api, auth, jobs, state, translations, version};

pub async fn serve(config: AppConfig) -> Result<()> {
    ensure_sqlite_dir(&config.database_url)?;
    ensure_dir_exists(&config.task_log_dir)?;

    let connect_opts = build_sqlite_connect_options(&config.database_url)?;

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_opts)
        .await
        .context("failed to open sqlite database")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to apply database migrations")?;

    let pragmas = read_sqlite_runtime_pragmas(&pool).await?;
    info!(
        journal_mode = %pragmas.journal_mode,
        busy_timeout_ms = pragmas.busy_timeout_ms,
        synchronous = pragmas.synchronous,
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
            .continuously_delete_expired(Duration::from_secs(60)),
    );
    let deletion_abort_handle = deletion_task.abort_handle();

    let oauth = state::build_oauth_client(&config)?;
    let http = reqwest::Client::builder()
        .user_agent("OctoRill")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build http client")?;

    let app_state = Arc::new(AppState {
        llm_scheduler: Arc::new(ai::LlmScheduler::new(config.ai_max_concurrency)),
        config: config.clone(),
        pool: pool.clone(),
        http,
        oauth,
        encryption_key: config.encryption_key.clone(),
        runtime_owner_id: crate::local_id::generate_local_id(),
    });

    jobs::recover_runtime_state_on_startup(app_state.as_ref()).await?;
    translations::recover_runtime_state_on_startup(app_state.as_ref()).await?;
    ai::recover_runtime_state_on_startup(app_state.as_ref()).await?;

    jobs::spawn_task_workers(app_state.clone(), config.job_worker_concurrency);
    let task_recovery_abort_handle = jobs::spawn_task_recovery_worker(app_state.clone());
    jobs::spawn_hourly_scheduler(app_state.clone());
    jobs::spawn_subscription_scheduler(app_state.clone());
    let model_catalog_abort_handle = config
        .ai
        .as_ref()
        .map(|_| ai::spawn_model_catalog_sync_task(app_state.clone()));
    let llm_call_retention_abort_handle = ai::spawn_llm_call_retention_task(app_state.clone());
    let llm_call_recovery_abort_handle = ai::spawn_llm_call_recovery_task(app_state.clone());
    let translation_scheduler_abort_handles =
        translations::spawn_translation_scheduler(app_state.clone());
    let translation_recovery_abort_handle =
        translations::spawn_translation_recovery_task(app_state.clone());

    let is_secure_cookie = config.public_base_url.scheme() == "https";
    let session_cookie_name = build_session_cookie_name(&config);
    let session_layer = SessionManagerLayer::new(session_store)
        .with_name(session_cookie_name)
        .with_secure(is_secure_cookie)
        .with_same_site(SameSite::Lax);

    let api_router = Router::new()
        .route("/health", get(api_health))
        .route("/version", get(api_version))
        .route("/me", get(api::me))
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
            get(api::admin_get_user_profile),
        )
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
            "/translate/requests/{request_id}/stream",
            get(translations::stream_translation_request),
        )
        .route(
            "/translate/releases/batch",
            post(translations::reject_legacy_translation_routes),
        )
        .route(
            "/translate/releases/batch/stream",
            post(translations::reject_legacy_translation_routes),
        )
        .route(
            "/translate/release",
            post(translations::reject_legacy_translation_routes),
        )
        .route(
            "/translate/release/detail/batch",
            post(translations::reject_legacy_translation_routes),
        )
        .route(
            "/translate/release/detail",
            post(translations::reject_legacy_translation_routes),
        )
        .route(
            "/translate/notifications/batch",
            post(translations::reject_legacy_translation_routes),
        )
        .route(
            "/translate/notification",
            post(translations::reject_legacy_translation_routes),
        )
        .route("/sync/starred", post(api::sync_starred))
        .route("/sync/releases", post(api::sync_releases))
        .route("/sync/notifications", post(api::sync_notifications));

    let mut app = Router::new()
        .nest("/api", api_router)
        .route("/auth/github/login", get(auth::github_login))
        .route("/auth/github/callback", get(auth::github_callback))
        .route("/auth/logout", get(auth::logout))
        .with_state(app_state)
        .layer(session_layer);

    if let Some(static_dir) = config.static_dir.clone() {
        let index = static_dir.join("index.html");
        let spa_shell = ServeFile::new(index.clone());
        app = app
            .route_service("/", spa_shell.clone())
            .route_service("/admin", spa_shell.clone())
            .route_service("/admin/{*path}", spa_shell)
            .fallback_service(ServeDir::new(static_dir).not_found_service(ServeFile::new(index)));
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

    let addr: SocketAddr = config.bind_addr;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind TCP listener")?;

    info!(%addr, "listening");

    let mut abort_handles = vec![
        deletion_abort_handle,
        llm_call_retention_abort_handle,
        llm_call_recovery_abort_handle,
        task_recovery_abort_handle,
        translation_recovery_abort_handle,
    ];
    abort_handles.extend(translation_scheduler_abort_handles);
    if let Some(handle) = model_catalog_abort_handle {
        abort_handles.push(handle);
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(abort_handles))
        .await
        .context("http server exited")?;

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

async fn shutdown_signal(abort_handles: Vec<AbortHandle>) {
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

fn build_session_cookie_name(config: &AppConfig) -> String {
    let host = config.public_base_url.host_str().unwrap_or("localhost");
    let port = config
        .public_base_url
        .port_or_known_default()
        .unwrap_or(config.bind_addr.port());
    let raw = format!("octo_rill_sid_{host}_{port}");
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::{
        api_health, api_version, build_sqlite_connect_options, read_sqlite_runtime_pragmas,
    };
    use sqlx::sqlite::SqlitePoolOptions;

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

    #[tokio::test]
    async fn sqlite_runtime_pragmas_enable_wal_and_busy_timeout() {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-server-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let database_url = format!("sqlite:{}", database_path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
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
}
