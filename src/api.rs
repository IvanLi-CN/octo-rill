use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_sessions::Session;
use url::Url;

use crate::{ai, jobs, sync};
use crate::{error::ApiError, state::AppState};

fn parse_repo_full_name_from_release_url(html_url: &str) -> Option<String> {
    let parsed = Url::parse(html_url).ok()?;
    let host = parsed.host_str()?;
    if host != "github.com" && host != "www.github.com" {
        return None;
    }
    let mut segments = parsed.path_segments()?;
    let owner = segments.next()?.trim();
    let repo = segments.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn resolve_release_full_name(html_url: &str, repo_id: i64) -> String {
    parse_repo_full_name_from_release_url(html_url).unwrap_or_else(|| format!("unknown/{repo_id}"))
}

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
    is_admin: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct UserRow {
    id: i64,
    github_user_id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
    is_admin: i64,
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MeResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;

    let row = sqlx::query_as::<_, UserRow>(
        r#"
        SELECT id, github_user_id, login, name, avatar_url, email, is_admin
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
            is_admin: row.is_admin != 0,
        },
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminUserItem {
    id: i64,
    github_user_id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
    is_admin: bool,
    is_disabled: bool,
    last_active_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminUsersListResponse {
    items: Vec<AdminUserItem>,
    page: i64,
    page_size: i64,
    total: i64,
    guard: AdminUsersGuardSummary,
}

#[derive(Debug, Serialize)]
pub struct AdminUsersGuardSummary {
    admin_total: i64,
    active_admin_total: i64,
}

#[derive(Debug, Deserialize)]
pub struct AdminUsersQuery {
    query: Option<String>,
    role: Option<String>,
    status: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct AdminUserPatchRequest {
    is_admin: Option<bool>,
    is_disabled: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
struct AdminUserUpdateGuard {
    acting_user_id: i64,
    target_user_id: i64,
    target_is_admin: bool,
    target_is_disabled: bool,
    next_is_admin: bool,
    next_is_disabled: bool,
    admin_count: i64,
    active_admin_count: i64,
}

fn guard_admin_user_update(guard: AdminUserUpdateGuard) -> Result<(), ApiError> {
    if guard.target_user_id == guard.acting_user_id && guard.next_is_disabled {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "cannot_disable_self",
            "admin cannot disable self",
        ));
    }

    if guard.target_is_admin && !guard.next_is_admin && guard.admin_count <= 1 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "last_admin_guard",
            "at least one admin is required",
        ));
    }

    let target_is_active_admin = guard.target_is_admin && !guard.target_is_disabled;
    let next_is_active_admin = guard.next_is_admin && !guard.next_is_disabled;
    if target_is_active_admin && !next_is_active_admin && guard.active_admin_count <= 1 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "last_admin_guard",
            "at least one active admin is required",
        ));
    }

    Ok(())
}

fn admin_users_offset(page: i64, page_size: i64) -> Result<i64, ApiError> {
    page.checked_sub(1)
        .and_then(|value| value.checked_mul(page_size))
        .ok_or_else(|| ApiError::bad_request("page is too large"))
}

pub async fn admin_list_users(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<AdminUsersQuery>,
) -> Result<Json<AdminUsersListResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;

    let role = query.role.unwrap_or_else(|| "all".to_owned());
    if role != "all" && role != "admin" && role != "user" {
        return Err(ApiError::bad_request("invalid role filter"));
    }
    let status = query.status.unwrap_or_else(|| "all".to_owned());
    if status != "all" && status != "enabled" && status != "disabled" {
        return Err(ApiError::bad_request("invalid status filter"));
    }

    let page = query.page.unwrap_or(1);
    if page < 1 {
        return Err(ApiError::bad_request("page must be >= 1"));
    }
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = admin_users_offset(page, page_size)?;

    let query_text = query.query.unwrap_or_default().trim().to_lowercase();
    let query_like = format!("%{query_text}%");

    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM users
        WHERE
          (? = '' OR lower(login) LIKE ? OR lower(COALESCE(name, '')) LIKE ? OR lower(COALESCE(email, '')) LIKE ?)
          AND (? = 'all' OR (? = 'admin' AND is_admin = 1) OR (? = 'user' AND is_admin = 0))
          AND (? = 'all' OR (? = 'enabled' AND is_disabled = 0) OR (? = 'disabled' AND is_disabled = 1))
        "#,
    )
    .bind(query_text.as_str())
    .bind(query_like.as_str())
    .bind(query_like.as_str())
    .bind(query_like.as_str())
    .bind(role.as_str())
    .bind(role.as_str())
    .bind(role.as_str())
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(status.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let items = sqlx::query_as::<_, AdminUserItem>(
        r#"
        SELECT
          id,
          github_user_id,
          login,
          name,
          avatar_url,
          email,
          is_admin,
          is_disabled,
          last_active_at,
          created_at,
          updated_at
        FROM users
        WHERE
          (? = '' OR lower(login) LIKE ? OR lower(COALESCE(name, '')) LIKE ? OR lower(COALESCE(email, '')) LIKE ?)
          AND (? = 'all' OR (? = 'admin' AND is_admin = 1) OR (? = 'user' AND is_admin = 0))
          AND (? = 'all' OR (? = 'enabled' AND is_disabled = 0) OR (? = 'disabled' AND is_disabled = 1))
        ORDER BY created_at ASC, id ASC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(query_text.as_str())
    .bind(query_like.as_str())
    .bind(query_like.as_str())
    .bind(query_like.as_str())
    .bind(role.as_str())
    .bind(role.as_str())
    .bind(role.as_str())
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let admin_total =
        sqlx::query_scalar::<_, i64>(r#"SELECT COUNT(*) FROM users WHERE is_admin = 1"#)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?;

    let active_admin_total = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM users WHERE is_admin = 1 AND is_disabled = 0"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(AdminUsersListResponse {
        items,
        page,
        page_size,
        total,
        guard: AdminUsersGuardSummary {
            admin_total,
            active_admin_total,
        },
    }))
}

pub async fn admin_patch_user(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(target_user_id): Path<i64>,
    Json(req): Json<AdminUserPatchRequest>,
) -> Result<Json<AdminUserItem>, ApiError> {
    let acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;

    if req.is_admin.is_none() && req.is_disabled.is_none() {
        return Err(ApiError::bad_request(
            "at least one field (is_admin/is_disabled) is required",
        ));
    }

    #[derive(Debug, sqlx::FromRow)]
    struct AdminPatchTargetRow {
        id: i64,
        is_admin: i64,
        is_disabled: i64,
    }

    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
    let target = sqlx::query_as::<_, AdminPatchTargetRow>(
        r#"
        SELECT id, is_admin, is_disabled
        FROM users
        WHERE id = ?
        "#,
    )
    .bind(target_user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let Some(target) = target else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "user not found",
        ));
    };

    let next_is_admin = req.is_admin.unwrap_or(target.is_admin != 0);
    let next_is_disabled = req.is_disabled.unwrap_or(target.is_disabled != 0);

    let target_is_admin = target.is_admin != 0;
    let target_is_disabled = target.is_disabled != 0;
    let target_is_active_admin = target_is_admin && !target_is_disabled;
    let next_is_active_admin = next_is_admin && !next_is_disabled;

    let admin_count = if target_is_admin && !next_is_admin {
        sqlx::query_scalar::<_, i64>(r#"SELECT COUNT(*) FROM users WHERE is_admin = 1"#)
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::internal)?
    } else {
        0
    };

    let active_admin_count = if target_is_active_admin && !next_is_active_admin {
        sqlx::query_scalar::<_, i64>(
            r#"SELECT COUNT(*) FROM users WHERE is_admin = 1 AND is_disabled = 0"#,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?
    } else {
        0
    };

    guard_admin_user_update(AdminUserUpdateGuard {
        acting_user_id,
        target_user_id: target.id,
        target_is_admin,
        target_is_disabled,
        next_is_admin,
        next_is_disabled,
        admin_count,
        active_admin_count,
    })?;

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE users
        SET is_admin = ?, is_disabled = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(if next_is_admin { 1_i64 } else { 0_i64 })
    .bind(if next_is_disabled { 1_i64 } else { 0_i64 })
    .bind(now.as_str())
    .bind(target_user_id)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let updated = sqlx::query_as::<_, AdminUserItem>(
        r#"
        SELECT
          id,
          github_user_id,
          login,
          name,
          avatar_url,
          email,
          is_admin,
          is_disabled,
          last_active_at,
          created_at,
          updated_at
        FROM users
        WHERE id = ?
        "#,
    )
    .bind(target_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(updated))
}

#[derive(Debug, Serialize)]
pub struct AdminUserProfileResponse {
    user_id: i64,
    daily_brief_utc_time: String,
    last_active_at: Option<String>,
}

pub async fn admin_get_user_profile(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(user_id): Path<i64>,
) -> Result<Json<AdminUserProfileResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;

    let row = sqlx::query_as::<_, (String, Option<String>)>(
        r#"
        SELECT daily_brief_utc_time, last_active_at
        FROM users
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some((daily_brief_utc_time, last_active_at)) = row else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "user not found",
        ));
    };

    Ok(Json(AdminUserProfileResponse {
        user_id,
        daily_brief_utc_time,
        last_active_at,
    }))
}

#[derive(Debug, Serialize)]
pub struct AdminJobsOverviewResponse {
    queued: i64,
    running: i64,
    failed_24h: i64,
    succeeded_24h: i64,
    enabled_scheduled_slots: i64,
    total_scheduled_slots: i64,
}

pub async fn admin_jobs_overview(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<AdminJobsOverviewResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;

    let queued =
        sqlx::query_scalar::<_, i64>(r#"SELECT COUNT(*) FROM job_tasks WHERE status = 'queued'"#)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?;
    let running =
        sqlx::query_scalar::<_, i64>(r#"SELECT COUNT(*) FROM job_tasks WHERE status = 'running'"#)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?;
    let failed_24h = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM job_tasks
        WHERE status = 'failed'
          AND datetime(finished_at) >= datetime('now', '-1 day')
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let succeeded_24h = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM job_tasks
        WHERE status = 'succeeded'
          AND datetime(finished_at) >= datetime('now', '-1 day')
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let enabled_scheduled_slots = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM daily_brief_hour_slots WHERE enabled = 1"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let total_scheduled_slots =
        sqlx::query_scalar::<_, i64>(r#"SELECT COUNT(*) FROM daily_brief_hour_slots"#)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?;

    Ok(Json(AdminJobsOverviewResponse {
        queued,
        running,
        failed_24h,
        succeeded_24h,
        enabled_scheduled_slots,
        total_scheduled_slots,
    }))
}

pub async fn admin_jobs_events_sse(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Response, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    Ok(jobs::admin_jobs_sse_response(state))
}

#[derive(Debug, Deserialize)]
pub struct AdminRealtimeTasksQuery {
    status: Option<String>,
    task_type: Option<String>,
    exclude_task_type: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminRealtimeTaskItem {
    id: String,
    task_type: String,
    status: String,
    source: String,
    requested_by: Option<i64>,
    parent_task_id: Option<String>,
    cancel_requested: bool,
    error_message: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminRealtimeTaskDetailItem {
    id: String,
    task_type: String,
    status: String,
    source: String,
    requested_by: Option<i64>,
    parent_task_id: Option<String>,
    cancel_requested: bool,
    error_message: Option<String>,
    payload_json: String,
    result_json: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminRealtimeTasksResponse {
    items: Vec<AdminRealtimeTaskItem>,
    page: i64,
    page_size: i64,
    total: i64,
}

pub async fn admin_list_realtime_tasks(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<AdminRealtimeTasksQuery>,
) -> Result<Json<AdminRealtimeTasksResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = admin_users_offset(page, page_size)?;
    let status = query.status.unwrap_or_else(|| "all".to_owned());
    let task_type = query.task_type.unwrap_or_default();
    let exclude_task_type = query.exclude_task_type.unwrap_or_default();

    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM job_tasks
        WHERE (? = 'all' OR status = ?)
          AND (? = '' OR task_type = ?)
          AND (? = '' OR task_type != ?)
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(task_type.as_str())
    .bind(task_type.as_str())
    .bind(exclude_task_type.as_str())
    .bind(exclude_task_type.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let items = sqlx::query_as::<_, AdminRealtimeTaskItem>(
        r#"
        SELECT
          id,
          task_type,
          status,
          source,
          requested_by,
          parent_task_id,
          cancel_requested,
          error_message,
          created_at,
          started_at,
          finished_at,
          updated_at
        FROM job_tasks
        WHERE (? = 'all' OR status = ?)
          AND (? = '' OR task_type = ?)
          AND (? = '' OR task_type != ?)
        ORDER BY created_at DESC, id DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(task_type.as_str())
    .bind(task_type.as_str())
    .bind(exclude_task_type.as_str())
    .bind(exclude_task_type.as_str())
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(AdminRealtimeTasksResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminTaskEventItem {
    id: i64,
    event_type: String,
    payload_json: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminRealtimeTaskDetailResponse {
    task: AdminRealtimeTaskDetailItem,
    events: Vec<AdminTaskEventItem>,
}

pub async fn admin_get_realtime_task_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(task_id): Path<String>,
) -> Result<Json<AdminRealtimeTaskDetailResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;

    let task = sqlx::query_as::<_, AdminRealtimeTaskDetailItem>(
        r#"
        SELECT
          id,
          task_type,
          status,
          source,
          requested_by,
          parent_task_id,
          cancel_requested,
          error_message,
          payload_json,
          result_json,
          created_at,
          started_at,
          finished_at,
          updated_at
        FROM job_tasks
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(task_id.as_str())
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "task not found"))?;

    let events = sqlx::query_as::<_, AdminTaskEventItem>(
        r#"
        SELECT id, event_type, payload_json, created_at
        FROM job_task_events
        WHERE task_id = ?
        ORDER BY id DESC
        LIMIT 200
        "#,
    )
    .bind(task_id.as_str())
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(AdminRealtimeTaskDetailResponse { task, events }))
}

#[derive(Debug, Serialize)]
pub struct AdminTaskActionResponse {
    task_id: String,
    status: String,
}

fn map_job_action_error(err: anyhow::Error) -> ApiError {
    match err.to_string().as_str() {
        "task not found" => ApiError::new(StatusCode::NOT_FOUND, "not_found", "task not found"),
        "only finished tasks can be retried" => ApiError::new(
            StatusCode::CONFLICT,
            "invalid_task_state",
            "only finished tasks can be retried",
        ),
        _ => ApiError::internal(err),
    }
}

pub async fn admin_retry_realtime_task(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(task_id): Path<String>,
) -> Result<Json<AdminTaskActionResponse>, ApiError> {
    let acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let task = jobs::retry_task(state.as_ref(), task_id.as_str(), acting_user_id)
        .await
        .map_err(map_job_action_error)?;

    Ok(Json(AdminTaskActionResponse {
        task_id: task.task_id,
        status: task.status,
    }))
}

pub async fn admin_cancel_realtime_task(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(task_id): Path<String>,
) -> Result<Json<AdminTaskActionResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let status = jobs::cancel_task(state.as_ref(), task_id.as_str())
        .await
        .map_err(map_job_action_error)?;

    Ok(Json(AdminTaskActionResponse { task_id, status }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminScheduledSlotItem {
    hour_utc: i64,
    enabled: bool,
    last_dispatch_at: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminScheduledSlotsResponse {
    items: Vec<AdminScheduledSlotItem>,
}

pub async fn admin_list_scheduled_slots(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<AdminScheduledSlotsResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let items = sqlx::query_as::<_, AdminScheduledSlotItem>(
        r#"
        SELECT hour_utc, enabled, last_dispatch_at, updated_at
        FROM daily_brief_hour_slots
        ORDER BY hour_utc ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(AdminScheduledSlotsResponse { items }))
}

#[derive(Debug, Deserialize)]
pub struct AdminPatchScheduledSlotRequest {
    enabled: bool,
}

pub async fn admin_patch_scheduled_slot(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(hour_utc): Path<i64>,
    Json(req): Json<AdminPatchScheduledSlotRequest>,
) -> Result<Json<AdminScheduledSlotItem>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    if !(0..=23).contains(&hour_utc) {
        return Err(ApiError::bad_request("hour_utc must be 0..23"));
    }

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE daily_brief_hour_slots
        SET enabled = ?, updated_at = ?
        WHERE hour_utc = ?
        "#,
    )
    .bind(if req.enabled { 1_i64 } else { 0_i64 })
    .bind(now.as_str())
    .bind(hour_utc)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let item = sqlx::query_as::<_, AdminScheduledSlotItem>(
        r#"
        SELECT hour_utc, enabled, last_dispatch_at, updated_at
        FROM daily_brief_hour_slots
        WHERE hour_utc = ?
        LIMIT 1
        "#,
    )
    .bind(hour_utc)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "slot not found"))?;

    Ok(Json(item))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminLlmCallItem {
    id: String,
    status: String,
    source: String,
    model: String,
    requested_by: Option<i64>,
    parent_task_id: Option<String>,
    parent_task_type: Option<String>,
    max_tokens: i64,
    attempt_count: i64,
    scheduler_wait_ms: i64,
    first_token_wait_ms: Option<i64>,
    duration_ms: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    total_tokens: Option<i64>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminLlmCallDetailItem {
    id: String,
    status: String,
    source: String,
    model: String,
    requested_by: Option<i64>,
    parent_task_id: Option<String>,
    parent_task_type: Option<String>,
    max_tokens: i64,
    attempt_count: i64,
    scheduler_wait_ms: i64,
    first_token_wait_ms: Option<i64>,
    duration_ms: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    total_tokens: Option<i64>,
    input_messages_json: Option<String>,
    output_messages_json: Option<String>,
    prompt_text: String,
    response_text: Option<String>,
    error_text: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminLlmCallsResponse {
    items: Vec<AdminLlmCallItem>,
    page: i64,
    page_size: i64,
    total: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminLlmSchedulerStatusResponse {
    scheduler_enabled: bool,
    request_interval_ms: i64,
    waiting_calls: i64,
    in_flight_calls: i64,
    next_slot_in_ms: i64,
    calls_24h: i64,
    failed_24h: i64,
    avg_wait_ms_24h: Option<i64>,
    avg_duration_ms_24h: Option<i64>,
    last_success_at: Option<String>,
    last_failure_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminLlmCallsQuery {
    status: Option<String>,
    source: Option<String>,
    requested_by: Option<i64>,
    started_from: Option<String>,
    started_to: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

fn parse_llm_calls_filter_timestamp(
    value: Option<String>,
    field: &str,
) -> Result<Option<String>, ApiError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(trimmed)
        .map_err(|_| ApiError::bad_request(format!("{field} must be RFC3339")))?;
    Ok(Some(parsed.to_rfc3339()))
}

pub async fn admin_get_llm_scheduler_status(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<AdminLlmSchedulerStatusResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let runtime = ai::llm_scheduler_runtime_status().await;

    let cutoff = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
    let (calls_24h, failed_24h, avg_wait_raw, avg_duration_raw) =
        sqlx::query_as::<_, (i64, i64, Option<f64>, Option<f64>)>(
            r#"
            SELECT
              COUNT(*) AS calls_24h,
              COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_24h,
              AVG(CAST(scheduler_wait_ms AS REAL)) AS avg_wait_ms_24h,
              AVG(CAST(duration_ms AS REAL)) AS avg_duration_ms_24h
            FROM llm_calls
            WHERE created_at >= ?
            "#,
        )
        .bind(cutoff.as_str())
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::internal)?;

    let last_success_at = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT MAX(finished_at)
        FROM llm_calls
        WHERE status = 'succeeded'
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let last_failure_at = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT MAX(finished_at)
        FROM llm_calls
        WHERE status = 'failed'
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(AdminLlmSchedulerStatusResponse {
        scheduler_enabled: state.config.ai.is_some(),
        request_interval_ms: runtime.request_interval_ms,
        waiting_calls: runtime.waiting_calls,
        in_flight_calls: runtime.in_flight_calls,
        next_slot_in_ms: runtime.next_slot_in_ms,
        calls_24h,
        failed_24h,
        avg_wait_ms_24h: avg_wait_raw.map(|value| value.round() as i64),
        avg_duration_ms_24h: avg_duration_raw.map(|value| value.round() as i64),
        last_success_at,
        last_failure_at,
    }))
}

pub async fn admin_list_llm_calls(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<AdminLlmCallsQuery>,
) -> Result<Json<AdminLlmCallsResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = admin_users_offset(page, page_size)?;
    let status = query.status.unwrap_or_else(|| "all".to_owned());
    if !matches!(
        status.as_str(),
        "all" | "queued" | "running" | "succeeded" | "failed"
    ) {
        return Err(ApiError::bad_request("invalid status filter"));
    }

    let source = query.source.unwrap_or_default().trim().to_owned();
    let started_from = parse_llm_calls_filter_timestamp(query.started_from, "started_from")?;
    let started_to = parse_llm_calls_filter_timestamp(query.started_to, "started_to")?;

    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM llm_calls
        WHERE (? = 'all' OR status = ?)
          AND (? = '' OR source = ?)
          AND (? IS NULL OR requested_by = ?)
          AND (? IS NULL OR unixepoch(COALESCE(started_at, created_at)) >= unixepoch(?))
          AND (? IS NULL OR unixepoch(COALESCE(started_at, created_at)) <= unixepoch(?))
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(source.as_str())
    .bind(source.as_str())
    .bind(query.requested_by)
    .bind(query.requested_by)
    .bind(started_from.as_deref())
    .bind(started_from.as_deref())
    .bind(started_to.as_deref())
    .bind(started_to.as_deref())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let items = sqlx::query_as::<_, AdminLlmCallItem>(
        r#"
        SELECT
          id,
          status,
          source,
          model,
          requested_by,
          parent_task_id,
          parent_task_type,
          max_tokens,
          attempt_count,
          scheduler_wait_ms,
          first_token_wait_ms,
          duration_ms,
          input_tokens,
          output_tokens,
          cached_input_tokens,
          total_tokens,
          created_at,
          started_at,
          finished_at,
          updated_at
        FROM llm_calls
        WHERE (? = 'all' OR status = ?)
          AND (? = '' OR source = ?)
          AND (? IS NULL OR requested_by = ?)
          AND (? IS NULL OR unixepoch(COALESCE(started_at, created_at)) >= unixepoch(?))
          AND (? IS NULL OR unixepoch(COALESCE(started_at, created_at)) <= unixepoch(?))
        ORDER BY created_at DESC, id DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(source.as_str())
    .bind(source.as_str())
    .bind(query.requested_by)
    .bind(query.requested_by)
    .bind(started_from.as_deref())
    .bind(started_from.as_deref())
    .bind(started_to.as_deref())
    .bind(started_to.as_deref())
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(AdminLlmCallsResponse {
        items,
        page,
        page_size,
        total,
    }))
}

pub async fn admin_get_llm_call_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(call_id): Path<String>,
) -> Result<Json<AdminLlmCallDetailItem>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;

    let item = sqlx::query_as::<_, AdminLlmCallDetailItem>(
        r#"
        SELECT
          id,
          status,
          source,
          model,
          requested_by,
          parent_task_id,
          parent_task_type,
          max_tokens,
          attempt_count,
          scheduler_wait_ms,
          first_token_wait_ms,
          duration_ms,
          input_tokens,
          output_tokens,
          cached_input_tokens,
          total_tokens,
          input_messages_json,
          output_messages_json,
          prompt_text,
          response_text,
          error_text,
          created_at,
          started_at,
          finished_at,
          updated_at
        FROM llm_calls
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(call_id.as_str())
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "llm call not found"))?;

    Ok(Json(item))
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
    let user_id = require_active_user_id(state.as_ref(), &session).await?;

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
    let user_id = require_active_user_id(state.as_ref(), &session).await?;

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
pub struct ReleaseDetailResponse {
    release_id: String,
    repo_full_name: Option<String>,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    is_prerelease: i64,
    is_draft: i64,
    translated: Option<TranslatedItem>,
}

pub async fn get_release_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(release_id_raw): Path<String>,
) -> Result<Json<ReleaseDetailResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let release_id = parse_release_id_param(&release_id_raw)?;

    #[derive(Debug, sqlx::FromRow)]
    struct ReleaseDetailRow {
        repo_id: i64,
        release_id: i64,
        repo_full_name: Option<String>,
        tag_name: String,
        name: Option<String>,
        body: Option<String>,
        html_url: String,
        published_at: Option<String>,
        is_prerelease: i64,
        is_draft: i64,
        trans_source_hash: Option<String>,
        trans_title: Option<String>,
        trans_summary: Option<String>,
    }

    let row = sqlx::query_as::<_, ReleaseDetailRow>(
        r#"
        SELECT
          r.repo_id,
          r.release_id,
          sr.full_name AS repo_full_name,
          r.tag_name,
          r.name,
          r.body,
          r.html_url,
          r.published_at,
          r.is_prerelease,
          r.is_draft,
          t.source_hash AS trans_source_hash,
          t.title AS trans_title,
          t.summary AS trans_summary
        FROM releases r
        LEFT JOIN starred_repos sr
          ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
        LEFT JOIN ai_translations t
          ON t.user_id = r.user_id
          AND t.entity_type = 'release_detail'
          AND t.entity_id = CAST(r.release_id AS TEXT)
          AND t.lang = 'zh-CN'
        WHERE r.user_id = ? AND r.release_id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(release_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "release not found",
        ));
    };

    let original_title = row
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&row.tag_name)
        .to_owned();
    let original_body = row.body.clone().unwrap_or_default();
    let resolved_full_name = resolve_release_full_name(&row.html_url, row.repo_id);
    let source_hash =
        release_detail_source_hash(&resolved_full_name, &original_title, &original_body);
    let translation_fresh = row.trans_source_hash.as_deref() == Some(source_hash.as_str());

    let translated = if state.config.ai.is_none() {
        Some(TranslatedItem {
            lang: "zh-CN".to_owned(),
            status: "disabled".to_owned(),
            title: None,
            summary: None,
        })
    } else if translation_fresh
        && release_detail_translation_ready(
            Some(original_body.as_str()),
            row.trans_summary.as_deref(),
        )
    {
        Some(TranslatedItem {
            lang: "zh-CN".to_owned(),
            status: "ready".to_owned(),
            title: row.trans_title.clone(),
            summary: row.trans_summary.clone(),
        })
    } else {
        Some(TranslatedItem {
            lang: "zh-CN".to_owned(),
            status: "missing".to_owned(),
            title: None,
            summary: None,
        })
    };

    Ok(Json(ReleaseDetailResponse {
        release_id: row.release_id.to_string(),
        repo_full_name: row.repo_full_name.or(Some(resolved_full_name)),
        tag_name: row.tag_name,
        name: row.name,
        body: row.body,
        html_url: row.html_url,
        published_at: row.published_at,
        is_prerelease: row.is_prerelease,
        is_draft: row.is_draft,
        translated,
    }))
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
    let user_id = require_active_user_id(state.as_ref(), &session).await?;

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
    window_start: Option<String>,
    window_end: Option<String>,
    content_markdown: String,
    created_at: String,
}

pub async fn list_briefs(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<Vec<BriefItem>>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;

    #[derive(Debug, sqlx::FromRow)]
    struct BriefRow {
        date: String,
        content_markdown: String,
        created_at: String,
    }

    let rows = sqlx::query_as::<_, BriefRow>(
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

    let at = state.config.ai_daily_at_local;
    let items = rows
        .into_iter()
        .map(|r| {
            let (window_start, window_end) = at
                .and_then(|at| {
                    chrono::NaiveDate::parse_from_str(&r.date, "%Y-%m-%d")
                        .ok()
                        .map(|d| (d, at))
                })
                .map(|(d, at)| {
                    let (start, end) = ai::compute_window_for_key_date(d, at);
                    (Some(start.to_rfc3339()), Some(end.to_rfc3339()))
                })
                .unwrap_or((None, None));

            BriefItem {
                date: r.date,
                window_start,
                window_end,
                content_markdown: r.content_markdown,
                created_at: r.created_at,
            }
        })
        .collect::<Vec<_>>();

    Ok(Json(items))
}

#[derive(Debug, Deserialize)]
pub struct ReturnModeQuery {
    return_mode: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum ReturnMode {
    Sync,
    TaskId,
    Sse,
}

impl ReturnMode {
    fn from_query(query: &ReturnModeQuery) -> Result<Self, ApiError> {
        let raw = query
            .return_mode
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("sync");
        match raw {
            "sync" => Ok(Self::Sync),
            "task_id" => Ok(Self::TaskId),
            "sse" => Ok(Self::Sse),
            _ => Err(ApiError::bad_request(
                "invalid return_mode, expected sync|task_id|sse",
            )),
        }
    }
}

#[derive(Debug, Serialize)]
struct TaskAcceptedResponse {
    mode: String,
    task_id: String,
    task_type: String,
    status: String,
}

async fn enqueue_or_stream_task(
    state: Arc<AppState>,
    mode: ReturnMode,
    new_task: jobs::NewTask,
) -> Result<Response, ApiError> {
    let task = jobs::enqueue_task(state.as_ref(), new_task)
        .await
        .map_err(ApiError::internal)?;

    match mode {
        ReturnMode::TaskId => Ok(Json(TaskAcceptedResponse {
            mode: "task_id".to_owned(),
            task_id: task.task_id,
            task_type: task.task_type,
            status: task.status,
        })
        .into_response()),
        ReturnMode::Sse => Ok(jobs::task_sse_response(state, task.task_id)),
        ReturnMode::Sync => Err(ApiError::internal("unexpected sync return mode")),
    }
}

async fn run_with_api_llm_context<F, T>(source: &str, requested_by: Option<i64>, fut: F) -> T
where
    F: Future<Output = T>,
{
    ai::with_llm_call_context(
        ai::LlmCallContext {
            source: source.to_owned(),
            requested_by,
            parent_task_id: None,
            parent_task_type: None,
        },
        fut,
    )
    .await
}

pub async fn sync_starred(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let mode = ReturnMode::from_query(&mode_query)?;

    if matches!(mode, ReturnMode::Sync) {
        let res = sync::sync_starred(state.as_ref(), user_id)
            .await
            .map_err(ApiError::internal)?;
        return Ok(Json(res).into_response());
    }

    enqueue_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_SYNC_STARRED.to_owned(),
            payload: json!({ "user_id": user_id }),
            source: "api.sync_starred".to_owned(),
            requested_by: Some(user_id),
            parent_task_id: None,
        },
    )
    .await
}

pub async fn sync_releases(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let mode = ReturnMode::from_query(&mode_query)?;

    if matches!(mode, ReturnMode::Sync) {
        let res = sync::sync_releases(state.as_ref(), user_id)
            .await
            .map_err(ApiError::internal)?;
        return Ok(Json(res).into_response());
    }

    enqueue_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_SYNC_RELEASES.to_owned(),
            payload: json!({ "user_id": user_id }),
            source: "api.sync_releases".to_owned(),
            requested_by: Some(user_id),
            parent_task_id: None,
        },
    )
    .await
}

pub async fn sync_notifications(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let mode = ReturnMode::from_query(&mode_query)?;

    if matches!(mode, ReturnMode::Sync) {
        let res = sync::sync_notifications(state.as_ref(), user_id)
            .await
            .map_err(ApiError::internal)?;
        return Ok(Json(res).into_response());
    }

    enqueue_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_SYNC_NOTIFICATIONS.to_owned(),
            payload: json!({ "user_id": user_id }),
            source: "api.sync_notifications".to_owned(),
            requested_by: Some(user_id),
            parent_task_id: None,
        },
    )
    .await
}

#[derive(Debug, Serialize)]
pub struct BriefGenerateResponse {
    date: String,
    window_start: Option<String>,
    window_end: Option<String>,
    content_markdown: String,
}

pub async fn generate_brief(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let mode = ReturnMode::from_query(&mode_query)?;

    if !matches!(mode, ReturnMode::Sync) {
        return enqueue_or_stream_task(
            state,
            mode,
            jobs::NewTask {
                task_type: jobs::TASK_BRIEF_GENERATE.to_owned(),
                payload: json!({ "user_id": user_id }),
                source: "api.generate_brief".to_owned(),
                requested_by: Some(user_id),
                parent_task_id: None,
            },
        )
        .await;
    }

    let content = run_with_api_llm_context(
        "api.generate_brief.sync",
        Some(user_id),
        ai::generate_daily_brief(state.as_ref(), user_id),
    )
    .await
    .map_err(ApiError::internal)?;

    // The brief row is keyed by date. Read the most recent one to include window hints.
    let row = sqlx::query_as::<_, (String,)>(
        r#"
        SELECT date
        FROM briefs
        WHERE user_id = ?
        ORDER BY date DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let at = state.config.ai_daily_at_local;
    let date = row
        .map(|r| r.0)
        .unwrap_or_else(|| chrono::Local::now().date_naive().to_string());
    let (window_start, window_end) = at
        .and_then(|at| {
            chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .ok()
                .map(|d| (d, at))
        })
        .map(|(d, at)| {
            let (start, end) = ai::compute_window_for_key_date(d, at);
            (Some(start.to_rfc3339()), Some(end.to_rfc3339()))
        })
        .unwrap_or((None, None));

    Ok(Json(BriefGenerateResponse {
        date,
        window_start,
        window_end,
        content_markdown: content,
    })
    .into_response())
}

#[derive(Debug, Serialize)]
pub struct ReactionTokenCheckSummary {
    state: String, // idle | valid | invalid | error
    message: Option<String>,
    checked_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReactionTokenStatusResponse {
    configured: bool,
    masked_token: Option<String>,
    check: ReactionTokenCheckSummary,
}

#[derive(Debug, Deserialize)]
pub struct ReactionTokenRequest {
    token: String,
}

#[derive(Debug, Serialize)]
pub struct ReactionTokenCheckResponse {
    state: String, // valid | invalid
    message: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ReactionTokenStatusRow {
    masked_token: String,
    last_check_state: String,
    last_check_message: Option<String>,
    last_checked_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct ReactionTokenSecretRow {
    token_ciphertext: Vec<u8>,
    token_nonce: Vec<u8>,
}

fn mask_pat_token(token: &str) -> String {
    let trimmed = token.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() <= 8 {
        return "********".to_owned();
    }
    let head = chars[..4].iter().collect::<String>();
    let tail = chars[chars.len().saturating_sub(4)..]
        .iter()
        .collect::<String>();
    format!("{head}...{tail}")
}

fn has_public_repo_scope(scopes: &str) -> bool {
    scopes
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .any(|scope| scope == "public_repo")
}

async fn check_reaction_pat_with_github(
    state: &AppState,
    token: &str,
) -> Result<ReactionTokenCheckResponse, ApiError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(ApiError::bad_request("token is required"));
    }

    let resp = state
        .http
        .get("https://api.github.com/user")
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(ApiError::internal)?;

    let status = resp.status();
    let headers = resp.headers().clone();
    let body = resp.text().await.map_err(ApiError::internal)?;

    if status == reqwest::StatusCode::OK {
        let scopes = headers
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !scopes.is_empty() && !has_repo_scope(scopes) && !has_public_repo_scope(scopes) {
            return Ok(ReactionTokenCheckResponse {
                state: "invalid".to_owned(),
                message: "classic PAT needs public_repo (public) or repo (private)".to_owned(),
            });
        }
        return Ok(ReactionTokenCheckResponse {
            state: "valid".to_owned(),
            message: "token is valid".to_owned(),
        });
    }

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(ReactionTokenCheckResponse {
            state: "invalid".to_owned(),
            message: "token is invalid or expired".to_owned(),
        });
    }

    if status == reqwest::StatusCode::FORBIDDEN {
        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .map(str::trim);
        if remaining == Some("0") || is_rate_limit_message(&body) {
            return Err(github_rate_limited_error());
        }
        return Ok(ReactionTokenCheckResponse {
            state: "invalid".to_owned(),
            message: "token cannot access GitHub user API; check PAT permissions".to_owned(),
        });
    }

    Err(ApiError::new(
        StatusCode::BAD_GATEWAY,
        "github_unavailable",
        format!("github check failed with status {status}"),
    ))
}

async fn load_reaction_pat_status_row(
    state: &AppState,
    user_id: i64,
) -> Result<Option<ReactionTokenStatusRow>, ApiError> {
    sqlx::query_as::<_, ReactionTokenStatusRow>(
        r#"
        SELECT masked_token, last_check_state, last_check_message, last_checked_at
        FROM reaction_pat_tokens
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)
}

async fn load_reaction_pat_token(
    state: &AppState,
    user_id: i64,
) -> Result<Option<String>, ApiError> {
    let row = sqlx::query_as::<_, ReactionTokenSecretRow>(
        r#"
        SELECT token_ciphertext, token_nonce
        FROM reaction_pat_tokens
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    row.map(|r| {
        state
            .encryption_key
            .decrypt_str(&r.token_ciphertext, &r.token_nonce)
            .map_err(|_| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "pat_invalid",
                    "PAT is invalid or expired",
                )
            })
    })
    .transpose()
}

async fn persist_reaction_pat_check_result(
    state: &AppState,
    user_id: i64,
    check_state: &str,
    check_message: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE reaction_pat_tokens
        SET last_check_state = ?,
            last_check_message = ?,
            last_checked_at = ?,
            updated_at = ?
        WHERE user_id = ?
        "#,
    )
    .bind(check_state)
    .bind(check_message)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

pub async fn reaction_token_status(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<ReactionTokenStatusResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let row = load_reaction_pat_status_row(state.as_ref(), user_id).await?;
    let Some(row) = row else {
        return Ok(Json(ReactionTokenStatusResponse {
            configured: false,
            masked_token: None,
            check: ReactionTokenCheckSummary {
                state: "idle".to_owned(),
                message: None,
                checked_at: None,
            },
        }));
    };

    Ok(Json(ReactionTokenStatusResponse {
        configured: true,
        masked_token: Some(row.masked_token),
        check: ReactionTokenCheckSummary {
            state: match row.last_check_state.as_str() {
                "valid" => "valid".to_owned(),
                "invalid" => "invalid".to_owned(),
                "error" => "error".to_owned(),
                _ => "idle".to_owned(),
            },
            message: row.last_check_message,
            checked_at: row.last_checked_at,
        },
    }))
}

pub async fn check_reaction_token(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<ReactionTokenRequest>,
) -> Result<Json<ReactionTokenCheckResponse>, ApiError> {
    let _ = require_active_user_id(state.as_ref(), &session).await?;
    let checked = check_reaction_pat_with_github(state.as_ref(), req.token.as_str()).await?;
    Ok(Json(checked))
}

pub async fn upsert_reaction_token(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<ReactionTokenRequest>,
) -> Result<Json<ReactionTokenStatusResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let token = req.token.trim();
    if token.is_empty() {
        return Err(ApiError::bad_request("token is required"));
    }

    let checked = check_reaction_pat_with_github(state.as_ref(), token).await?;
    if checked.state != "valid" {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_invalid",
            checked.message,
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let encrypted = state
        .encryption_key
        .encrypt_str(token)
        .map_err(ApiError::internal)?;
    let masked = mask_pat_token(token);

    sqlx::query(
        r#"
        INSERT INTO reaction_pat_tokens (
          user_id, token_ciphertext, token_nonce, masked_token,
          last_check_state, last_check_message, last_checked_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
          token_ciphertext = excluded.token_ciphertext,
          token_nonce = excluded.token_nonce,
          masked_token = excluded.masked_token,
          last_check_state = excluded.last_check_state,
          last_check_message = excluded.last_check_message,
          last_checked_at = excluded.last_checked_at,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(user_id)
    .bind(encrypted.ciphertext)
    .bind(encrypted.nonce)
    .bind(&masked)
    .bind("valid")
    .bind("token is valid")
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(ReactionTokenStatusResponse {
        configured: true,
        masked_token: Some(masked),
        check: ReactionTokenCheckSummary {
            state: "valid".to_owned(),
            message: Some("token is valid".to_owned()),
            checked_at: Some(now),
        },
    }))
}

#[derive(Debug, Deserialize)]
pub struct FeedQuery {
    cursor: Option<String>,
    limit: Option<i64>,
    types: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedResponse {
    items: Vec<FeedItem>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedItem {
    kind: String,
    ts: String,
    id: String,
    repo_full_name: Option<String>,
    title: Option<String>,
    excerpt: Option<String>,
    subtitle: Option<String>,
    reason: Option<String>,
    subject_type: Option<String>,
    html_url: Option<String>,
    unread: Option<i64>,
    translated: Option<TranslatedItem>,
    reactions: Option<ReleaseReactions>,
}

#[derive(Debug, Serialize)]
pub struct TranslatedItem {
    lang: String,
    status: String, // ready | missing | disabled
    title: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ReleaseReactions {
    counts: ReleaseReactionCounts,
    viewer: ReleaseReactionViewer,
    status: String, // ready | sync_required
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct ReleaseReactionCounts {
    plus1: i64,
    laugh: i64,
    heart: i64,
    hooray: i64,
    rocket: i64,
    eyes: i64,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct ReleaseReactionViewer {
    plus1: bool,
    laugh: bool,
    heart: bool,
    hooray: bool,
    rocket: bool,
    eyes: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct FeedRow {
    kind: String,
    sort_ts: String,
    ts: String,
    id_key: String,
    entity_id: String,
    release_id: Option<i64>,
    release_node_id: Option<String>,
    repo_full_name: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    reason: Option<String>,
    subject_type: Option<String>,
    html_url: Option<String>,
    unread: Option<i64>,
    release_body: Option<String>,
    react_plus1: Option<i64>,
    react_laugh: Option<i64>,
    react_heart: Option<i64>,
    react_hooray: Option<i64>,
    react_rocket: Option<i64>,
    react_eyes: Option<i64>,
    trans_source_hash: Option<String>,
    trans_title: Option<String>,
    trans_summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseReactionContent {
    Plus1,
    Laugh,
    Heart,
    Hooray,
    Rocket,
    Eyes,
}

impl ReleaseReactionContent {
    fn from_client_str(raw: &str) -> Option<Self> {
        match raw {
            "plus1" => Some(Self::Plus1),
            "laugh" => Some(Self::Laugh),
            "heart" => Some(Self::Heart),
            "hooray" => Some(Self::Hooray),
            "rocket" => Some(Self::Rocket),
            "eyes" => Some(Self::Eyes),
            _ => None,
        }
    }

    fn as_graphql_enum(self) -> &'static str {
        match self {
            Self::Plus1 => "THUMBS_UP",
            Self::Laugh => "LAUGH",
            Self::Heart => "HEART",
            Self::Hooray => "HOORAY",
            Self::Rocket => "ROCKET",
            Self::Eyes => "EYES",
        }
    }
}

fn parse_cursor(cursor: &str) -> Result<(String, i64, String), ApiError> {
    let mut it = cursor.split('|');
    let sort_ts = it
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("invalid cursor"))?;
    let kind = it
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("invalid cursor"))?;
    let id_key = it
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("invalid cursor"))?;
    if it.next().is_some() {
        return Err(ApiError::bad_request("invalid cursor"));
    }

    let kind_rank = match kind.as_str() {
        "release" => 1,
        "notification" => 0,
        _ => return Err(ApiError::bad_request("invalid cursor kind")),
    };

    Ok((sort_ts, kind_rank, id_key))
}

fn validate_feed_types(types: Option<&str>) -> Result<(), ApiError> {
    // Feed is releases-only. Inbox belongs to its own API (`/api/notifications`) and UI tab.
    let Some(types) = types else {
        return Ok(());
    };
    for part in types.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match part {
            "releases" | "release" => {}
            "notifications" | "notification" | "inbox" => {
                return Err(ApiError::bad_request(
                    "feed only supports releases; use /api/notifications for inbox items",
                ));
            }
            _ => return Err(ApiError::bad_request(format!("invalid types: {part}"))),
        }
    }
    Ok(())
}

fn parse_release_id_param(raw: &str) -> Result<i64, ApiError> {
    let release_id_raw = raw.trim();
    if release_id_raw.is_empty() {
        return Err(ApiError::bad_request("release_id is required"));
    }
    release_id_raw
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("release_id must be an integer string"))
}

fn release_detail_source_hash(
    repo_full_name: &str,
    original_title: &str,
    original_body: &str,
) -> String {
    let source = format!(
        "v=1\nkind=release_detail\nrepo={}\ntitle={}\nbody={}\n",
        repo_full_name, original_title, original_body
    );
    ai::sha256_hex(&source)
}

fn release_detail_translation_ready(body: Option<&str>, summary: Option<&str>) -> bool {
    let body_has_content = body.map(str::trim).is_some_and(|s| !s.is_empty());
    if !body_has_content {
        return true;
    }
    summary.map(str::trim).is_some_and(|s| !s.is_empty())
}

fn has_repo_scope(scopes: &str) -> bool {
    scopes
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .any(|scope| scope == "repo")
}

fn github_reauth_required_error() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "reauth_required",
        "repo scope required; re-login via GitHub OAuth",
    )
}

fn github_rate_limited_error() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "github rate limit exceeded; retry later",
    )
}

fn github_access_restricted_error() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "forbidden",
        "github denied reaction access for this repository (OAuth app restrictions or org policy)",
    )
}

fn is_rate_limit_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("rate limit")
}

fn is_reauth_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("bad credentials") || lower.contains("requires authentication")
}

fn is_access_restricted_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("resource not accessible by integration")
        || lower.contains("saml")
        || lower.contains("oauth app access restrictions")
}

fn github_graphql_http_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> Option<ApiError> {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Some(github_reauth_required_error());
    }
    if status != reqwest::StatusCode::FORBIDDEN {
        return None;
    }

    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);
    if remaining == Some("0") || is_rate_limit_message(body) {
        return Some(github_rate_limited_error());
    }

    if is_reauth_message(body) {
        return Some(github_reauth_required_error());
    }
    if is_access_restricted_message(body) {
        return Some(github_access_restricted_error());
    }

    None
}

fn github_graphql_errors_to_api_error(errors: &[GraphQlError]) -> Option<ApiError> {
    if errors.iter().any(|e| is_rate_limit_message(&e.message)) {
        return Some(github_rate_limited_error());
    }
    if errors.iter().any(|e| is_reauth_message(&e.message)) {
        return Some(github_reauth_required_error());
    }
    if errors
        .iter()
        .any(|e| is_access_restricted_message(&e.message))
    {
        return Some(github_access_restricted_error());
    }
    None
}

fn truncate_chars<'a>(s: &'a str, max_chars: usize) -> std::borrow::Cow<'a, str> {
    if s.chars().count() > max_chars {
        std::borrow::Cow::Owned(s.chars().take(max_chars).collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

fn release_excerpt(body: Option<&str>) -> Option<String> {
    let body = body?;
    let normalized = body.replace("\r\n", "\n");

    // Preserve the markdown layout (headings/lists/line breaks) so original/translated views
    // keep a similar reading structure.
    let mut lines: Vec<String> = Vec::new();
    let mut content_lines = 0usize;
    let mut in_code = false;
    for raw in normalized.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }

        if trimmed.is_empty() {
            if !lines.is_empty() && lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }

        lines.push(trimmed.to_owned());
        content_lines += 1;
        if content_lines >= 18 {
            break;
        }
    }

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    if !lines.is_empty() {
        let out = lines.join("\n");
        return Some(truncate_chars(&out, 900).into_owned());
    }

    // Fallback: preserve the first non-empty lines even when formatting is irregular.
    let mut fallback: Vec<String> = Vec::new();
    let mut in_code = false;
    for raw in normalized.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }

        if trimmed.is_empty() {
            if !fallback.is_empty() && fallback.last().is_some_and(|line| !line.is_empty()) {
                fallback.push(String::new());
            }
            continue;
        }

        fallback.push(trimmed.to_owned());
        if fallback.len() >= 12 {
            break;
        }
    }

    while fallback.last().is_some_and(|line| line.is_empty()) {
        fallback.pop();
    }

    if fallback.is_empty() {
        None
    } else {
        let fallback = fallback.join("\n");
        Some(truncate_chars(&fallback, 900).into_owned())
    }
}

#[derive(Debug, Clone)]
struct StreamCursor {
    sort_ts: String,
    id_key: String,
}

fn parse_stream_cursor(raw: &str) -> Result<StreamCursor, ApiError> {
    let mut it = raw.split('|');
    let sort_ts = it
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("invalid mixed cursor"))?;
    let id_key = it
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("invalid mixed cursor"))?;
    if it.next().is_some() {
        return Err(ApiError::bad_request("invalid mixed cursor"));
    }
    Ok(StreamCursor { sort_ts, id_key })
}

fn parse_release_cursor(cursor: &str) -> Result<Option<StreamCursor>, ApiError> {
    let mut recognized = false;
    let mut release: Option<StreamCursor> = None;

    for part in cursor
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if let Some(v) = part.strip_prefix("r=") {
            release = Some(parse_stream_cursor(v)?);
            recognized = true;
            continue;
        }
        if let Some(v) = part.strip_prefix("release=") {
            release = Some(parse_stream_cursor(v)?);
            recognized = true;
            continue;
        }

        // Ignore notification parts from legacy mixed cursors; feed is releases-only.
        if part.starts_with("n=") || part.starts_with("notification=") {
            recognized = true;
            continue;
        }
    }

    if recognized {
        return Ok(release);
    }

    // Backward-compat: previous cursor format was "<sort_ts>|<kind>|<id_key>".
    let (sort_ts, kind_rank, id_key) = parse_cursor(cursor)?;
    match kind_rank {
        1 => Ok(Some(StreamCursor { sort_ts, id_key })),
        0 => Ok(None),
        _ => Ok(None),
    }
}

async fn fetch_feed_releases(
    state: &AppState,
    user_id: i64,
    cursor: Option<&StreamCursor>,
    limit: i64,
) -> Result<Vec<FeedRow>, ApiError> {
    let base_sql = r#"
        WITH items AS (
          SELECT
            'release' AS kind,
            COALESCE(r.published_at, r.created_at, r.updated_at) AS sort_ts,
            COALESCE(r.published_at, r.created_at, r.updated_at) AS ts,
            printf('%020d', r.release_id) AS id_key,
            CAST(r.release_id AS TEXT) AS entity_id,
            r.release_id AS release_id,
            r.node_id AS release_node_id,
            sr.full_name AS repo_full_name,
            COALESCE(NULLIF(TRIM(r.name), ''), r.tag_name) AS title,
            NULL AS subtitle,
            NULL AS reason,
            NULL AS subject_type,
            r.html_url AS html_url,
            NULL AS unread,
            r.body AS release_body,
            r.react_plus1 AS react_plus1,
            r.react_laugh AS react_laugh,
            r.react_heart AS react_heart,
            r.react_hooray AS react_hooray,
            r.react_rocket AS react_rocket,
            r.react_eyes AS react_eyes
          FROM releases r
          JOIN starred_repos sr
            ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
          WHERE r.user_id = ?
        )
        SELECT
          i.kind, i.sort_ts, i.ts, i.id_key, i.entity_id, i.release_id, i.release_node_id,
          i.repo_full_name, i.title, i.subtitle, i.reason, i.subject_type, i.html_url, i.unread,
          i.release_body, i.react_plus1, i.react_laugh, i.react_heart, i.react_hooray, i.react_rocket, i.react_eyes,
          t.source_hash AS trans_source_hash,
          t.title AS trans_title,
          t.summary AS trans_summary
        FROM items i
        LEFT JOIN ai_translations t
          ON t.user_id = ? AND t.entity_type = 'release' AND t.entity_id = i.entity_id AND t.lang = 'zh-CN'
    "#;

    let (sql, has_cursor) = if cursor.is_some() {
        (
            format!(
                r#"
                {base_sql}
                WHERE (
                  i.sort_ts < ?
                  OR (i.sort_ts = ? AND i.id_key < ?)
                )
                ORDER BY i.sort_ts DESC, i.id_key DESC
                LIMIT ?
                "#
            ),
            true,
        )
    } else {
        (
            format!(
                r#"
                {base_sql}
                ORDER BY i.sort_ts DESC, i.id_key DESC
                LIMIT ?
                "#
            ),
            false,
        )
    };

    let mut qy = sqlx::query_as::<_, FeedRow>(&sql)
        .bind(user_id)
        .bind(user_id);
    if has_cursor {
        let c = cursor.expect("cursor");
        qy = qy.bind(&c.sort_ts).bind(&c.sort_ts).bind(&c.id_key);
    }
    qy = qy.bind(limit);

    qy.fetch_all(&state.pool).await.map_err(ApiError::internal)
}

fn release_counts_from_row(r: &FeedRow) -> ReleaseReactionCounts {
    ReleaseReactionCounts {
        plus1: r.react_plus1.unwrap_or(0),
        laugh: r.react_laugh.unwrap_or(0),
        heart: r.react_heart.unwrap_or(0),
        hooray: r.react_hooray.unwrap_or(0),
        rocket: r.react_rocket.unwrap_or(0),
        eyes: r.react_eyes.unwrap_or(0),
    }
}

fn release_reactions_status(r: &FeedRow) -> &'static str {
    if r.release_node_id
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .is_none()
    {
        "sync_required"
    } else {
        "ready"
    }
}

fn apply_group_to_reactions(
    counts: &mut ReleaseReactionCounts,
    viewer: &mut ReleaseReactionViewer,
    content: &str,
    total_count: i64,
    viewer_has_reacted: bool,
) {
    match content {
        "THUMBS_UP" => {
            counts.plus1 = total_count;
            viewer.plus1 = viewer_has_reacted;
        }
        "LAUGH" => {
            counts.laugh = total_count;
            viewer.laugh = viewer_has_reacted;
        }
        "HEART" => {
            counts.heart = total_count;
            viewer.heart = viewer_has_reacted;
        }
        "HOORAY" => {
            counts.hooray = total_count;
            viewer.hooray = viewer_has_reacted;
        }
        "ROCKET" => {
            counts.rocket = total_count;
            viewer.rocket = viewer_has_reacted;
        }
        "EYES" => {
            counts.eyes = total_count;
            viewer.eyes = viewer_has_reacted;
        }
        _ => {}
    }
}

fn counts_from_groups(groups: &[GraphQlReactionGroup]) -> ReleaseReactionCounts {
    let mut counts = ReleaseReactionCounts::default();
    let mut viewer = ReleaseReactionViewer::default();
    for group in groups {
        apply_group_to_reactions(
            &mut counts,
            &mut viewer,
            group.content.as_str(),
            group.reactors.total_count,
            group.viewer_has_reacted,
        );
    }
    counts
}

fn viewer_from_groups(groups: &[GraphQlReactionGroup]) -> ReleaseReactionViewer {
    let mut counts = ReleaseReactionCounts::default();
    let mut viewer = ReleaseReactionViewer::default();
    for group in groups {
        apply_group_to_reactions(
            &mut counts,
            &mut viewer,
            group.content.as_str(),
            group.reactors.total_count,
            group.viewer_has_reacted,
        );
    }
    viewer
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReleaseReactionsData {
    nodes: Vec<Option<GraphQlReleaseNode>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReleaseNode {
    id: String,
    reaction_groups: Vec<GraphQlReactionGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReactionGroup {
    content: String,
    viewer_has_reacted: bool,
    reactors: GraphQlReactors,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReactors {
    total_count: i64,
}

#[derive(Debug, Clone)]
struct LiveReleaseReactions {
    counts: ReleaseReactionCounts,
    viewer: ReleaseReactionViewer,
}

async fn fetch_live_release_reactions(
    state: &AppState,
    access_token: &str,
    node_ids: &[String],
) -> Result<std::collections::HashMap<String, LiveReleaseReactions>, ApiError> {
    if node_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let query = r#"
      query($ids: [ID!]!) {
        nodes(ids: $ids) {
          ... on Release {
            id
            reactionGroups {
              content
              viewerHasReacted
              reactors(first: 1) {
                totalCount
              }
            }
          }
        }
      }
    "#;

    let payload = serde_json::json!({
        "query": query,
        "variables": { "ids": node_ids },
    });

    let resp = state
        .http
        .post("https://api.github.com/graphql")
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&payload)
        .send()
        .await
        .map_err(ApiError::internal)?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let headers = resp.headers().clone();
        let body = resp.text().await.map_err(ApiError::internal)?;
        if let Some(err) = github_graphql_http_error(status, &headers, &body) {
            return Err(err);
        }
        return Err(ApiError::internal(format!(
            "github graphql returned {status}: {body}"
        )));
    }

    let resp = resp
        .error_for_status()
        .map_err(ApiError::internal)?
        .json::<GraphQlResponse<GraphQlReleaseReactionsData>>()
        .await
        .map_err(ApiError::internal)?;

    let GraphQlResponse { data, errors } = resp;
    if let Some(errors) = errors
        && !errors.is_empty()
    {
        // `nodes(ids: ...)` can legitimately return partial data with per-node auth errors
        // (e.g. some private releases are inaccessible). Keep usable nodes instead of
        // downgrading the whole page to reauth-required.
        if data.is_none() {
            if let Some(err) = github_graphql_errors_to_api_error(&errors) {
                return Err(err);
            }
            let msg = errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ApiError::internal(format!("github graphql error: {msg}")));
        }
    }

    let mut out = std::collections::HashMap::new();
    let nodes = data.map(|d| d.nodes).unwrap_or_default();
    for node in nodes.into_iter().flatten() {
        out.insert(
            node.id,
            LiveReleaseReactions {
                counts: counts_from_groups(&node.reaction_groups),
                viewer: viewer_from_groups(&node.reaction_groups),
            },
        );
    }
    Ok(out)
}

async fn persist_release_reaction_counts(
    state: &AppState,
    user_id: i64,
    release_id: i64,
    counts: &ReleaseReactionCounts,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE releases
        SET react_plus1 = ?,
            react_laugh = ?,
            react_heart = ?,
            react_hooray = ?,
            react_rocket = ?,
            react_eyes = ?,
            updated_at = ?
        WHERE user_id = ? AND release_id = ?
        "#,
    )
    .bind(counts.plus1)
    .bind(counts.laugh)
    .bind(counts.heart)
    .bind(counts.hooray)
    .bind(counts.rocket)
    .bind(counts.eyes)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(user_id)
    .bind(release_id)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

fn feed_item_from_row(
    r: FeedRow,
    ai_enabled: bool,
    live_reactions: Option<&LiveReleaseReactions>,
) -> FeedItem {
    let excerpt = match r.kind.as_str() {
        "release" => release_excerpt(r.release_body.as_deref()),
        _ => None,
    };

    let source = match r.kind.as_str() {
        "release" => format!(
            // v=4: translation input is the excerpt shown in the UI (not a free-form summary of
            // the entire body) so "中文/原文" toggles show comparable content.
            "v=4\nkind=release\nrepo={}\ntitle={}\nexcerpt={}\n",
            r.repo_full_name.as_deref().unwrap_or(""),
            r.title.as_deref().unwrap_or(""),
            truncate_chars(excerpt.as_deref().unwrap_or(""), 2000),
        ),
        "notification" => format!(
            "kind=notification\nrepo={}\ntitle={}\nreason={}\nsubject_type={}\n",
            r.repo_full_name.as_deref().unwrap_or(""),
            r.title.as_deref().unwrap_or(""),
            r.reason.as_deref().unwrap_or(""),
            r.subject_type.as_deref().unwrap_or(""),
        ),
        _ => String::new(),
    };

    let translated = if !ai_enabled {
        Some(TranslatedItem {
            lang: "zh-CN".to_owned(),
            status: "disabled".to_owned(),
            title: None,
            summary: None,
        })
    } else {
        let current_hash = ai::sha256_hex(&source);
        if r.trans_source_hash.as_deref() == Some(current_hash.as_str()) {
            let mut title = r.trans_title.clone().filter(|s| !s.trim().is_empty());
            let mut summary = r.trans_summary.clone().filter(|s| !s.trim().is_empty());
            let mut status = "ready".to_owned();

            // Some early cached entries accidentally stored the entire JSON blob in `summary`.
            // Try to recover it without forcing an AI call; if we can't, mark as missing so the
            // client can re-translate.
            if let Some(raw) = summary.as_deref() {
                let t = raw.trim_start();
                if t.starts_with('{') || t.starts_with("\"{") {
                    if let Some((t_title, t_summary)) = extract_translation_from_json_blob(raw) {
                        if title.is_none() {
                            title = t_title;
                        }
                        if t_summary.is_some() {
                            summary = t_summary;
                        }
                    } else {
                        status = "missing".to_owned();
                        title = None;
                        summary = None;
                    }
                }
            }
            if status == "ready"
                && let (Some(src), Some(s)) = (excerpt.as_deref(), summary.as_deref())
                && !markdown_structure_preserved(src, s)
            {
                status = "missing".to_owned();
                title = None;
                summary = None;
            }

            Some(TranslatedItem {
                lang: "zh-CN".to_owned(),
                status,
                title,
                summary,
            })
        } else {
            Some(TranslatedItem {
                lang: "zh-CN".to_owned(),
                status: "missing".to_owned(),
                title: None,
                summary: None,
            })
        }
    };

    let status = release_reactions_status(&r);
    let mut counts = release_counts_from_row(&r);
    let mut viewer = ReleaseReactionViewer::default();
    if let Some(live) = live_reactions {
        counts = live.counts.clone();
        viewer = live.viewer.clone();
    }

    FeedItem {
        kind: r.kind,
        ts: r.ts,
        id: r.entity_id,
        repo_full_name: r.repo_full_name,
        title: r.title,
        excerpt,
        subtitle: r.subtitle,
        reason: r.reason,
        subject_type: r.subject_type,
        html_url: r.html_url,
        unread: r.unread,
        translated,
        reactions: Some(ReleaseReactions {
            counts,
            viewer,
            status: status.to_owned(),
        }),
    }
}

pub async fn list_feed(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(q): Query<FeedQuery>,
) -> Result<Json<FeedResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    validate_feed_types(q.types.as_deref())?;

    let limit = q.limit.unwrap_or(30).clamp(1, 100);
    let cursor = q.cursor.as_deref().map(str::trim).filter(|s| !s.is_empty());

    // Accept legacy cursors from the previous "mixed feed" implementation, but only use the
    // release stream cursor since feed is now releases-only.
    let release_cursor = match cursor {
        Some(c) => parse_release_cursor(c)?,
        None => None,
    };

    let rows = fetch_feed_releases(state.as_ref(), user_id, release_cursor.as_ref(), limit).await?;
    let ai_enabled = state.config.ai.is_some();

    let mut node_ids: Vec<String> = Vec::new();
    let mut release_by_node = std::collections::HashMap::<String, i64>::new();
    for row in &rows {
        if let (Some(release_id), Some(node_id)) = (row.release_id, row.release_node_id.as_deref())
        {
            let node_id = node_id.trim();
            if !node_id.is_empty() {
                node_ids.push(node_id.to_owned());
                release_by_node.insert(node_id.to_owned(), release_id);
            }
        }
    }
    node_ids.sort();
    node_ids.dedup();

    let mut live_reactions_by_node =
        std::collections::HashMap::<String, LiveReleaseReactions>::new();
    let reaction_pat = load_reaction_pat_token(state.as_ref(), user_id)
        .await
        .ok()
        .flatten();
    if !node_ids.is_empty()
        && let Some(pat) = reaction_pat
        && let Ok(live) = fetch_live_release_reactions(state.as_ref(), &pat, &node_ids).await
    {
        for (node_id, reaction) in &live {
            if let Some(release_id) = release_by_node.get(node_id) {
                let _ = persist_release_reaction_counts(
                    state.as_ref(),
                    user_id,
                    *release_id,
                    &reaction.counts,
                )
                .await;
            }
        }
        live_reactions_by_node = live;
    }

    let mut items = Vec::with_capacity(rows.len());
    let mut next_cursor: Option<String> = None;
    for (idx, r) in rows.into_iter().enumerate() {
        if idx == limit.saturating_sub(1) as usize {
            // Cursor format: "<sort_ts>|release|<id_key>" (backward compatible with parse_cursor).
            next_cursor = Some(format!("{}|release|{}", r.sort_ts, r.id_key));
        }
        let live = r
            .release_node_id
            .as_deref()
            .and_then(|id| live_reactions_by_node.get(id));
        items.push(feed_item_from_row(r, ai_enabled, live));
    }

    // If we returned fewer than limit, there's no next page.
    if items.len() < limit as usize {
        next_cursor = None;
    }

    Ok(Json(FeedResponse { items, next_cursor }))
}

#[derive(Debug, Deserialize)]
pub struct ToggleReleaseReactionRequest {
    release_id: String,
    content: String,
}

#[derive(Debug, Serialize)]
pub struct ToggleReleaseReactionResponse {
    release_id: String,
    reactions: ReleaseReactions,
}

#[derive(Debug, sqlx::FromRow)]
struct ReleaseReactionRow {
    release_id: i64,
    node_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddReactionData {
    add_reaction: Option<GraphQlMutationPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveReactionData {
    remove_reaction: Option<GraphQlMutationPayload>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlMutationPayload {
    subject: Option<GraphQlReleaseSubject>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlReleaseSubject {
    id: String,
    reaction_groups: Vec<GraphQlReactionGroup>,
}

async fn mutate_release_reaction(
    state: &AppState,
    access_token: &str,
    node_id: &str,
    content: ReleaseReactionContent,
    currently_reacted: bool,
) -> Result<LiveReleaseReactions, ApiError> {
    let (query, key) = if currently_reacted {
        (
            r#"
            mutation($input: RemoveReactionInput!) {
              removeReaction(input: $input) {
                subject {
                  ... on Release {
                    id
                    reactionGroups {
                      content
                      viewerHasReacted
                      reactors(first: 1) {
                        totalCount
                      }
                    }
                  }
                }
              }
            }
            "#,
            "removeReaction",
        )
    } else {
        (
            r#"
            mutation($input: AddReactionInput!) {
              addReaction(input: $input) {
                subject {
                  ... on Release {
                    id
                    reactionGroups {
                      content
                      viewerHasReacted
                      reactors(first: 1) {
                        totalCount
                      }
                    }
                  }
                }
              }
            }
            "#,
            "addReaction",
        )
    };

    let payload = serde_json::json!({
        "query": query,
        "variables": {
            "input": {
                "subjectId": node_id,
                "content": content.as_graphql_enum(),
            }
        }
    });

    let resp = state
        .http
        .post("https://api.github.com/graphql")
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&payload)
        .send()
        .await
        .map_err(ApiError::internal)?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let headers = resp.headers().clone();
        let body = resp.text().await.map_err(ApiError::internal)?;
        if let Some(err) = github_graphql_http_error(status, &headers, &body) {
            return Err(err);
        }
        return Err(ApiError::internal(format!(
            "github graphql returned {status}: {body}"
        )));
    }

    let resp = resp.error_for_status().map_err(ApiError::internal)?;

    let body = resp.text().await.map_err(ApiError::internal)?;
    if key == "removeReaction" {
        let parsed = serde_json::from_str::<GraphQlResponse<RemoveReactionData>>(&body)
            .map_err(ApiError::internal)?;
        if let Some(errors) = parsed.errors
            && !errors.is_empty()
        {
            if let Some(err) = github_graphql_errors_to_api_error(&errors) {
                return Err(err);
            }
            let msg = errors
                .into_iter()
                .map(|e| e.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ApiError::internal(format!("github graphql error: {msg}")));
        }
        let Some(data) = parsed.data else {
            return Err(ApiError::internal("missing graphql data"));
        };
        let Some(payload) = data.remove_reaction else {
            return Err(ApiError::internal("missing removeReaction payload"));
        };
        let Some(subject) = payload.subject else {
            return Err(ApiError::internal("missing mutation subject"));
        };
        let _subject_id = subject.id;
        return Ok(LiveReleaseReactions {
            counts: counts_from_groups(&subject.reaction_groups),
            viewer: viewer_from_groups(&subject.reaction_groups),
        });
    }

    let parsed = serde_json::from_str::<GraphQlResponse<AddReactionData>>(&body)
        .map_err(ApiError::internal)?;
    if let Some(errors) = parsed.errors
        && !errors.is_empty()
    {
        if let Some(err) = github_graphql_errors_to_api_error(&errors) {
            return Err(err);
        }
        let msg = errors
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ApiError::internal(format!("github graphql error: {msg}")));
    }
    let Some(data) = parsed.data else {
        return Err(ApiError::internal("missing graphql data"));
    };
    let Some(payload) = data.add_reaction else {
        return Err(ApiError::internal("missing addReaction payload"));
    };
    let Some(subject) = payload.subject else {
        return Err(ApiError::internal("missing mutation subject"));
    };
    let _subject_id = subject.id;
    Ok(LiveReleaseReactions {
        counts: counts_from_groups(&subject.reaction_groups),
        viewer: viewer_from_groups(&subject.reaction_groups),
    })
}

pub async fn toggle_release_reaction(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<ToggleReleaseReactionRequest>,
) -> Result<Json<ToggleReleaseReactionResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let release_id_raw = req.release_id.trim();
    if release_id_raw.is_empty() {
        return Err(ApiError::bad_request("release_id is required"));
    }
    let release_id = release_id_raw
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("release_id must be an integer string"))?;

    let Some(content) = ReleaseReactionContent::from_client_str(req.content.trim()) else {
        return Err(ApiError::bad_request("invalid reaction content"));
    };

    let token = match load_reaction_pat_token(state.as_ref(), user_id).await {
        Ok(Some(token)) => token,
        Ok(None) => {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "pat_required",
                "release reactions require a GitHub PAT",
            ));
        }
        Err(err) if err.code() == "pat_invalid" => {
            let _ = persist_reaction_pat_check_result(
                state.as_ref(),
                user_id,
                "invalid",
                Some("PAT is invalid or expired"),
            )
            .await;
            return Err(err);
        }
        Err(err) => return Err(err),
    };

    let row = sqlx::query_as::<_, ReleaseReactionRow>(
        r#"
        SELECT release_id, node_id
        FROM releases
        WHERE user_id = ? AND release_id = ?
        "#,
    )
    .bind(user_id)
    .bind(release_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "release not found",
        ));
    };

    let Some(node_id) = row
        .node_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "sync_required",
            "release reaction data is not ready; sync releases first",
        ));
    };

    let current =
        match fetch_live_release_reactions(state.as_ref(), &token, &[node_id.to_owned()]).await {
            Ok(v) => v,
            Err(err) if err.code() == "reauth_required" => {
                let _ = persist_reaction_pat_check_result(
                    state.as_ref(),
                    user_id,
                    "invalid",
                    Some("PAT is invalid or expired"),
                )
                .await;
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "pat_invalid",
                    "PAT is invalid or expired",
                ));
            }
            Err(err) => return Err(err),
        };
    let Some(current_reactions) = current.get(node_id) else {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "pat_forbidden",
            "PAT cannot access this release repository; check token repository access",
        ));
    };
    let currently_reacted = match content {
        ReleaseReactionContent::Plus1 => current_reactions.viewer.plus1,
        ReleaseReactionContent::Laugh => current_reactions.viewer.laugh,
        ReleaseReactionContent::Heart => current_reactions.viewer.heart,
        ReleaseReactionContent::Hooray => current_reactions.viewer.hooray,
        ReleaseReactionContent::Rocket => current_reactions.viewer.rocket,
        ReleaseReactionContent::Eyes => current_reactions.viewer.eyes,
    };

    let updated =
        match mutate_release_reaction(state.as_ref(), &token, node_id, content, currently_reacted)
            .await
        {
            Ok(v) => v,
            Err(err) if err.code() == "reauth_required" => {
                let _ = persist_reaction_pat_check_result(
                    state.as_ref(),
                    user_id,
                    "invalid",
                    Some("PAT is invalid or expired"),
                )
                .await;
                return Err(ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "pat_invalid",
                    "PAT is invalid or expired",
                ));
            }
            Err(err) => return Err(err),
        };
    let _ =
        persist_reaction_pat_check_result(state.as_ref(), user_id, "valid", Some("PAT is valid"))
            .await;
    persist_release_reaction_counts(state.as_ref(), user_id, row.release_id, &updated.counts)
        .await?;

    Ok(Json(ToggleReleaseReactionResponse {
        release_id: row.release_id.to_string(),
        reactions: ReleaseReactions {
            counts: updated.counts,
            viewer: updated.viewer,
            status: "ready".to_owned(),
        },
    }))
}

#[derive(Debug, Deserialize)]
pub struct TranslateReleaseRequest {
    release_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TranslateReleasesBatchRequest {
    release_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TranslateReleaseDetailRequest {
    release_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TranslateReleaseDetailBatchRequest {
    release_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TranslateNotificationRequest {
    thread_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TranslateNotificationsBatchRequest {
    thread_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslateResponse {
    lang: String,
    status: String, // ready | disabled
    title: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslateBatchResponse {
    items: Vec<TranslateBatchItem>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TranslateBatchItem {
    id: String,
    lang: String,
    status: String, // ready | disabled | missing | error | processing(stream)
    title: Option<String>,
    summary: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct TranslateBatchStreamEvent {
    event: &'static str, // item | done | error
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<TranslateBatchItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn send_batch_stream_event(
    tx: &mpsc::Sender<Result<Bytes, Infallible>>,
    event: TranslateBatchStreamEvent,
) -> bool {
    let payload = match serde_json::to_string(&event) {
        Ok(mut line) => {
            line.push('\n');
            line
        }
        Err(err) => {
            tracing::warn!(?err, "serialize batch stream event failed");
            return false;
        }
    };
    tx.send(Ok(Bytes::from(payload))).await.is_ok()
}

fn accumulate_batch_item_stats(
    item: &TranslateBatchItem,
    ready_count: &mut usize,
    disabled_count: &mut usize,
    missing_count: &mut usize,
    error_count: &mut usize,
) {
    match item.status.as_str() {
        "ready" => *ready_count += 1,
        "disabled" => *disabled_count += 1,
        "missing" => *missing_count += 1,
        "error" => *error_count += 1,
        _ => {}
    }
}

fn translate_response_from_batch_item(
    item: TranslateBatchItem,
) -> Result<TranslateResponse, ApiError> {
    let status = match item.status.as_str() {
        "disabled" => "disabled",
        "ready" => "ready",
        "missing" => {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                item.error
                    .unwrap_or_else(|| "translation target not found".to_owned()),
            ));
        }
        "error" => {
            return Err(ApiError::internal(
                item.error
                    .unwrap_or_else(|| "translation failed".to_owned()),
            ));
        }
        other => {
            return Err(ApiError::internal(format!(
                "unexpected translation status: {other}"
            )));
        }
    };

    Ok(TranslateResponse {
        lang: item.lang,
        status: status.to_owned(),
        title: item.title,
        summary: item.summary,
    })
}

#[derive(Debug, Deserialize)]
struct TranslationJson {
    title_zh: Option<String>,
    summary_md: Option<String>,
    body_md: Option<String>,
}

fn strip_markdown_code_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let Some((_, rest)) = rest.split_once('\n') else {
        return trimmed;
    };
    let rest = rest.trim();
    rest.strip_suffix("```").map(str::trim).unwrap_or(trimmed)
}

fn extract_json_object_span(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end < start {
        return None;
    }
    Some(raw[start..=end].trim())
}

fn extract_json_array_span(raw: &str) -> Option<&str> {
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    if end < start {
        return None;
    }
    Some(raw[start..=end].trim())
}

fn parse_json_value_relaxed(raw: &str) -> Option<serde_json::Value> {
    fn parse_direct(raw: &str) -> Option<serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .or_else(|| {
                let inner = serde_json::from_str::<String>(raw).ok()?;
                serde_json::from_str::<serde_json::Value>(&inner).ok()
            })
    }

    let trimmed = raw.trim();
    parse_direct(trimmed)
        .or_else(|| parse_direct(strip_markdown_code_fence(trimmed)))
        .or_else(|| extract_json_object_span(trimmed).and_then(parse_direct))
        .or_else(|| extract_json_array_span(trimmed).and_then(parse_direct))
}

fn value_as_i64(raw: &serde_json::Value) -> Option<i64> {
    if let Some(v) = raw.as_i64() {
        return Some(v);
    }
    if let Some(v) = raw.as_u64() {
        return i64::try_from(v).ok();
    }
    raw.as_str().and_then(|s| s.trim().parse::<i64>().ok())
}

fn value_as_usize(raw: &serde_json::Value) -> Option<usize> {
    if let Some(v) = raw.as_u64() {
        return usize::try_from(v).ok();
    }
    if let Some(v) = raw.as_i64() {
        return usize::try_from(v).ok();
    }
    raw.as_str().and_then(|s| s.trim().parse::<usize>().ok())
}

fn value_as_id_string(raw: &serde_json::Value) -> Option<String> {
    match raw {
        serde_json::Value::String(v) => {
            let v = v.trim();
            if v.is_empty() {
                None
            } else {
                Some(v.to_owned())
            }
        }
        serde_json::Value::Number(_) => Some(raw.to_string()),
        _ => None,
    }
}

fn object_field_as_string(
    obj: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    for key in keys {
        let Some(raw) = obj.get(*key) else {
            continue;
        };
        let Some(value) = value_as_id_string(raw) else {
            continue;
        };
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}

fn extract_items_array(value: &serde_json::Value) -> Option<&[serde_json::Value]> {
    match value {
        serde_json::Value::Array(items) => Some(items.as_slice()),
        serde_json::Value::Object(map) => map
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .or_else(|| {
                map.get("data")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::as_slice)
            })
            .or_else(|| {
                map.get("translations")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::as_slice)
            }),
        _ => None,
    }
}

fn parse_translation_json(raw: &str) -> Option<TranslationJson> {
    fn parse_direct(raw: &str) -> Option<TranslationJson> {
        let parsed = serde_json::from_str::<TranslationJson>(raw)
            .ok()
            .or_else(|| {
                let inner = serde_json::from_str::<String>(raw).ok()?;
                serde_json::from_str::<TranslationJson>(&inner).ok()
            })?;

        Some(TranslationJson {
            title_zh: parsed.title_zh,
            summary_md: parsed.summary_md.map(|s| s.replace("\\n", "\n")),
            body_md: parsed.body_md.map(|s| s.replace("\\n", "\n")),
        })
    }

    let trimmed = raw.trim();
    parse_direct(trimmed)
        .or_else(|| parse_direct(strip_markdown_code_fence(trimmed)))
        .or_else(|| extract_json_object_span(trimmed).and_then(parse_direct))
}

fn extract_translation_from_json_blob(raw: &str) -> Option<(Option<String>, Option<String>)> {
    let parsed = parse_translation_json(raw)?;
    let title = parsed
        .title_zh
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned());
    let summary = parsed
        .summary_md
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned());

    if title.is_none() && summary.is_none() {
        None
    } else {
        Some((title, summary))
    }
}

#[derive(Debug, Deserialize)]
struct BatchReleaseTranslationPayload {
    items: Vec<BatchReleaseTranslationItem>,
}

#[derive(Debug, Deserialize)]
struct BatchReleaseTranslationItem {
    release_id: i64,
    title_zh: Option<String>,
    summary_md: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BatchNotificationTranslationPayload {
    items: Vec<BatchNotificationTranslationItem>,
}

#[derive(Debug, Deserialize)]
struct BatchNotificationTranslationItem {
    thread_id: String,
    title_zh: Option<String>,
    summary_md: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BatchReleaseDetailTranslationPayload {
    items: Vec<BatchReleaseDetailTranslationItem>,
}

#[derive(Debug, Deserialize)]
struct BatchReleaseDetailTranslationItem {
    chunk_index: usize,
    summary_md: String,
}

fn parse_unique_release_ids(raw_ids: &[String], max_items: usize) -> Result<Vec<i64>, ApiError> {
    if raw_ids.is_empty() {
        return Err(ApiError::bad_request("release_ids is required"));
    }
    if raw_ids.len() > max_items {
        return Err(ApiError::bad_request(format!(
            "release_ids supports at most {max_items} items"
        )));
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in raw_ids {
        let release_id = parse_release_id_param(raw)?;
        if seen.insert(release_id) {
            out.push(release_id);
        }
    }
    if out.is_empty() {
        return Err(ApiError::bad_request("release_ids is required"));
    }
    Ok(out)
}

fn parse_unique_thread_ids(raw_ids: &[String], max_items: usize) -> Result<Vec<String>, ApiError> {
    if raw_ids.is_empty() {
        return Err(ApiError::bad_request("thread_ids is required"));
    }
    if raw_ids.len() > max_items {
        return Err(ApiError::bad_request(format!(
            "thread_ids supports at most {max_items} items"
        )));
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in raw_ids {
        let thread_id = raw.trim();
        if thread_id.is_empty() {
            continue;
        }
        if seen.insert(thread_id.to_owned()) {
            out.push(thread_id.to_owned());
        }
    }
    if out.is_empty() {
        return Err(ApiError::bad_request("thread_ids is required"));
    }
    Ok(out)
}

fn parse_batch_release_translation_payload(raw: &str) -> Option<BatchReleaseTranslationPayload> {
    let value = parse_json_value_relaxed(raw)?;
    let items = extract_items_array(&value)?;
    let mut out = Vec::new();

    for entry in items {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let Some(release_id) = obj
            .get("release_id")
            .and_then(value_as_i64)
            .or_else(|| obj.get("id").and_then(value_as_i64))
        else {
            continue;
        };

        let title_zh = object_field_as_string(obj, &["title_zh", "title", "title_cn"]);
        let summary_md = object_field_as_string(
            obj,
            &["summary_md", "summary", "body_md", "body", "summary_zh"],
        )
        .map(|s| s.replace("\\n", "\n"));
        out.push(BatchReleaseTranslationItem {
            release_id,
            title_zh,
            summary_md,
        });
    }

    if out.is_empty() {
        None
    } else {
        Some(BatchReleaseTranslationPayload { items: out })
    }
}

fn parse_batch_notification_translation_payload(
    raw: &str,
) -> Option<BatchNotificationTranslationPayload> {
    let value = parse_json_value_relaxed(raw)?;
    let items = extract_items_array(&value)?;
    let mut out = Vec::new();

    for entry in items {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let Some(thread_id) = obj
            .get("thread_id")
            .and_then(value_as_id_string)
            .or_else(|| obj.get("id").and_then(value_as_id_string))
        else {
            continue;
        };
        let title_zh = object_field_as_string(obj, &["title_zh", "title", "title_cn"]);
        let summary_md = object_field_as_string(
            obj,
            &["summary_md", "summary", "body_md", "body", "summary_zh"],
        )
        .map(|s| s.replace("\\n", "\n"));
        out.push(BatchNotificationTranslationItem {
            thread_id,
            title_zh,
            summary_md,
        });
    }

    if out.is_empty() {
        None
    } else {
        Some(BatchNotificationTranslationPayload { items: out })
    }
}

fn parse_batch_release_detail_translation_payload(
    raw: &str,
) -> Option<BatchReleaseDetailTranslationPayload> {
    let value = parse_json_value_relaxed(raw)?;
    let items = extract_items_array(&value)?;
    let mut out = Vec::new();

    for entry in items {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let Some(chunk_index) = obj
            .get("chunk_index")
            .and_then(value_as_usize)
            .or_else(|| obj.get("index").and_then(value_as_usize))
            .or_else(|| obj.get("id").and_then(value_as_usize))
        else {
            continue;
        };
        let Some(summary_md) = object_field_as_string(
            obj,
            &["summary_md", "summary", "body_md", "body", "summary_zh"],
        ) else {
            continue;
        };
        out.push(BatchReleaseDetailTranslationItem {
            chunk_index,
            summary_md: summary_md.replace("\\n", "\n"),
        });
    }

    if out.is_empty() {
        None
    } else {
        Some(BatchReleaseDetailTranslationPayload { items: out })
    }
}

fn line_prefix_kind(line: &str) -> &'static str {
    let t = line.trim();
    if t.is_empty() {
        return "blank";
    }
    if t.starts_with('#') {
        return "heading";
    }
    if t.starts_with("> ") || t == ">" {
        return "blockquote";
    }
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return "ul";
    }
    if let Some((head, tail)) = t.split_once('.')
        && !head.is_empty()
        && head.chars().all(|c| c.is_ascii_digit())
        && tail.starts_with(' ')
    {
        return "ol";
    }
    "plain"
}

fn markdown_structure_preserved(source: &str, translated: &str) -> bool {
    let normalized_source = source.replace("\r\n", "\n");
    let src_lines: Vec<&str> = normalized_source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    let normalized_translated = translated.replace("\r\n", "\n");
    let dst_lines: Vec<&str> = normalized_translated
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if src_lines.is_empty() {
        return true;
    }
    if dst_lines.is_empty() || src_lines.len() != dst_lines.len() {
        return false;
    }

    src_lines.iter().zip(dst_lines.iter()).all(|(s, d)| {
        if line_prefix_kind(s) != line_prefix_kind(d) {
            return false;
        }

        let src_bold_pairs = s.matches("**").count() / 2;
        let dst_bold_pairs = d.matches("**").count() / 2;
        if src_bold_pairs > 0 && dst_bold_pairs == 0 {
            return false;
        }

        let src_code_pairs = s.matches('`').count() / 2;
        let dst_code_pairs = d.matches('`').count() / 2;
        if src_code_pairs > 0 && dst_code_pairs == 0 {
            return false;
        }
        true
    })
}

fn split_markdown_chunks(input: &str, max_chars: usize) -> Vec<String> {
    if input.is_empty() {
        return vec![String::new()];
    }
    if max_chars == 0 {
        return vec![input.replace("\r\n", "\n")];
    }

    let normalized = input.replace("\r\n", "\n");
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for segment in normalized.split_inclusive('\n') {
        let seg_len = segment.chars().count();

        if seg_len > max_chars {
            if !current.is_empty() {
                chunks.push(current);
                current = String::new();
                current_len = 0;
            }

            let chars: Vec<char> = segment.chars().collect();
            for part in chars.chunks(max_chars) {
                chunks.push(part.iter().collect());
            }
            continue;
        }

        if !current.is_empty() && current_len + seg_len > max_chars {
            chunks.push(current);
            current = String::new();
            current_len = 0;
        }

        current.push_str(segment);
        current_len += seg_len;
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

fn preserve_chunk_trailing_newline(source_chunk: &str, translated_chunk: String) -> String {
    if source_chunk.ends_with('\n') && !translated_chunk.ends_with('\n') {
        return format!("{translated_chunk}\n");
    }
    translated_chunk
}

const RELEASE_DETAIL_CHUNK_MAX_CHARS: usize = 2200;
const RELEASE_DETAIL_CHUNK_MAX_TOKENS: u32 = 2200;

fn extract_translation_fields(raw: &str) -> (Option<String>, Option<String>) {
    let parsed = parse_translation_json(raw);
    let out_title = parsed
        .as_ref()
        .and_then(|p| p.title_zh.as_deref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned());
    let out_summary = parsed
        .as_ref()
        .and_then(|p| p.summary_md.as_deref().or(p.body_md.as_deref()))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned())
        .or_else(|| {
            let s = raw.trim();
            // Fallback to raw text only when it's plain markdown/plaintext.
            // If the model output looks like a JSON blob but failed to parse,
            // treat it as invalid instead of persisting broken cache content.
            if s.is_empty() || looks_like_json_blob(s) {
                None
            } else {
                Some(s.to_owned())
            }
        });
    (out_title, out_summary)
}

fn release_translation_prompt(
    repo: &str,
    title: &str,
    excerpt: &str,
    previous_summary: Option<&str>,
) -> String {
    match previous_summary {
        Some(previous_summary) => format!(
            "Repo: {repo}\nOriginal title: {title}\n\nRelease notes excerpt:\n{excerpt}\n\n上一次翻译（不合格，Markdown 结构丢失）：\n{previous_summary}\n\n请重新翻译并输出严格 JSON（不要 markdown code block）：\n{{\"title_zh\": \"...\", \"summary_md\": \"...\"}}\n\n硬性要求：\n1) summary_md 的非空行数必须与 excerpt 完全一致；\n2) 每行保持相同 Markdown 前缀（#, -, 1., >）；\n3) 若原文该行包含 ** 或 `，译文该行也必须保留；\n4) 仅翻译文字，不得合并、拆分或删除行；\n5) 不新增信息，不输出 URL。"
        ),
        None => format!(
            "Repo: {repo}\nOriginal title: {title}\n\nRelease notes excerpt:\n{excerpt}\n\n请把这条 Release 的标题与内容翻译为中文，输出严格 JSON（不要 markdown code block）：\n{{\"title_zh\": \"...\", \"summary_md\": \"...\"}}\n\n硬性要求：\n1) summary_md 的非空行数必须与 excerpt 完全一致；\n2) 每行保持相同 Markdown 前缀（#, -, 1., >）；\n3) 若原文该行包含 ** 或 `，译文该行也必须保留；\n4) 仅翻译文字，不得合并、拆分或删除行；\n5) 不新增信息，不输出 URL。"
        ),
    }
}

struct TranslationUpsert<'a> {
    entity_type: &'a str,
    entity_id: &'a str,
    lang: &'a str,
    source_hash: &'a str,
    title: Option<&'a str>,
    summary: Option<&'a str>,
}

async fn upsert_translation(
    state: &AppState,
    user_id: i64,
    t: TranslationUpsert<'_>,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO ai_translations (user_id, entity_type, entity_id, lang, source_hash, title, summary, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, entity_type, entity_id, lang) DO UPDATE SET
          source_hash = excluded.source_hash,
          title = excluded.title,
          summary = excluded.summary,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(user_id)
    .bind(t.entity_type)
    .bind(t.entity_id)
    .bind(t.lang)
    .bind(t.source_hash)
    .bind(t.title)
    .bind(t.summary)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

#[derive(Debug, Clone)]
struct ReleaseBatchCandidate {
    release_id: i64,
    entity_id: String,
    full_name: String,
    title: String,
    excerpt: String,
    source_hash: String,
}

fn build_release_batch_prompt(batch: &[ReleaseBatchCandidate]) -> String {
    let mut prompt = String::from(
        "你会收到多条 GitHub Release 片段。请逐条翻译为中文。\n\
输出严格 JSON（不要 markdown code block）：\n\
{\"items\":[{\"release_id\":123,\"title_zh\":\"...\",\"summary_md\":\"...\"}]}\n\
硬性要求：\n\
1) 每个输入 release_id 必须在输出 items 中出现一次；\n\
2) 不新增事实，不输出任何 URL；\n\
3) summary_md 保持 Markdown 结构，不合并/拆分输入内容。\n",
    );

    for item in batch {
        prompt.push_str("\n---\n");
        prompt.push_str(&format!("release_id: {}\n", item.release_id));
        prompt.push_str(&format!("repo: {}\n", item.full_name));
        prompt.push_str(&format!("title: {}\n", item.title));
        prompt.push_str("excerpt:\n");
        prompt.push_str(&item.excerpt);
        prompt.push('\n');
    }
    prompt
}

const RELEASE_BATCH_MAX_TOKENS: u32 = 1_400;
const RELEASE_BATCH_OVERHEAD_TOKENS: u32 = 320;
const NOTIFICATION_BATCH_MAX_TOKENS: u32 = 1_100;
const NOTIFICATION_BATCH_OVERHEAD_TOKENS: u32 = 220;

fn ai_error_is_non_retryable(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    let status_422_non_context =
        msg.contains("ai returned 422") && !msg.contains("context") && !msg.contains("length");
    msg.contains("invalid_model_error")
        || msg.contains("model not found")
        || msg.contains("ai returned 401")
        || msg.contains("ai returned 403")
        || msg.contains("insufficient_quota")
        || status_422_non_context
}

fn normalize_translation_fields(
    raw_title: Option<String>,
    raw_summary: Option<String>,
) -> (Option<String>, Option<String>) {
    let mut title = raw_title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let mut summary = raw_summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    // Guard against model outputs where summary field accidentally embeds a full JSON blob.
    if let Some(raw) = summary.as_deref() {
        let trimmed = raw.trim_start();
        let looks_like_embedded_json = trimmed.starts_with('{')
            || trimmed.starts_with("\"{")
            || trimmed.contains("\"summary_md\"")
            || trimmed.contains("\"title_zh\"")
            || trimmed.contains("\"body_md\"");
        if looks_like_embedded_json {
            if let Some((blob_title, blob_summary)) = extract_translation_from_json_blob(raw) {
                if title.is_none() {
                    title = blob_title;
                }
                summary = blob_summary;
            } else {
                summary = None;
            }
        }
    }

    (title, summary)
}

async fn translate_release_single_candidate_with_ai(
    state: &AppState,
    item: &ReleaseBatchCandidate,
) -> Option<(Option<String>, Option<String>)> {
    let prompt = release_translation_prompt(&item.full_name, &item.title, &item.excerpt, None);
    let raw = ai::chat_completion(
        state,
        "你是一个助理，负责把 GitHub Release 标题与发布说明翻译为中文（严格保留 Markdown 结构与标记，不新增信息）。不要包含任何 URL。",
        &prompt,
        900,
    )
    .await
    .ok()?;

    let (mut out_title, mut out_summary) = extract_translation_fields(&raw);
    if let Some(summary) = out_summary.as_deref()
        && !markdown_structure_preserved(&item.excerpt, summary)
    {
        let retry_prompt =
            release_translation_prompt(&item.full_name, &item.title, &item.excerpt, Some(summary));
        if let Ok(retry_raw) = ai::chat_completion(
            state,
            "你是一个助理，负责把 GitHub Release 标题与发布说明翻译为中文（严格保留 Markdown 结构与标记，不新增信息）。不要包含任何 URL。",
            &retry_prompt,
            900,
        )
        .await
        {
            let (retry_title, retry_summary) = extract_translation_fields(&retry_raw);
            if retry_summary
                .as_deref()
                .is_some_and(|s| markdown_structure_preserved(&item.excerpt, s))
            {
                out_title = retry_title.or(out_title);
                out_summary = retry_summary;
            }
        }
    }

    if out_title.is_none() && out_summary.is_none() {
        None
    } else if !item.excerpt.trim().is_empty() && out_summary.is_none() {
        // A translated title without body summary regresses UX because the item becomes non-retryable.
        None
    } else {
        Some((out_title, out_summary))
    }
}

async fn plan_release_translation_groups(
    state: &AppState,
    pending: &[ReleaseBatchCandidate],
) -> Vec<Vec<usize>> {
    let budget_info = ai::compute_input_budget_with_source(state, RELEASE_BATCH_MAX_TOKENS).await;
    let budget = budget_info.input_budget;
    let estimated = pending
        .iter()
        .map(|item| {
            ai::estimate_text_tokens(&item.title)
                .saturating_add(ai::estimate_text_tokens(&item.excerpt))
                .saturating_add(64)
        })
        .collect::<Vec<_>>();
    let groups = ai::pack_batch_indices(&estimated, budget, RELEASE_BATCH_OVERHEAD_TOKENS);
    let split_count = groups.len().saturating_sub(1);
    let saved_calls = pending.len().saturating_sub(groups.len());
    let estimated_tokens = estimated.iter().copied().sum::<u32>();
    tracing::info!(
        batch_size = pending.len(),
        estimated_tokens,
        split_count,
        saved_calls,
        fallback_source = budget_info.fallback_source,
        input_budget = budget_info.input_budget,
        model_input_limit = budget_info.model_input_limit,
        "release translation batch plan"
    );
    groups
}

async fn translate_release_candidate_group_with_ai(
    state: &AppState,
    batch: &[ReleaseBatchCandidate],
) -> (HashMap<i64, (Option<String>, Option<String>)>, bool) {
    let prompt = build_release_batch_prompt(batch);
    let raw = ai::chat_completion(
        state,
        "你是一个批量翻译助手，负责把 GitHub Release 标题与片段翻译为中文。",
        &prompt,
        RELEASE_BATCH_MAX_TOKENS,
    )
    .await;

    let mut translated = HashMap::new();
    let mut abort_remaining_batches = false;
    match raw {
        Ok(raw) => {
            if let Some(payload) = parse_batch_release_translation_payload(&raw) {
                for item in payload.items {
                    if let Some(source) = batch
                        .iter()
                        .find(|candidate| candidate.release_id == item.release_id)
                    {
                        let (title, summary) =
                            normalize_translation_fields(item.title_zh, item.summary_md);
                        if !source.excerpt.trim().is_empty() && summary.is_none() {
                            continue;
                        }
                        if summary
                            .as_deref()
                            .is_some_and(|s| !markdown_structure_preserved(&source.excerpt, s))
                        {
                            continue;
                        }
                        if title.is_some() || summary.is_some() {
                            translated.insert(item.release_id, (title, summary));
                        }
                    }
                }
            } else {
                tracing::warn!(
                    "release batch translation response parse failed; fallback to single"
                );
            }
        }
        Err(err) => {
            if ai_error_is_non_retryable(&err) {
                abort_remaining_batches = true;
                tracing::warn!(
                    ?err,
                    "release batch translation upstream error is non-retryable; skipping single fallback"
                );
            } else {
                tracing::warn!(?err, "release batch translation failed; fallback to single");
            }
        }
    }

    if !abort_remaining_batches {
        for item in batch {
            if translated.contains_key(&item.release_id) {
                continue;
            }
            if let Some(res) = translate_release_single_candidate_with_ai(state, item).await {
                translated.insert(item.release_id, res);
            }
        }
    }

    (translated, abort_remaining_batches)
}

async fn translate_release_candidates_with_ai(
    state: &AppState,
    pending: &[ReleaseBatchCandidate],
) -> HashMap<i64, (Option<String>, Option<String>)> {
    if pending.is_empty() {
        return HashMap::new();
    }

    let groups = plan_release_translation_groups(state, pending).await;

    let mut translated = HashMap::new();
    let mut abort_remaining_batches = false;
    for batch_indices in groups {
        if abort_remaining_batches {
            break;
        }
        let batch = batch_indices
            .iter()
            .map(|idx| pending[*idx].clone())
            .collect::<Vec<_>>();
        let (batch_translated, abort_after_batch) =
            translate_release_candidate_group_with_ai(state, &batch).await;
        for (release_id, values) in batch_translated {
            translated.insert(release_id, values);
        }
        if abort_after_batch {
            abort_remaining_batches = true;
        }
    }

    translated
}

#[derive(Debug, sqlx::FromRow)]
struct ReleaseBatchSourceRow {
    release_id: i64,
    full_name: String,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct TranslationCacheRow {
    entity_id: String,
    source_hash: String,
    title: Option<String>,
    summary: Option<String>,
}

fn looks_like_json_blob(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with("\"{") {
        return true;
    }

    let unfenced = strip_markdown_code_fence(trimmed).trim_start();
    if unfenced.starts_with('{') || unfenced.starts_with("\"{") {
        return true;
    }

    trimmed.contains("\"summary_md\"")
        || trimmed.contains("\"title_zh\"")
        || trimmed.contains("\"body_md\"")
}

fn release_cache_entry_reusable(cache: &TranslationCacheRow, excerpt: &str) -> bool {
    if cache.summary.as_deref().is_some_and(looks_like_json_blob) {
        return false;
    }
    let summary_is_usable = cache
        .summary
        .as_deref()
        .is_some_and(|s| markdown_structure_preserved(excerpt, s));
    let title_only_cache = cache.summary.is_none() && cache.title.is_some();
    summary_is_usable || title_only_cache
}

#[derive(Debug)]
struct PreparedReleaseBatch {
    candidates: Vec<ReleaseBatchCandidate>,
    pending: Vec<ReleaseBatchCandidate>,
    translated: HashMap<i64, (Option<String>, Option<String>)>,
    missing: HashSet<i64>,
}

async fn prepare_release_batch(
    state: &AppState,
    user_id: i64,
    release_ids: &[i64],
) -> Result<PreparedReleaseBatch, ApiError> {
    if state.config.ai.is_none() {
        return Ok(PreparedReleaseBatch {
            candidates: Vec::new(),
            pending: Vec::new(),
            translated: HashMap::new(),
            missing: HashSet::new(),
        });
    }

    let mut source_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"
        SELECT r.release_id, sr.full_name, r.tag_name, r.name, r.body
        FROM releases r
        JOIN starred_repos sr
          ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
        WHERE r.user_id = "#,
    );
    source_query.push_bind(user_id);
    source_query.push(" AND r.release_id IN (");
    {
        let mut separated = source_query.separated(", ");
        for release_id in release_ids {
            separated.push_bind(release_id);
        }
    }
    source_query.push(")");

    let source_rows = source_query
        .build_query_as::<ReleaseBatchSourceRow>()
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;

    let mut source_by_id = HashMap::new();
    for row in source_rows {
        source_by_id.insert(row.release_id, row);
    }

    let mut candidates = Vec::new();
    let mut missing = HashSet::new();
    for release_id in release_ids {
        let Some(row) = source_by_id.get(release_id) else {
            missing.insert(*release_id);
            continue;
        };
        let title = row
            .name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&row.tag_name)
            .to_owned();
        let body = row.body.clone().unwrap_or_default();
        let excerpt = release_excerpt(Some(&body)).unwrap_or_default();
        let excerpt = if excerpt.chars().count() > 2_000 {
            excerpt.chars().take(2_000).collect::<String>()
        } else {
            excerpt
        };
        let source = format!(
            "v=4\nkind=release\nrepo={}\ntitle={}\nexcerpt={}\n",
            row.full_name, title, excerpt
        );
        candidates.push(ReleaseBatchCandidate {
            release_id: *release_id,
            entity_id: release_id.to_string(),
            full_name: row.full_name.clone(),
            title,
            excerpt,
            source_hash: ai::sha256_hex(&source),
        });
    }

    let mut cache_by_entity: HashMap<String, TranslationCacheRow> = HashMap::new();
    if !candidates.is_empty() {
        let mut cache_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            SELECT entity_id, source_hash, title, summary
            FROM ai_translations
            WHERE user_id = "#,
        );
        cache_query.push_bind(user_id);
        cache_query.push(" AND entity_type = 'release' AND lang = 'zh-CN' AND entity_id IN (");
        {
            let mut separated = cache_query.separated(", ");
            for item in &candidates {
                separated.push_bind(&item.entity_id);
            }
        }
        cache_query.push(")");

        let cache_rows = cache_query
            .build_query_as::<TranslationCacheRow>()
            .fetch_all(&state.pool)
            .await
            .map_err(ApiError::internal)?;
        for row in cache_rows {
            cache_by_entity.insert(row.entity_id.clone(), row);
        }
    }

    let mut pending = Vec::new();
    let mut translated = HashMap::<i64, (Option<String>, Option<String>)>::new();

    for item in &candidates {
        let cache = cache_by_entity.get(&item.entity_id);
        if let Some(cache) = cache
            && cache.source_hash == item.source_hash
        {
            if let Some(raw) = cache.summary.as_deref()
                && looks_like_json_blob(raw)
                && let Some((t_title, t_summary)) = extract_translation_from_json_blob(raw)
            {
                let out_title = t_title.or_else(|| cache.title.clone());
                let out_summary = t_summary.or_else(|| cache.summary.clone());
                if out_title.is_some() || out_summary.is_some() {
                    translated.insert(item.release_id, (out_title, out_summary));
                    continue;
                }
            }
            if release_cache_entry_reusable(cache, &item.excerpt) {
                translated.insert(
                    item.release_id,
                    (cache.title.clone(), cache.summary.clone()),
                );
                continue;
            }
        }
        pending.push(item.clone());
    }

    Ok(PreparedReleaseBatch {
        candidates,
        pending,
        translated,
        missing,
    })
}

fn build_release_batch_item(
    release_id: i64,
    missing: &HashSet<i64>,
    translated: &HashMap<i64, (Option<String>, Option<String>)>,
) -> TranslateBatchItem {
    if missing.contains(&release_id) {
        return TranslateBatchItem {
            id: release_id.to_string(),
            lang: "zh-CN".to_owned(),
            status: "missing".to_owned(),
            title: None,
            summary: None,
            error: Some("release not found".to_owned()),
        };
    }

    if let Some((title, summary)) = translated.get(&release_id) {
        return TranslateBatchItem {
            id: release_id.to_string(),
            lang: "zh-CN".to_owned(),
            status: "ready".to_owned(),
            title: title.clone(),
            summary: summary.clone(),
            error: None,
        };
    }

    TranslateBatchItem {
        id: release_id.to_string(),
        lang: "zh-CN".to_owned(),
        status: "error".to_owned(),
        title: None,
        summary: None,
        error: Some("translation failed".to_owned()),
    }
}

async fn upsert_release_batch_translations(
    state: &AppState,
    user_id: i64,
    candidates: &[ReleaseBatchCandidate],
    translated: &HashMap<i64, (Option<String>, Option<String>)>,
) -> Result<(), ApiError> {
    for item in candidates {
        if let Some((title, summary)) = translated.get(&item.release_id) {
            upsert_translation(
                state,
                user_id,
                TranslationUpsert {
                    entity_type: "release",
                    entity_id: &item.entity_id,
                    lang: "zh-CN",
                    source_hash: &item.source_hash,
                    title: title.as_deref(),
                    summary: summary.as_deref(),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn translate_releases_batch_internal(
    state: &AppState,
    user_id: i64,
    release_ids: &[i64],
) -> Result<Vec<TranslateBatchItem>, ApiError> {
    if state.config.ai.is_none() {
        return Ok(release_ids
            .iter()
            .map(|release_id| TranslateBatchItem {
                id: release_id.to_string(),
                lang: "zh-CN".to_owned(),
                status: "disabled".to_owned(),
                title: None,
                summary: None,
                error: None,
            })
            .collect());
    }

    let mut prepared = prepare_release_batch(state, user_id, release_ids).await?;
    let translated_pending = translate_release_candidates_with_ai(state, &prepared.pending).await;
    for (release_id, values) in translated_pending {
        prepared.translated.insert(release_id, values);
    }

    upsert_release_batch_translations(state, user_id, &prepared.candidates, &prepared.translated)
        .await?;

    Ok(release_ids
        .iter()
        .map(|release_id| {
            build_release_batch_item(*release_id, &prepared.missing, &prepared.translated)
        })
        .collect())
}

pub async fn translate_releases_batch_for_user(
    state: &AppState,
    user_id: i64,
    release_ids: &[i64],
) -> Result<TranslateBatchResponse, ApiError> {
    let items = translate_releases_batch_internal(state, user_id, release_ids).await?;
    Ok(TranslateBatchResponse { items })
}

async fn translate_releases_batch_stream_worker(
    state: Arc<AppState>,
    user_id: i64,
    release_ids: Vec<i64>,
    task_id: String,
    tx: mpsc::Sender<Result<Bytes, Infallible>>,
) {
    let mut ready_count = 0usize;
    let mut disabled_count = 0usize;
    let mut missing_count = 0usize;
    let mut error_count = 0usize;

    let context = ai::LlmCallContext {
        source: "api.translate_releases_batch_stream".to_owned(),
        requested_by: Some(user_id),
        parent_task_id: Some(task_id.clone()),
        parent_task_type: Some(jobs::TASK_TRANSLATE_RELEASE_BATCH.to_owned()),
    };

    let result = ai::with_llm_call_context(context, async {
        jobs::append_task_event(
            state.as_ref(),
            task_id.as_str(),
            "task.progress",
            json!({
                "task_id": task_id.as_str(),
                "stage": "collect",
                "total_releases": release_ids.len(),
            }),
        )
        .await
        .map_err(ApiError::internal)?;

        if state.config.ai.is_none() {
            for release_id in &release_ids {
                let item = TranslateBatchItem {
                    id: release_id.to_string(),
                    lang: "zh-CN".to_owned(),
                    status: "disabled".to_owned(),
                    title: None,
                    summary: None,
                    error: None,
                };
                if !send_batch_stream_event(
                    &tx,
                    TranslateBatchStreamEvent {
                        event: "item",
                        item: Some(item.clone()),
                        error: None,
                    },
                )
                .await
                {
                    return Err(ApiError::internal("stream client disconnected"));
                }

                accumulate_batch_item_stats(
                    &item,
                    &mut ready_count,
                    &mut disabled_count,
                    &mut missing_count,
                    &mut error_count,
                );
                jobs::append_task_event(
                    state.as_ref(),
                    task_id.as_str(),
                    "task.progress",
                    json!({
                        "task_id": task_id.as_str(),
                        "stage": "release",
                        "release_id": item.id,
                        "item_status": item.status,
                    }),
                )
                .await
                .map_err(ApiError::internal)?;
            }
            if !send_batch_stream_event(
                &tx,
                TranslateBatchStreamEvent {
                    event: "done",
                    item: None,
                    error: None,
                },
            )
            .await
            {
                return Err(ApiError::internal("stream client disconnected"));
            }
            return Ok(());
        }

        let mut prepared = prepare_release_batch(state.as_ref(), user_id, &release_ids).await?;
        let pending_ids = prepared
            .pending
            .iter()
            .map(|item| item.release_id)
            .collect::<HashSet<_>>();

        for release_id in &release_ids {
            if pending_ids.contains(release_id) {
                continue;
            }
            let item =
                build_release_batch_item(*release_id, &prepared.missing, &prepared.translated);
            if !send_batch_stream_event(
                &tx,
                TranslateBatchStreamEvent {
                    event: "item",
                    item: Some(item.clone()),
                    error: None,
                },
            )
            .await
            {
                return Err(ApiError::internal("stream client disconnected"));
            }

            accumulate_batch_item_stats(
                &item,
                &mut ready_count,
                &mut disabled_count,
                &mut missing_count,
                &mut error_count,
            );
            jobs::append_task_event(
                state.as_ref(),
                task_id.as_str(),
                "task.progress",
                json!({
                    "task_id": task_id.as_str(),
                    "stage": "release",
                    "release_id": item.id,
                    "item_status": item.status,
                }),
            )
            .await
            .map_err(ApiError::internal)?;
        }

        if !prepared.pending.is_empty() {
            // Emit immediate in-progress items so the frontend receives stream bytes early
            // instead of waiting for the first upstream translation response.
            for item in &prepared.pending {
                if !send_batch_stream_event(
                    &tx,
                    TranslateBatchStreamEvent {
                        event: "item",
                        item: Some(TranslateBatchItem {
                            id: item.release_id.to_string(),
                            lang: "zh-CN".to_owned(),
                            status: "processing".to_owned(),
                            title: None,
                            summary: None,
                            error: None,
                        }),
                        error: None,
                    },
                )
                .await
                {
                    return Err(ApiError::internal("stream client disconnected"));
                }
            }

            let groups = plan_release_translation_groups(state.as_ref(), &prepared.pending).await;
            let mut abort_remaining_batches = false;
            for batch_indices in groups {
                let batch = batch_indices
                    .iter()
                    .map(|idx| prepared.pending[*idx].clone())
                    .collect::<Vec<_>>();

                if !abort_remaining_batches {
                    let (batch_translated, abort_after_batch) =
                        translate_release_candidate_group_with_ai(state.as_ref(), &batch).await;
                    if !batch_translated.is_empty() {
                        upsert_release_batch_translations(
                            state.as_ref(),
                            user_id,
                            &batch,
                            &batch_translated,
                        )
                        .await?;
                        for (release_id, values) in batch_translated {
                            prepared.translated.insert(release_id, values);
                        }
                    }
                    if abort_after_batch {
                        abort_remaining_batches = true;
                    }
                }

                for item in &batch {
                    let out = build_release_batch_item(
                        item.release_id,
                        &prepared.missing,
                        &prepared.translated,
                    );
                    if !send_batch_stream_event(
                        &tx,
                        TranslateBatchStreamEvent {
                            event: "item",
                            item: Some(out.clone()),
                            error: None,
                        },
                    )
                    .await
                    {
                        return Err(ApiError::internal("stream client disconnected"));
                    }

                    accumulate_batch_item_stats(
                        &out,
                        &mut ready_count,
                        &mut disabled_count,
                        &mut missing_count,
                        &mut error_count,
                    );
                    jobs::append_task_event(
                        state.as_ref(),
                        task_id.as_str(),
                        "task.progress",
                        json!({
                            "task_id": task_id.as_str(),
                            "stage": "release",
                            "release_id": out.id,
                            "item_status": out.status,
                        }),
                    )
                    .await
                    .map_err(ApiError::internal)?;
                }
            }
        }

        if !send_batch_stream_event(
            &tx,
            TranslateBatchStreamEvent {
                event: "done",
                item: None,
                error: None,
            },
        )
        .await
        {
            return Err(ApiError::internal("stream client disconnected"));
        }
        Ok::<(), ApiError>(())
    })
    .await;

    match result {
        Ok(()) => {
            let summary = json!({
                "total": release_ids.len(),
                "ready": ready_count,
                "disabled": disabled_count,
                "missing": missing_count,
                "error": error_count,
            });
            let _ = jobs::complete_task(
                state.as_ref(),
                task_id.as_str(),
                jobs::STATUS_SUCCEEDED,
                Some(summary.clone()),
                None,
            )
            .await;
            let _ = jobs::append_task_event(
                state.as_ref(),
                task_id.as_str(),
                "task.completed",
                json!({
                    "task_id": task_id.as_str(),
                    "status": jobs::STATUS_SUCCEEDED,
                    "summary": summary,
                }),
            )
            .await;
        }
        Err(err) => {
            let error_message = format!("{}: stream worker failed", err.code());
            let _ = jobs::complete_task(
                state.as_ref(),
                task_id.as_str(),
                jobs::STATUS_FAILED,
                None,
                Some(error_message.clone()),
            )
            .await;
            let _ = jobs::append_task_event(
                state.as_ref(),
                task_id.as_str(),
                "task.completed",
                json!({
                    "task_id": task_id.as_str(),
                    "status": jobs::STATUS_FAILED,
                    "error": error_message,
                }),
            )
            .await;
            let _ = send_batch_stream_event(
                &tx,
                TranslateBatchStreamEvent {
                    event: "error",
                    item: None,
                    error: Some(error_message),
                },
            )
            .await;
        }
    }
}

pub async fn translate_releases_batch(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateReleasesBatchRequest>,
) -> Result<Json<TranslateBatchResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let release_ids = parse_unique_release_ids(&req.release_ids, 60)?;
    let items = run_with_api_llm_context(
        "api.translate_releases_batch",
        Some(user_id),
        translate_releases_batch_internal(state.as_ref(), user_id, &release_ids),
    )
    .await?;
    Ok(Json(TranslateBatchResponse { items }))
}

pub async fn translate_releases_batch_stream(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateReleasesBatchRequest>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let release_ids = parse_unique_release_ids(&req.release_ids, 60)?;
    let tracking_task = jobs::start_inline_task(
        state.as_ref(),
        jobs::NewTask {
            task_type: jobs::TASK_TRANSLATE_RELEASE_BATCH.to_owned(),
            payload: json!({
                "user_id": user_id,
                "release_ids": release_ids.clone(),
            }),
            source: "api.translate_releases_batch_stream".to_owned(),
            requested_by: Some(user_id),
            parent_task_id: None,
        },
    )
    .await
    .map_err(ApiError::internal)?;

    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(64);
    let state_cloned = state.clone();
    let tracking_task_id = tracking_task.task_id;

    tokio::spawn(async move {
        translate_releases_batch_stream_worker(
            state_cloned,
            user_id,
            release_ids,
            tracking_task_id,
            tx,
        )
        .await;
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(response)
}

pub async fn translate_release_for_user(
    state: &AppState,
    user_id: i64,
    release_id_raw: &str,
) -> Result<TranslateResponse, ApiError> {
    let release_id = parse_release_id_param(release_id_raw)?;
    let mut items = translate_releases_batch_internal(state, user_id, &[release_id]).await?;
    let Some(item) = items.pop() else {
        return Err(ApiError::internal("missing translation result"));
    };
    translate_response_from_batch_item(item)
}

pub async fn translate_release(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
    Json(req): Json<TranslateReleaseRequest>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let release_id = req.release_id.trim().to_owned();
    let mode = ReturnMode::from_query(&mode_query)?;

    if matches!(mode, ReturnMode::Sync) {
        let translated = run_with_api_llm_context(
            "api.translate_release.sync",
            Some(user_id),
            translate_release_for_user(state.as_ref(), user_id, release_id.as_str()),
        )
        .await?;
        return Ok(Json(translated).into_response());
    }

    enqueue_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_TRANSLATE_RELEASE.to_owned(),
            payload: json!({
                "user_id": user_id,
                "release_id": release_id,
            }),
            source: "api.translate_release".to_owned(),
            requested_by: Some(user_id),
            parent_task_id: None,
        },
    )
    .await
}

async fn translate_release_detail_chunk(
    state: &AppState,
    repo_full_name: &str,
    original_title: &str,
    chunk: &str,
    current: usize,
    total: usize,
) -> Result<String, ApiError> {
    let prompt = format!(
        "Repo: {repo}\nTitle: {title}\nChunk: {current}/{total}\n\nRelease notes chunk (Markdown):\n{chunk}\n\n请把这段 GitHub Release notes 翻译成中文 Markdown，要求：\n1) 保留原有 Markdown 结构（标题/列表/表格/引用/代码块）；\n2) 保留链接 URL 与代码；\n3) 不新增、不删减信息；\n4) 只输出翻译后的 Markdown，不要解释。",
        repo = repo_full_name,
        title = original_title,
        current = current,
        total = total,
        chunk = chunk,
    );

    let translated = ai::chat_completion(
        state,
        "你是一个严谨的技术文档翻译助手，负责把 GitHub Release notes 翻译成中文并保持 Markdown 结构。",
        &prompt,
        RELEASE_DETAIL_CHUNK_MAX_TOKENS,
    )
    .await
    .map_err(ApiError::internal)?;
    let translated = preserve_chunk_trailing_newline(chunk, translated);
    if markdown_structure_preserved(chunk, &translated) {
        return Ok(translated);
    }

    let retry_prompt = format!(
        "Repo: {repo}\nTitle: {title}\nChunk: {current}/{total}\n\nRelease notes chunk (Markdown):\n{chunk}\n\n上一次译文（结构不一致，需重译）：\n{translated}\n\n请重新翻译，并严格满足：\n1) 译文非空行数必须与原文完全一致；\n2) 每行保留相同 Markdown 前缀（#, -, 1., >）；\n3) 保留链接 URL 与代码；\n4) 不新增、不删减信息；\n5) 只输出翻译后的 Markdown，不要解释。",
        repo = repo_full_name,
        title = original_title,
        current = current,
        total = total,
        chunk = chunk,
        translated = translated,
    );
    let retry = ai::chat_completion(
        state,
        "你是一个严谨的技术文档翻译助手，负责把 GitHub Release notes 翻译成中文并保持 Markdown 结构。",
        &retry_prompt,
        RELEASE_DETAIL_CHUNK_MAX_TOKENS,
    )
    .await
    .map_err(ApiError::internal)?;
    let retry = preserve_chunk_trailing_newline(chunk, retry);
    if !markdown_structure_preserved(chunk, &retry) {
        return Err(ApiError::internal(
            "release detail translation failed to preserve markdown structure",
        ));
    }
    Ok(retry)
}

fn build_release_detail_batch_prompt(
    repo_full_name: &str,
    original_title: &str,
    chunks: &[(usize, String)],
    total: usize,
) -> String {
    let mut prompt = format!(
        "Repo: {repo}\nTitle: {title}\nTotal chunks: {total}\n\n请把下面多个 Markdown chunk 翻译为中文，输出严格 JSON（不要 markdown code block）：\n\
{{\"items\":[{{\"chunk_index\":1,\"summary_md\":\"...\"}}]}}\n\
要求：\n\
1) chunk_index 必须与输入一致；\n\
2) 每个 chunk 的 Markdown 结构必须保持；\n\
3) 保留 URL 与代码，不新增信息。\n",
        repo = repo_full_name,
        title = original_title,
        total = total
    );

    for (chunk_index, chunk) in chunks {
        prompt.push_str("\n---\n");
        prompt.push_str(&format!("chunk_index: {}\n", chunk_index));
        prompt.push_str("chunk_markdown:\n");
        prompt.push_str(chunk);
        prompt.push('\n');
    }
    prompt
}

async fn translate_release_detail_chunks_batched(
    state: &AppState,
    repo_full_name: &str,
    original_title: &str,
    chunks: &[String],
) -> Result<Vec<String>, ApiError> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    const CHUNK_BATCH_OVERHEAD_TOKENS: u32 = 320;
    let budget_info =
        ai::compute_input_budget_with_source(state, RELEASE_DETAIL_CHUNK_MAX_TOKENS).await;
    let budget = budget_info.input_budget;
    let estimated = chunks
        .iter()
        .map(|chunk| ai::estimate_text_tokens(chunk).saturating_add(48))
        .collect::<Vec<_>>();
    let grouped = ai::pack_batch_indices(&estimated, budget, CHUNK_BATCH_OVERHEAD_TOKENS);
    let split_count = grouped.len().saturating_sub(1);
    let saved_calls = chunks.len().saturating_sub(grouped.len());
    let estimated_tokens = estimated.iter().copied().sum::<u32>();
    tracing::info!(
        batch_size = chunks.len(),
        estimated_tokens,
        split_count,
        saved_calls,
        fallback_source = budget_info.fallback_source,
        input_budget = budget_info.input_budget,
        model_input_limit = budget_info.model_input_limit,
        "release detail chunk batch plan"
    );

    let mut translated = vec![String::new(); chunks.len()];
    for batch_indices in grouped {
        let batch_chunks = batch_indices
            .iter()
            .map(|idx| (idx + 1, chunks[*idx].clone()))
            .collect::<Vec<_>>();

        let prompt = build_release_detail_batch_prompt(
            repo_full_name,
            original_title,
            &batch_chunks,
            chunks.len(),
        );
        let raw = ai::chat_completion(
            state,
            "你是一个严谨的技术文档翻译助手，负责把 GitHub Release notes chunk 批量翻译成中文并保持 Markdown 结构。",
            &prompt,
            RELEASE_DETAIL_CHUNK_MAX_TOKENS,
        )
        .await;

        let mut parsed = HashMap::<usize, String>::new();
        match raw {
            Ok(raw) => {
                if let Some(payload) = parse_batch_release_detail_translation_payload(&raw) {
                    for item in payload.items {
                        if item.chunk_index == 0 || item.chunk_index > chunks.len() {
                            continue;
                        }
                        parsed.insert(item.chunk_index - 1, item.summary_md);
                    }
                } else {
                    tracing::warn!(
                        "release detail chunk batch response parse failed; fallback to single chunks"
                    );
                }
            }
            Err(err) => {
                if ai_error_is_non_retryable(&err) {
                    tracing::warn!(
                        ?err,
                        "release detail chunk batch error is non-retryable; skipping single fallback"
                    );
                    return Err(ApiError::internal(
                        "release detail translation unavailable: upstream model/channel rejected request",
                    ));
                }
                tracing::warn!(
                    ?err,
                    "release detail chunk batch failed; fallback to single"
                );
            }
        }

        for idx in batch_indices {
            let source = &chunks[idx];
            let mut out = parsed
                .remove(&idx)
                .map(|translated_chunk| preserve_chunk_trailing_newline(source, translated_chunk));

            if out
                .as_deref()
                .is_none_or(|candidate| !markdown_structure_preserved(source, candidate))
            {
                out = Some(
                    translate_release_detail_chunk(
                        state,
                        repo_full_name,
                        original_title,
                        source,
                        idx + 1,
                        chunks.len(),
                    )
                    .await?,
                );
            }

            translated[idx] = out.unwrap_or_default();
        }
    }

    Ok(translated)
}

async fn translate_release_detail_internal(
    state: &AppState,
    user_id: i64,
    release_id: i64,
) -> Result<TranslateResponse, ApiError> {
    if state.config.ai.is_none() {
        return Ok(TranslateResponse {
            lang: "zh-CN".to_owned(),
            status: "disabled".to_owned(),
            title: None,
            summary: None,
        });
    }

    #[derive(Debug, sqlx::FromRow)]
    struct ReleaseDetailSourceRow {
        repo_id: i64,
        html_url: String,
        tag_name: String,
        name: Option<String>,
        body: Option<String>,
    }

    let row = sqlx::query_as::<_, ReleaseDetailSourceRow>(
        r#"
        SELECT r.repo_id, r.html_url, r.tag_name, r.name, r.body
        FROM releases r
        WHERE r.user_id = ? AND r.release_id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(release_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "release not found",
        ));
    };

    let original_title = row
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&row.tag_name)
        .to_owned();
    let original_body = row.body.unwrap_or_default();
    let repo_full_name = resolve_release_full_name(&row.html_url, row.repo_id);

    let source_hash = release_detail_source_hash(&repo_full_name, &original_title, &original_body);
    let entity_id = release_id.to_string();

    #[derive(Debug, sqlx::FromRow)]
    struct TranslationRow {
        source_hash: String,
        title: Option<String>,
        summary: Option<String>,
    }
    let cached = sqlx::query_as::<_, TranslationRow>(
        r#"
        SELECT source_hash, title, summary
        FROM ai_translations
        WHERE user_id = ? AND entity_type = 'release_detail' AND entity_id = ? AND lang = 'zh-CN'
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(&entity_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    if let Some(cached) = cached
        && cached.source_hash == source_hash
        && release_detail_translation_ready(Some(original_body.as_str()), cached.summary.as_deref())
    {
        return Ok(TranslateResponse {
            lang: "zh-CN".to_owned(),
            status: "ready".to_owned(),
            title: cached.title,
            summary: cached.summary,
        });
    }

    let translated_title = ai::chat_completion(
        state,
        "你是一个翻译助手，只把 GitHub Release 标题翻译成自然中文。输出纯文本，不要解释。",
        &format!(
            "Repo: {}\nOriginal title: {}\n\n输出中文标题：",
            repo_full_name, original_title
        ),
        120,
    )
    .await
    .ok()
    .and_then(|s| {
        let title = s.trim();
        if title.is_empty() {
            None
        } else {
            Some(title.to_owned())
        }
    });

    let body_markdown = if original_body.trim().is_empty() {
        String::new()
    } else {
        let chunks = split_markdown_chunks(&original_body, RELEASE_DETAIL_CHUNK_MAX_CHARS);
        let translated_chunks = translate_release_detail_chunks_batched(
            state,
            &repo_full_name,
            &original_title,
            &chunks,
        )
        .await?;
        translated_chunks.join("")
    };
    let translated_summary = (!body_markdown.trim().is_empty()).then_some(body_markdown);
    if !release_detail_translation_ready(
        Some(original_body.as_str()),
        translated_summary.as_deref(),
    ) {
        return Err(ApiError::internal(
            "release detail translation produced empty summary",
        ));
    }

    upsert_translation(
        state,
        user_id,
        TranslationUpsert {
            entity_type: "release_detail",
            entity_id: &entity_id,
            lang: "zh-CN",
            source_hash: &source_hash,
            title: translated_title.as_deref(),
            summary: translated_summary.as_deref(),
        },
    )
    .await?;

    Ok(TranslateResponse {
        lang: "zh-CN".to_owned(),
        status: "ready".to_owned(),
        title: translated_title,
        summary: translated_summary,
    })
}

pub async fn translate_release_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
    Json(req): Json<TranslateReleaseDetailRequest>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let release_id = req.release_id.trim().to_owned();
    let mode = ReturnMode::from_query(&mode_query)?;

    if matches!(mode, ReturnMode::Sync) {
        let translated = run_with_api_llm_context(
            "api.translate_release_detail.sync",
            Some(user_id),
            translate_release_detail_for_user(state.as_ref(), user_id, release_id.as_str()),
        )
        .await?;
        return Ok(Json(translated).into_response());
    }

    enqueue_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_TRANSLATE_RELEASE_DETAIL.to_owned(),
            payload: json!({
                "user_id": user_id,
                "release_id": release_id,
            }),
            source: "api.translate_release_detail".to_owned(),
            requested_by: Some(user_id),
            parent_task_id: None,
        },
    )
    .await
}

pub async fn translate_release_detail_for_user(
    state: &AppState,
    user_id: i64,
    release_id_raw: &str,
) -> Result<TranslateResponse, ApiError> {
    let release_id = parse_release_id_param(release_id_raw)?;
    let mut items = translate_release_detail_batch_internal(state, user_id, &[release_id]).await?;
    let Some(item) = items.pop() else {
        return Err(ApiError::internal("missing translation result"));
    };
    translate_response_from_batch_item(item)
}
async fn translate_release_detail_batch_internal(
    state: &AppState,
    user_id: i64,
    release_ids: &[i64],
) -> Result<Vec<TranslateBatchItem>, ApiError> {
    let mut items = Vec::with_capacity(release_ids.len());
    for release_id in release_ids {
        match translate_release_detail_internal(state, user_id, *release_id).await {
            Ok(translated) => items.push(TranslateBatchItem {
                id: release_id.to_string(),
                lang: translated.lang,
                status: translated.status,
                title: translated.title,
                summary: translated.summary,
                error: None,
            }),
            Err(err) if err.code() == "not_found" => items.push(TranslateBatchItem {
                id: release_id.to_string(),
                lang: "zh-CN".to_owned(),
                status: "missing".to_owned(),
                title: None,
                summary: None,
                error: Some("release not found".to_owned()),
            }),
            Err(err) => {
                tracing::warn!(
                    release_id,
                    error_code = err.code(),
                    "release detail translation failed inside batch"
                );
                items.push(TranslateBatchItem {
                    id: release_id.to_string(),
                    lang: "zh-CN".to_owned(),
                    status: "error".to_owned(),
                    title: None,
                    summary: None,
                    error: Some("translation failed".to_owned()),
                });
            }
        }
    }
    Ok(items)
}

pub async fn translate_release_detail_batch(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateReleaseDetailBatchRequest>,
) -> Result<Json<TranslateBatchResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let release_ids = parse_unique_release_ids(&req.release_ids, 20)?;
    let items = run_with_api_llm_context(
        "api.translate_release_detail_batch",
        Some(user_id),
        translate_release_detail_batch_internal(state.as_ref(), user_id, &release_ids),
    )
    .await?;
    Ok(Json(TranslateBatchResponse { items }))
}

#[derive(Debug, Clone)]
struct NotificationBatchCandidate {
    thread_id: String,
    repo_full_name: String,
    subject_title: String,
    reason: String,
    subject_type: String,
    source_hash: String,
}

fn build_notification_batch_prompt(items: &[NotificationBatchCandidate]) -> String {
    let mut prompt = String::from(
        "你会收到多条 GitHub Inbox 通知，请逐条翻译并给出简短建议。\n\
输出严格 JSON（不要 markdown code block）：\n\
{\"items\":[{\"thread_id\":\"123\",\"title_zh\":\"...\",\"summary_md\":\"- ...\"}]}\n\
要求：\n\
1) 每个输入 thread_id 必须在输出里出现；\n\
2) summary_md 1-3 条；\n\
3) 不输出 URL，不新增事实。\n",
    );
    for item in items {
        prompt.push_str("\n---\n");
        prompt.push_str(&format!("thread_id: {}\n", item.thread_id));
        prompt.push_str(&format!("repo: {}\n", item.repo_full_name));
        prompt.push_str(&format!("title: {}\n", item.subject_title));
        prompt.push_str(&format!("reason: {}\n", item.reason));
        prompt.push_str(&format!("type: {}\n", item.subject_type));
    }
    prompt
}

async fn translate_notification_single_candidate_with_ai(
    state: &AppState,
    item: &NotificationBatchCandidate,
) -> Option<(Option<String>, Option<String>)> {
    let prompt = format!(
        "Repo: {repo}\nOriginal title: {title}\nReason: {reason}\nType: {subject_type}\n\n请把这条 Inbox 通知翻译/解释为中文，输出严格 JSON（不要 markdown code block）：\n{{\"title_zh\": \"...\", \"summary_md\": \"- ...\"}}\n\n要求：summary_md 1-3 条；给出建议动作；不包含任何 URL。",
        repo = item.repo_full_name,
        title = item.subject_title,
        reason = item.reason,
        subject_type = item.subject_type,
    );

    let raw = ai::chat_completion(
        state,
        "你是一个助理，负责把 GitHub Notifications 条目转写为中文标题与简短建议（Markdown）。不要包含任何 URL。",
        &prompt,
        500,
    )
    .await
    .ok()?;
    let parsed = parse_translation_json(&raw);
    let title = parsed
        .as_ref()
        .and_then(|p| p.title_zh.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let summary = parsed
        .as_ref()
        .and_then(|p| p.summary_md.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            let s = raw.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_owned())
            }
        });
    if title.is_none() && summary.is_none() {
        None
    } else {
        Some((title, summary))
    }
}

async fn translate_notification_candidates_with_ai(
    state: &AppState,
    pending: &[NotificationBatchCandidate],
) -> HashMap<String, (Option<String>, Option<String>)> {
    if pending.is_empty() {
        return HashMap::new();
    }

    let budget_info =
        ai::compute_input_budget_with_source(state, NOTIFICATION_BATCH_MAX_TOKENS).await;
    let budget = budget_info.input_budget;
    let estimated = pending
        .iter()
        .map(|item| {
            ai::estimate_text_tokens(&item.repo_full_name)
                .saturating_add(ai::estimate_text_tokens(&item.subject_title))
                .saturating_add(ai::estimate_text_tokens(&item.reason))
                .saturating_add(ai::estimate_text_tokens(&item.subject_type))
                .saturating_add(32)
        })
        .collect::<Vec<_>>();
    let groups = ai::pack_batch_indices(&estimated, budget, NOTIFICATION_BATCH_OVERHEAD_TOKENS);
    let split_count = groups.len().saturating_sub(1);
    let saved_calls = pending.len().saturating_sub(groups.len());
    let estimated_tokens = estimated.iter().copied().sum::<u32>();
    tracing::info!(
        batch_size = pending.len(),
        estimated_tokens,
        split_count,
        saved_calls,
        fallback_source = budget_info.fallback_source,
        input_budget = budget_info.input_budget,
        model_input_limit = budget_info.model_input_limit,
        "notification translation batch plan"
    );

    let mut translated = HashMap::new();
    let mut abort_remaining_batches = false;
    for batch_indices in groups {
        if abort_remaining_batches {
            break;
        }
        let batch = batch_indices
            .iter()
            .map(|idx| pending[*idx].clone())
            .collect::<Vec<_>>();
        let prompt = build_notification_batch_prompt(&batch);
        let raw = ai::chat_completion(
            state,
            "你是一个批量翻译助手，负责把 GitHub Notifications 条目转写为中文标题与建议。",
            &prompt,
            NOTIFICATION_BATCH_MAX_TOKENS,
        )
        .await;

        match raw {
            Ok(raw) => {
                if let Some(payload) = parse_batch_notification_translation_payload(&raw) {
                    for item in payload.items {
                        if !batch
                            .iter()
                            .any(|candidate| candidate.thread_id == item.thread_id)
                        {
                            continue;
                        }
                        let (title, summary) =
                            normalize_translation_fields(item.title_zh, item.summary_md);
                        if title.is_some() || summary.is_some() {
                            translated.insert(item.thread_id, (title, summary));
                        }
                    }
                } else {
                    tracing::warn!(
                        "notification batch translation response parse failed; fallback to single"
                    );
                }
            }
            Err(err) => {
                if ai_error_is_non_retryable(&err) {
                    abort_remaining_batches = true;
                    tracing::warn!(
                        ?err,
                        "notification batch translation upstream error is non-retryable; skipping single fallback"
                    );
                } else {
                    tracing::warn!(
                        ?err,
                        "notification batch translation failed; fallback to single"
                    );
                }
            }
        }

        if !abort_remaining_batches {
            for item in &batch {
                if translated.contains_key(&item.thread_id) {
                    continue;
                }
                if let Some(res) =
                    translate_notification_single_candidate_with_ai(state, item).await
                {
                    translated.insert(item.thread_id.clone(), res);
                }
            }
        }
    }

    translated
}

#[derive(Debug, sqlx::FromRow)]
struct NotificationBatchSourceRow {
    thread_id: String,
    repo_full_name: Option<String>,
    subject_title: Option<String>,
    reason: Option<String>,
    subject_type: Option<String>,
}

async fn translate_notifications_batch_internal(
    state: &AppState,
    user_id: i64,
    thread_ids: &[String],
) -> Result<Vec<TranslateBatchItem>, ApiError> {
    if state.config.ai.is_none() {
        return Ok(thread_ids
            .iter()
            .map(|thread_id| TranslateBatchItem {
                id: thread_id.clone(),
                lang: "zh-CN".to_owned(),
                status: "disabled".to_owned(),
                title: None,
                summary: None,
                error: None,
            })
            .collect());
    }

    let mut source_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"
        SELECT thread_id, repo_full_name, subject_title, reason, subject_type
        FROM notifications
        WHERE user_id = "#,
    );
    source_query.push_bind(user_id);
    source_query.push(" AND thread_id IN (");
    {
        let mut separated = source_query.separated(", ");
        for thread_id in thread_ids {
            separated.push_bind(thread_id);
        }
    }
    source_query.push(")");

    let rows = source_query
        .build_query_as::<NotificationBatchSourceRow>()
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let mut rows_by_id = HashMap::new();
    for row in rows {
        rows_by_id.insert(row.thread_id.clone(), row);
    }

    let mut candidates = Vec::new();
    let mut missing = HashSet::new();
    for thread_id in thread_ids {
        let Some(row) = rows_by_id.get(thread_id) else {
            missing.insert(thread_id.clone());
            continue;
        };
        let source = format!(
            "kind=notification\nrepo={}\ntitle={}\nreason={}\nsubject_type={}\n",
            row.repo_full_name.as_deref().unwrap_or(""),
            row.subject_title.as_deref().unwrap_or(""),
            row.reason.as_deref().unwrap_or(""),
            row.subject_type.as_deref().unwrap_or(""),
        );
        candidates.push(NotificationBatchCandidate {
            thread_id: thread_id.clone(),
            repo_full_name: row
                .repo_full_name
                .clone()
                .unwrap_or_else(|| "(unknown repo)".to_owned()),
            subject_title: row
                .subject_title
                .clone()
                .unwrap_or_else(|| "(no title)".to_owned()),
            reason: row.reason.clone().unwrap_or_default(),
            subject_type: row.subject_type.clone().unwrap_or_default(),
            source_hash: ai::sha256_hex(&source),
        });
    }

    let mut cache_by_id = HashMap::<String, TranslationCacheRow>::new();
    if !candidates.is_empty() {
        let mut cache_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            SELECT entity_id, source_hash, title, summary
            FROM ai_translations
            WHERE user_id = "#,
        );
        cache_query.push_bind(user_id);
        cache_query.push(" AND entity_type = 'notification' AND lang = 'zh-CN' AND entity_id IN (");
        {
            let mut separated = cache_query.separated(", ");
            for item in &candidates {
                separated.push_bind(&item.thread_id);
            }
        }
        cache_query.push(")");

        let cache_rows = cache_query
            .build_query_as::<TranslationCacheRow>()
            .fetch_all(&state.pool)
            .await
            .map_err(ApiError::internal)?;
        for row in cache_rows {
            cache_by_id.insert(row.entity_id.clone(), row);
        }
    }

    let mut translated = HashMap::<String, (Option<String>, Option<String>)>::new();
    let mut pending = Vec::new();
    for item in &candidates {
        if let Some(cache) = cache_by_id.get(&item.thread_id)
            && cache.source_hash == item.source_hash
        {
            if let Some(raw) = cache.summary.as_deref()
                && looks_like_json_blob(raw)
                && let Some((t_title, t_summary)) = extract_translation_from_json_blob(raw)
            {
                let out_title = t_title.or_else(|| cache.title.clone());
                let out_summary = t_summary.or_else(|| cache.summary.clone());
                if out_title.is_some() || out_summary.is_some() {
                    translated.insert(item.thread_id.clone(), (out_title, out_summary));
                    continue;
                }
            }

            let cache_is_json_blob = cache.summary.as_deref().is_some_and(looks_like_json_blob);
            if !cache_is_json_blob {
                translated.insert(
                    item.thread_id.clone(),
                    (cache.title.clone(), cache.summary.clone()),
                );
                continue;
            }
        }
        pending.push(item.clone());
    }

    let pending_translated = translate_notification_candidates_with_ai(state, &pending).await;
    for (thread_id, value) in pending_translated {
        translated.insert(thread_id, value);
    }

    for item in &candidates {
        if let Some((title, summary)) = translated.get(&item.thread_id) {
            upsert_translation(
                state,
                user_id,
                TranslationUpsert {
                    entity_type: "notification",
                    entity_id: &item.thread_id,
                    lang: "zh-CN",
                    source_hash: &item.source_hash,
                    title: title.as_deref(),
                    summary: summary.as_deref(),
                },
            )
            .await?;
        }
    }

    let mut out = Vec::new();
    for thread_id in thread_ids {
        if missing.contains(thread_id) {
            out.push(TranslateBatchItem {
                id: thread_id.clone(),
                lang: "zh-CN".to_owned(),
                status: "missing".to_owned(),
                title: None,
                summary: None,
                error: Some("notification not found".to_owned()),
            });
            continue;
        }
        if let Some((title, summary)) = translated.get(thread_id) {
            out.push(TranslateBatchItem {
                id: thread_id.clone(),
                lang: "zh-CN".to_owned(),
                status: "ready".to_owned(),
                title: title.clone(),
                summary: summary.clone(),
                error: None,
            });
        } else {
            out.push(TranslateBatchItem {
                id: thread_id.clone(),
                lang: "zh-CN".to_owned(),
                status: "error".to_owned(),
                title: None,
                summary: None,
                error: Some("translation failed".to_owned()),
            });
        }
    }

    Ok(out)
}

pub async fn translate_notifications_batch(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateNotificationsBatchRequest>,
) -> Result<Json<TranslateBatchResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let thread_ids = parse_unique_thread_ids(&req.thread_ids, 60)?;
    let items = run_with_api_llm_context(
        "api.translate_notifications_batch",
        Some(user_id),
        translate_notifications_batch_internal(state.as_ref(), user_id, &thread_ids),
    )
    .await?;
    Ok(Json(TranslateBatchResponse { items }))
}

pub async fn translate_notification(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
    Json(req): Json<TranslateNotificationRequest>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let thread_id = req.thread_id.trim().to_owned();
    let mode = ReturnMode::from_query(&mode_query)?;

    if matches!(mode, ReturnMode::Sync) {
        let translated = run_with_api_llm_context(
            "api.translate_notification.sync",
            Some(user_id),
            translate_notification_for_user(state.as_ref(), user_id, thread_id.as_str()),
        )
        .await?;
        return Ok(Json(translated).into_response());
    }

    enqueue_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_TRANSLATE_NOTIFICATION.to_owned(),
            payload: json!({
                "user_id": user_id,
                "thread_id": thread_id,
            }),
            source: "api.translate_notification".to_owned(),
            requested_by: Some(user_id),
            parent_task_id: None,
        },
    )
    .await
}

pub async fn translate_notification_for_user(
    state: &AppState,
    user_id: i64,
    thread_id_raw: &str,
) -> Result<TranslateResponse, ApiError> {
    let thread_id = thread_id_raw.trim().to_owned();
    if thread_id.is_empty() {
        return Err(ApiError::bad_request("thread_id is required"));
    }

    let mut items =
        translate_notifications_batch_internal(state, user_id, std::slice::from_ref(&thread_id))
            .await?;
    let Some(item) = items.pop() else {
        return Err(ApiError::internal("missing translation result"));
    };
    translate_response_from_batch_item(item)
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

fn ensure_account_enabled(is_disabled: bool) -> Result<(), ApiError> {
    if is_disabled {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct SessionAccessRow {
    is_disabled: i64,
}

async fn require_active_user_id(state: &AppState, session: &Session) -> Result<i64, ApiError> {
    let user_id = require_user_id(session).await?;
    let row = sqlx::query_as::<_, SessionAccessRow>(
        r#"
        SELECT is_disabled
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

    if let Err(err) = ensure_account_enabled(row.is_disabled != 0) {
        session.clear().await;
        return Err(err);
    }

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE users
        SET last_active_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now.as_str())
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(user_id)
}

async fn require_admin_user_id(state: &AppState, session: &Session) -> Result<i64, ApiError> {
    let user_id = require_active_user_id(state, session).await?;
    let is_admin =
        sqlx::query_scalar::<_, i64>(r#"SELECT is_admin FROM users WHERE id = ? LIMIT 1"#)
            .bind(user_id)
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?;
    if is_admin == 0 {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden_admin_only",
            "admin permission required",
        ));
    }
    Ok(user_id)
}

#[cfg(test)]
mod tests {
    use super::{
        AdminLlmCallsQuery, AdminUserPatchRequest, AdminUserUpdateGuard, AdminUsersQuery, FeedRow,
        GraphQlError, TranslateBatchItem, TranslationCacheRow, admin_get_llm_call_detail,
        admin_get_llm_scheduler_status, admin_list_llm_calls, admin_list_users, admin_patch_user,
        admin_users_offset, ai_error_is_non_retryable, ensure_account_enabled,
        extract_translation_fields, github_graphql_errors_to_api_error, github_graphql_http_error,
        guard_admin_user_update, has_repo_scope, looks_like_json_blob, map_job_action_error,
        markdown_structure_preserved, normalize_translation_fields,
        parse_batch_notification_translation_payload,
        parse_batch_release_detail_translation_payload, parse_batch_release_translation_payload,
        parse_release_id_param, parse_repo_full_name_from_release_url, parse_translation_json,
        parse_unique_release_ids, parse_unique_thread_ids, preserve_chunk_trailing_newline,
        release_cache_entry_reusable, release_detail_source_hash, release_detail_translation_ready,
        release_excerpt, release_reactions_status, resolve_release_full_name,
        split_markdown_chunks, translate_response_from_batch_item,
    };
    use std::{net::SocketAddr, sync::Arc};

    use crate::{
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        state::{AppState, build_oauth_client},
    };
    use axum::{
        Json,
        extract::{Path, Query, State},
    };
    use reqwest::header::{HeaderMap, HeaderValue};
    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
    use tower_sessions::{MemoryStore, Session};
    use url::Url;

    fn test_feed_row(node_id: Option<&str>) -> FeedRow {
        FeedRow {
            kind: "release".to_owned(),
            sort_ts: "2026-01-01T00:00:00Z".to_owned(),
            ts: "2026-01-01T00:00:00Z".to_owned(),
            id_key: "1".to_owned(),
            entity_id: "1".to_owned(),
            release_id: Some(1),
            release_node_id: node_id.map(str::to_owned),
            repo_full_name: None,
            title: None,
            subtitle: None,
            reason: None,
            subject_type: None,
            html_url: None,
            unread: None,
            release_body: None,
            react_plus1: None,
            react_laugh: None,
            react_heart: None,
            react_hooray: None,
            react_rocket: None,
            react_eyes: None,
            trans_source_hash: None,
            trans_title: None,
            trans_summary: None,
        }
    }

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite memory db");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");

        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, created_at, updated_at)
            VALUES (1, 30215105, 'IvanLi-CN', ?, ?)
            "#,
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed user");

        pool
    }

    fn setup_state(pool: SqlitePool) -> Arc<AppState> {
        let encryption_key =
            EncryptionKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                .expect("build encryption key");
        let config = AppConfig {
            bind_addr: "127.0.0.1:58090"
                .parse::<SocketAddr>()
                .expect("parse bind addr"),
            public_base_url: Url::parse("http://127.0.0.1:58090").expect("parse public base url"),
            database_url: "sqlite::memory:".to_owned(),
            static_dir: None,
            encryption_key: encryption_key.clone(),
            github: GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse github redirect"),
            },
            ai: None,
            ai_model_context_limit: None,
            ai_daily_at_local: None,
        };
        let oauth = build_oauth_client(&config).expect("build oauth client");
        Arc::new(AppState {
            config,
            pool,
            http: reqwest::Client::new(),
            oauth,
            encryption_key,
        })
    }

    async fn setup_session(user_id: i64) -> Session {
        let store = Arc::new(MemoryStore::default());
        let session = Session::new(None, store, None);
        session
            .insert("user_id", user_id)
            .await
            .expect("insert session user id");
        session
    }

    async fn seed_user(pool: &SqlitePool, id: i64, login: &str, is_admin: i64, is_disabled: i64) {
        let created_at = format!("2026-02-23T00:00:{id:02}Z");
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, is_admin, is_disabled, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(30000000_i64 + id)
        .bind(login)
        .bind(is_admin)
        .bind(is_disabled)
        .bind(created_at.as_str())
        .bind(created_at.as_str())
        .execute(pool)
        .await
        .expect("seed test user");
    }

    async fn seed_release(pool: &SqlitePool, repo_id: i64, release_id: i64) {
        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO releases (
              user_id, repo_id, release_id, tag_name, name, body, html_url,
              published_at, created_at, is_prerelease, is_draft, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?)
            "#,
        )
        .bind(1_i64)
        .bind(repo_id)
        .bind(release_id)
        .bind("v1.2.3")
        .bind("Release v1.2.3")
        .bind("- item")
        .bind("https://github.com/openai/codex/releases/tag/v1.2.3")
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed release");
    }

    async fn seed_star(pool: &SqlitePool, repo_id: i64) {
        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO starred_repos (
              user_id, repo_id, full_name, owner_login, name,
              description, html_url, stargazed_at, is_private, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, ?)
            "#,
        )
        .bind(1_i64)
        .bind(repo_id)
        .bind("openai/codex")
        .bind("openai")
        .bind("codex")
        .bind("octo rill test")
        .bind("https://github.com/openai/codex")
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed starred");
    }

    async fn seed_llm_call(
        pool: &SqlitePool,
        call_id: &str,
        status: &str,
        source: &str,
        requested_by: Option<i64>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO llm_calls (
              id,
              status,
              source,
              model,
              requested_by,
              parent_task_id,
              parent_task_type,
              max_tokens,
              attempt_count,
              scheduler_wait_ms,
              first_token_wait_ms,
              duration_ms,
              input_tokens,
              output_tokens,
              cached_input_tokens,
              total_tokens,
              input_messages_json,
              output_messages_json,
              prompt_text,
              response_text,
              error_text,
              created_at,
              started_at,
              finished_at,
              updated_at
            )
            VALUES (?, ?, ?, 'gpt-4o-mini', ?, NULL, NULL, 900, 1, 120, 340, 800, 120, 55, 20, 175, '[{"role":"system","content":"s"},{"role":"user","content":"u"}]', '[{"role":"assistant","content":"ok"}]', 'prompt', 'ok', NULL, ?, ?, ?, ?)
            "#,
        )
        .bind(call_id)
        .bind(status)
        .bind(source)
        .bind(requested_by)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(pool)
        .await
        .expect("seed llm call");
    }

    #[test]
    fn has_repo_scope_accepts_comma_delimited_scopes() {
        assert!(has_repo_scope("read:user,user:email,repo,notifications"));
    }

    #[test]
    fn has_repo_scope_accepts_space_delimited_scopes() {
        assert!(has_repo_scope("read:user user:email repo notifications"));
    }

    #[test]
    fn has_repo_scope_rejects_missing_repo_scope() {
        assert!(!has_repo_scope("read:user,user:email,notifications"));
    }

    #[test]
    fn ensure_account_enabled_rejects_disabled_user() {
        let err = ensure_account_enabled(true).expect_err("disabled user should fail");
        assert_eq!(err.code(), "account_disabled");
    }

    #[test]
    fn guard_admin_user_update_rejects_disabling_self() {
        let err = guard_admin_user_update(AdminUserUpdateGuard {
            acting_user_id: 7,
            target_user_id: 7,
            target_is_admin: true,
            target_is_disabled: false,
            next_is_admin: true,
            next_is_disabled: true,
            admin_count: 2,
            active_admin_count: 2,
        })
        .expect_err("disabling self must fail");
        assert_eq!(err.code(), "cannot_disable_self");
    }

    #[test]
    fn guard_admin_user_update_rejects_demoting_last_admin() {
        let err = guard_admin_user_update(AdminUserUpdateGuard {
            acting_user_id: 1,
            target_user_id: 2,
            target_is_admin: true,
            target_is_disabled: false,
            next_is_admin: false,
            next_is_disabled: false,
            admin_count: 1,
            active_admin_count: 1,
        })
        .expect_err("last admin demotion must fail");
        assert_eq!(err.code(), "last_admin_guard");
    }

    #[test]
    fn guard_admin_user_update_rejects_disabling_last_active_admin() {
        let err = guard_admin_user_update(AdminUserUpdateGuard {
            acting_user_id: 1,
            target_user_id: 2,
            target_is_admin: true,
            target_is_disabled: false,
            next_is_admin: true,
            next_is_disabled: true,
            admin_count: 2,
            active_admin_count: 1,
        })
        .expect_err("last active admin disable must fail");
        assert_eq!(err.code(), "last_admin_guard");
    }

    #[test]
    fn admin_users_offset_supports_first_page() {
        assert_eq!(admin_users_offset(1, 20).expect("first page offset"), 0);
    }

    #[test]
    fn admin_users_offset_rejects_overflow_page() {
        let err = admin_users_offset(i64::MAX, 100).expect_err("overflow offset must be rejected");
        assert_eq!(err.code(), "bad_request");
    }

    #[test]
    fn map_job_action_error_maps_not_found() {
        let err = map_job_action_error(anyhow::anyhow!("task not found"));
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn map_job_action_error_maps_invalid_state() {
        let err = map_job_action_error(anyhow::anyhow!("only finished tasks can be retried"));
        assert_eq!(err.code(), "invalid_task_state");
    }

    #[tokio::test]
    async fn admin_list_users_rejects_non_admin_session() {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "viewer", 0, 0).await;
        let state = setup_state(pool);
        let session = setup_session(2).await;

        let err = admin_list_users(
            State(state),
            session,
            Query(AdminUsersQuery {
                query: None,
                role: None,
                status: None,
                page: None,
                page_size: None,
            }),
        )
        .await
        .expect_err("non-admin user should be rejected");

        assert_eq!(err.code(), "forbidden_admin_only");
    }

    #[tokio::test]
    async fn admin_list_users_clears_session_for_disabled_user() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_disabled = 1 WHERE id = 1"#)
            .execute(&pool)
            .await
            .expect("disable seeded admin");
        let state = setup_state(pool);
        let session = setup_session(1).await;
        let probe = session.clone();

        let err = admin_list_users(
            State(state),
            session,
            Query(AdminUsersQuery {
                query: None,
                role: None,
                status: None,
                page: None,
                page_size: None,
            }),
        )
        .await
        .expect_err("disabled user should be blocked");

        assert_eq!(err.code(), "account_disabled");
        let remaining = probe
            .get::<i64>("user_id")
            .await
            .expect("read session user id");
        assert!(remaining.is_none(), "disabled session should be cleared");
    }

    #[tokio::test]
    async fn admin_patch_user_rejects_demoting_last_admin_via_handler() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = 1"#)
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        let state = setup_state(pool);
        let session = setup_session(1).await;

        let err = admin_patch_user(
            State(state),
            session,
            Path(1_i64),
            Json(AdminUserPatchRequest {
                is_admin: Some(false),
                is_disabled: None,
            }),
        )
        .await
        .expect_err("last admin demotion should fail");

        assert_eq!(err.code(), "last_admin_guard");
    }

    #[tokio::test]
    async fn admin_list_llm_calls_rejects_non_admin_session() {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "viewer", 0, 0).await;
        let state = setup_state(pool);
        let session = setup_session(2).await;

        let err = admin_list_llm_calls(
            State(state),
            session,
            Query(AdminLlmCallsQuery {
                status: None,
                source: None,
                requested_by: None,
                started_from: None,
                started_to: None,
                page: None,
                page_size: None,
            }),
        )
        .await
        .expect_err("non-admin user should be rejected");

        assert_eq!(err.code(), "forbidden_admin_only");
    }

    #[tokio::test]
    async fn admin_list_llm_calls_filters_status_and_source() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = 1"#)
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            "call-failed",
            "failed",
            "api.translate_releases_batch",
            Some(1),
        )
        .await;
        seed_llm_call(
            &pool,
            "call-ok",
            "succeeded",
            "api.translate_release",
            Some(1),
        )
        .await;

        let state = setup_state(pool);
        let session = setup_session(1).await;

        let resp = admin_list_llm_calls(
            State(state),
            session,
            Query(AdminLlmCallsQuery {
                status: Some("failed".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(1),
                started_from: None,
                started_to: None,
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("admin llm call list should pass")
        .0;

        assert_eq!(resp.total, 1);
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].id, "call-failed");
        assert_eq!(resp.items[0].status, "failed");
    }

    #[tokio::test]
    async fn admin_list_llm_calls_accepts_zulu_started_filters() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = 1"#)
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            "call-zulu",
            "succeeded",
            "api.translate_releases_batch",
            Some(1),
        )
        .await;

        let state = setup_state(pool);
        let session = setup_session(1).await;
        let started_from = (chrono::Utc::now() - chrono::Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let started_to = (chrono::Utc::now() + chrono::Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let resp = admin_list_llm_calls(
            State(state),
            session,
            Query(AdminLlmCallsQuery {
                status: Some("all".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(1),
                started_from: Some(started_from),
                started_to: Some(started_to),
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("admin llm call list should accept zulu time filters")
        .0;

        assert_eq!(resp.total, 1);
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].id, "call-zulu");
    }

    #[tokio::test]
    async fn admin_get_llm_call_detail_returns_not_found() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = 1"#)
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        let state = setup_state(pool);
        let session = setup_session(1).await;

        let err = admin_get_llm_call_detail(State(state), session, Path("missing".to_owned()))
            .await
            .expect_err("missing llm call should fail");
        assert_eq!(err.code(), "not_found");
    }

    #[tokio::test]
    async fn admin_get_llm_call_detail_includes_tokens_and_messages() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = 1"#)
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            "call-detail",
            "succeeded",
            "api.translate_releases_batch",
            Some(1),
        )
        .await;

        let state = setup_state(pool);
        let session = setup_session(1).await;
        let resp = admin_get_llm_call_detail(State(state), session, Path("call-detail".to_owned()))
            .await
            .expect("llm call detail should pass")
            .0;

        assert_eq!(resp.id, "call-detail");
        assert_eq!(resp.first_token_wait_ms, Some(340));
        assert_eq!(resp.input_tokens, Some(120));
        assert_eq!(resp.output_tokens, Some(55));
        assert_eq!(resp.cached_input_tokens, Some(20));
        assert_eq!(resp.total_tokens, Some(175));
        assert!(
            resp.input_messages_json
                .as_deref()
                .is_some_and(|value| value.contains("\"role\":\"user\""))
        );
        assert!(
            resp.output_messages_json
                .as_deref()
                .is_some_and(|value| value.contains("\"assistant\""))
        );
    }

    #[tokio::test]
    async fn admin_get_llm_scheduler_status_reads_aggregates() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = 1"#)
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            "call-1",
            "failed",
            "api.translate_releases_batch",
            Some(1),
        )
        .await;
        seed_llm_call(
            &pool,
            "call-2",
            "succeeded",
            "api.translate_releases_batch",
            Some(1),
        )
        .await;

        let state = setup_state(pool);
        let session = setup_session(1).await;
        let resp = admin_get_llm_scheduler_status(State(state), session)
            .await
            .expect("status should succeed")
            .0;

        assert_eq!(resp.calls_24h, 2);
        assert_eq!(resp.failed_24h, 1);
        assert!(resp.avg_wait_ms_24h.is_some());
    }

    #[tokio::test]
    async fn migration_backfills_earliest_user_as_admin() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite memory db");

        sqlx::raw_sql(
            r#"
            CREATE TABLE users (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              github_user_id INTEGER NOT NULL UNIQUE,
              login TEXT NOT NULL,
              name TEXT,
              avatar_url TEXT,
              email TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("create legacy users table");

        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, created_at, updated_at)
            VALUES (2, 200, 'later', '2026-02-25T09:00:00Z', '2026-02-25T09:00:00Z'),
                   (1, 100, 'earlier', '2026-02-25T08:00:00Z', '2026-02-25T08:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("insert legacy users");

        sqlx::raw_sql(include_str!(
            "../migrations/0006_user_admin_and_disable.sql"
        ))
        .execute(&pool)
        .await
        .expect("apply admin migration");

        #[derive(Debug, sqlx::FromRow)]
        struct UserAdminState {
            id: i64,
            is_admin: i64,
            is_disabled: i64,
        }

        let rows = sqlx::query_as::<_, UserAdminState>(
            r#"
            SELECT id, is_admin, is_disabled
            FROM users
            ORDER BY id ASC
            "#,
        )
        .fetch_all(&pool)
        .await
        .expect("query migrated users");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, 1);
        assert_eq!(rows[0].is_admin, 1);
        assert_eq!(rows[0].is_disabled, 0);
        assert_eq!(rows[1].id, 2);
        assert_eq!(rows[1].is_admin, 0);
    }

    #[test]
    fn parse_release_id_param_requires_integer_string() {
        assert_eq!(parse_release_id_param("123").expect("release id"), 123);
        assert!(parse_release_id_param("12a").is_err());
        assert!(parse_release_id_param("   ").is_err());
    }

    #[test]
    fn parse_unique_release_ids_rejects_oversized_requests() {
        let raw_ids = (1..=61).map(|id| id.to_string()).collect::<Vec<_>>();
        let err =
            parse_unique_release_ids(&raw_ids, 60).expect_err("oversized release batch rejected");
        assert_eq!(err.code(), "bad_request");
    }

    #[test]
    fn parse_unique_thread_ids_rejects_oversized_requests() {
        let raw_ids = (1..=61)
            .map(|id| format!("thread-{id}"))
            .collect::<Vec<_>>();
        let err =
            parse_unique_thread_ids(&raw_ids, 60).expect_err("oversized thread batch rejected");
        assert_eq!(err.code(), "bad_request");
    }

    #[test]
    fn api_non_retryable_error_keeps_rate_limit_retryable_for_fallback() {
        assert!(!ai_error_is_non_retryable(&anyhow::anyhow!(
            "ai returned 429: upstream rate limit"
        )));
    }

    #[test]
    fn translate_response_from_batch_item_keeps_ready_status() {
        let item = TranslateBatchItem {
            id: "1".to_owned(),
            lang: "zh-CN".to_owned(),
            status: "ready".to_owned(),
            title: Some("标题".to_owned()),
            summary: Some("摘要".to_owned()),
            error: None,
        };
        let response =
            translate_response_from_batch_item(item).expect("ready batch item should succeed");
        let json = serde_json::to_value(response).expect("serialize response");
        assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("ready"));
        assert_eq!(json.get("title").and_then(|v| v.as_str()), Some("标题"));
    }

    #[test]
    fn translate_response_from_batch_item_maps_error_to_internal() {
        let item = TranslateBatchItem {
            id: "1".to_owned(),
            lang: "zh-CN".to_owned(),
            status: "error".to_owned(),
            title: None,
            summary: None,
            error: Some("translation failed".to_owned()),
        };
        let err = translate_response_from_batch_item(item).expect_err("error item should fail");
        assert_eq!(err.code(), "internal_error");
    }

    #[test]
    fn translate_response_from_batch_item_maps_missing_to_not_found() {
        let item = TranslateBatchItem {
            id: "1".to_owned(),
            lang: "zh-CN".to_owned(),
            status: "missing".to_owned(),
            title: None,
            summary: None,
            error: Some("release not found".to_owned()),
        };
        let err = translate_response_from_batch_item(item).expect_err("missing item should fail");
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn release_cache_entry_reusable_accepts_title_only_cache() {
        let cache = TranslationCacheRow {
            entity_id: "1".to_owned(),
            source_hash: "hash".to_owned(),
            title: Some("标题".to_owned()),
            summary: None,
        };
        assert!(release_cache_entry_reusable(&cache, "body excerpt"));
    }

    #[test]
    fn release_cache_entry_reusable_rejects_json_blob_summary() {
        let cache = TranslationCacheRow {
            entity_id: "1".to_owned(),
            source_hash: "hash".to_owned(),
            title: Some("标题".to_owned()),
            summary: Some("{\"title_zh\":\"标题\"}".to_owned()),
        };
        assert!(looks_like_json_blob(cache.summary.as_deref().unwrap_or("")));
        assert!(!release_cache_entry_reusable(&cache, "body excerpt"));
    }

    #[test]
    fn release_detail_translation_ready_requires_summary_for_non_empty_body() {
        let body = "- item";
        assert!(!release_detail_translation_ready(Some(body), None));
        assert!(release_detail_translation_ready(Some(body), Some("- 条目")));
    }

    #[test]
    fn release_detail_translation_ready_allows_empty_body_without_summary() {
        assert!(release_detail_translation_ready(Some(""), None));
        assert!(release_detail_translation_ready(Some("   \n"), None));
        assert!(release_detail_translation_ready(None, None));
    }

    #[test]
    fn release_detail_source_hash_changes_with_content() {
        let hash1 = release_detail_source_hash("acme/repo", "v1.0.0", "line one");
        let hash2 = release_detail_source_hash("acme/repo", "v1.0.0", "line two");
        assert_ne!(hash1, hash2);
    }

    #[tokio::test]
    async fn release_detail_query_is_readable_without_star() {
        let pool = setup_pool().await;
        seed_release(&pool, 42, 120).await;

        #[derive(Debug, sqlx::FromRow)]
        struct Row {
            release_id: i64,
            repo_full_name: Option<String>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT r.release_id, sr.full_name AS repo_full_name
            FROM releases r
            LEFT JOIN starred_repos sr
              ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
            WHERE r.user_id = ? AND r.release_id = ?
            LIMIT 1
            "#,
        )
        .bind(1_i64)
        .bind(120_i64)
        .fetch_optional(&pool)
        .await
        .expect("query detail");

        let row = row.expect("detail row");
        assert_eq!(row.release_id, 120);
        assert!(row.repo_full_name.is_none());
    }

    #[tokio::test]
    async fn list_releases_query_keeps_star_visibility_filter() {
        let pool = setup_pool().await;
        seed_release(&pool, 42, 120).await;

        let count_without_star: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM releases r
            JOIN starred_repos sr
              ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
            WHERE r.user_id = ?
            "#,
        )
        .bind(1_i64)
        .fetch_one(&pool)
        .await
        .expect("count releases without star");
        assert_eq!(count_without_star, 0);

        seed_star(&pool, 42).await;
        let count_with_star: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM releases r
            JOIN starred_repos sr
              ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
            WHERE r.user_id = ?
            "#,
        )
        .bind(1_i64)
        .fetch_one(&pool)
        .await
        .expect("count releases with star");
        assert_eq!(count_with_star, 1);
    }

    #[test]
    fn github_graphql_http_error_marks_rate_limit_403() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        let err = github_graphql_http_error(reqwest::StatusCode::FORBIDDEN, &headers, "")
            .expect("expected mapped error");
        assert_eq!(err.code(), "rate_limited");
    }

    #[test]
    fn github_graphql_http_error_marks_auth_403() {
        let headers = HeaderMap::new();
        let err = github_graphql_http_error(
            reqwest::StatusCode::FORBIDDEN,
            &headers,
            "Requires authentication",
        )
        .expect("expected mapped error");
        assert_eq!(err.code(), "reauth_required");
    }

    #[test]
    fn github_graphql_http_error_marks_org_restriction_403() {
        let headers = HeaderMap::new();
        let err = github_graphql_http_error(
            reqwest::StatusCode::FORBIDDEN,
            &headers,
            "OAuth App access restrictions are enabled",
        )
        .expect("expected mapped error");
        assert_eq!(err.code(), "forbidden");
    }

    #[test]
    fn github_graphql_errors_to_api_error_marks_org_restriction() {
        let errors = vec![GraphQlError {
            message: "OAuth App access restrictions are enabled".to_owned(),
        }];
        let err = github_graphql_errors_to_api_error(&errors).expect("expected mapped error");
        assert_eq!(err.code(), "forbidden");
    }

    #[test]
    fn release_reactions_status_requires_per_item_live_data() {
        let row = test_feed_row(Some("R_node"));
        assert_eq!(release_reactions_status(&row), "ready");
    }

    #[test]
    fn release_reactions_status_ready_with_live_data() {
        let row = test_feed_row(Some("R_node"));
        assert_eq!(release_reactions_status(&row), "ready");
    }

    #[test]
    fn release_reactions_status_sync_required_without_node_id() {
        let row = test_feed_row(None);
        assert_eq!(release_reactions_status(&row), "sync_required");
    }

    #[test]
    fn release_excerpt_keeps_markdown_structure() {
        let body = r#"
# Changelog

- Added **markdown** rendering
- Keep `inline code` markers
1. Ordered item

```bash
echo should_not_be_in_excerpt
```
"#;

        let excerpt = release_excerpt(Some(body)).expect("excerpt");
        assert!(excerpt.contains("# Changelog"));
        assert!(excerpt.contains("- Added **markdown** rendering"));
        assert!(excerpt.contains("- Keep `inline code` markers"));
        assert!(excerpt.contains("1. Ordered item"));
        assert!(!excerpt.contains("should_not_be_in_excerpt"));
    }

    #[test]
    fn release_excerpt_fallback_keeps_newlines() {
        let body = "First line\nSecond line\n\nThird line";
        let excerpt = release_excerpt(Some(body)).expect("excerpt");
        assert!(excerpt.contains("First line\nSecond line"));
        assert!(excerpt.contains("\n\nThird line"));
    }

    #[test]
    fn parse_translation_json_accepts_fenced_json() {
        let raw = r#"```json
{"title_zh":"标题","summary_md":"- **加粗**\\n- `code`"}
```"#;
        let parsed = parse_translation_json(raw).expect("parse translation json");
        assert_eq!(parsed.title_zh.as_deref(), Some("标题"));
        assert_eq!(parsed.summary_md.as_deref(), Some("- **加粗**\n- `code`"));
    }

    #[test]
    fn parse_batch_release_translation_payload_accepts_relaxed_fields() {
        let raw = r#"```json
{"items":[{"id":"123","title":"标题","summary":"- 第一行\\n- 第二行"}]}
```"#;
        let parsed =
            parse_batch_release_translation_payload(raw).expect("parse release batch payload");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].release_id, 123);
        assert_eq!(parsed.items[0].title_zh.as_deref(), Some("标题"));
        assert_eq!(
            parsed.items[0].summary_md.as_deref(),
            Some("- 第一行\n- 第二行")
        );
    }

    #[test]
    fn normalize_translation_fields_extracts_embedded_json_blob() {
        let (title, summary) = normalize_translation_fields(
            None,
            Some(r#"{"title_zh":"发布 1.2.3","summary_md":"- 第一行\\n- 第二行"}"#.to_owned()),
        );
        assert_eq!(title.as_deref(), Some("发布 1.2.3"));
        assert_eq!(summary.as_deref(), Some("- 第一行\n- 第二行"));
    }

    #[test]
    fn normalize_translation_fields_discards_malformed_json_blob() {
        let (title, summary) = normalize_translation_fields(
            Some("原始标题".to_owned()),
            Some(r#"{"title_zh":"发布 1.2.3","summary_md":""#.to_owned()),
        );
        assert_eq!(title.as_deref(), Some("原始标题"));
        assert_eq!(summary, None);
    }

    #[test]
    fn extract_translation_fields_rejects_malformed_json_blob() {
        let (title, summary) =
            extract_translation_fields(r#"{"title_zh":"发布 1.2.3","summary_md":""#);
        assert_eq!(title, None);
        assert_eq!(summary, None);
    }

    #[test]
    fn looks_like_json_blob_accepts_fenced_json() {
        let raw = "```json\n{\"title_zh\":\"标题\",\"summary_md\":\"- 一行\"}\n```";
        assert!(looks_like_json_blob(raw));
    }

    #[test]
    fn parse_batch_notification_translation_payload_accepts_array_root() {
        let raw = r#"[{"id":"thread-1","title_cn":"标题","body":"- 第一行\\n- 第二行"}]"#;
        let parsed = parse_batch_notification_translation_payload(raw)
            .expect("parse notification batch payload");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].thread_id, "thread-1");
        assert_eq!(parsed.items[0].title_zh.as_deref(), Some("标题"));
        assert_eq!(
            parsed.items[0].summary_md.as_deref(),
            Some("- 第一行\n- 第二行")
        );
    }

    #[test]
    fn parse_batch_release_detail_payload_accepts_string_index() {
        let raw = r#"{"translations":[{"index":"2","summary":"- a\\n- b"}]}"#;
        let parsed = parse_batch_release_detail_translation_payload(raw)
            .expect("parse release detail batch payload");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].chunk_index, 2);
        assert_eq!(parsed.items[0].summary_md, "- a\n- b");
    }

    #[test]
    fn markdown_structure_requires_inline_markers() {
        let source = "- **Nightly** build from `main`\n- Keep **bold** marker";
        let translated_missing = "- 夜间构建来自 main\n- 请保留强调";
        let translated_ok = "- **夜间**构建来自 `main`\n- 请保留 **强调** 标记";
        assert!(!markdown_structure_preserved(source, translated_missing));
        assert!(markdown_structure_preserved(source, translated_ok));
    }

    #[test]
    fn split_markdown_chunks_preserves_order() {
        let md = "line1\nline2\nline3\nline4";
        let chunks = split_markdown_chunks(md, 12);
        assert!(chunks.len() >= 2);
        let rebuilt = chunks.join("");
        assert_eq!(rebuilt, md);
    }

    #[test]
    fn split_markdown_chunks_splits_overlong_single_line() {
        let md = "a".repeat(25);
        let chunks = split_markdown_chunks(&md, 8);
        assert!(chunks.len() >= 4);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 8));
        assert_eq!(chunks.join(""), md);
    }

    #[test]
    fn parse_repo_full_name_from_release_url_extracts_owner_repo() {
        let full_name = parse_repo_full_name_from_release_url(
            "https://github.com/acme/rocket/releases/tag/v1.8.0",
        );
        assert_eq!(full_name.as_deref(), Some("acme/rocket"));
    }

    #[test]
    fn resolve_release_full_name_falls_back_when_url_invalid() {
        let full_name = resolve_release_full_name("https://example.com/not-github", 42);
        assert_eq!(full_name, "unknown/42");
    }

    #[test]
    fn preserve_chunk_trailing_newline_keeps_chunk_boundaries() {
        assert_eq!(
            preserve_chunk_trailing_newline("line\n", "译文".to_owned()),
            "译文\n"
        );
        assert_eq!(
            preserve_chunk_trailing_newline("line", "译文".to_owned()),
            "译文"
        );
        assert_eq!(
            preserve_chunk_trailing_newline("line\n", "译文\n".to_owned()),
            "译文\n"
        );
    }
}
