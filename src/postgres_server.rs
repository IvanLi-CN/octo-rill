use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
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
use sqlx::{FromRow, Postgres, QueryBuilder};
use std::convert::Infallible;
use tokio::sync::watch;
use tokio::time::{MissedTickBehavior, interval};
use tower_sessions::{Expiry, Session, cookie::SameSite, session_store::ExpiredDeletion};
use tracing::{info, warn};

use crate::{
    config::AppConfig,
    crypto::EncryptedSecret,
    local_id,
    postgres_runtime::PostgresRuntime,
    postgres_runtime_owner,
    postgres_tasks::{NotificationsExecutor, ReleasesExecutor},
    postgres_worker::TaskExecutorRegistry,
    state::GitHubOAuthClient,
};

#[derive(Clone)]
struct PostgresServerState {
    runtime: PostgresRuntime,
    owner_id: Arc<str>,
    linuxdo_available: bool,
    github_oauth: GitHubOAuthClient,
    http: reqwest::Client,
    encryption_key: crate::crypto::EncryptionKey,
    task_registry: TaskExecutorRegistry,
}

/// PostgreSQL-native service boundary.
///
/// This entry point deliberately owns the PG pool and owner lease instead of
/// passing a PostgreSQL connection into the SQLite `AppState`. The router is
/// intentionally small until each business vertical has a native PG query
/// implementation; unsupported routes therefore fail closed at the boundary.
pub async fn serve(config: AppConfig) -> Result<()> {
    let runtime = crate::postgres_runtime::connect_from_config(&config).await?;
    crate::postgres_runtime::verify_application_schema(&runtime.pool).await?;
    let owner_id: Arc<str> = local_id::generate_local_id().into();
    postgres_runtime_owner::register(&runtime.pool, &owner_id, Utc::now()).await?;

    let http = reqwest::Client::builder()
        .user_agent("OctoRill")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("build PostgreSQL service HTTP client")?;
    let task_registry = TaskExecutorRegistry::builder()
        .register(
            "sync.releases",
            ReleasesExecutor::new(
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
        .build();
    let state = PostgresServerState {
        runtime: runtime.clone(),
        owner_id: owner_id.clone(),
        linuxdo_available: config.linuxdo.is_some(),
        github_oauth: crate::state::build_oauth_client(&config)?,
        http,
        encryption_key: config.encryption_key.clone(),
        task_registry,
    };
    info!(
        task_types = ?state.task_registry.registered_task_types(),
        "PostgreSQL task executors registered"
    );
    let worker_registry = state.task_registry.clone();
    let app = Router::new()
        .route("/health", get(health))
        .route("/version", get(version))
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
        .route("/api/tasks/{task_id}/events", get(task_events))
        .route("/api/tasks/{task_id}", get(task_status))
        .route(
            "/api/tasks/{task_id}/cancel",
            axum::routing::post(cancel_task),
        )
        .route("/api/admin/jobs/overview", get(admin_jobs_overview))
        .route("/api/admin/dashboard", get(admin_dashboard))
        .route("/api/admin/jobs/events", get(admin_job_events))
        .route("/api/admin/users", get(admin_users))
        .route(
            "/api/admin/jobs/realtime/{task_id}/cancel",
            axum::routing::post(admin_cancel_task),
        )
        .route("/api/admin/jobs/realtime/{task_id}", get(admin_task_detail))
        .route(
            "/api/admin/jobs/realtime/{task_id}/retry",
            axum::routing::post(admin_retry_task),
        )
        .route("/api/admin/jobs/scheduled", get(admin_scheduled_slots))
        .route(
            "/api/admin/jobs/scheduled/{hour_utc}",
            axum::routing::patch(admin_patch_scheduled_slot),
        )
        .route(
            "/api/admin/users/{user_id}",
            get(admin_user_detail).patch(admin_patch_user),
        )
        .route(
            "/api/admin/users/{user_id}/profile",
            get(admin_user_profile),
        )
        .route("/api/admin/jobs/realtime", get(admin_realtime_tasks))
        .route("/api/public/metrics/v1/octo-rill", get(public_metrics))
        .route("/api/feed", get(feed))
        .route("/api/dashboard/feed", get(dashboard_feed))
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
        .route("/auth/github/callback", get(github_callback))
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
    let (worker_shutdown, worker_shutdown_rx) = watch::channel(false);
    let worker_pool = runtime.pool.clone();
    let worker_owner = owner_id.clone();
    let worker = tokio::spawn(async move {
        if std::env::var("OCTORILL_PG_WORKER_ENABLED").as_deref() != Ok("1") {
            return Ok::<(), anyhow::Error>(());
        }
        crate::postgres_worker::run_registered_until_shutdown(
            &worker_pool,
            &worker_owner,
            &worker_registry,
            worker_shutdown_rx,
            Duration::from_millis(250),
        )
        .await
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
) -> impl IntoResponse {
    let expected = session
        .get::<String>(SESSION_KEY_OAUTH_STATE)
        .await
        .ok()
        .flatten();
    let _ = session.remove::<String>(SESSION_KEY_OAUTH_STATE).await;
    if expected.as_deref() != Some(query.state.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"code": "invalid_oauth_state"})),
        )
            .into_response();
    }
    let token = match state
        .github_oauth
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(&state.http)
        .await
    {
        Ok(token) => token,
        Err(error) => {
            warn!(%error, "PostgreSQL GitHub token exchange failed");
            return (
                StatusCode::BAD_GATEWAY,
                axum::Json(json!({"code": "github_token_exchange_failed"})),
            )
                .into_response();
        }
    };
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
    let user = match crate::github::fetch_user(&state.http, &access_token).await {
        Ok(user) => user,
        Err(error) => {
            warn!(%error, "PostgreSQL GitHub user lookup failed");
            return (
                StatusCode::BAD_GATEWAY,
                axum::Json(json!({"code": "github_user_lookup_failed"})),
            )
                .into_response();
        }
    };
    let email = if user.email.is_some() {
        user.email.clone()
    } else {
        crate::github::fetch_primary_email(&state.http, &access_token)
            .await
            .ok()
            .flatten()
    };
    let encrypted = match state.encryption_key.encrypt_str(&access_token) {
        Ok(encrypted) => encrypted,
        Err(error) => {
            warn!(%error, "PostgreSQL GitHub token encryption failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "token_encryption_failed"})),
            )
                .into_response();
        }
    };
    match persist_github_login(&state, &user, email.as_deref(), &scopes, encrypted).await {
        Ok(user_id) => match session.insert(SESSION_KEY_USER_ID, user_id).await {
            Ok(()) => axum::response::Redirect::to("/").into_response(),
            Err(error) => {
                warn!(%error, "PostgreSQL login session save failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"code": "session_save_failed"})),
                )
                    .into_response()
            }
        },
        Err(error) => {
            warn!(%error, "PostgreSQL GitHub login persistence failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "login_persistence_failed"})),
            )
                .into_response()
        }
    }
}

async fn persist_github_login(
    state: &PostgresServerState,
    user: &crate::github::GitHubUser,
    email: Option<&str>,
    scopes: &str,
    encrypted: EncryptedSecret,
) -> Result<String, sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let mut tx = state.runtime.pool.begin().await?;
    sqlx::query(
        "INSERT INTO public.users (id, github_user_id, login, name, avatar_url, email, created_at, updated_at, last_active_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$7,$7) \
         ON CONFLICT (github_user_id) DO UPDATE SET login=EXCLUDED.login, name=EXCLUDED.name, avatar_url=EXCLUDED.avatar_url, email=EXCLUDED.email, updated_at=EXCLUDED.updated_at, last_active_at=EXCLUDED.last_active_at",
    )
    .bind(local_id::generate_local_id())
    .bind(user.id)
    .bind(&user.login)
    .bind(user.name.as_deref())
    .bind(user.avatar_url.as_deref())
    .bind(email)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let user_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.users WHERE github_user_id = $1 LIMIT 1",
    )
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO public.github_connections (id,user_id,github_user_id,login,name,avatar_url,email,access_token_ciphertext,access_token_nonce,scopes,linked_at,updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$11) \
         ON CONFLICT (github_user_id) DO UPDATE SET user_id=EXCLUDED.user_id,login=EXCLUDED.login,name=EXCLUDED.name,avatar_url=EXCLUDED.avatar_url,email=EXCLUDED.email,access_token_ciphertext=EXCLUDED.access_token_ciphertext,access_token_nonce=EXCLUDED.access_token_nonce,scopes=EXCLUDED.scopes,updated_at=EXCLUDED.updated_at",
    )
    .bind(local_id::generate_local_id())
    .bind(&user_id)
    .bind(user.id)
    .bind(&user.login)
    .bind(user.name.as_deref())
    .bind(user.avatar_url.as_deref())
    .bind(email)
    .bind(encrypted.ciphertext)
    .bind(encrypted.nonce)
    .bind(scopes)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(user_id)
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
        "SELECT daily_brief_schedule_local_time, daily_brief_time_zone, include_own_releases, last_active_at FROM public.users WHERE id = $1 AND is_disabled = 0 AND paused_at IS NULL",
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
            (
                StatusCode::OK,
                axum::Json(json!({
                    "release_id": row.get::<i64, _>("release_id").to_string(),
                    "repo_full_name": row.get::<String, _>("full_name"),
                    "tag_name": row.get::<String, _>("tag_name"),
                    "name": row.get::<Option<String>, _>("name"),
                    "body": row.get::<Option<String>, _>("body"),
                    "html_url": row.get::<String, _>("html_url"),
                    "published_at": row.get::<Option<String>, _>("published_at"),
                    "is_prerelease": row.get::<i64, _>("is_prerelease") != 0,
                    "is_draft": row.get::<i64, _>("is_draft") != 0,
                    "translated": null,
                    "smart": null
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
        "SELECT ob.repo_id, ob.repo_full_name AS full_name, split_part(ob.repo_full_name, '/', 1) AS owner_login, split_part(ob.repo_full_name, '/', 2) AS name, 'https://github.com/' || ob.repo_full_name AS html_url, ob.updated_at, (SELECT COUNT(*) FROM public.repo_releases r WHERE r.repo_id = ob.repo_id AND COALESCE(r.is_draft, 0) = 0) AS release_count, ob.owner_avatar_url, ob.open_graph_image_url, ob.uses_custom_open_graph_image FROM public.owned_repo_star_baselines ob WHERE ob.user_id = $1 AND position('/' in ob.repo_full_name) > 1 AND lower(split_part(ob.repo_full_name, '/', 1)) = lower($2) ORDER BY lower(ob.repo_full_name) ASC",
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

#[derive(Debug, Deserialize)]
struct PgFeedQuery {
    limit: Option<i64>,
}

async fn load_feed_items(
    state: &PostgresServerState,
    user_id: &str,
    limit: i64,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    #[derive(Debug, FromRow)]
    struct Row {
        id: String,
        resource_type: String,
        title: Option<String>,
        body: Option<String>,
        repo_full_name: Option<String>,
        source_time: Option<String>,
        unread: Option<i64>,
        target_url: Option<String>,
    }
    let rows = sqlx::query_as::<_, Row>(
        "SELECT id, resource_type, title, body, repo_full_name, source_time, unread, target_url FROM public.search_documents WHERE (user_id IS NULL OR user_id = $1) ORDER BY source_time DESC NULLS LAST, id DESC LIMIT $2",
    )
    .bind(user_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(&state.runtime.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            json!({
                "kind": row.resource_type,
                "ts": row.source_time.clone().unwrap_or_default(),
                "id": row.id,
                "repo_full_name": row.repo_full_name,
                "title": row.title,
                "body": row.body,
                "body_truncated": false,
                "subtitle": null,
                "reason": null,
                "subject_type": null,
                "html_url": row.target_url,
                "unread": row.unread,
                "repo_visual": null,
                "actor": null,
                "translated": null,
                "smart": null,
                "reactions": null
            })
        })
        .collect())
}

async fn feed(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<PgFeedQuery>,
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
    match load_feed_items(&state, &user_id, query.limit.unwrap_or(30)).await {
        Ok(items) => (
            StatusCode::OK,
            axum::Json(json!({"items": items, "next_cursor": null})),
        ),
        Err(error) => {
            warn!(%error, "PostgreSQL feed query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
        }
    }
}

async fn dashboard_feed(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<PgFeedQuery>,
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
    match load_feed_items(&state, &user_id, query.limit.unwrap_or(30)).await {
        Ok(items) => {
            let count = items.len();
            (
                StatusCode::OK,
                axum::Json(
                    json!({"sections": [{"id": "pg-canonical", "display_date": null, "kind": "raw", "window_start": null, "window_end": null, "brief": null, "items": items, "supplemental_items": [], "supplemental_next_cursor": null, "items_next_cursor": null, "item_count": count, "activity_count": count, "can_generate_brief": false}], "next_cursor": null}),
                ),
            )
        }
        Err(error) => {
            warn!(%error, "PostgreSQL dashboard feed query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
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
    match sqlx::query("UPDATE public.job_tasks SET cancel_requested = 1, updated_at = $1 WHERE id = $2 AND requested_by = $3 AND status IN ('queued', 'running')")
        .bind(Utc::now())
        .bind(&task_id)
        .bind(&user_id)
        .execute(&state.runtime.pool)
        .await
    {
        Ok(result) if result.rows_affected() == 1 => (StatusCode::OK, axum::Json(json!({"task_id": task_id, "cancel_requested": true}))).into_response(),
        Ok(_) => (StatusCode::NOT_FOUND, axum::Json(json!({"code": "not_found"}))).into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL task cancellation failed");
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({"code": "database_error"}))).into_response()
        }
    }
}

async fn require_admin(
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

async fn admin_jobs_overview(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    match sqlx::query_as::<_, (String, i64)>(
        "SELECT status, COUNT(*) FROM public.job_tasks GROUP BY status ORDER BY status",
    )
    .fetch_all(&state.runtime.pool)
    .await
    {
        Ok(rows) => {
            let counts = rows
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>();
            (
                StatusCode::OK,
                axum::Json(
                    json!({"counts": counts, "source": "postgresql", "generated_at": Utc::now()}),
                ),
            )
                .into_response()
        }
        Err(error) => {
            warn!(%error, "PostgreSQL admin job overview query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_dashboard(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    let users = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT COUNT(*), COUNT(*) FILTER (WHERE is_disabled = 0 AND paused_at IS NULL), COUNT(*) FILTER (WHERE is_admin = 1)
         FROM public.users",
    )
    .fetch_one(&state.runtime.pool)
    .await;
    let tasks = sqlx::query_as::<_, (String, i64)>(
        "SELECT status, COUNT(*) FROM public.job_tasks GROUP BY status ORDER BY status",
    )
    .fetch_all(&state.runtime.pool)
    .await;
    match (users, tasks) {
        (Ok((total_users, active_users, admin_users)), Ok(tasks)) => {
            let task_counts = tasks
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>();
            (
                StatusCode::OK,
                axum::Json(json!({
                    "source": "postgresql",
                    "generated_at": Utc::now(),
                    "users": {"total": total_users, "active": active_users, "admins": admin_users},
                    "tasks": {"counts": task_counts},
                })),
            )
                .into_response()
        }
        (Err(error), _) | (_, Err(error)) => {
            warn!(%error, "PostgreSQL admin dashboard query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

async fn admin_job_events(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    match sqlx::query(
        "SELECT id, task_id, event_type, payload_json, created_at FROM public.job_task_events ORDER BY created_at DESC, id DESC LIMIT 200",
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
                        "id": row.get::<String, _>("id"),
                        "task_id": row.get::<String, _>("task_id"),
                        "event_type": row.get::<String, _>("event_type"),
                        "payload": serde_json::from_str::<serde_json::Value>(&row.get::<String, _>("payload_json")).unwrap_or(json!({})),
                        "created_at": row.get::<chrono::DateTime<Utc>, _>("created_at"),
                    })
                })
                .collect::<Vec<_>>();
            (
                StatusCode::OK,
                axum::Json(json!({"items": items, "source": "postgresql"})),
            )
                .into_response()
        }
        Err(error) => {
            warn!(%error, "PostgreSQL admin task events query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
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

async fn admin_user_profile(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    match sqlx::query(
        "SELECT id, daily_brief_schedule_local_time, daily_brief_time_zone, include_own_releases, last_active_at, webhook_push_enabled, webhook_push_desired_state \
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

async fn admin_realtime_tasks(
    State(state): State<PostgresServerState>,
    session: Session,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    let rows = sqlx::query(
        "SELECT id, task_type, status, source, requested_by, created_at, updated_at, error_message FROM public.job_tasks WHERE status IN ('queued', 'running') ORDER BY created_at ASC, id ASC LIMIT 200",
    )
    .fetch_all(&state.runtime.pool)
    .await;
    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let items = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "task_id": row.get::<String, _>("id"),
                        "task_type": row.get::<String, _>("task_type"),
                        "status": row.get::<String, _>("status"),
                        "source": row.get::<String, _>("source"),
                        "requested_by": row.get::<Option<String>, _>("requested_by"),
                        "created_at": row.get::<chrono::DateTime<Utc>, _>("created_at"),
                        "updated_at": row.get::<chrono::DateTime<Utc>, _>("updated_at"),
                        "error": row.get::<Option<String>, _>("error_message")
                    })
                })
                .collect::<Vec<_>>();
            (StatusCode::OK, axum::Json(json!({"items": items}))).into_response()
        }
        Err(error) => {
            warn!(%error, "PostgreSQL realtime task query failed");
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
    match sqlx::query(
        "UPDATE public.job_tasks SET cancel_requested = 1, updated_at = $1 WHERE id = $2 AND status IN ('queued', 'running')",
    )
    .bind(Utc::now())
    .bind(&task_id)
    .execute(&state.runtime.pool)
    .await
    {
        Ok(result) if result.rows_affected() == 1 => (
            StatusCode::OK,
            axum::Json(json!({"task_id": task_id, "cancel_requested": true})),
        )
            .into_response(),
        Ok(_) => (
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

async fn admin_task_detail(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, axum::Json(json!({"code": "admin_required"}))).into_response();
    }
    let task = sqlx::query(
        "SELECT id, task_type, status, source, requested_by, payload_json, result_json, error_message, cancel_requested, runtime_owner_id, lease_heartbeat_at, created_at, started_at, finished_at, updated_at FROM public.job_tasks WHERE id = $1",
    )
    .bind(&task_id)
    .fetch_optional(&state.runtime.pool)
    .await;
    match task {
        Ok(Some(row)) => {
            use sqlx::Row;
            let event_count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM public.job_task_events WHERE task_id = $1",
            )
            .bind(&task_id)
            .fetch_one(&state.runtime.pool)
            .await
            .unwrap_or_default();
            (
                StatusCode::OK,
                axum::Json(json!({
                    "id": row.get::<String, _>("id"),
                    "task_type": row.get::<String, _>("task_type"),
                    "status": row.get::<String, _>("status"),
                    "source": row.get::<String, _>("source"),
                    "requested_by": row.get::<Option<String>, _>("requested_by"),
                    "payload": serde_json::from_str::<serde_json::Value>(&row.get::<String, _>("payload_json")).unwrap_or(json!({})),
                    "result": row.get::<Option<String>, _>("result_json").and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok()),
                    "error": row.get::<Option<String>, _>("error_message"),
                    "cancel_requested": row.get::<i64, _>("cancel_requested") != 0,
                    "runtime_owner_id": row.get::<Option<String>, _>("runtime_owner_id"),
                    "lease_heartbeat_at": row.get::<Option<chrono::DateTime<Utc>>, _>("lease_heartbeat_at"),
                    "created_at": row.get::<chrono::DateTime<Utc>, _>("created_at"),
                    "started_at": row.get::<Option<chrono::DateTime<Utc>>, _>("started_at"),
                    "finished_at": row.get::<Option<chrono::DateTime<Utc>>, _>("finished_at"),
                    "updated_at": row.get::<chrono::DateTime<Utc>, _>("updated_at"),
                    "event_count": event_count,
                    "source": "postgresql"
                })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"code": "task_not_found"})),
        )
            .into_response(),
        Err(error) => {
            warn!(%error, task_id = %task_id, "PostgreSQL admin task detail query failed");
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
    let now = Utc::now();
    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            warn!(%error, "PostgreSQL admin task retry transaction failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "database_error"})),
            )
                .into_response();
        }
    };
    let updated = sqlx::query(
        "UPDATE public.job_tasks SET status = 'queued', cancel_requested = 0, result_json = NULL, error_message = NULL, runtime_owner_id = NULL, lease_heartbeat_at = NULL, available_at = $1, started_at = NULL, finished_at = NULL, updated_at = $1 WHERE id = $2 AND status IN ('failed', 'cancelled')",
    )
    .bind(now)
    .bind(&task_id)
    .execute(&mut *tx)
    .await;
    match updated {
        Ok(result) if result.rows_affected() == 1 => {
            let payload = json!({"task_id": task_id, "status": "queued", "reason": "admin_retry"});
            let event = sqlx::query(
                "INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at) VALUES ($1, $2, 'task.retried', $3, $4)",
            )
            .bind(local_id::generate_local_id())
            .bind(&task_id)
            .bind(payload.to_string())
            .bind(now)
            .execute(&mut *tx)
            .await;
            match event {
                Ok(_) => match tx.commit().await {
                    Ok(()) => (
                        StatusCode::ACCEPTED,
                        axum::Json(json!({"task_id": task_id, "status": "queued"})),
                    )
                        .into_response(),
                    Err(error) => {
                        warn!(%error, task_id = %task_id, "PostgreSQL admin task retry commit failed");
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(json!({"code": "database_error"})),
                        )
                            .into_response()
                    }
                },
                Err(error) => {
                    warn!(%error, task_id = %task_id, "PostgreSQL admin task retry event failed");
                    let _ = tx.rollback().await;
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
                StatusCode::CONFLICT,
                axum::Json(json!({"code": "task_not_retryable"})),
            )
                .into_response()
        }
        Err(error) => {
            warn!(%error, task_id = %task_id, "PostgreSQL admin task retry update failed");
            let _ = tx.rollback().await;
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

#[derive(Debug, FromRow)]
struct PgSearchRow {
    id: String,
    resource_type: String,
    title: Option<String>,
    body: Option<String>,
    repo_full_name: Option<String>,
    source_time: Option<String>,
    unread: Option<i64>,
    target_path: Option<String>,
    target_url: Option<String>,
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
    let mut query = QueryBuilder::<Postgres>::new(
        "SELECT id, resource_type, title, body, repo_full_name, source_time, unread, target_path, target_url \
         FROM public.search_documents WHERE (user_id IS NULL OR user_id = ",
    );
    query.push_bind(&user_id).push(")");
    if !parsed.terms.is_empty() {
        query
            .push(" AND search_vector @@ websearch_to_tsquery('octo_rill_simple', ")
            .push_bind(parsed.terms.join(" "))
            .push(")");
    }
    if let Some(resource_type) = parsed.resource_type {
        query.push(" AND resource_type = ").push_bind(resource_type);
    }
    if let Some(owner) = parsed.owner {
        query.push(" AND owner_login = ").push_bind(owner);
    }
    if let Some(repo) = parsed.repo {
        query.push(" AND repo_full_name = ").push_bind(repo);
    }
    if parsed.unread {
        query.push(" AND unread <> 0");
    }
    query.push(" ORDER BY source_time DESC NULLS LAST, id DESC LIMIT 20");
    let rows = match query
        .build_query_as::<PgSearchRow>()
        .fetch_all(&state.runtime.pool)
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            warn!(%error, "PostgreSQL search query failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({"code": "search_failed"})),
            );
        }
    };
    let items = rows
        .into_iter()
        .map(|row| {
            let repository = row.repo_full_name.as_ref().and_then(|full_name| {
                full_name
                    .split_once('/')
                    .map(|(owner, name)| json!({"owner": owner, "name": name, "full_name": full_name}))
            });
            json!({
                "id": row.id,
                "type": row.resource_type,
                "title": row.title.unwrap_or_default(),
                "excerpt": row.body.map(|body| body.chars().take(240).collect::<String>()),
                "repository": repository,
                "updated_at": row.source_time,
                "source_time": row.source_time,
                "unread": row.unread.unwrap_or_default() != 0,
                "is_following": null,
                "matched_lane": "canonical",
                "matched_lanes": ["canonical"],
                "target": row.target_path.or(row.target_url).map(|href| json!({"href": href, "lane": null})),
            })
        })
        .collect::<Vec<_>>();
    (
        StatusCode::OK,
        axum::Json(json!({
            "items": items,
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
