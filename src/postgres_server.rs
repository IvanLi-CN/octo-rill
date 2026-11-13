use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use chrono::Utc;
use serde_json::json;
use tokio::time::{MissedTickBehavior, interval};
use tower_sessions::{Expiry, Session, cookie::SameSite, session_store::ExpiredDeletion};
use tracing::{info, warn};

use crate::{
    config::AppConfig, local_id, postgres_runtime::PostgresRuntime, postgres_runtime_owner,
};

#[derive(Clone)]
struct PostgresServerState {
    runtime: PostgresRuntime,
    owner_id: Arc<str>,
}

/// PostgreSQL-native service boundary.
///
/// This entry point deliberately owns the PG pool and owner lease instead of
/// passing a PostgreSQL connection into the SQLite `AppState`. The router is
/// intentionally small until each business vertical has a native PG query
/// implementation; unsupported routes therefore fail closed at the boundary.
pub async fn serve(config: AppConfig) -> Result<()> {
    let runtime = crate::postgres_runtime::connect_from_config(&config).await?;
    let owner_id: Arc<str> = local_id::generate_local_id().into();
    postgres_runtime_owner::register(&runtime.pool, &owner_id, Utc::now()).await?;

    let state = PostgresServerState {
        runtime: runtime.clone(),
        owner_id: owner_id.clone(),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/version", get(version))
        .route("/auth/logout", get(logout))
        .with_state(state);
    let session_layer =
        crate::session_store::CoordinatedSessionLayer::new(runtime.sessions.clone())
            .with_name("octo_rill_sid")
            .with_secure(config.public_base_url.scheme() == "https")
            .with_same_site(SameSite::Lax)
            .with_expiry(Expiry::OnInactivity(time::Duration::days(30)));
    let app = app.layer(session_layer);

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

    let serve_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("PostgreSQL service exited");
    heartbeat.abort();
    session_cleanup.abort();
    postgres_runtime_owner::unregister(&runtime.pool, &owner_id).await?;
    runtime.pool.close().await;
    serve_result
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

async fn health(State(state): State<PostgresServerState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.runtime.pool)
        .await
    {
        Ok(_) => (StatusCode::OK, axum::Json(json!({"ok": true}))),
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
