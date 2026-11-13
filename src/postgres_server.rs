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
use tokio::time::{MissedTickBehavior, interval};
use tower_sessions::{Expiry, Session, cookie::SameSite, session_store::ExpiredDeletion};
use tracing::{info, warn};

use crate::{
    config::AppConfig, crypto::EncryptedSecret, local_id, postgres_runtime::PostgresRuntime,
    postgres_runtime_owner, state::GitHubOAuthClient,
};

#[derive(Clone)]
struct PostgresServerState {
    runtime: PostgresRuntime,
    owner_id: Arc<str>,
    github_oauth: GitHubOAuthClient,
    http: reqwest::Client,
    encryption_key: crate::crypto::EncryptionKey,
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
        github_oauth: crate::state::build_oauth_client(&config)?,
        http: reqwest::Client::builder()
            .user_agent("OctoRill")
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("build PostgreSQL service HTTP client")?,
        encryption_key: config.encryption_key.clone(),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/version", get(version))
        .route("/api/me", get(me))
        .route("/api/me/resume", axum::routing::post(me_resume))
        .route("/api/me/personal-repos", get(personal_repos))
        .route("/api/search", get(search))
        .route("/api/starred", get(list_starred))
        .route("/api/releases", get(list_releases))
        .route("/api/notifications", get(list_notifications))
        .route("/api/repos/following", get(list_following_repos))
        .route("/api/tasks/{task_id}/events", get(task_events))
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
    .bind(now)
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
