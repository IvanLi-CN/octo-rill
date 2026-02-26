use std::{net::SocketAddr, path::Path, str::FromStr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    http::{HeaderValue, Method},
    routing::{get, patch, post, put},
};
use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
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
use crate::state::AppState;
use crate::{ai, api, auth, jobs, state};

pub async fn serve(config: AppConfig) -> Result<()> {
    ensure_sqlite_dir(&config.database_url)?;

    let connect_opts = SqliteConnectOptions::from_str(&config.database_url)
        .context("invalid DATABASE_URL for sqlite")?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_opts)
        .await
        .context("failed to open sqlite database")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to apply database migrations")?;

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
        config: config.clone(),
        pool: pool.clone(),
        http,
        oauth,
        encryption_key: config.encryption_key.clone(),
    });

    jobs::spawn_task_worker(app_state.clone());
    jobs::spawn_hourly_scheduler(app_state.clone());
    let model_catalog_abort_handle = config
        .ai
        .as_ref()
        .map(|_| ai::spawn_model_catalog_sync_task(app_state.clone()));

    let is_secure_cookie = config.public_base_url.scheme() == "https";
    let session_cookie_name = build_session_cookie_name(&config);
    let session_layer = SessionManagerLayer::new(session_store)
        .with_name(session_cookie_name)
        .with_secure(is_secure_cookie)
        .with_same_site(SameSite::Lax);

    let api_router = Router::new()
        .route("/health", get(api_health))
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
        .route("/admin/jobs/realtime", get(api::admin_list_realtime_tasks))
        .route(
            "/admin/jobs/realtime/{task_id}",
            get(api::admin_get_realtime_task_detail),
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
        app = app
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

    let mut abort_handles = vec![deletion_abort_handle];
    if let Some(handle) = model_catalog_abort_handle {
        abort_handles.push(handle);
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(abort_handles))
        .await
        .context("http server exited")?;

    Ok(())
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
    axum::Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
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
