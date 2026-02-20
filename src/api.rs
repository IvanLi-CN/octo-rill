use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use tower_sessions::Session;

use crate::{ai, sync};
use crate::{error::ApiError, state::AppState};

#[derive(Debug, Serialize)]
pub struct MeResponse {
    user: UserSummary,
}

#[derive(Debug, Serialize)]
pub struct UserSummary {
    id: i64,
    github_user_id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct UserRow {
    id: i64,
    github_user_id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MeResponse>, ApiError> {
    let Some(user_id) = session
        .get::<i64>("user_id")
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "not logged in",
        ));
    };

    let row = sqlx::query_as::<_, UserRow>(
        r#"
        SELECT id, github_user_id, login, name, avatar_url, email
        FROM users
        WHERE id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        session.clear().await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "session user not found",
        ));
    };

    Ok(Json(MeResponse {
        user: UserSummary {
            id: row.id,
            github_user_id: row.github_user_id,
            login: row.login,
            name: row.name,
            avatar_url: row.avatar_url,
            email: row.email,
        },
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StarredRepoItem {
    repo_id: i64,
    full_name: String,
    description: Option<String>,
    html_url: String,
    stargazed_at: Option<String>,
    is_private: i64,
}

pub async fn list_starred(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<Vec<StarredRepoItem>>, ApiError> {
    let user_id = require_user_id(&session).await?;

    let repos = sqlx::query_as::<_, StarredRepoItem>(
        r#"
        SELECT repo_id, full_name, description, html_url, stargazed_at, is_private
        FROM starred_repos
        WHERE user_id = ?
        ORDER BY stargazed_at DESC
        LIMIT 2000
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(repos))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ReleaseItem {
    full_name: String,
    tag_name: String,
    name: Option<String>,
    published_at: Option<String>,
    html_url: String,
    is_prerelease: i64,
    is_draft: i64,
}

pub async fn list_releases(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<Vec<ReleaseItem>>, ApiError> {
    let user_id = require_user_id(&session).await?;

    let items = sqlx::query_as::<_, ReleaseItem>(
        r#"
        SELECT sr.full_name, r.tag_name, r.name, r.published_at, r.html_url, r.is_prerelease, r.is_draft
        FROM releases r
        JOIN starred_repos sr
          ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
        WHERE r.user_id = ?
        ORDER BY COALESCE(r.published_at, r.created_at) DESC
        LIMIT 200
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(items))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NotificationItem {
    thread_id: String,
    repo_full_name: Option<String>,
    subject_title: Option<String>,
    subject_type: Option<String>,
    reason: Option<String>,
    updated_at: Option<String>,
    unread: i64,
    html_url: Option<String>,
}

pub async fn list_notifications(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<Vec<NotificationItem>>, ApiError> {
    let user_id = require_user_id(&session).await?;

    let items = sqlx::query_as::<_, NotificationItem>(
        r#"
        SELECT thread_id, repo_full_name, subject_title, subject_type, reason, updated_at, unread, html_url
        FROM notifications
        WHERE user_id = ?
        ORDER BY updated_at DESC
        LIMIT 200
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(items))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct BriefItem {
    date: String,
    content_markdown: String,
    created_at: String,
}

pub async fn list_briefs(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<Vec<BriefItem>>, ApiError> {
    let user_id = require_user_id(&session).await?;

    let items = sqlx::query_as::<_, BriefItem>(
        r#"
        SELECT date, content_markdown, created_at
        FROM briefs
        WHERE user_id = ?
        ORDER BY date DESC
        LIMIT 30
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(items))
}

pub async fn sync_starred(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<sync::SyncStarredResult>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let res = sync::sync_starred(state.as_ref(), user_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(res))
}

pub async fn sync_releases(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<sync::SyncReleasesResult>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let res = sync::sync_releases(state.as_ref(), user_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(res))
}

pub async fn sync_notifications(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<sync::SyncNotificationsResult>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let res = sync::sync_notifications(state.as_ref(), user_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(res))
}

#[derive(Debug, Serialize)]
pub struct BriefGenerateResponse {
    content_markdown: String,
}

pub async fn generate_brief(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<BriefGenerateResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let content = ai::generate_daily_brief(state.as_ref(), user_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(BriefGenerateResponse {
        content_markdown: content,
    }))
}

async fn require_user_id(session: &Session) -> Result<i64, ApiError> {
    let Some(user_id) = session
        .get::<i64>("user_id")
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "not logged in",
        ));
    };
    Ok(user_id)
}
