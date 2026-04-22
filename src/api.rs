use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, extract::State};
use chrono::{Datelike, TimeZone};
use chrono_tz::Tz;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use tokio::{io::AsyncReadExt, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tower_sessions::Session;
use url::Url;

use crate::{admin_runtime, ai, briefs, jobs, local_id, sync};
use crate::{
    error::ApiError,
    passkeys::{
        PasskeySummary, PendingPasskeyCredentialSession, load_passkey_summaries,
        pending_passkey_bind_is_expired,
    },
    state::AppState,
};

const SESSION_PENDING_ACCESS_SYNC_REASON: &str = "pending_access_sync_reason";
const SESSION_ACTIVITY_TOUCHED_AT: &str = "activity_touched_at";
const SESSION_ACTIVITY_TOUCH_INTERVAL_SECS: i64 = 15 * 60;
const ADMIN_DASHBOARD_TASK_TYPES: [(&str, &str); 3] = [
    (jobs::TASK_TRANSLATE_RELEASE_BATCH, "翻译"),
    (jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH, "润色"),
    (jobs::TASK_BRIEF_DAILY_SLOT, "日报"),
];

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

fn parse_internal_brief_release_id(target: &str) -> Option<i64> {
    let base = Url::parse("https://octorill.local/").expect("valid local base url");
    let joined = base.join(target.trim()).ok()?;

    if joined.host_str() != Some("octorill.local") {
        return None;
    }

    let tab = joined
        .query_pairs()
        .find_map(|(k, v)| (k == "tab").then_some(v.into_owned()))?;
    if tab != "briefs" {
        return None;
    }

    let raw_release = joined
        .query_pairs()
        .find_map(|(k, v)| (k == "release").then_some(v.into_owned()))?;
    if !raw_release.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    raw_release.parse::<i64>().ok()
}

fn extract_brief_release_ids(markdown: &str) -> Vec<i64> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut i = 0usize;

    while i < markdown.len() {
        let rest = &markdown[i..];

        if rest.starts_with('[')
            && let Some(text_end_rel) = rest.find("](")
            && let Some(url_end_rel) = rest[text_end_rel + 2..].find(')')
        {
            let url_start = i + text_end_rel + 2;
            let url_end = url_start + url_end_rel;
            let target = &markdown[url_start..url_end];
            if let Some(release_id) = parse_internal_brief_release_id(target)
                && seen.insert(release_id)
            {
                ids.push(release_id);
            }
            i = url_end + 1;
            continue;
        }

        let mut chars = rest.chars();
        let ch = chars.next().expect("rest is non-empty");
        i += ch.len_utf8();
    }

    ids
}

fn brief_contains_release_link(markdown: &str, release_id: i64) -> bool {
    extract_brief_release_ids(markdown)
        .into_iter()
        .any(|candidate| candidate == release_id)
}

fn brief_uses_markdown_release_fallback(generation_source: &str) -> bool {
    matches!(
        generation_source,
        "legacy" | "history_recompute_failed" | "content_refresh_failed"
    )
}

async fn user_has_brief_access_to_release(
    state: &AppState,
    user_id: &str,
    release_id: i64,
) -> Result<bool, ApiError> {
    let membership_exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM brief_release_memberships m
        JOIN briefs b ON b.id = m.brief_id
        WHERE b.user_id = ?
          AND m.release_id = ?
        "#,
    )
    .bind(user_id)
    .bind(release_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    if membership_exists > 0 {
        return Ok(true);
    }

    let hint = format!("%release={release_id}%");
    let briefs = sqlx::query_scalar::<_, String>(
        r#"
        SELECT content_markdown
        FROM briefs
        WHERE user_id = ?
          AND generation_source IN ('legacy', 'history_recompute_failed', 'content_refresh_failed')
          AND content_markdown LIKE ?
        "#,
    )
    .bind(user_id)
    .bind(hint)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(briefs
        .iter()
        .any(|markdown| brief_contains_release_link(markdown, release_id)))
}

pub(crate) fn parse_local_id_param(raw: String, field: &str) -> Result<String, ApiError> {
    local_id::normalize_local_id(&raw)
        .ok_or_else(|| ApiError::bad_request(format!("invalid {field}")))
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    user: UserSummary,
    access_sync: AccessSyncBootstrap,
    dashboard: DashboardBootstrap,
}

#[derive(Debug, Serialize)]
pub struct UserSummary {
    id: String,
    github_user_id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
    is_admin: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct MeUserRow {
    id: String,
    is_admin: i64,
    is_disabled: i64,
    last_active_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct MeFirstGitHubRow {
    github_user_id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
    email: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct MeLinuxDoAvatarRow {
    avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct AccessSyncBootstrap {
    task_id: Option<String>,
    task_type: Option<String>,
    event_path: Option<String>,
    reason: String,
}

#[derive(Debug, Serialize)]
struct DashboardBootstrap {
    daily_boundary_local: String,
    daily_boundary_time_zone: Option<String>,
    daily_boundary_utc_offset_minutes: i32,
}

impl AccessSyncBootstrap {
    fn none() -> Self {
        Self {
            task_id: None,
            task_type: None,
            event_path: None,
            reason: "none".to_owned(),
        }
    }

    fn from_task(task: jobs::EnqueuedTask, reason: impl Into<String>) -> Self {
        let task_id = task.task_id;
        Self {
            event_path: Some(format!("/api/tasks/{task_id}/events")),
            task_id: Some(task_id),
            task_type: Some(task.task_type),
            reason: reason.into(),
        }
    }
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MeResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let row = sqlx::query_as::<_, MeUserRow>(
        r#"
        SELECT id, is_admin, is_disabled, last_active_at
        FROM users
        WHERE id = ?
        "#,
    )
    .bind(&user_id)
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

    let first_github = sqlx::query_as::<_, MeFirstGitHubRow>(
        r#"
        SELECT github_user_id, login, name, avatar_url, email
        FROM github_connections
        WHERE user_id = ?
        ORDER BY linked_at ASC, id ASC
        LIMIT 1
        "#,
    )
    .bind(&row.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some(first_github) = first_github else {
        session.clear().await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "session user has no github connection",
        ));
    };

    let linuxdo_avatar = sqlx::query_as::<_, MeLinuxDoAvatarRow>(
        r#"
        SELECT avatar_url
        FROM linuxdo_connections
        WHERE user_id = ?
        LIMIT 1
        "#,
    )
    .bind(&row.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
    .and_then(|record| record.avatar_url);

    let access_sync = maybe_bootstrap_access_sync(state.as_ref(), &session, &row).await?;
    touch_user_last_active_at(state.as_ref(), &row.id).await?;
    let preferences = briefs::load_daily_brief_preferences(state.as_ref(), &row.id)
        .await
        .map_err(ApiError::internal)?;
    let daily_boundary_local = briefs::format_daily_brief_local_time(preferences.local_time);
    let daily_boundary_time_zone = Some(preferences.time_zone.clone());
    let daily_boundary_utc_offset_minutes =
        briefs::current_utc_offset_minutes(&preferences, chrono::Utc::now())
            .map_err(ApiError::internal)?;

    Ok(Json(MeResponse {
        user: UserSummary {
            id: row.id,
            github_user_id: first_github.github_user_id,
            login: first_github.login,
            name: first_github.name,
            avatar_url: linuxdo_avatar.or(first_github.avatar_url),
            email: first_github.email,
            is_admin: row.is_admin != 0,
        },
        access_sync,
        dashboard: DashboardBootstrap {
            daily_boundary_local,
            daily_boundary_time_zone,
            daily_boundary_utc_offset_minutes,
        },
    }))
}

fn last_active_is_stale(last_active_at: Option<&str>) -> bool {
    let Some(last_active_at) = last_active_at else {
        return true;
    };
    let Ok(last_active_at) = chrono::DateTime::parse_from_rfc3339(last_active_at) else {
        return true;
    };
    chrono::Utc::now().signed_duration_since(last_active_at.with_timezone(&chrono::Utc))
        >= chrono::Duration::hours(1)
}

async fn maybe_bootstrap_access_sync(
    state: &AppState,
    session: &Session,
    row: &MeUserRow,
) -> Result<AccessSyncBootstrap, ApiError> {
    if let Some(task) = jobs::find_inflight_task_for_requester(
        state,
        jobs::TASK_SYNC_ACCESS_REFRESH,
        row.id.as_str(),
    )
    .await
    .map_err(ApiError::internal)?
    {
        clear_pending_access_sync_reason(session).await?;
        return Ok(AccessSyncBootstrap::from_task(task, "reused_inflight"));
    }

    let pending_reason = load_pending_access_sync_reason(session).await?;
    let reason = if let Some(reason) = pending_reason.as_deref() {
        reason
    } else if row.last_active_at.is_some() {
        "inactive_over_1h"
    } else {
        "first_visit"
    };

    if pending_reason.is_none() && !last_active_is_stale(row.last_active_at.as_deref()) {
        return Ok(AccessSyncBootstrap::none());
    }

    let task = jobs::enqueue_singleton_task_for_requester(
        state,
        jobs::NewTask {
            task_type: jobs::TASK_SYNC_ACCESS_REFRESH.to_owned(),
            payload: json!({ "user_id": row.id.clone() }),
            source: "api.me".to_owned(),
            requested_by: Some(row.id.clone()),
            parent_task_id: None,
        },
    )
    .await
    .map_err(ApiError::internal)?;
    clear_pending_access_sync_reason(session).await?;
    Ok(AccessSyncBootstrap::from_task(task, reason))
}

async fn touch_user_last_active_at(state: &AppState, user_id: &str) -> Result<(), ApiError> {
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
    Ok(())
}

async fn load_pending_access_sync_reason(session: &Session) -> Result<Option<String>, ApiError> {
    session
        .get::<String>(SESSION_PENDING_ACCESS_SYNC_REASON)
        .await
        .map_err(ApiError::internal)
}

async fn mark_pending_access_sync_reason(session: &Session, reason: &str) -> Result<(), ApiError> {
    session
        .insert(SESSION_PENDING_ACCESS_SYNC_REASON, reason)
        .await
        .map_err(ApiError::internal)
}

async fn clear_pending_access_sync_reason(session: &Session) -> Result<(), ApiError> {
    session
        .remove::<String>(SESSION_PENDING_ACCESS_SYNC_REASON)
        .await
        .map(|_| ())
        .map_err(ApiError::internal)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminUserItem {
    id: String,
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

#[derive(Debug, Clone)]
struct AdminUserUpdateGuard {
    acting_user_id: String,
    target_user_id: String,
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
    Path(target_user_id): Path<String>,
    Json(req): Json<AdminUserPatchRequest>,
) -> Result<Json<AdminUserItem>, ApiError> {
    let acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let target_user_id = parse_local_id_param(target_user_id, "user_id")?;

    if req.is_admin.is_none() && req.is_disabled.is_none() {
        return Err(ApiError::bad_request(
            "at least one field (is_admin/is_disabled) is required",
        ));
    }

    #[derive(Debug, sqlx::FromRow)]
    struct AdminPatchTargetRow {
        id: String,
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
    .bind(&target_user_id)
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
    .bind(&target_user_id)
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
    .bind(&target_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(updated))
}

#[derive(Debug, Serialize)]
pub struct DailyBriefProfileResponse {
    user_id: String,
    daily_brief_local_time: String,
    daily_brief_time_zone: String,
    include_own_releases: bool,
    last_active_at: Option<String>,
}

pub type AdminUserProfileResponse = DailyBriefProfileResponse;
pub type MeProfileResponse = DailyBriefProfileResponse;

#[derive(Debug, Deserialize)]
pub struct DailyBriefProfilePatchRequest {
    daily_brief_local_time: String,
    daily_brief_time_zone: String,
    #[serde(default)]
    include_own_releases: Option<bool>,
}

#[derive(Debug, sqlx::FromRow)]
struct DailyBriefProfileRow {
    daily_brief_local_time: Option<String>,
    daily_brief_time_zone: Option<String>,
    include_own_releases: i64,
    daily_brief_utc_time: String,
    last_active_at: Option<String>,
}

async fn load_daily_brief_profile(
    state: &AppState,
    user_id: &str,
) -> Result<DailyBriefProfileResponse, ApiError> {
    let row = sqlx::query_as::<_, DailyBriefProfileRow>(
        r#"
        SELECT
          daily_brief_local_time,
          daily_brief_time_zone,
          include_own_releases,
          daily_brief_utc_time,
          last_active_at
        FROM users
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "user not found",
        ));
    };

    let preferences = briefs::derive_daily_brief_preferences(
        &state.config,
        row.daily_brief_local_time.as_deref(),
        row.daily_brief_time_zone.as_deref(),
        Some(row.daily_brief_utc_time.as_str()),
        chrono::Utc::now(),
    );

    Ok(DailyBriefProfileResponse {
        user_id: user_id.to_owned(),
        daily_brief_local_time: briefs::format_daily_brief_local_time(preferences.local_time),
        daily_brief_time_zone: preferences.time_zone,
        include_own_releases: row.include_own_releases != 0,
        last_active_at: row.last_active_at,
    })
}

async fn persist_daily_brief_profile(
    state: &AppState,
    user_id: &str,
    req: DailyBriefProfilePatchRequest,
) -> Result<DailyBriefProfileResponse, ApiError> {
    let local_time = briefs::parse_daily_brief_local_time(&req.daily_brief_local_time)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let time_zone = briefs::parse_daily_brief_time_zone(&req.daily_brief_time_zone)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    briefs::validate_hour_aligned_time_zone(&time_zone, chrono::Utc::now())
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let enabled_hours = briefs::load_enabled_daily_brief_scheduler_hours(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let missing_hours =
        briefs::missing_daily_brief_scheduler_hours(local_time, &time_zone, &enabled_hours)
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
    if !missing_hours.is_empty() {
        let missing_hours = missing_hours
            .into_iter()
            .map(|hour| format!("{hour:02}:00Z"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ApiError::bad_request(format!(
            "invalid daily brief schedule for current scheduler configuration (missing enabled UTC slots: {missing_hours})"
        )));
    }

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE users
        SET daily_brief_local_time = ?,
            daily_brief_time_zone = ?,
            include_own_releases = COALESCE(?, include_own_releases),
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(briefs::format_daily_brief_local_time(local_time))
    .bind(time_zone)
    .bind(
        req.include_own_releases
            .map(|value| if value { 1_i64 } else { 0_i64 }),
    )
    .bind(now.as_str())
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    load_daily_brief_profile(state, user_id).await
}

pub async fn me_get_profile(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MeProfileResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    Ok(Json(
        load_daily_brief_profile(state.as_ref(), &user_id).await?,
    ))
}

pub async fn me_patch_profile(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<DailyBriefProfilePatchRequest>,
) -> Result<Json<MeProfileResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    Ok(Json(
        persist_daily_brief_profile(state.as_ref(), &user_id, req).await?,
    ))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LinuxDoConnectionResponse {
    linuxdo_user_id: i64,
    username: String,
    name: Option<String>,
    avatar_url: Option<String>,
    trust_level: i64,
    active: bool,
    silenced: bool,
    linked_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct MeLinuxDoResponse {
    available: bool,
    connection: Option<LinuxDoConnectionResponse>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct GitHubConnectionResponse {
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

#[derive(Debug, Serialize)]
pub struct MeGitHubConnectionsResponse {
    items: Vec<GitHubConnectionResponse>,
}

#[derive(Debug, Serialize)]
pub struct AuthBindContextLinuxDoResponse {
    linuxdo_user_id: i64,
    username: String,
    name: Option<String>,
    avatar_url: String,
    trust_level: i64,
    active: bool,
    silenced: bool,
}

#[derive(Debug, Serialize)]
pub struct AuthBindContextResponse {
    linuxdo_available: bool,
    pending_linuxdo: Option<AuthBindContextLinuxDoResponse>,
    pending_passkey: Option<AuthBindContextPasskeyResponse>,
}

#[derive(Debug, Serialize)]
pub struct AuthBindContextPasskeyResponse {
    label: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct MePasskeysResponse {
    items: Vec<PasskeySummary>,
}

pub async fn me_get_linuxdo(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MeLinuxDoResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let connection = sqlx::query_as::<_, LinuxDoConnectionResponse>(
        r#"
        SELECT
          linuxdo_user_id,
          username,
          name,
          avatar_url,
          trust_level,
          active,
          silenced,
          linked_at,
          updated_at
        FROM linuxdo_connections
        WHERE user_id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(MeLinuxDoResponse {
        available: state.config.linuxdo.is_some(),
        connection,
    }))
}

pub async fn me_delete_linuxdo(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MeLinuxDoResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    sqlx::query(
        r#"
        DELETE FROM linuxdo_connections
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(MeLinuxDoResponse {
        available: state.config.linuxdo.is_some(),
        connection: None,
    }))
}

pub async fn auth_get_bind_context(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<AuthBindContextResponse>, ApiError> {
    #[derive(Debug, Deserialize)]
    struct PendingLinuxDoSession {
        linuxdo_user_id: i64,
        username: String,
        name: Option<String>,
        avatar_url: String,
        trust_level: i64,
        active: bool,
        silenced: bool,
    }

    let pending_linuxdo = session
        .get::<PendingLinuxDoSession>("pending_linuxdo")
        .await
        .map_err(ApiError::internal)?
        .map(|pending| AuthBindContextLinuxDoResponse {
            linuxdo_user_id: pending.linuxdo_user_id,
            username: pending.username,
            name: pending.name,
            avatar_url: pending.avatar_url,
            trust_level: pending.trust_level,
            active: pending.active,
            silenced: pending.silenced,
        });
    let pending_passkey = session
        .get::<PendingPasskeyCredentialSession>("pending_passkey_credential")
        .await
        .map_err(ApiError::internal)?;
    let pending_passkey = if let Some(pending_passkey) = pending_passkey {
        if pending_passkey_bind_is_expired(&pending_passkey) {
            let _ = session
                .remove::<PendingPasskeyCredentialSession>("pending_passkey_credential")
                .await;
            None
        } else {
            Some(AuthBindContextPasskeyResponse {
                label: pending_passkey.label,
                created_at: pending_passkey.created_at,
            })
        }
    } else {
        None
    };

    Ok(Json(AuthBindContextResponse {
        linuxdo_available: state.config.linuxdo.is_some(),
        pending_linuxdo,
        pending_passkey,
    }))
}

pub async fn me_get_github_connections(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MeGitHubConnectionsResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let items = sqlx::query_as::<_, GitHubConnectionResponse>(
        r#"
        SELECT
          id,
          github_user_id,
          login,
          name,
          avatar_url,
          email,
          scopes,
          linked_at,
          updated_at
        FROM github_connections
        WHERE user_id = ?
        ORDER BY linked_at ASC, id ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(MeGitHubConnectionsResponse { items }))
}

pub async fn me_get_passkeys(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<MePasskeysResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let items = load_passkey_summaries(state.as_ref(), user_id.as_str()).await?;
    Ok(Json(MePasskeysResponse { items }))
}

pub async fn me_delete_github_connection(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(connection_id): Path<String>,
) -> Result<Json<MeGitHubConnectionsResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let connection_id = parse_local_id_param(connection_id, "connection_id")?;

    let exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM github_connections
        WHERE id = ?
          AND user_id = ?
        "#,
    )
    .bind(connection_id.as_str())
    .bind(user_id.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if exists == 0 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "github_connection_not_found",
            "github connection not found",
        ));
    }

    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM github_connections
        WHERE user_id = ?
        "#,
    )
    .bind(user_id.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if total <= 1 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "last_github_connection_guard",
            "at least one github connection is required",
        ));
    }

    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query(
        r#"
        DELETE FROM github_connections
        WHERE id = ?
          AND user_id = ?
        "#,
    )
    .bind(connection_id.as_str())
    .bind(user_id.as_str())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query(
        r#"
        DELETE FROM reaction_pat_tokens
        WHERE user_id = ?
          AND owner_github_connection_id = ?
        "#,
    )
    .bind(user_id.as_str())
    .bind(connection_id.as_str())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;

    me_get_github_connections(State(state), session).await
}

pub async fn me_delete_passkey(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(passkey_id): Path<String>,
) -> Result<Json<MePasskeysResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let passkey_id = parse_local_id_param(passkey_id, "passkey_id")?;

    let exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM user_passkeys
        WHERE id = ?
          AND user_id = ?
        "#,
    )
    .bind(passkey_id.as_str())
    .bind(user_id.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if exists == 0 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "passkey_not_found",
            "passkey not found",
        ));
    }

    sqlx::query(
        r#"
        DELETE FROM user_passkeys
        WHERE id = ?
          AND user_id = ?
        "#,
    )
    .bind(passkey_id.as_str())
    .bind(user_id.as_str())
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    me_get_passkeys(State(state), session).await
}

pub async fn admin_get_user_profile(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(user_id): Path<String>,
) -> Result<Json<AdminUserProfileResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let user_id = parse_local_id_param(user_id, "user_id")?;
    Ok(Json(
        load_daily_brief_profile(state.as_ref(), &user_id).await?,
    ))
}

pub async fn admin_patch_user_profile(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(user_id): Path<String>,
    Json(req): Json<DailyBriefProfilePatchRequest>,
) -> Result<Json<AdminUserProfileResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let user_id = parse_local_id_param(user_id, "user_id")?;
    Ok(Json(
        persist_daily_brief_profile(state.as_ref(), &user_id, req).await?,
    ))
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

#[derive(Debug, Deserialize)]
pub struct AdminDashboardQuery {
    window: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardResponse {
    generated_at: String,
    time_zone: String,
    summary: AdminDashboardSummary,
    today_live: AdminDashboardTodayLive,
    status_breakdown: AdminDashboardStatusBreakdown,
    task_share: Vec<AdminDashboardTaskShareItem>,
    trend_points: Vec<AdminDashboardTrendPoint>,
    window_meta: AdminDashboardWindowMeta,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardSummary {
    total_users: i64,
    active_users_today: i64,
    ongoing_tasks_total: i64,
    queued_tasks: i64,
    running_tasks: i64,
    ongoing_by_task: AdminDashboardOngoingTaskBreakdown,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardOngoingTaskBreakdown {
    translations: i64,
    summaries: i64,
    briefs: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardTodayLive {
    date: String,
    total_users: i64,
    active_users: i64,
    ongoing_tasks_total: i64,
    queued_tasks: i64,
    running_tasks: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardStatusBreakdown {
    queued_total: i64,
    running_total: i64,
    succeeded_total: i64,
    failed_total: i64,
    canceled_total: i64,
    total: i64,
    items: Vec<AdminDashboardTaskStatusItem>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AdminDashboardTaskStatusItem {
    task_type: String,
    label: String,
    queued: i64,
    running: i64,
    succeeded: i64,
    failed: i64,
    canceled: i64,
    total: i64,
    success_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardTaskShareItem {
    task_type: String,
    label: String,
    total: i64,
    share_ratio: f64,
    success_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardTrendPoint {
    date: String,
    label: String,
    total_users: i64,
    active_users: i64,
    translations_total: i64,
    translations_failed: i64,
    summaries_total: i64,
    summaries_failed: i64,
    briefs_total: i64,
    briefs_failed: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminDashboardWindowMeta {
    selected_window: String,
    available_windows: Vec<String>,
    window_start: String,
    window_end: String,
    point_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct AdminDashboardRollupRow {
    rollup_date: String,
    task_type: String,
    total_users: i64,
    active_users: i64,
    queued_count: i64,
    running_count: i64,
    succeeded_count: i64,
    failed_count: i64,
    canceled_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct AdminDashboardStatusCountRow {
    queued_count: i64,
    running_count: i64,
    succeeded_count: i64,
    failed_count: i64,
    canceled_count: i64,
}

const ADMIN_DASHBOARD_DEFAULT_WINDOW_DAYS: i64 = 7;
pub(crate) const ADMIN_DASHBOARD_PREAGGREGATE_DAYS: i64 = 30;
const ADMIN_DASHBOARD_WINDOW_OPTIONS: [(&str, i64); 2] = [("7d", 7), ("30d", 30)];

fn admin_dashboard_system_time_zone(state: &AppState) -> Tz {
    briefs::default_daily_brief_time_zone(&state.config)
        .parse::<Tz>()
        .unwrap_or(chrono_tz::UTC)
}

fn resolve_admin_dashboard_window_days(raw: Option<&str>) -> Result<(String, i64), ApiError> {
    let trimmed = raw.unwrap_or_default().trim();
    if trimmed.is_empty() {
        return Ok(("7d".to_owned(), ADMIN_DASHBOARD_DEFAULT_WINDOW_DAYS));
    }
    ADMIN_DASHBOARD_WINDOW_OPTIONS
        .iter()
        .find_map(|(label, days)| (*label == trimmed).then_some((label.to_string(), *days)))
        .ok_or_else(|| ApiError::bad_request("invalid window"))
}

fn local_day_bounds_utc(
    time_zone: Tz,
    day: chrono::NaiveDate,
) -> Result<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>), ApiError> {
    let start_local = time_zone
        .with_ymd_and_hms(day.year(), day.month(), day.day(), 0, 0, 0)
        .single()
        .or_else(|| {
            time_zone
                .with_ymd_and_hms(day.year(), day.month(), day.day(), 0, 0, 0)
                .earliest()
        })
        .ok_or_else(|| ApiError::internal(anyhow::anyhow!("invalid local day start")))?;
    let next_day = day
        .succ_opt()
        .ok_or_else(|| ApiError::internal(anyhow::anyhow!("invalid next local day")))?;
    let end_local = time_zone
        .with_ymd_and_hms(next_day.year(), next_day.month(), next_day.day(), 0, 0, 0)
        .single()
        .or_else(|| {
            time_zone
                .with_ymd_and_hms(next_day.year(), next_day.month(), next_day.day(), 0, 0, 0)
                .earliest()
        })
        .ok_or_else(|| ApiError::internal(anyhow::anyhow!("invalid local day end")))?;
    Ok((
        start_local.with_timezone(&chrono::Utc),
        end_local.with_timezone(&chrono::Utc),
    ))
}

async fn count_admin_dashboard_total_users_now(pool: &sqlx::SqlitePool) -> Result<i64, ApiError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM users
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn count_admin_dashboard_total_users_at(
    pool: &sqlx::SqlitePool,
    end_at: &str,
) -> Result<i64, ApiError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM users
        WHERE julianday(created_at) < julianday(?)
        "#,
    )
    .bind(end_at)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn count_admin_dashboard_active_users_between(
    pool: &sqlx::SqlitePool,
    start_at: &str,
    end_at: &str,
) -> Result<i64, ApiError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM users
        WHERE last_active_at IS NOT NULL
          AND julianday(last_active_at) >= julianday(?)
          AND julianday(last_active_at) < julianday(?)
        "#,
    )
    .bind(start_at)
    .bind(end_at)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn load_admin_dashboard_task_status_counts(
    pool: &sqlx::SqlitePool,
    task_type: &str,
    start_at: &str,
    end_at: &str,
) -> Result<AdminDashboardStatusCountRow, ApiError> {
    sqlx::query_as::<_, AdminDashboardStatusCountRow>(
        r#"
        SELECT
          COALESCE(SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END), 0) AS queued_count,
          COALESCE(SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END), 0) AS running_count,
          COALESCE(SUM(CASE WHEN status = 'succeeded' THEN 1 ELSE 0 END), 0) AS succeeded_count,
          COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_count,
          COALESCE(SUM(CASE WHEN status = 'canceled' THEN 1 ELSE 0 END), 0) AS canceled_count
        FROM job_tasks
        WHERE task_type = ?
          AND julianday(created_at) >= julianday(?)
          AND julianday(created_at) < julianday(?)
        "#,
    )
    .bind(task_type)
    .bind(start_at)
    .bind(end_at)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn upsert_admin_dashboard_rollup_for_day(
    state: &AppState,
    time_zone: Tz,
    day: chrono::NaiveDate,
) -> Result<(), ApiError> {
    let (start_utc, end_utc) = local_day_bounds_utc(time_zone, day)?;
    let start_at = start_utc.to_rfc3339();
    let end_at = end_utc.to_rfc3339();
    let total_users = count_admin_dashboard_total_users_at(&state.pool, end_at.as_str()).await?;
    let active_users =
        count_admin_dashboard_active_users_between(&state.pool, start_at.as_str(), end_at.as_str())
            .await?;
    let updated_at = chrono::Utc::now().to_rfc3339();
    let day_value = day.format("%Y-%m-%d").to_string();
    let time_zone_value = time_zone.name().to_owned();

    for (task_type, _) in ADMIN_DASHBOARD_TASK_TYPES {
        let counts = load_admin_dashboard_task_status_counts(
            &state.pool,
            task_type,
            start_at.as_str(),
            end_at.as_str(),
        )
        .await?;
        sqlx::query(
            r#"
            INSERT INTO admin_dashboard_daily_rollups (
              rollup_date,
              time_zone,
              task_type,
              total_users,
              active_users,
              queued_count,
              running_count,
              succeeded_count,
              failed_count,
              canceled_count,
              updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(rollup_date, time_zone, task_type)
            DO UPDATE SET
              total_users = excluded.total_users,
              active_users = excluded.active_users,
              queued_count = excluded.queued_count,
              running_count = excluded.running_count,
              succeeded_count = excluded.succeeded_count,
              failed_count = excluded.failed_count,
              canceled_count = excluded.canceled_count,
              updated_at = excluded.updated_at
            "#,
        )
        .bind(day_value.as_str())
        .bind(time_zone_value.as_str())
        .bind(task_type)
        .bind(total_users)
        .bind(active_users)
        .bind(counts.queued_count)
        .bind(counts.running_count)
        .bind(counts.succeeded_count)
        .bind(counts.failed_count)
        .bind(counts.canceled_count)
        .bind(updated_at.as_str())
        .execute(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    }

    Ok(())
}

pub(crate) async fn refresh_admin_dashboard_rollups(
    state: &AppState,
    days: i64,
) -> Result<(), ApiError> {
    let time_zone = admin_dashboard_system_time_zone(state);
    let now_local = chrono::Utc::now().with_timezone(&time_zone);
    let end_day = now_local.date_naive();
    let start_day = end_day
        .checked_sub_signed(chrono::Duration::days(days.saturating_sub(1)))
        .ok_or_else(|| {
            ApiError::internal(anyhow::anyhow!("invalid admin dashboard refresh range"))
        })?;

    let mut day = start_day;
    loop {
        upsert_admin_dashboard_rollup_for_day(state, time_zone, day).await?;
        if day >= end_day {
            break;
        }
        day = day.succ_opt().ok_or_else(|| {
            ApiError::internal(anyhow::anyhow!(
                "invalid admin dashboard refresh day iteration"
            ))
        })?;
    }

    Ok(())
}

fn build_admin_dashboard_task_status_item(
    task_type: &str,
    label: &str,
    counts: &AdminDashboardStatusCountRow,
) -> AdminDashboardTaskStatusItem {
    let total = counts.queued_count
        + counts.running_count
        + counts.succeeded_count
        + counts.failed_count
        + counts.canceled_count;
    let finished_total = counts.succeeded_count + counts.failed_count + counts.canceled_count;
    let success_rate = if finished_total <= 0 {
        0.0
    } else {
        counts.succeeded_count as f64 / finished_total as f64
    };

    AdminDashboardTaskStatusItem {
        task_type: task_type.to_owned(),
        label: label.to_owned(),
        queued: counts.queued_count,
        running: counts.running_count,
        succeeded: counts.succeeded_count,
        failed: counts.failed_count,
        canceled: counts.canceled_count,
        total,
        success_rate,
    }
}

fn build_admin_dashboard_task_share_item(
    total_tasks: i64,
    item: &AdminDashboardTaskStatusItem,
) -> AdminDashboardTaskShareItem {
    let share_ratio = if total_tasks <= 0 {
        0.0
    } else {
        item.total as f64 / total_tasks as f64
    };
    AdminDashboardTaskShareItem {
        task_type: item.task_type.clone(),
        label: item.label.clone(),
        total: item.total,
        share_ratio,
        success_rate: item.success_rate,
    }
}

async fn load_admin_dashboard_rollups(
    pool: &sqlx::SqlitePool,
    time_zone: Tz,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<AdminDashboardRollupRow>, ApiError> {
    sqlx::query_as::<_, AdminDashboardRollupRow>(
        r#"
        SELECT
          rollup_date,
          task_type,
          total_users,
          active_users,
          queued_count,
          running_count,
          succeeded_count,
          failed_count,
          canceled_count
        FROM admin_dashboard_daily_rollups
        WHERE time_zone = ?
          AND rollup_date >= ?
          AND rollup_date <= ?
        ORDER BY rollup_date ASC, task_type ASC
        "#,
    )
    .bind(time_zone.name())
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)
}

async fn load_admin_dashboard_ongoing_counts(
    pool: &sqlx::SqlitePool,
) -> Result<AdminDashboardOngoingTaskBreakdown, ApiError> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT task_type, COUNT(*) AS total
        FROM job_tasks
        WHERE status IN ('queued', 'running')
          AND task_type IN (?, ?, ?)
        GROUP BY task_type
        "#,
    )
    .bind(jobs::TASK_TRANSLATE_RELEASE_BATCH)
    .bind(jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH)
    .bind(jobs::TASK_BRIEF_DAILY_SLOT)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;

    let mut translations = 0_i64;
    let mut summaries = 0_i64;
    let mut briefs = 0_i64;
    for (task_type, total) in rows {
        match task_type.as_str() {
            jobs::TASK_TRANSLATE_RELEASE_BATCH => translations = total,
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH => summaries = total,
            jobs::TASK_BRIEF_DAILY_SLOT => briefs = total,
            _ => {}
        }
    }
    Ok(AdminDashboardOngoingTaskBreakdown {
        translations,
        summaries,
        briefs,
    })
}

async fn count_admin_dashboard_live_tasks_by_status(
    pool: &sqlx::SqlitePool,
    status: &str,
) -> Result<i64, ApiError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM job_tasks
        WHERE status = ?
          AND task_type IN (?, ?, ?)
        "#,
    )
    .bind(status)
    .bind(jobs::TASK_TRANSLATE_RELEASE_BATCH)
    .bind(jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH)
    .bind(jobs::TASK_BRIEF_DAILY_SLOT)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn load_admin_dashboard_today_live_snapshot(
    state: &AppState,
    time_zone: Tz,
    now_utc: chrono::DateTime<chrono::Utc>,
) -> Result<(AdminDashboardTodayLive, AdminDashboardStatusBreakdown), ApiError> {
    let now_local = now_utc.with_timezone(&time_zone);
    let today = now_local.date_naive();
    let (start_utc, _) = local_day_bounds_utc(time_zone, today)?;
    let start_at = start_utc.to_rfc3339();
    let end_at = now_utc.to_rfc3339();

    let total_users = count_admin_dashboard_total_users_now(&state.pool).await?;
    let active_users =
        count_admin_dashboard_active_users_between(&state.pool, start_at.as_str(), end_at.as_str())
            .await?;
    let ongoing_by_task = load_admin_dashboard_ongoing_counts(&state.pool).await?;
    let queued_tasks =
        count_admin_dashboard_live_tasks_by_status(&state.pool, jobs::STATUS_QUEUED).await?;
    let running_tasks =
        count_admin_dashboard_live_tasks_by_status(&state.pool, jobs::STATUS_RUNNING).await?;
    let ongoing_tasks_total =
        ongoing_by_task.translations + ongoing_by_task.summaries + ongoing_by_task.briefs;

    let mut items = Vec::with_capacity(ADMIN_DASHBOARD_TASK_TYPES.len());
    for (task_type, label) in ADMIN_DASHBOARD_TASK_TYPES {
        let counts = load_admin_dashboard_task_status_counts(
            &state.pool,
            task_type,
            start_at.as_str(),
            end_at.as_str(),
        )
        .await?;
        items.push(build_admin_dashboard_task_status_item(
            task_type, label, &counts,
        ));
    }

    let queued_total = items.iter().map(|item| item.queued).sum::<i64>();
    let running_total = items.iter().map(|item| item.running).sum::<i64>();
    let succeeded_total = items.iter().map(|item| item.succeeded).sum::<i64>();
    let failed_total = items.iter().map(|item| item.failed).sum::<i64>();
    let canceled_total = items.iter().map(|item| item.canceled).sum::<i64>();
    let total = items.iter().map(|item| item.total).sum::<i64>();

    Ok((
        AdminDashboardTodayLive {
            date: today.format("%Y-%m-%d").to_string(),
            total_users,
            active_users,
            ongoing_tasks_total,
            queued_tasks,
            running_tasks,
        },
        AdminDashboardStatusBreakdown {
            queued_total,
            running_total,
            succeeded_total,
            failed_total,
            canceled_total,
            total,
            items,
        },
    ))
}

pub async fn admin_dashboard(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<AdminDashboardQuery>,
) -> Result<Json<AdminDashboardResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let now_utc = chrono::Utc::now();
    let (selected_window, window_days) =
        resolve_admin_dashboard_window_days(query.window.as_deref())?;
    let time_zone = admin_dashboard_system_time_zone(state.as_ref());
    let now_local = now_utc.with_timezone(&time_zone);
    let end_day = now_local.date_naive();
    let start_day = end_day
        .checked_sub_signed(chrono::Duration::days(window_days - 1))
        .ok_or_else(|| {
            ApiError::internal(anyhow::anyhow!("invalid admin dashboard rollup range"))
        })?;

    let rollups = load_admin_dashboard_rollups(
        &state.pool,
        time_zone,
        start_day.format("%Y-%m-%d").to_string().as_str(),
        end_day.format("%Y-%m-%d").to_string().as_str(),
    )
    .await?;
    let (today_live, status_breakdown) =
        load_admin_dashboard_today_live_snapshot(state.as_ref(), time_zone, now_utc).await?;
    let ongoing_by_task = load_admin_dashboard_ongoing_counts(&state.pool).await?;

    let today_value = today_live.date.clone();
    let mut trend_points_by_date = HashMap::<String, AdminDashboardTrendPoint>::new();
    let mut day = start_day;
    loop {
        let day_value = day.format("%Y-%m-%d").to_string();
        trend_points_by_date.insert(
            day_value.clone(),
            AdminDashboardTrendPoint {
                label: day_value.get(5..).unwrap_or(day_value.as_str()).to_owned(),
                date: day_value,
                total_users: 0,
                active_users: 0,
                translations_total: 0,
                translations_failed: 0,
                summaries_total: 0,
                summaries_failed: 0,
                briefs_total: 0,
                briefs_failed: 0,
            },
        );
        if day >= end_day {
            break;
        }
        day = day.succ_opt().ok_or_else(|| {
            ApiError::internal(anyhow::anyhow!(
                "invalid admin dashboard trend day iteration"
            ))
        })?;
    }

    for row in rollups {
        let point = trend_points_by_date
            .entry(row.rollup_date.clone())
            .or_insert_with(|| AdminDashboardTrendPoint {
                label: row
                    .rollup_date
                    .get(5..)
                    .unwrap_or(row.rollup_date.as_str())
                    .to_owned(),
                date: row.rollup_date.clone(),
                total_users: row.total_users,
                active_users: row.active_users,
                translations_total: 0,
                translations_failed: 0,
                summaries_total: 0,
                summaries_failed: 0,
                briefs_total: 0,
                briefs_failed: 0,
            });
        point.total_users = row.total_users;
        point.active_users = row.active_users;
        match row.task_type.as_str() {
            jobs::TASK_TRANSLATE_RELEASE_BATCH => {
                point.translations_total = row.queued_count
                    + row.running_count
                    + row.succeeded_count
                    + row.failed_count
                    + row.canceled_count;
                point.translations_failed = row.failed_count;
            }
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH => {
                point.summaries_total = row.queued_count
                    + row.running_count
                    + row.succeeded_count
                    + row.failed_count
                    + row.canceled_count;
                point.summaries_failed = row.failed_count;
            }
            jobs::TASK_BRIEF_DAILY_SLOT => {
                point.briefs_total = row.queued_count
                    + row.running_count
                    + row.succeeded_count
                    + row.failed_count
                    + row.canceled_count;
                point.briefs_failed = row.failed_count;
            }
            _ => {}
        }
    }

    let today_point = trend_points_by_date
        .entry(today_value.clone())
        .or_insert_with(|| AdminDashboardTrendPoint {
            label: today_value
                .get(5..)
                .unwrap_or(today_value.as_str())
                .to_owned(),
            date: today_value.clone(),
            total_users: 0,
            active_users: 0,
            translations_total: 0,
            translations_failed: 0,
            summaries_total: 0,
            summaries_failed: 0,
            briefs_total: 0,
            briefs_failed: 0,
        });
    today_point.total_users = today_live.total_users;
    today_point.active_users = today_live.active_users;
    for item in &status_breakdown.items {
        match item.task_type.as_str() {
            jobs::TASK_TRANSLATE_RELEASE_BATCH => {
                today_point.translations_total = item.total;
                today_point.translations_failed = item.failed;
            }
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH => {
                today_point.summaries_total = item.total;
                today_point.summaries_failed = item.failed;
            }
            jobs::TASK_BRIEF_DAILY_SLOT => {
                today_point.briefs_total = item.total;
                today_point.briefs_failed = item.failed;
            }
            _ => {}
        }
    }

    let mut trend_points = trend_points_by_date.into_values().collect::<Vec<_>>();
    trend_points.sort_by(|left, right| left.date.cmp(&right.date));

    let task_share = status_breakdown
        .items
        .iter()
        .map(|item| build_admin_dashboard_task_share_item(status_breakdown.total, item))
        .collect::<Vec<_>>();

    Ok(Json(AdminDashboardResponse {
        generated_at: now_utc.to_rfc3339(),
        time_zone: time_zone.name().to_owned(),
        summary: AdminDashboardSummary {
            total_users: today_live.total_users,
            active_users_today: today_live.active_users,
            ongoing_tasks_total: today_live.ongoing_tasks_total,
            queued_tasks: today_live.queued_tasks,
            running_tasks: today_live.running_tasks,
            ongoing_by_task,
        },
        today_live,
        status_breakdown,
        task_share,
        trend_points,
        window_meta: AdminDashboardWindowMeta {
            selected_window,
            available_windows: ADMIN_DASHBOARD_WINDOW_OPTIONS
                .iter()
                .map(|(label, _)| (*label).to_owned())
                .collect(),
            window_start: start_day.format("%Y-%m-%d").to_string(),
            window_end: today_value,
            point_count: window_days,
        },
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
    task_group: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminRealtimeTaskItem {
    id: String,
    task_type: String,
    status: String,
    source: String,
    requested_by: Option<String>,
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
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    cancel_requested: bool,
    error_message: Option<String>,
    payload_json: String,
    result_json: Option<String>,
    #[serde(skip_serializing)]
    log_file_path: Option<String>,
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
    let task_group = query.task_group.unwrap_or_else(|| "all".to_owned());
    let scheduled_daily_task = jobs::SCHEDULED_TASK_TYPES[0];
    let scheduled_subscription_task = jobs::SCHEDULED_TASK_TYPES[1];

    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM job_tasks
        WHERE (? = 'all' OR status = ?)
          AND (? = '' OR task_type = ?)
          AND (? = '' OR task_type != ?)
          AND (
            ? = 'all'
            OR (? = 'scheduled' AND task_type IN (?, ?))
            OR (? = 'realtime' AND task_type NOT IN (?, ?))
          )
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(task_type.as_str())
    .bind(task_type.as_str())
    .bind(exclude_task_type.as_str())
    .bind(exclude_task_type.as_str())
    .bind(task_group.as_str())
    .bind(task_group.as_str())
    .bind(scheduled_daily_task)
    .bind(scheduled_subscription_task)
    .bind(task_group.as_str())
    .bind(scheduled_daily_task)
    .bind(scheduled_subscription_task)
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
          AND (
            ? = 'all'
            OR (? = 'scheduled' AND task_type IN (?, ?))
            OR (? = 'realtime' AND task_type NOT IN (?, ?))
          )
        ORDER BY
          unixepoch(created_at) DESC,
          created_at DESC,
          id DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(task_type.as_str())
    .bind(task_type.as_str())
    .bind(exclude_task_type.as_str())
    .bind(exclude_task_type.as_str())
    .bind(task_group.as_str())
    .bind(task_group.as_str())
    .bind(jobs::TASK_BRIEF_DAILY_SLOT)
    .bind(jobs::TASK_SYNC_SUBSCRIPTIONS)
    .bind(task_group.as_str())
    .bind(jobs::TASK_BRIEF_DAILY_SLOT)
    .bind(jobs::TASK_SYNC_SUBSCRIPTIONS)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let items = items
        .into_iter()
        .filter(|item| match task_group.as_str() {
            "scheduled" => jobs::is_scheduled_task_type(&item.task_type),
            "realtime" => !jobs::is_scheduled_task_type(&item.task_type),
            _ => true,
        })
        .collect::<Vec<_>>();

    Ok(Json(AdminRealtimeTasksResponse {
        items,
        page,
        page_size,
        total,
    }))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminTaskEventItem {
    id: String,
    event_type: String,
    payload_json: String,
    created_at: String,
}

const ADMIN_TASK_DETAIL_EVENT_LIMIT: i64 = 200;
const ADMIN_SYNC_SUBSCRIPTION_EVENT_LIMIT: i64 = 20;

#[derive(Debug, Serialize)]
pub struct AdminTaskEventMeta {
    returned: i64,
    total: i64,
    limit: i64,
    truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminBusinessOutcome {
    code: String, // ok | partial | failed | disabled | unknown
    label: String,
    message: String,
}

#[derive(Debug, Serialize)]
pub struct AdminTaskDiagnostics {
    business_outcome: AdminBusinessOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    translate_release_batch: Option<AdminTranslateReleaseBatchDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    brief_daily_slot: Option<AdminBriefDailySlotDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    brief_generate: Option<AdminBriefGenerateDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    brief_history_recompute: Option<AdminBriefHistoryRecomputeDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    brief_refresh_content: Option<AdminBriefRefreshContentDiagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_subscriptions: Option<AdminSyncSubscriptionsDiagnostics>,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslateReleaseBatchDiagnostics {
    target_user_id: Option<String>,
    release_total: i64,
    summary: AdminTranslateReleaseBatchSummary,
    progress: AdminTranslateReleaseBatchProgress,
    items: Vec<AdminTranslateReleaseBatchItemDiagnostic>,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslateReleaseBatchSummary {
    total: i64,
    ready: i64,
    missing: i64,
    disabled: i64,
    error: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslateReleaseBatchProgress {
    processed: i64,
    last_stage: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslateReleaseBatchItemDiagnostic {
    release_id: String,
    item_status: String,
    item_error: Option<String>,
    last_event_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminBriefDailySlotDiagnostics {
    hour_utc: Option<i64>,
    summary: AdminBriefDailySlotSummary,
    users: Vec<AdminBriefDailySlotUserDiagnostic>,
}

#[derive(Debug, Serialize)]
pub struct AdminBriefDailySlotSummary {
    total_users: i64,
    progressed_users: i64,
    succeeded_users: i64,
    failed_users: i64,
    canceled: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminBriefDailySlotUserDiagnostic {
    user_id: String,
    key_date: Option<String>,
    local_boundary: Option<String>,
    time_zone: Option<String>,
    window_start_utc: Option<String>,
    window_end_utc: Option<String>,
    state: String, // succeeded | failed | running
    error: Option<String>,
    last_event_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminBriefGenerateDiagnostics {
    target_user_id: Option<String>,
    brief_id: Option<String>,
    content_length: Option<i64>,
    key_date: Option<String>,
    date: Option<String>,
    window_start_utc: Option<String>,
    window_end_utc: Option<String>,
    effective_time_zone: Option<String>,
    effective_local_boundary: Option<String>,
    release_count: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdminBriefHistoryRecomputeDiagnostics {
    total: i64,
    processed: i64,
    succeeded: i64,
    failed: i64,
    current_brief_id: Option<String>,
    last_error: Option<String>,
    canceled: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminBriefRefreshContentDiagnostics {
    total: i64,
    processed: i64,
    succeeded: i64,
    failed: i64,
    current_brief_id: Option<String>,
    last_error: Option<String>,
    canceled: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminSyncSubscriptionStarDiagnostics {
    total_users: i64,
    succeeded_users: i64,
    failed_users: i64,
    total_repos: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminSyncSubscriptionReleaseDiagnostics {
    total_repos: i64,
    succeeded_repos: i64,
    failed_repos: i64,
    candidate_failures: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminSyncSubscriptionSocialDiagnostics {
    total_users: i64,
    succeeded_users: i64,
    failed_users: i64,
    repo_stars: i64,
    followers: i64,
    events: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminSyncSubscriptionNotificationsDiagnostics {
    total_users: i64,
    succeeded_users: i64,
    failed_users: i64,
    notifications: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct AdminSyncSubscriptionEventItem {
    id: String,
    stage: String,
    event_type: String,
    severity: String,
    recoverable: bool,
    attempt: i64,
    user_id: Option<String>,
    repo_id: Option<i64>,
    repo_full_name: Option<String>,
    message: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminSyncSubscriptionsDiagnostics {
    trigger: Option<String>,
    schedule_key: Option<String>,
    skipped: bool,
    skip_reason: Option<String>,
    log_available: bool,
    log_download_path: Option<String>,
    star: AdminSyncSubscriptionStarDiagnostics,
    release: AdminSyncSubscriptionReleaseDiagnostics,
    social: AdminSyncSubscriptionSocialDiagnostics,
    notifications: AdminSyncSubscriptionNotificationsDiagnostics,
    releases_written: i64,
    critical_events: i64,
    recent_events: Vec<AdminSyncSubscriptionEventItem>,
}

#[derive(Debug, sqlx::FromRow)]
struct AdminSyncSubscriptionEventRow {
    id: String,
    stage: String,
    event_type: String,
    severity: String,
    recoverable: bool,
    attempt: i64,
    user_id: Option<String>,
    repo_id: Option<i64>,
    repo_full_name: Option<String>,
    payload_json: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminRealtimeTaskDetailResponse {
    task: AdminRealtimeTaskDetailItem,
    events: Vec<AdminTaskEventItem>,
    event_meta: AdminTaskEventMeta,
    diagnostics: Option<AdminTaskDiagnostics>,
}

fn parse_json_value(raw: Option<&str>) -> Option<serde_json::Value> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    serde_json::from_str(raw).ok()
}

fn json_value_to_string(value: &serde_json::Value) -> Option<String> {
    if let Some(raw) = value.as_str() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    } else if value.is_number() || value.is_boolean() {
        Some(value.to_string())
    } else {
        None
    }
}

fn json_value_to_i64(value: &serde_json::Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    value
        .as_str()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| raw.parse::<i64>().ok())
}

fn json_value_to_bool(value: &serde_json::Value) -> Option<bool> {
    if let Some(boolean) = value.as_bool() {
        return Some(boolean);
    }
    if let Some(number) = value.as_i64() {
        return Some(number != 0);
    }
    value.as_str().map(str::trim).and_then(|raw| match raw {
        "1" | "true" | "TRUE" => Some(true),
        "0" | "false" | "FALSE" => Some(false),
        _ => None,
    })
}

fn json_object_get_i64(
    object: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<i64> {
    object
        .and_then(|obj| obj.get(key))
        .and_then(json_value_to_i64)
}

fn json_object_get_string(
    object: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<String> {
    object
        .and_then(|obj| obj.get(key))
        .and_then(json_value_to_string)
}

fn json_object_get_local_id(
    object: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<String> {
    object.and_then(|obj| obj.get(key)).and_then(|value| {
        value
            .as_str()
            .map(ToOwned::to_owned)
            .or_else(|| value.as_i64().map(|id| id.to_string()))
    })
}

fn json_object_get_bool(
    object: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<bool> {
    object
        .and_then(|obj| obj.get(key))
        .and_then(json_value_to_bool)
}

fn json_object_get_object<'a>(
    object: Option<&'a serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    object
        .and_then(|obj| obj.get(key))
        .and_then(serde_json::Value::as_object)
}

fn sync_subscription_log_download_path(task_id: &str) -> String {
    format!(
        "/api/admin/jobs/realtime/{}/log",
        urlencoding::encode(task_id)
    )
}

fn business_outcome(code: &str, label: &str, message: impl Into<String>) -> AdminBusinessOutcome {
    AdminBusinessOutcome {
        code: code.to_owned(),
        label: label.to_owned(),
        message: message.into(),
    }
}

fn task_events_for_diagnostics(events: &[AdminTaskEventItem]) -> Vec<&AdminTaskEventItem> {
    let mut ordered_events = events.iter().collect::<Vec<_>>();
    ordered_events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    ordered_events
}

fn build_translate_release_batch_diagnostics(
    task: &AdminRealtimeTaskDetailItem,
    events: &[AdminTaskEventItem],
) -> (AdminBusinessOutcome, AdminTranslateReleaseBatchDiagnostics) {
    let payload_value = parse_json_value(Some(task.payload_json.as_str()));
    let payload_object = payload_value
        .as_ref()
        .and_then(serde_json::Value::as_object);
    let result_value = parse_json_value(task.result_json.as_deref());
    let result_object = result_value.as_ref().and_then(serde_json::Value::as_object);

    let target_user_id = json_object_get_local_id(payload_object, "user_id");

    let release_total_from_payload = payload_object
        .and_then(|obj| obj.get("release_ids"))
        .and_then(serde_json::Value::as_array)
        .map(|items| i64::try_from(items.len()).unwrap_or(0))
        .unwrap_or(0);

    let mut summary = AdminTranslateReleaseBatchSummary {
        total: json_object_get_i64(result_object, "total").unwrap_or(0),
        ready: json_object_get_i64(result_object, "ready").unwrap_or(0),
        missing: json_object_get_i64(result_object, "missing").unwrap_or(0),
        disabled: json_object_get_i64(result_object, "disabled").unwrap_or(0),
        error: json_object_get_i64(result_object, "error").unwrap_or(0),
    };

    if summary.total == 0 {
        summary.total = release_total_from_payload;
    }

    let ordered_events = task_events_for_diagnostics(events);

    let mut processed = 0_i64;
    let mut last_stage: Option<String> = None;
    let mut item_pos_by_release_id = HashMap::<String, usize>::new();
    let mut items: Vec<AdminTranslateReleaseBatchItemDiagnostic> = Vec::new();
    for event in ordered_events {
        if event.event_type != "task.progress" {
            continue;
        }
        let payload_value = parse_json_value(Some(event.payload_json.as_str()));
        let payload_object = payload_value
            .as_ref()
            .and_then(serde_json::Value::as_object);
        let Some(stage) = json_object_get_string(payload_object, "stage") else {
            continue;
        };
        last_stage = Some(stage.clone());
        if stage != "release" {
            continue;
        }

        processed += 1;
        let release_id = json_object_get_string(payload_object, "release_id")
            .unwrap_or_else(|| "unknown".to_owned());
        let item_status = json_object_get_string(payload_object, "item_status")
            .unwrap_or_else(|| "unknown".to_owned());
        let item_error = json_object_get_string(payload_object, "item_error");
        if let Some(pos) = item_pos_by_release_id.get(&release_id).copied() {
            items[pos].item_status = item_status;
            items[pos].item_error = item_error;
            items[pos].last_event_at = event.created_at.clone();
            continue;
        }
        let pos = items.len();
        item_pos_by_release_id.insert(release_id.clone(), pos);
        items.push(AdminTranslateReleaseBatchItemDiagnostic {
            release_id,
            item_status,
            item_error,
            last_event_at: event.created_at.clone(),
        });
    }

    if summary.total == 0 {
        summary.total = processed;
    }

    let release_total = summary.total.max(release_total_from_payload);
    let progress = AdminTranslateReleaseBatchProgress {
        processed,
        last_stage,
    };

    let outcome = if task.status == jobs::STATUS_FAILED {
        business_outcome(
            "failed",
            "业务失败",
            task.error_message
                .clone()
                .unwrap_or_else(|| "任务执行失败，未产出可用翻译。".to_owned()),
        )
    } else if task.status == jobs::STATUS_CANCELED {
        business_outcome("partial", "已取消", "任务已取消，结果可能不完整。")
    } else if task.status == jobs::STATUS_QUEUED || task.status == jobs::STATUS_RUNNING {
        business_outcome("unknown", "处理中", "任务正在执行中，结果尚未稳定。")
    } else if release_total == 0 {
        business_outcome("ok", "无需翻译", "任务未命中可翻译的 Release。")
    } else if summary.disabled == release_total {
        business_outcome("disabled", "AI 已禁用", "翻译能力未启用，任务未进行翻译。")
    } else if summary.error > 0 && summary.ready == 0 {
        business_outcome("failed", "业务失败", "任务已运行完成，但全部翻译项失败。")
    } else if summary.error > 0 || summary.missing > 0 {
        business_outcome(
            "partial",
            "部分成功",
            "部分 Release 翻译成功，部分失败或缺失。",
        )
    } else if summary.ready > 0 && summary.ready >= release_total {
        business_outcome("ok", "业务成功", "所有目标 Release 已完成翻译。")
    } else if summary.ready > 0 {
        business_outcome("partial", "部分成功", "已有翻译结果，但统计未完全收敛。")
    } else {
        business_outcome(
            "unknown",
            "结果未知",
            "任务状态显示成功，但缺少可判定的翻译结果统计。",
        )
    };

    (
        outcome,
        AdminTranslateReleaseBatchDiagnostics {
            target_user_id,
            release_total,
            summary,
            progress,
            items,
        },
    )
}

fn upsert_daily_slot_user_diag(
    users: &mut Vec<AdminBriefDailySlotUserDiagnostic>,
    index: &mut HashMap<String, usize>,
    user_id: String,
    event_created_at: &str,
) -> usize {
    if let Some(pos) = index.get(&user_id).copied() {
        users[pos].last_event_at = event_created_at.to_owned();
        return pos;
    }
    let pos = users.len();
    users.push(AdminBriefDailySlotUserDiagnostic {
        user_id: user_id.clone(),
        key_date: None,
        local_boundary: None,
        time_zone: None,
        window_start_utc: None,
        window_end_utc: None,
        state: "running".to_owned(),
        error: None,
        last_event_at: event_created_at.to_owned(),
    });
    index.insert(user_id, pos);
    pos
}

fn build_brief_daily_slot_diagnostics(
    task: &AdminRealtimeTaskDetailItem,
    events: &[AdminTaskEventItem],
) -> (AdminBusinessOutcome, AdminBriefDailySlotDiagnostics) {
    let payload_value = parse_json_value(Some(task.payload_json.as_str()));
    let payload_object = payload_value
        .as_ref()
        .and_then(serde_json::Value::as_object);
    let result_value = parse_json_value(task.result_json.as_deref());
    let result_object = result_value.as_ref().and_then(serde_json::Value::as_object);

    let hour_utc = json_object_get_i64(payload_object, "hour_utc");
    let mut collected_total_users = 0_i64;
    let mut progressed_users = 0_i64;
    let mut summary_total_from_event: Option<i64> = None;
    let mut summary_succeeded_from_event: Option<i64> = None;
    let mut summary_failed_from_event: Option<i64> = None;
    let mut summary_canceled_from_event: Option<bool> = None;
    let mut succeeded_from_events = 0_i64;
    let mut failed_from_events = 0_i64;

    let mut users: Vec<AdminBriefDailySlotUserDiagnostic> = Vec::new();
    let mut user_index = HashMap::<String, usize>::new();
    let ordered_events = task_events_for_diagnostics(events);
    for event in ordered_events {
        if event.event_type != "task.progress" {
            continue;
        }
        let payload_value = parse_json_value(Some(event.payload_json.as_str()));
        let payload_object = payload_value
            .as_ref()
            .and_then(serde_json::Value::as_object);
        let Some(stage) = json_object_get_string(payload_object, "stage") else {
            continue;
        };
        match stage.as_str() {
            "collect" => {
                if let Some(total_users) = json_object_get_i64(payload_object, "total_users") {
                    collected_total_users = total_users;
                }
            }
            "generate" => {
                if let Some(index_value) = json_object_get_i64(payload_object, "index") {
                    progressed_users = progressed_users.max(index_value);
                }
                if let Some(user_id) = json_object_get_local_id(payload_object, "user_id") {
                    let pos = upsert_daily_slot_user_diag(
                        &mut users,
                        &mut user_index,
                        user_id,
                        &event.created_at,
                    );
                    if let Some(key_date) = json_object_get_string(payload_object, "key_date") {
                        users[pos].key_date = Some(key_date);
                    }
                    users[pos].local_boundary =
                        json_object_get_string(payload_object, "local_boundary");
                    users[pos].time_zone = json_object_get_string(payload_object, "time_zone");
                    users[pos].window_start_utc =
                        json_object_get_string(payload_object, "window_start_utc");
                    users[pos].window_end_utc =
                        json_object_get_string(payload_object, "window_end_utc");
                }
            }
            "user_failed" => {
                failed_from_events += 1;
                if let Some(user_id) = json_object_get_local_id(payload_object, "user_id") {
                    let pos = upsert_daily_slot_user_diag(
                        &mut users,
                        &mut user_index,
                        user_id,
                        &event.created_at,
                    );
                    users[pos].state = "failed".to_owned();
                    users[pos].error = json_object_get_string(payload_object, "error");
                    if let Some(key_date) = json_object_get_string(payload_object, "key_date") {
                        users[pos].key_date = Some(key_date);
                    }
                    users[pos].local_boundary =
                        json_object_get_string(payload_object, "local_boundary");
                    users[pos].time_zone = json_object_get_string(payload_object, "time_zone");
                }
            }
            "user_succeeded" => {
                succeeded_from_events += 1;
                if let Some(user_id) = json_object_get_local_id(payload_object, "user_id") {
                    let pos = upsert_daily_slot_user_diag(
                        &mut users,
                        &mut user_index,
                        user_id,
                        &event.created_at,
                    );
                    users[pos].state = "succeeded".to_owned();
                    users[pos].error = None;
                    if let Some(key_date) = json_object_get_string(payload_object, "key_date") {
                        users[pos].key_date = Some(key_date);
                    }
                    users[pos].local_boundary =
                        json_object_get_string(payload_object, "local_boundary");
                    users[pos].time_zone = json_object_get_string(payload_object, "time_zone");
                    users[pos].window_start_utc =
                        json_object_get_string(payload_object, "window_start_utc");
                    users[pos].window_end_utc =
                        json_object_get_string(payload_object, "window_end_utc");
                }
            }
            "summary" => {
                summary_total_from_event = json_object_get_i64(payload_object, "total");
                summary_succeeded_from_event = json_object_get_i64(payload_object, "succeeded");
                summary_failed_from_event = json_object_get_i64(payload_object, "failed");
                summary_canceled_from_event = json_object_get_bool(payload_object, "canceled");
            }
            _ => {}
        }
    }

    let total_users = summary_total_from_event
        .or_else(|| json_object_get_i64(result_object, "total"))
        .unwrap_or(collected_total_users.max(i64::try_from(users.len()).unwrap_or(0)));
    let succeeded_users = summary_succeeded_from_event
        .or_else(|| json_object_get_i64(result_object, "succeeded"))
        .unwrap_or(succeeded_from_events);
    let failed_users = summary_failed_from_event
        .or_else(|| json_object_get_i64(result_object, "failed"))
        .unwrap_or(failed_from_events);
    let canceled = summary_canceled_from_event
        .or_else(|| json_object_get_bool(result_object, "canceled"))
        .unwrap_or(false);

    let summary = AdminBriefDailySlotSummary {
        total_users,
        progressed_users,
        succeeded_users,
        failed_users,
        canceled,
    };

    users.sort_by_key(|item| item.user_id.clone());

    let outcome = if task.status == jobs::STATUS_FAILED {
        business_outcome(
            "failed",
            "业务失败",
            task.error_message
                .clone()
                .unwrap_or_else(|| "日报任务执行失败。".to_owned()),
        )
    } else if task.status == jobs::STATUS_CANCELED {
        business_outcome(
            "partial",
            "已取消",
            "任务在执行中被取消，部分用户可能未处理。",
        )
    } else if task.status == jobs::STATUS_QUEUED || task.status == jobs::STATUS_RUNNING {
        business_outcome("unknown", "处理中", "任务正在执行中，结果尚未稳定。")
    } else if canceled {
        business_outcome(
            "partial",
            "已取消",
            "任务在执行中被取消，部分用户可能未处理。",
        )
    } else if total_users == 0 {
        business_outcome("ok", "无需执行", "当前小时槽没有可执行用户。")
    } else if failed_users > 0 && succeeded_users == 0 {
        business_outcome(
            "failed",
            "业务失败",
            "日报任务执行完成，但全部用户处理失败。",
        )
    } else if failed_users > 0 {
        business_outcome("partial", "部分成功", "部分用户日报生成成功，部分失败。")
    } else if succeeded_users > 0 {
        business_outcome("ok", "业务成功", "日报任务已完成，目标用户处理成功。")
    } else {
        business_outcome(
            "unknown",
            "结果未知",
            "任务已完成，但缺少可判定的用户执行结果。",
        )
    };

    (
        outcome,
        AdminBriefDailySlotDiagnostics {
            hour_utc,
            summary,
            users,
        },
    )
}

fn build_brief_generate_diagnostics(
    task: &AdminRealtimeTaskDetailItem,
) -> (AdminBusinessOutcome, AdminBriefGenerateDiagnostics) {
    let payload_value = parse_json_value(Some(task.payload_json.as_str()));
    let payload_object = payload_value
        .as_ref()
        .and_then(serde_json::Value::as_object);
    let result_value = parse_json_value(task.result_json.as_deref());
    let result_object = result_value.as_ref().and_then(serde_json::Value::as_object);

    let content_length = json_object_get_i64(result_object, "content_length");
    let diagnostics = AdminBriefGenerateDiagnostics {
        target_user_id: json_object_get_local_id(payload_object, "user_id"),
        brief_id: json_object_get_local_id(result_object, "brief_id"),
        content_length,
        key_date: json_object_get_string(payload_object, "key_date"),
        date: json_object_get_string(result_object, "date"),
        window_start_utc: json_object_get_string(result_object, "window_start_utc"),
        window_end_utc: json_object_get_string(result_object, "window_end_utc"),
        effective_time_zone: json_object_get_string(result_object, "effective_time_zone"),
        effective_local_boundary: json_object_get_string(result_object, "effective_local_boundary"),
        release_count: json_object_get_i64(result_object, "release_count"),
    };

    let outcome = if task.status == jobs::STATUS_FAILED {
        business_outcome(
            "failed",
            "业务失败",
            task.error_message
                .clone()
                .unwrap_or_else(|| "日报生成失败。".to_owned()),
        )
    } else if task.status == jobs::STATUS_CANCELED {
        business_outcome("partial", "已取消", "任务在执行中被取消，结果可能不完整。")
    } else if task.status == jobs::STATUS_SUCCEEDED {
        if content_length.unwrap_or(0) > 0 {
            business_outcome("ok", "业务成功", "日报内容已生成并写入。")
        } else {
            business_outcome(
                "unknown",
                "结果未知",
                "任务已完成，但缺少可见的日报内容长度。",
            )
        }
    } else {
        business_outcome("unknown", "处理中", "任务尚未完成。")
    };

    (outcome, diagnostics)
}

fn build_brief_history_recompute_diagnostics(
    task: &AdminRealtimeTaskDetailItem,
    events: &[AdminTaskEventItem],
) -> (AdminBusinessOutcome, AdminBriefHistoryRecomputeDiagnostics) {
    let result_value = parse_json_value(task.result_json.as_deref());
    let result_object = result_value.as_ref().and_then(serde_json::Value::as_object);

    let mut total_from_event: Option<i64> = None;
    let mut processed_from_event: Option<i64> = None;
    let mut succeeded_from_event: Option<i64> = None;
    let mut failed_from_event: Option<i64> = None;
    let mut canceled_from_event: Option<bool> = None;
    let ordered_events = task_events_for_diagnostics(events);
    let mut current_brief_id: Option<String> = None;
    let mut last_error: Option<String> = None;

    for event in ordered_events {
        if event.event_type != "task.progress" {
            continue;
        }
        let payload_value = parse_json_value(Some(event.payload_json.as_str()));
        let payload_object = payload_value
            .as_ref()
            .and_then(serde_json::Value::as_object);
        let Some(stage) = json_object_get_string(payload_object, "stage") else {
            continue;
        };
        match stage.as_str() {
            "recompute" | "brief_succeeded" => {
                current_brief_id = json_object_get_string(payload_object, "brief_id");
            }
            "brief_failed" => {
                current_brief_id = json_object_get_string(payload_object, "brief_id");
                last_error = json_object_get_string(payload_object, "error");
            }
            "summary" => {
                total_from_event = json_object_get_i64(payload_object, "total");
                processed_from_event = json_object_get_i64(payload_object, "processed");
                succeeded_from_event = json_object_get_i64(payload_object, "succeeded");
                failed_from_event = json_object_get_i64(payload_object, "failed");
                canceled_from_event = json_object_get_bool(payload_object, "canceled");
                current_brief_id = None;
            }
            _ => {}
        }
    }

    let total = total_from_event
        .or_else(|| json_object_get_i64(result_object, "total"))
        .unwrap_or(0);
    let processed = processed_from_event
        .or_else(|| json_object_get_i64(result_object, "processed"))
        .unwrap_or(0);
    let succeeded = succeeded_from_event
        .or_else(|| json_object_get_i64(result_object, "succeeded"))
        .unwrap_or(0);
    let failed = failed_from_event
        .or_else(|| json_object_get_i64(result_object, "failed"))
        .unwrap_or(0);
    let canceled = canceled_from_event
        .or_else(|| json_object_get_bool(result_object, "canceled"))
        .unwrap_or(false);

    let diagnostics = AdminBriefHistoryRecomputeDiagnostics {
        total,
        processed,
        succeeded,
        failed,
        current_brief_id,
        last_error,
        canceled,
    };

    let outcome = if task.status == jobs::STATUS_FAILED {
        business_outcome(
            "failed",
            "业务失败",
            task.error_message
                .clone()
                .unwrap_or_else(|| "历史日报重算失败。".to_owned()),
        )
    } else if task.status == jobs::STATUS_CANCELED || canceled {
        business_outcome("partial", "已取消", "历史日报重算在执行中被取消。")
    } else if total == 0 {
        business_outcome("ok", "无需执行", "没有遗留旧日报需要重算。")
    } else if failed > 0 && succeeded == 0 {
        business_outcome("failed", "业务失败", "历史日报重算未成功处理任何遗留日报。")
    } else if failed > 0 {
        business_outcome("partial", "部分成功", "历史日报已部分重算，仍有失败项。")
    } else if succeeded > 0 {
        business_outcome("ok", "业务成功", "历史日报重算已完成。")
    } else {
        business_outcome("unknown", "处理中", "历史日报重算尚未完成。")
    };

    (outcome, diagnostics)
}

fn build_brief_refresh_content_diagnostics(
    task: &AdminRealtimeTaskDetailItem,
    events: &[AdminTaskEventItem],
) -> (AdminBusinessOutcome, AdminBriefRefreshContentDiagnostics) {
    let result_value = parse_json_value(task.result_json.as_deref());
    let result_object = result_value.as_ref().and_then(serde_json::Value::as_object);

    let mut total_from_event: Option<i64> = None;
    let mut processed_from_event: Option<i64> = None;
    let mut succeeded_from_event: Option<i64> = None;
    let mut failed_from_event: Option<i64> = None;
    let mut canceled_from_event: Option<bool> = None;
    let ordered_events = task_events_for_diagnostics(events);
    let mut current_brief_id: Option<String> = None;
    let mut last_error: Option<String> = None;

    for event in ordered_events {
        if event.event_type != "task.progress" {
            continue;
        }
        let payload_value = parse_json_value(Some(event.payload_json.as_str()));
        let payload_object = payload_value
            .as_ref()
            .and_then(serde_json::Value::as_object);
        let Some(stage) = json_object_get_string(payload_object, "stage") else {
            continue;
        };
        match stage.as_str() {
            "collect" => {
                total_from_event = json_object_get_i64(payload_object, "total_briefs");
            }
            "refresh" => {
                if let Some(index) = json_object_get_i64(payload_object, "index") {
                    processed_from_event = Some(processed_from_event.unwrap_or(0).max(index));
                }
                if let Some(total) = json_object_get_i64(payload_object, "total") {
                    total_from_event = Some(total_from_event.unwrap_or(0).max(total));
                }
                current_brief_id = json_object_get_string(payload_object, "brief_id");
            }
            "brief_succeeded" => {
                if let Some(index) = json_object_get_i64(payload_object, "index") {
                    processed_from_event = Some(processed_from_event.unwrap_or(0).max(index));
                }
                if let Some(total) = json_object_get_i64(payload_object, "total") {
                    total_from_event = Some(total_from_event.unwrap_or(0).max(total));
                }
                succeeded_from_event = Some(succeeded_from_event.unwrap_or(0) + 1);
                current_brief_id = json_object_get_string(payload_object, "brief_id");
            }
            "brief_failed" => {
                if let Some(index) = json_object_get_i64(payload_object, "index") {
                    processed_from_event = Some(processed_from_event.unwrap_or(0).max(index));
                }
                if let Some(total) = json_object_get_i64(payload_object, "total") {
                    total_from_event = Some(total_from_event.unwrap_or(0).max(total));
                }
                failed_from_event = Some(failed_from_event.unwrap_or(0) + 1);
                current_brief_id = json_object_get_string(payload_object, "brief_id");
                last_error = json_object_get_string(payload_object, "error");
            }
            "summary" => {
                total_from_event = json_object_get_i64(payload_object, "total");
                processed_from_event = json_object_get_i64(payload_object, "processed");
                succeeded_from_event = json_object_get_i64(payload_object, "succeeded");
                failed_from_event = json_object_get_i64(payload_object, "failed");
                canceled_from_event = json_object_get_bool(payload_object, "canceled");
                current_brief_id = None;
            }
            _ => {}
        }
    }

    let total = total_from_event
        .or_else(|| json_object_get_i64(result_object, "total"))
        .unwrap_or(0);
    let processed = processed_from_event
        .or_else(|| json_object_get_i64(result_object, "processed"))
        .unwrap_or(0);
    let succeeded = succeeded_from_event
        .or_else(|| json_object_get_i64(result_object, "succeeded"))
        .unwrap_or(0);
    let failed = failed_from_event
        .or_else(|| json_object_get_i64(result_object, "failed"))
        .unwrap_or(0);
    let canceled = canceled_from_event
        .or_else(|| json_object_get_bool(result_object, "canceled"))
        .unwrap_or(false);

    let diagnostics = AdminBriefRefreshContentDiagnostics {
        total,
        processed,
        succeeded,
        failed,
        current_brief_id,
        last_error,
        canceled,
    };

    let outcome = if task.status == jobs::STATUS_FAILED {
        business_outcome(
            "failed",
            "业务失败",
            task.error_message
                .clone()
                .unwrap_or_else(|| "日报内容修复失败。".to_owned()),
        )
    } else if task.status == jobs::STATUS_CANCELED || canceled {
        business_outcome("partial", "已取消", "日报内容修复在执行中被取消。")
    } else if total == 0 {
        business_outcome("ok", "无需执行", "没有命中过时格式的日报需要修复。")
    } else if failed > 0 && succeeded == 0 {
        business_outcome("failed", "业务失败", "日报内容修复未成功处理任何候选日报。")
    } else if failed > 0 {
        business_outcome("partial", "部分成功", "日报内容已部分修复，仍有失败项。")
    } else if succeeded > 0 {
        business_outcome("ok", "业务成功", "日报内容修复已完成。")
    } else {
        business_outcome("unknown", "处理中", "日报内容修复尚未完成。")
    };

    (outcome, diagnostics)
}

fn map_sync_subscription_event(
    row: AdminSyncSubscriptionEventRow,
) -> AdminSyncSubscriptionEventItem {
    let payload_value = parse_json_value(row.payload_json.as_deref());
    let payload_object = payload_value
        .as_ref()
        .and_then(serde_json::Value::as_object);

    AdminSyncSubscriptionEventItem {
        id: row.id,
        stage: row.stage,
        event_type: row.event_type,
        severity: row.severity,
        recoverable: row.recoverable,
        attempt: row.attempt,
        user_id: row.user_id,
        repo_id: row.repo_id,
        repo_full_name: row.repo_full_name,
        message: json_object_get_string(payload_object, "message"),
        created_at: row.created_at,
    }
}

fn build_sync_subscriptions_diagnostics(
    task: &AdminRealtimeTaskDetailItem,
    recent_events: &[AdminSyncSubscriptionEventItem],
) -> (AdminBusinessOutcome, AdminSyncSubscriptionsDiagnostics) {
    let payload_value = parse_json_value(Some(task.payload_json.as_str()));
    let payload_object = payload_value
        .as_ref()
        .and_then(serde_json::Value::as_object);
    let result_value = parse_json_value(task.result_json.as_deref());
    let result_object = result_value.as_ref().and_then(serde_json::Value::as_object);
    let star_object = json_object_get_object(result_object, "star");
    let release_object = json_object_get_object(result_object, "release");
    let social_object = json_object_get_object(result_object, "social");
    let notifications_object = json_object_get_object(result_object, "notifications");

    let skipped = json_object_get_bool(result_object, "skipped").unwrap_or(false);
    let skip_reason = json_object_get_string(result_object, "skip_reason");
    let trigger = json_object_get_string(payload_object, "trigger");
    let schedule_key = json_object_get_string(payload_object, "schedule_key");
    let star = AdminSyncSubscriptionStarDiagnostics {
        total_users: json_object_get_i64(star_object, "total_users").unwrap_or(0),
        succeeded_users: json_object_get_i64(star_object, "succeeded_users").unwrap_or(0),
        failed_users: json_object_get_i64(star_object, "failed_users").unwrap_or(0),
        total_repos: json_object_get_i64(star_object, "total_repos").unwrap_or(0),
    };
    let release = AdminSyncSubscriptionReleaseDiagnostics {
        total_repos: json_object_get_i64(release_object, "total_repos").unwrap_or(0),
        succeeded_repos: json_object_get_i64(release_object, "succeeded_repos").unwrap_or(0),
        failed_repos: json_object_get_i64(release_object, "failed_repos").unwrap_or(0),
        candidate_failures: json_object_get_i64(release_object, "candidate_failures").unwrap_or(0),
    };
    let social = AdminSyncSubscriptionSocialDiagnostics {
        total_users: json_object_get_i64(social_object, "total_users").unwrap_or(0),
        succeeded_users: json_object_get_i64(social_object, "succeeded_users").unwrap_or(0),
        failed_users: json_object_get_i64(social_object, "failed_users").unwrap_or(0),
        repo_stars: json_object_get_i64(social_object, "repo_stars").unwrap_or(0),
        followers: json_object_get_i64(social_object, "followers").unwrap_or(0),
        events: json_object_get_i64(social_object, "events").unwrap_or(0),
    };
    let notifications = AdminSyncSubscriptionNotificationsDiagnostics {
        total_users: json_object_get_i64(notifications_object, "total_users").unwrap_or(0),
        succeeded_users: json_object_get_i64(notifications_object, "succeeded_users").unwrap_or(0),
        failed_users: json_object_get_i64(notifications_object, "failed_users").unwrap_or(0),
        notifications: json_object_get_i64(notifications_object, "notifications").unwrap_or(0),
    };
    let releases_written = json_object_get_i64(result_object, "releases_written").unwrap_or(0);
    let critical_events = json_object_get_i64(result_object, "critical_events").unwrap_or(0);
    let log_available = task
        .log_file_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(std::path::Path::new)
        .is_some_and(std::path::Path::is_file);
    let log_download_path = log_available.then(|| sync_subscription_log_download_path(&task.id));

    let outcome = if task.status == jobs::STATUS_FAILED {
        business_outcome(
            "failed",
            "业务失败",
            task.error_message
                .clone()
                .unwrap_or_else(|| "订阅同步任务执行失败。".to_owned()),
        )
    } else if skipped {
        business_outcome(
            "disabled",
            "已跳过",
            match skip_reason.as_deref() {
                Some("previous_run_active") => {
                    "上一轮订阅同步仍在执行，本轮仅记录跳过结果。".to_owned()
                }
                Some(other) => format!("本轮任务已跳过：{other}"),
                None => "本轮任务已跳过。".to_owned(),
            },
        )
    } else if task.status == jobs::STATUS_CANCELED {
        business_outcome("partial", "已取消", "任务在执行中被取消，结果可能不完整。")
    } else if task.status == jobs::STATUS_QUEUED || task.status == jobs::STATUS_RUNNING {
        business_outcome("unknown", "处理中", "任务正在执行中，结果尚未稳定。")
    } else if star.total_users == 0 {
        business_outcome("ok", "无需执行", "当前没有可同步的启用用户。")
    } else if star.succeeded_users == 0 && star.failed_users > 0 {
        business_outcome(
            "failed",
            "业务失败",
            "Star 阶段全部失败，未进入仓库抓取阶段。",
        )
    } else if release.total_repos > 0 && release.succeeded_repos == 0 && release.failed_repos > 0 {
        business_outcome(
            "failed",
            "业务失败",
            "Release 阶段全部失败，未能写入任何仓库结果。",
        )
    } else if star.failed_users > 0
        || release.failed_repos > 0
        || social.failed_users > 0
        || notifications.failed_users > 0
        || critical_events > 0
    {
        business_outcome(
            "partial",
            "部分成功",
            "任务已完成，但存在失败或关键告警，请查看最近关键事件。",
        )
    } else if release.total_repos == 0 {
        business_outcome(
            "ok",
            "业务成功",
            "Star 阶段已完成，本轮没有需要抓取 Release 的仓库。",
        )
    } else {
        business_outcome("ok", "业务成功", "订阅同步任务已完成。")
    };

    (
        outcome,
        AdminSyncSubscriptionsDiagnostics {
            trigger,
            schedule_key,
            skipped,
            skip_reason,
            log_available,
            log_download_path,
            star,
            release,
            social,
            notifications,
            releases_written,
            critical_events,
            recent_events: recent_events.to_vec(),
        },
    )
}

fn build_task_diagnostics(
    task: &AdminRealtimeTaskDetailItem,
    events: &[AdminTaskEventItem],
    subscription_events: &[AdminSyncSubscriptionEventItem],
) -> Option<AdminTaskDiagnostics> {
    match task.task_type.as_str() {
        jobs::TASK_TRANSLATE_RELEASE_BATCH | jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH => {
            let (business_outcome, diagnostics) =
                build_translate_release_batch_diagnostics(task, events);
            Some(AdminTaskDiagnostics {
                business_outcome,
                translate_release_batch: Some(diagnostics),
                brief_daily_slot: None,
                brief_generate: None,
                brief_history_recompute: None,
                brief_refresh_content: None,
                sync_subscriptions: None,
            })
        }
        jobs::TASK_BRIEF_DAILY_SLOT => {
            let (business_outcome, diagnostics) = build_brief_daily_slot_diagnostics(task, events);
            Some(AdminTaskDiagnostics {
                business_outcome,
                translate_release_batch: None,
                brief_daily_slot: Some(diagnostics),
                brief_generate: None,
                brief_history_recompute: None,
                brief_refresh_content: None,
                sync_subscriptions: None,
            })
        }
        jobs::TASK_BRIEF_GENERATE => {
            let (business_outcome, diagnostics) = build_brief_generate_diagnostics(task);
            Some(AdminTaskDiagnostics {
                business_outcome,
                translate_release_batch: None,
                brief_daily_slot: None,
                brief_generate: Some(diagnostics),
                brief_history_recompute: None,
                brief_refresh_content: None,
                sync_subscriptions: None,
            })
        }
        jobs::TASK_BRIEF_HISTORY_RECOMPUTE => {
            let (business_outcome, diagnostics) =
                build_brief_history_recompute_diagnostics(task, events);
            Some(AdminTaskDiagnostics {
                business_outcome,
                translate_release_batch: None,
                brief_daily_slot: None,
                brief_generate: None,
                brief_history_recompute: Some(diagnostics),
                brief_refresh_content: None,
                sync_subscriptions: None,
            })
        }
        jobs::TASK_BRIEF_REFRESH_CONTENT => {
            let (business_outcome, diagnostics) =
                build_brief_refresh_content_diagnostics(task, events);
            Some(AdminTaskDiagnostics {
                business_outcome,
                translate_release_batch: None,
                brief_daily_slot: None,
                brief_generate: None,
                brief_history_recompute: None,
                brief_refresh_content: Some(diagnostics),
                sync_subscriptions: None,
            })
        }
        jobs::TASK_SYNC_SUBSCRIPTIONS => {
            let (business_outcome, diagnostics) =
                build_sync_subscriptions_diagnostics(task, subscription_events);
            Some(AdminTaskDiagnostics {
                business_outcome,
                translate_release_batch: None,
                brief_daily_slot: None,
                brief_generate: None,
                brief_history_recompute: None,
                brief_refresh_content: None,
                sync_subscriptions: Some(diagnostics),
            })
        }
        _ => None,
    }
}

pub async fn admin_get_realtime_task_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(task_id): Path<String>,
) -> Result<Json<AdminRealtimeTaskDetailResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let task_id = parse_local_id_param(task_id, "task_id")?;

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
          log_file_path,
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

    let event_total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM job_task_events
        WHERE task_id = ?
        "#,
    )
    .bind(task_id.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let events = sqlx::query_as::<_, AdminTaskEventItem>(
        r#"
        SELECT id, event_type, payload_json, created_at
        FROM job_task_events
        WHERE task_id = ?
        ORDER BY rowid DESC
        LIMIT ?
        "#,
    )
    .bind(task_id.as_str())
    .bind(ADMIN_TASK_DETAIL_EVENT_LIMIT)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let returned = i64::try_from(events.len()).unwrap_or(ADMIN_TASK_DETAIL_EVENT_LIMIT);
    let event_total = event_total.max(returned);
    let event_meta = AdminTaskEventMeta {
        returned,
        total: event_total,
        limit: ADMIN_TASK_DETAIL_EVENT_LIMIT,
        truncated: event_total > returned,
    };

    let subscription_events = if task.task_type == jobs::TASK_SYNC_SUBSCRIPTIONS {
        let rows = sqlx::query_as::<_, AdminSyncSubscriptionEventRow>(
            r#"
            SELECT
              id,
              stage,
              event_type,
              severity,
              recoverable,
              attempt,
              user_id,
              repo_id,
              repo_full_name,
              payload_json,
              created_at
            FROM sync_subscription_events
            WHERE task_id = ?
            ORDER BY rowid DESC
            LIMIT ?
            "#,
        )
        .bind(task_id.as_str())
        .bind(ADMIN_SYNC_SUBSCRIPTION_EVENT_LIMIT)
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
        rows.into_iter().map(map_sync_subscription_event).collect()
    } else {
        Vec::new()
    };

    let diagnostics = build_task_diagnostics(&task, &events, &subscription_events);

    Ok(Json(AdminRealtimeTaskDetailResponse {
        task,
        events,
        event_meta,
        diagnostics,
    }))
}

pub async fn admin_download_realtime_task_log(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let task_id = parse_local_id_param(task_id, "task_id")?;

    let log_file_path = jobs::load_task_log_path(state.as_ref(), task_id.as_str())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "task log not found"))?;

    let file = tokio::fs::File::open(&log_file_path).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ApiError::new(StatusCode::NOT_FOUND, "not_found", "task log not found")
        } else {
            ApiError::internal(err)
        }
    })?;

    let stream = async_stream::stream! {
        let mut file = file;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            match file.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buffer[..read])),
                Err(err) => {
                    yield Err::<Bytes, std::io::Error>(err);
                    break;
                }
            }
        }
    };

    let filename = format!("{}.ndjson", task_id);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(r#"attachment; filename="{}""#, filename))
            .map_err(ApiError::internal)?,
    );

    Ok(response)
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
    let task_id = parse_local_id_param(task_id, "task_id")?;
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
    let task_id = parse_local_id_param(task_id, "task_id")?;
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
    requested_by: Option<String>,
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
    requested_by: Option<String>,
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
    max_concurrency: i64,
    ai_model_context_limit: Option<i64>,
    effective_model_input_limit: i64,
    effective_model_input_limit_source: String,
    available_slots: i64,
    waiting_calls: i64,
    in_flight_calls: i64,
    calls_24h: i64,
    failed_24h: i64,
    avg_wait_ms_24h: Option<i64>,
    avg_duration_ms_24h: Option<i64>,
    last_success_at: Option<String>,
    last_failure_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminLlmRuntimeConfigUpdateRequest {
    max_concurrency: i64,
    #[serde(default, deserialize_with = "deserialize_optional_nullable_i64")]
    ai_model_context_limit: Option<Option<i64>>,
}

#[derive(Debug, Deserialize)]
pub struct AdminLlmCallsQuery {
    status: Option<String>,
    source: Option<String>,
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    started_from: Option<String>,
    started_to: Option<String>,
    sort: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

fn llm_calls_status_rank(status: &str) -> i32 {
    match status {
        "running" => 0,
        "queued" => 1,
        _ => 2,
    }
}

fn apply_llm_call_admin_override(item: &mut AdminLlmCallItem, snapshot: &ai::LlmCallAdminOverride) {
    item.status = snapshot.status.clone();
    item.attempt_count = snapshot.attempt_count;
    item.scheduler_wait_ms = snapshot.scheduler_wait_ms;
    item.first_token_wait_ms = snapshot.first_token_wait_ms;
    item.duration_ms = snapshot.duration_ms;
    item.input_tokens = snapshot.input_tokens;
    item.output_tokens = snapshot.output_tokens;
    item.cached_input_tokens = snapshot.cached_input_tokens;
    item.total_tokens = snapshot.total_tokens;
    if let Some(started_at) = snapshot.started_at.clone() {
        item.started_at = Some(started_at);
    }
    if let Some(finished_at) = snapshot.finished_at.clone() {
        item.finished_at = Some(finished_at);
    }
    item.updated_at = snapshot.updated_at.clone();
}

fn apply_llm_call_detail_admin_override(
    item: &mut AdminLlmCallDetailItem,
    snapshot: &ai::LlmCallAdminOverride,
) {
    item.status = snapshot.status.clone();
    item.attempt_count = snapshot.attempt_count;
    item.scheduler_wait_ms = snapshot.scheduler_wait_ms;
    item.first_token_wait_ms = snapshot.first_token_wait_ms;
    item.duration_ms = snapshot.duration_ms;
    item.input_tokens = snapshot.input_tokens;
    item.output_tokens = snapshot.output_tokens;
    item.cached_input_tokens = snapshot.cached_input_tokens;
    item.total_tokens = snapshot.total_tokens;
    if let Some(output_messages_json) = snapshot.output_messages_json.clone() {
        item.output_messages_json = Some(output_messages_json);
    }
    if let Some(response_text) = snapshot.response_text.clone() {
        item.response_text = Some(response_text);
    }
    if let Some(error_text) = snapshot.error_text.clone() {
        item.error_text = Some(error_text);
    }
    if let Some(started_at) = snapshot.started_at.clone() {
        item.started_at = Some(started_at);
    }
    if let Some(finished_at) = snapshot.finished_at.clone() {
        item.finished_at = Some(finished_at);
    }
    item.updated_at = snapshot.updated_at.clone();
}

fn apply_llm_call_admin_overrides(
    items: &mut [AdminLlmCallItem],
    overrides: &HashMap<String, ai::LlmCallAdminOverride>,
) {
    for item in items {
        if let Some(snapshot) = overrides.get(&item.id) {
            apply_llm_call_admin_override(item, snapshot);
        }
    }
}

fn llm_call_matches_status_filter(item: &AdminLlmCallItem, status: &str) -> bool {
    status == "all" || item.status == status
}

fn llm_call_started_at_sort_key(item: &AdminLlmCallItem) -> i64 {
    item.started_at
        .as_deref()
        .map(llm_call_created_at_sort_key)
        .unwrap_or_else(|| llm_call_created_at_sort_key(&item.created_at))
}

fn llm_call_matches_started_filter(
    item: &AdminLlmCallItem,
    started_from: Option<&str>,
    started_to: Option<&str>,
) -> bool {
    let started_at = llm_call_started_at_sort_key(item);
    if let Some(lower_bound) = started_from.map(llm_call_created_at_sort_key)
        && started_at < lower_bound
    {
        return false;
    }
    if let Some(upper_bound) = started_to.map(llm_call_created_at_sort_key)
        && started_at > upper_bound
    {
        return false;
    }
    true
}

fn llm_call_created_at_sort_key(created_at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|value| value.timestamp_millis())
        .unwrap_or(i64::MIN)
}

fn sort_admin_llm_calls(items: &mut [AdminLlmCallItem], sort: &str) {
    items.sort_by(|left, right| match sort {
        "status_grouped" => llm_calls_status_rank(&left.status)
            .cmp(&llm_calls_status_rank(&right.status))
            .then_with(|| {
                llm_call_created_at_sort_key(&right.created_at)
                    .cmp(&llm_call_created_at_sort_key(&left.created_at))
            })
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| right.id.cmp(&left.id)),
        _ => llm_call_created_at_sort_key(&right.created_at)
            .cmp(&llm_call_created_at_sort_key(&left.created_at))
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| right.id.cmp(&left.id)),
    });
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
    admin_runtime::sync_persisted_runtime_settings(state.clone())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(
        load_admin_llm_scheduler_status_response(state.as_ref()).await?,
    ))
}

pub async fn admin_patch_llm_runtime_config(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<AdminLlmRuntimeConfigUpdateRequest>,
) -> Result<Json<AdminLlmSchedulerStatusResponse>, ApiError> {
    let _acting_user_id = require_admin_user_id(state.as_ref(), &session).await?;
    let max_concurrency = parse_positive_admin_concurrency(req.max_concurrency, "max_concurrency")?;
    let ai_model_context_limit = match req.ai_model_context_limit {
        Some(Some(value)) => Some(parse_positive_runtime_limit(
            value,
            "ai_model_context_limit",
        )?),
        Some(None) => None,
        None => admin_runtime::load_ai_model_context_limit(&state.pool)
            .await
            .map_err(ApiError::internal)?,
    };
    admin_runtime::update_llm_runtime_settings(
        &state.pool,
        max_concurrency,
        ai_model_context_limit,
    )
    .await
    .map_err(ApiError::internal)?;
    admin_runtime::sync_persisted_runtime_settings(state.clone())
        .await
        .map_err(ApiError::internal)?;

    Ok(Json(
        load_admin_llm_scheduler_status_response(state.as_ref()).await?,
    ))
}

async fn load_admin_llm_scheduler_status_response(
    state: &AppState,
) -> Result<AdminLlmSchedulerStatusResponse, ApiError> {
    let runtime = state.llm_scheduler.runtime_status();
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
    let effective_model_input_limit = ai::resolve_model_input_limit_with_source(state).await;
    let ai_model_context_limit = admin_runtime::load_ai_model_context_limit(&state.pool)
        .await
        .map_err(ApiError::internal)?
        .map(i64::from);

    Ok(AdminLlmSchedulerStatusResponse {
        scheduler_enabled: state.config.ai.is_some(),
        max_concurrency: runtime.max_concurrency,
        ai_model_context_limit,
        effective_model_input_limit: i64::from(effective_model_input_limit.0),
        effective_model_input_limit_source: effective_model_input_limit.1.to_owned(),
        available_slots: runtime.available_slots,
        waiting_calls: runtime.waiting_calls,
        in_flight_calls: runtime.in_flight_calls,
        calls_24h,
        failed_24h,
        avg_wait_ms_24h: avg_wait_raw.map(|value| value.round() as i64),
        avg_duration_ms_24h: avg_duration_raw.map(|value| value.round() as i64),
        last_success_at,
        last_failure_at,
    })
}

fn parse_positive_admin_concurrency(value: i64, field: &str) -> Result<usize, ApiError> {
    let parsed = usize::try_from(value)
        .map_err(|_| ApiError::bad_request(format!("{field} must be a positive integer")))?;
    if parsed == 0 {
        return Err(ApiError::bad_request(format!(
            "{field} must be a positive integer"
        )));
    }
    if parsed > tokio::sync::Semaphore::MAX_PERMITS {
        return Err(ApiError::bad_request(format!(
            "{field} must be a positive integer <= {}",
            tokio::sync::Semaphore::MAX_PERMITS
        )));
    }
    Ok(parsed)
}

fn parse_positive_runtime_limit(value: i64, field: &str) -> Result<u32, ApiError> {
    let parsed = u32::try_from(value)
        .map_err(|_| ApiError::bad_request(format!("{field} must be a positive integer")))?;
    if parsed == 0 {
        return Err(ApiError::bad_request(format!(
            "{field} must be a positive integer"
        )));
    }
    Ok(parsed)
}

fn deserialize_optional_nullable_i64<'de, D>(
    deserializer: D,
) -> Result<Option<Option<i64>>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Some(Option::<i64>::deserialize(deserializer)?))
}

struct AdminLlmCallListScope<'a> {
    status: Option<&'a str>,
    source: &'a str,
    requested_by: Option<&'a str>,
    parent_task_id: &'a str,
    started_from: Option<&'a str>,
    started_to: Option<&'a str>,
}

struct AdminLlmCallIdScope<'a> {
    include_ids: Option<&'a [String]>,
    exclude_ids: Option<&'a [String]>,
}

struct AdminLlmCallPage {
    limit: Option<i64>,
    offset: Option<i64>,
}

fn push_llm_call_filters(
    query: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    scope: &AdminLlmCallListScope<'_>,
    ids: &AdminLlmCallIdScope<'_>,
) {
    query.push(" WHERE 1 = 1");

    if let Some(status) = scope.status.filter(|value| *value != "all") {
        query.push(" AND status = ");
        query.push_bind(status.to_owned());
    }
    if !scope.source.is_empty() {
        query.push(" AND source = ");
        query.push_bind(scope.source.to_owned());
    }
    if let Some(requested_by) = scope.requested_by {
        query.push(" AND requested_by = ");
        query.push_bind(requested_by.to_owned());
    }
    if !scope.parent_task_id.is_empty() {
        query.push(" AND parent_task_id = ");
        query.push_bind(scope.parent_task_id.to_owned());
    }
    if let Some(started_from) = scope.started_from {
        query.push(" AND unixepoch(COALESCE(started_at, created_at)) >= unixepoch(");
        query.push_bind(started_from.to_owned());
        query.push(")");
    }
    if let Some(started_to) = scope.started_to {
        query.push(" AND unixepoch(COALESCE(started_at, created_at)) <= unixepoch(");
        query.push_bind(started_to.to_owned());
        query.push(")");
    }
    if let Some(include_ids) = ids.include_ids {
        if include_ids.is_empty() {
            query.push(" AND 1 = 0");
        } else {
            query.push(" AND id IN (");
            {
                let mut separated = query.separated(", ");
                for id in include_ids {
                    separated.push_bind(id.clone());
                }
            }
            query.push(")");
        }
    }
    if let Some(exclude_ids) = ids.exclude_ids.filter(|ids| !ids.is_empty()) {
        query.push(" AND id NOT IN (");
        {
            let mut separated = query.separated(", ");
            for id in exclude_ids {
                separated.push_bind(id.clone());
            }
        }
        query.push(")");
    }
}

const LLM_CALL_ORDER_BY_STATUS_GROUPED: &str = r#"
                ORDER BY
                  CASE
                    WHEN status = 'running' THEN 0
                    WHEN status = 'queued' THEN 1
                    ELSE 2
                  END,
                  julianday(created_at) DESC,
                  created_at DESC,
                  id DESC
                "#;
const LLM_CALL_ORDER_BY_CREATED_DESC: &str =
    " ORDER BY julianday(created_at) DESC, created_at DESC, id DESC";

fn llm_call_order_by_clause(scope: &AdminLlmCallListScope<'_>, sort: &str) -> &'static str {
    match sort {
        "status_grouped" if scope.status.unwrap_or("all") == "all" => {
            LLM_CALL_ORDER_BY_STATUS_GROUPED
        }
        _ => LLM_CALL_ORDER_BY_CREATED_DESC,
    }
}

fn push_llm_call_order_by(
    query: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    scope: &AdminLlmCallListScope<'_>,
    sort: &str,
) {
    query.push(llm_call_order_by_clause(scope, sort));
}

async fn count_admin_llm_calls(
    pool: &sqlx::SqlitePool,
    scope: &AdminLlmCallListScope<'_>,
    ids: &AdminLlmCallIdScope<'_>,
) -> Result<i64, ApiError> {
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"
        SELECT COUNT(*)
        FROM llm_calls
        "#,
    );
    push_llm_call_filters(&mut query, scope, ids);
    query
        .build_query_scalar::<i64>()
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)
}

async fn load_admin_llm_call_items(
    pool: &sqlx::SqlitePool,
    scope: &AdminLlmCallListScope<'_>,
    sort: &str,
    ids: &AdminLlmCallIdScope<'_>,
    page: AdminLlmCallPage,
) -> Result<Vec<AdminLlmCallItem>, ApiError> {
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
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
        "#,
    );
    push_llm_call_filters(&mut query, scope, ids);
    push_llm_call_order_by(&mut query, scope, sort);
    if let Some(limit) = page.limit {
        query.push(" LIMIT ");
        query.push_bind(limit);
    }
    if let Some(offset) = page.offset {
        query.push(" OFFSET ");
        query.push_bind(offset);
    }
    query
        .build_query_as::<AdminLlmCallItem>()
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)
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
    let requested_by = query.requested_by.clone();
    let parent_task_id = query.parent_task_id.unwrap_or_default().trim().to_owned();
    let started_from = parse_llm_calls_filter_timestamp(query.started_from, "started_from")?;
    let started_to = parse_llm_calls_filter_timestamp(query.started_to, "started_to")?;
    let sort = query.sort.unwrap_or_else(|| "created_desc".to_owned());
    if !matches!(sort.as_str(), "created_desc" | "status_grouped") {
        return Err(ApiError::bad_request("invalid sort filter"));
    }

    let base_scope = AdminLlmCallListScope {
        status: Some(status.as_str()),
        source: source.as_str(),
        requested_by: requested_by.as_deref(),
        parent_task_id: parent_task_id.as_str(),
        started_from: started_from.as_deref(),
        started_to: started_to.as_deref(),
    };
    let override_scope = AdminLlmCallListScope {
        status: None,
        source: source.as_str(),
        requested_by: requested_by.as_deref(),
        parent_task_id: parent_task_id.as_str(),
        started_from: None,
        started_to: None,
    };
    let no_ids = AdminLlmCallIdScope {
        include_ids: None,
        exclude_ids: None,
    };

    let overrides = state.llm_scheduler.admin_overrides().await;
    if overrides.is_empty() {
        let total = count_admin_llm_calls(&state.pool, &base_scope, &no_ids).await?;
        let items = load_admin_llm_call_items(
            &state.pool,
            &base_scope,
            sort.as_str(),
            &no_ids,
            AdminLlmCallPage {
                limit: Some(page_size),
                offset: Some(offset),
            },
        )
        .await?;

        return Ok(Json(AdminLlmCallsResponse {
            items,
            page,
            page_size,
            total,
        }));
    }

    let override_ids = overrides.keys().cloned().collect::<Vec<_>>();
    let include_override_ids = AdminLlmCallIdScope {
        include_ids: Some(&override_ids),
        exclude_ids: None,
    };
    let exclude_override_ids = AdminLlmCallIdScope {
        include_ids: None,
        exclude_ids: Some(&override_ids),
    };
    let mut override_items = load_admin_llm_call_items(
        &state.pool,
        &override_scope,
        sort.as_str(),
        &include_override_ids,
        AdminLlmCallPage {
            limit: None,
            offset: None,
        },
    )
    .await?;
    apply_llm_call_admin_overrides(&mut override_items, &overrides);
    override_items.retain(|item| {
        llm_call_matches_status_filter(item, status.as_str())
            && llm_call_matches_started_filter(item, started_from.as_deref(), started_to.as_deref())
    });
    sort_admin_llm_calls(&mut override_items, sort.as_str());

    let base_total = count_admin_llm_calls(&state.pool, &base_scope, &exclude_override_ids).await?;
    let total = base_total.saturating_add(i64::try_from(override_items.len()).unwrap_or(i64::MAX));

    let fetch_limit = offset.saturating_add(page_size);
    let mut items = load_admin_llm_call_items(
        &state.pool,
        &base_scope,
        sort.as_str(),
        &exclude_override_ids,
        AdminLlmCallPage {
            limit: Some(fetch_limit),
            offset: Some(0),
        },
    )
    .await?;
    items.extend(override_items);
    sort_admin_llm_calls(&mut items, sort.as_str());

    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    let size = usize::try_from(page_size).unwrap_or(100);
    let items = items.into_iter().skip(start).take(size).collect::<Vec<_>>();

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
    let call_id = parse_local_id_param(call_id, "call_id")?;

    let mut item = sqlx::query_as::<_, AdminLlmCallDetailItem>(
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

    if let Some(snapshot) = state.llm_scheduler.admin_overrides().await.get(&item.id) {
        apply_llm_call_detail_admin_override(&mut item, snapshot);
    }

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
    .bind(&user_id)
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
        FROM repo_releases r
        JOIN user_release_visible_repos sr
          ON sr.user_id = ? AND sr.repo_id = r.repo_id
        ORDER BY COALESCE(r.published_at, r.created_at) DESC
        LIMIT 200
        "#,
    )
     .bind(&user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(items))
}

#[derive(Debug, Serialize)]
pub struct ReleaseDetailResponse {
    release_id: String,
    repo_full_name: Option<String>,
    repo_visual: Option<RepoVisual>,
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
        owner_avatar_url: Option<String>,
        open_graph_image_url: Option<String>,
        uses_custom_open_graph_image: Option<i64>,
        tag_name: String,
        name: Option<String>,
        body: Option<String>,
        html_url: String,
        published_at: Option<String>,
        is_prerelease: i64,
        is_draft: i64,
        trans_source_hash: Option<String>,
        trans_status: Option<String>,
        trans_title: Option<String>,
        trans_summary: Option<String>,
        trans_error_text: Option<String>,
        trans_work_status: Option<String>,
    }

    let row = sqlx::query_as::<_, ReleaseDetailRow>(
        r#"
        SELECT
          r.repo_id,
          r.release_id,
          sr.full_name AS repo_full_name,
          sr.owner_avatar_url AS owner_avatar_url,
          sr.open_graph_image_url AS open_graph_image_url,
          sr.uses_custom_open_graph_image AS uses_custom_open_graph_image,
          r.tag_name,
          r.name,
          r.body,
          r.html_url,
          r.published_at,
          r.is_prerelease,
          r.is_draft,
          t.source_hash AS trans_source_hash,
          t.status AS trans_status,
          t.title AS trans_title,
          t.summary AS trans_summary,
          t.error_text AS trans_error_text,
          tw.status AS trans_work_status
        FROM repo_releases r
        LEFT JOIN user_release_visible_repos sr
          ON sr.user_id = ? AND sr.repo_id = r.repo_id
        LEFT JOIN ai_translations t
          ON t.user_id = ?
          AND t.entity_type = 'release_detail'
          AND t.entity_id = CAST(r.release_id AS TEXT)
          AND t.lang = 'zh-CN'
          AND t.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN translation_work_items tw
          ON tw.id = t.active_work_item_id
        WHERE r.release_id = ?
        LIMIT 1
        "#,
    )
    .bind(&user_id)
    .bind(&user_id)
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

    if row.repo_full_name.is_none()
        && !user_has_brief_access_to_release(state.as_ref(), &user_id, release_id).await?
    {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "release not found",
        ));
    }

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

    let refresh_in_flight = !translation_fresh
        && row.trans_status.as_deref() == Some("ready")
        && matches!(
            row.trans_work_status.as_deref(),
            Some("queued" | "batched" | "running")
        );

    let translated = if state.config.ai.is_none() {
        Some(translated_item("disabled", None, None, None, None))
    } else {
        match (translation_fresh, row.trans_status.as_deref()) {
            (true, Some("ready"))
                if release_detail_translation_ready(
                    Some(original_body.as_str()),
                    row.trans_summary.as_deref(),
                ) =>
            {
                Some(translated_item(
                    "ready",
                    row.trans_title.clone(),
                    row.trans_summary.clone(),
                    None,
                    None,
                ))
            }
            (true, Some("ready")) => Some(translated_item(
                "error",
                None,
                None,
                None,
                Some(RELEASE_DETAIL_MARKDOWN_MISMATCH_ERROR),
            )),
            (false, Some("ready")) if refresh_in_flight => {
                translated_ready_item(row.trans_title.clone(), row.trans_summary.clone(), None)
                    .or_else(|| Some(translated_missing_item(false)))
            }
            (true, Some(status)) => {
                translated_terminal_item(status, row.trans_error_text.as_deref())
            }
            _ => Some(translated_missing_item(true)),
        }
    };

    let repo_visual = repo_visual_from_parts(
        row.owner_avatar_url,
        row.open_graph_image_url,
        row.uses_custom_open_graph_image.unwrap_or(0) != 0,
    );

    Ok(Json(ReleaseDetailResponse {
        release_id: row.release_id.to_string(),
        repo_full_name: row.repo_full_name.or(Some(resolved_full_name)),
        repo_visual,
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
    id: String,
    date: String,
    window_start: Option<String>,
    window_end: Option<String>,
    effective_time_zone: Option<String>,
    effective_local_boundary: Option<String>,
    release_count: usize,
    release_ids: Vec<String>,
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
        id: String,
        date: String,
        window_start_utc: Option<String>,
        window_end_utc: Option<String>,
        effective_time_zone: Option<String>,
        effective_local_boundary: Option<String>,
        generation_source: String,
        content_markdown: String,
        created_at: String,
    }

    #[derive(Debug, sqlx::FromRow)]
    struct BriefMembershipRow {
        brief_id: String,
        release_id: i64,
    }

    let rows = sqlx::query_as::<_, BriefRow>(
        r#"
        SELECT
          id,
          date,
          window_start_utc,
          window_end_utc,
          effective_time_zone,
          effective_local_boundary,
          generation_source,
          content_markdown,
          created_at
        FROM briefs
        WHERE user_id = ?
        ORDER BY COALESCE(window_end_utc, created_at) DESC, created_at DESC, id DESC
        "#,
    )
    .bind(&user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let mut release_ids_by_brief = HashMap::<String, Vec<String>>::new();
    if !rows.is_empty() {
        let membership_rows = sqlx::query_as::<_, BriefMembershipRow>(
            r#"
            SELECT m.brief_id, m.release_id
            FROM brief_release_memberships m
            JOIN briefs b ON b.id = m.brief_id
            WHERE b.user_id = ?
            ORDER BY m.brief_id ASC, m.ordinal ASC
            "#,
        )
        .bind(&user_id)
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
        for row in membership_rows {
            release_ids_by_brief
                .entry(row.brief_id)
                .or_default()
                .push(row.release_id.to_string());
        }
    }

    let items = rows
        .into_iter()
        .map(|r| {
            let release_ids = release_ids_by_brief.remove(&r.id).unwrap_or_else(|| {
                if brief_uses_markdown_release_fallback(&r.generation_source) {
                    extract_brief_release_ids(&r.content_markdown)
                        .into_iter()
                        .map(|value| value.to_string())
                        .collect()
                } else {
                    Vec::new()
                }
            });
            BriefItem {
                id: r.id,
                date: r.date,
                window_start: r.window_start_utc,
                window_end: r.window_end_utc,
                effective_time_zone: r.effective_time_zone,
                effective_local_boundary: r.effective_local_boundary,
                release_count: release_ids.len(),
                release_ids,
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

#[derive(Debug, Clone)]
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

async fn enqueue_singleton_or_stream_task(
    state: Arc<AppState>,
    mode: ReturnMode,
    new_task: jobs::NewTask,
) -> Result<Response, ApiError> {
    let task = jobs::enqueue_singleton_task_for_requester(state.as_ref(), new_task)
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

pub async fn task_events_sse(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let task_exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM job_tasks
        WHERE id = ? AND requested_by = ?
        "#,
    )
    .bind(task_id.as_str())
    .bind(user_id.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if task_exists == 0 {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "task not found",
        ));
    }
    Ok(jobs::task_sse_response(state, task_id))
}

async fn run_with_api_llm_context<F, T>(source: &str, requested_by: Option<String>, fut: F) -> T
where
    F: Future<Output = T>,
{
    ai::with_llm_call_context(
        ai::LlmCallContext {
            source: source.to_owned(),
            requested_by,
            parent_task_id: None,
            parent_task_type: None,
            parent_translation_batch_id: None,
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
        let res = sync::sync_starred(state.as_ref(), user_id.as_str())
            .await
            .map_err(ApiError::internal)?;
        return Ok(Json(res).into_response());
    }

    enqueue_singleton_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_SYNC_STARRED.to_owned(),
            payload: json!({ "user_id": user_id.clone() }),
            source: "api.sync_starred".to_owned(),
            requested_by: Some(user_id.clone()),
            parent_task_id: None,
        },
    )
    .await
}

pub async fn sync_all(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let mode = ReturnMode::from_query(&mode_query)?;

    if matches!(mode, ReturnMode::Sync) {
        let body = execute_sync_all_sync_with(
            state.as_ref(),
            user_id.as_str(),
            |state, user_id| Box::pin(sync::sync_starred(state, user_id)),
            |state, user_id| Box::pin(sync::sync_releases(state, user_id)),
            |state, user_id| {
                Box::pin(async move {
                    sync::sync_social_activity_best_effort(state, user_id, "api.sync_all.sync")
                        .await
                })
            },
            |state, user_id| Box::pin(sync::sync_notifications(state, user_id)),
        )
        .await;
        return Ok(Json(body?).into_response());
    }

    enqueue_singleton_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_SYNC_ACCESS_REFRESH.to_owned(),
            payload: json!({ "user_id": user_id.clone() }),
            source: "api.sync_all".to_owned(),
            requested_by: Some(user_id.clone()),
            parent_task_id: None,
        },
    )
    .await
}

type SyncAllApiFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;
type SyncAllSocialFuture<'a> =
    Pin<Box<dyn Future<Output = (sync::SyncSocialActivityResult, Option<String>)> + Send + 'a>>;

async fn execute_sync_all_sync_with<SyncStarred, SyncReleases, SyncSocial, SyncNotifications>(
    state: &AppState,
    user_id: &str,
    sync_starred: SyncStarred,
    sync_releases: SyncReleases,
    sync_social: SyncSocial,
    sync_notifications: SyncNotifications,
) -> Result<Value, ApiError>
where
    SyncStarred: for<'a> Fn(&'a AppState, &'a str) -> SyncAllApiFuture<'a, sync::SyncStarredResult>,
    SyncReleases:
        for<'a> Fn(&'a AppState, &'a str) -> SyncAllApiFuture<'a, sync::SyncReleasesResult>,
    SyncSocial: for<'a> Fn(&'a AppState, &'a str) -> SyncAllSocialFuture<'a>,
    SyncNotifications:
        for<'a> Fn(&'a AppState, &'a str) -> SyncAllApiFuture<'a, sync::SyncNotificationsResult>,
{
    let starred = sync_starred(state, user_id)
        .await
        .map_err(ApiError::internal)?;
    let releases = sync_releases(state, user_id)
        .await
        .map_err(ApiError::internal)?;
    let (social, social_error) = sync_social(state, user_id).await;
    let notifications = sync_notifications(state, user_id)
        .await
        .map_err(ApiError::internal)?;

    let mut body = json!({
        "starred": starred,
        "releases": releases,
        "social": social,
        "notifications": notifications,
    });
    if let Some(error) = social_error {
        body["social_error"] = Value::String(error);
    }
    Ok(body)
}

pub async fn sync_releases(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let mode = ReturnMode::from_query(&mode_query)?;

    if matches!(mode, ReturnMode::Sync) {
        let res = sync::sync_releases(state.as_ref(), user_id.as_str())
            .await
            .map_err(ApiError::internal)?;
        return Ok(Json(res).into_response());
    }

    enqueue_singleton_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_SYNC_RELEASES.to_owned(),
            payload: json!({ "user_id": user_id.clone() }),
            source: "api.sync_releases".to_owned(),
            requested_by: Some(user_id.clone()),
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
        let res = sync::sync_notifications(state.as_ref(), user_id.as_str())
            .await
            .map_err(ApiError::internal)?;
        return Ok(Json(res).into_response());
    }

    enqueue_singleton_or_stream_task(
        state,
        mode,
        jobs::NewTask {
            task_type: jobs::TASK_SYNC_NOTIFICATIONS.to_owned(),
            payload: json!({ "user_id": user_id.clone() }),
            source: "api.sync_notifications".to_owned(),
            requested_by: Some(user_id.clone()),
            parent_task_id: None,
        },
    )
    .await
}

#[derive(Debug, Serialize)]
pub struct BriefGenerateResponse {
    id: String,
    date: String,
    window_start: String,
    window_end: String,
    effective_time_zone: String,
    effective_local_boundary: String,
    release_count: usize,
    release_ids: Vec<String>,
    content_markdown: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct BriefGenerateRequest {
    date: Option<String>,
}

pub async fn generate_brief(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(mode_query): Query<ReturnModeQuery>,
    payload: Option<Json<BriefGenerateRequest>>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let mode = ReturnMode::from_query(&mode_query)?;
    let requested_date = payload.and_then(|Json(body)| body.date);
    let key_date = requested_date
        .as_deref()
        .map(|value| {
            chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| ApiError::bad_request("invalid date, expected YYYY-MM-DD"))
        })
        .transpose()?;

    if !matches!(mode, ReturnMode::Sync) {
        return enqueue_or_stream_task(
            state,
            mode,
            jobs::NewTask {
                task_type: jobs::TASK_BRIEF_GENERATE.to_owned(),
                payload: json!({
                    "user_id": user_id.clone(),
                    "key_date": key_date.map(|value| value.to_string()),
                }),
                source: "api.generate_brief".to_owned(),
                requested_by: Some(user_id.clone()),
                parent_task_id: None,
            },
        )
        .await;
    }

    let snapshot = if let Some(key_date) = key_date {
        run_with_api_llm_context(
            "api.generate_brief.sync",
            Some(user_id.clone()),
            ai::generate_daily_brief_snapshot_for_key_date(
                state.as_ref(),
                user_id.as_str(),
                key_date,
            ),
        )
        .await
        .map_err(ApiError::internal)?
    } else {
        run_with_api_llm_context(
            "api.generate_brief.sync",
            Some(user_id.clone()),
            ai::generate_daily_brief_snapshot_for_current(state.as_ref(), user_id.as_str()),
        )
        .await
        .map_err(ApiError::internal)?
    };

    Ok(Json(BriefGenerateResponse {
        id: snapshot.id,
        date: snapshot.date,
        window_start: snapshot.window_start,
        window_end: snapshot.window_end,
        effective_time_zone: snapshot.effective_time_zone,
        effective_local_boundary: snapshot.effective_local_boundary,
        release_count: snapshot.release_ids.len(),
        release_ids: snapshot
            .release_ids
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
        content_markdown: snapshot.content_markdown,
    })
    .into_response())
}

#[derive(Debug, Serialize)]
pub struct ReactionTokenCheckSummary {
    state: String, // idle | valid | invalid | error
    message: Option<String>,
    checked_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ReactionTokenOwnerSummary {
    github_connection_id: String,
    github_user_id: i64,
    login: String,
}

#[derive(Debug, Serialize)]
pub struct ReactionTokenStatusResponse {
    configured: bool,
    masked_token: Option<String>,
    check: ReactionTokenCheckSummary,
    owner: Option<ReactionTokenOwnerSummary>,
}

#[derive(Debug, Deserialize)]
pub struct ReactionTokenRequest {
    token: String,
}

#[derive(Debug, Serialize)]
pub struct ReactionTokenCheckResponse {
    state: String, // valid | invalid
    message: String,
    owner: Option<ReactionTokenOwnerSummary>,
}

#[derive(Debug, sqlx::FromRow)]
struct ReactionTokenStatusRow {
    masked_token: String,
    last_check_state: String,
    last_check_message: Option<String>,
    last_checked_at: Option<String>,
    owner_github_connection_id: Option<String>,
    owner_github_user_id: Option<i64>,
    owner_login: Option<String>,
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
    user_id: Option<&str>,
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
        let github_user =
            serde_json::from_str::<serde_json::Value>(&body).map_err(ApiError::internal)?;
        let github_user_id = github_user
            .get("id")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| ApiError::internal("github user id missing from PAT check"))?;
        let github_login = github_user
            .get("login")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ApiError::internal("github login missing from PAT check"))?
            .to_owned();
        let scopes = headers
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !scopes.is_empty() && !has_repo_scope(scopes) && !has_public_repo_scope(scopes) {
            return Ok(ReactionTokenCheckResponse {
                state: "invalid".to_owned(),
                message: "classic PAT needs public_repo (public) or repo (private)".to_owned(),
                owner: None,
            });
        }

        let owner = if let Some(user_id) = user_id {
            let owner = sqlx::query_as::<_, (String, i64, String)>(
                r#"
                SELECT id, github_user_id, login
                FROM github_connections
                WHERE user_id = ?
                  AND github_user_id = ?
                LIMIT 1
                "#,
            )
            .bind(user_id)
            .bind(github_user_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(ApiError::internal)?;

            let Some((github_connection_id, github_user_id, login)) = owner else {
                return Ok(ReactionTokenCheckResponse {
                    state: "invalid".to_owned(),
                    message:
                        "PAT owner is not bound to the current OctoRill account; bind that GitHub account first"
                            .to_owned(),
                    owner: None,
                });
            };
            Some(ReactionTokenOwnerSummary {
                github_connection_id,
                github_user_id,
                login,
            })
        } else {
            Some(ReactionTokenOwnerSummary {
                github_connection_id: String::new(),
                github_user_id,
                login: github_login,
            })
        };

        return Ok(ReactionTokenCheckResponse {
            state: "valid".to_owned(),
            message: "token is valid".to_owned(),
            owner,
        });
    }

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(ReactionTokenCheckResponse {
            state: "invalid".to_owned(),
            message: "token is invalid or expired".to_owned(),
            owner: None,
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
            owner: None,
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
    user_id: &str,
) -> Result<Option<ReactionTokenStatusRow>, ApiError> {
    sqlx::query_as::<_, ReactionTokenStatusRow>(
        r#"
        SELECT
          masked_token,
          last_check_state,
          last_check_message,
          last_checked_at,
          owner_github_connection_id,
          owner_github_user_id,
          owner_login
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
    user_id: &str,
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
    user_id: &str,
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
    let row = load_reaction_pat_status_row(state.as_ref(), &user_id).await?;
    let Some(row) = row else {
        return Ok(Json(ReactionTokenStatusResponse {
            configured: false,
            masked_token: None,
            check: ReactionTokenCheckSummary {
                state: "idle".to_owned(),
                message: None,
                checked_at: None,
            },
            owner: None,
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
        owner: match (
            row.owner_github_connection_id,
            row.owner_github_user_id,
            row.owner_login,
        ) {
            (Some(github_connection_id), Some(github_user_id), Some(login)) => {
                Some(ReactionTokenOwnerSummary {
                    github_connection_id,
                    github_user_id,
                    login,
                })
            }
            _ => None,
        },
    }))
}

pub async fn check_reaction_token(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<ReactionTokenRequest>,
) -> Result<Json<ReactionTokenCheckResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let checked =
        check_reaction_pat_with_github(state.as_ref(), req.token.as_str(), Some(user_id.as_str()))
            .await?;
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

    let checked =
        check_reaction_pat_with_github(state.as_ref(), token, Some(user_id.as_str())).await?;
    if checked.state != "valid" {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_invalid",
            checked.message,
        ));
    }
    let owner = checked.owner.clone().ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "pat_invalid",
            "PAT owner is not bound to the current OctoRill account",
        )
    })?;

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
          last_check_state, last_check_message, last_checked_at, updated_at,
          owner_github_connection_id, owner_github_user_id, owner_login
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
          token_ciphertext = excluded.token_ciphertext,
          token_nonce = excluded.token_nonce,
          masked_token = excluded.masked_token,
          last_check_state = excluded.last_check_state,
          last_check_message = excluded.last_check_message,
          last_checked_at = excluded.last_checked_at,
          updated_at = excluded.updated_at,
          owner_github_connection_id = excluded.owner_github_connection_id,
          owner_github_user_id = excluded.owner_github_user_id,
          owner_login = excluded.owner_login
        "#,
    )
    .bind(user_id.as_str())
    .bind(encrypted.ciphertext)
    .bind(encrypted.nonce)
    .bind(&masked)
    .bind("valid")
    .bind("token is valid")
    .bind(&now)
    .bind(&now)
    .bind(owner.github_connection_id.as_str())
    .bind(owner.github_user_id)
    .bind(owner.login.as_str())
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
        owner: Some(owner),
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

#[derive(Debug, Serialize, Clone)]
pub struct RepoVisual {
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: bool,
}

#[derive(Debug, Serialize)]
pub struct FeedActor {
    login: String,
    avatar_url: Option<String>,
    html_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FeedItem {
    kind: String,
    ts: String,
    id: String,
    repo_full_name: Option<String>,
    repo_visual: Option<RepoVisual>,
    title: Option<String>,
    body: Option<String>,
    body_truncated: bool,
    subtitle: Option<String>,
    reason: Option<String>,
    subject_type: Option<String>,
    html_url: Option<String>,
    unread: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<FeedActor>,
    translated: Option<TranslatedItem>,
    smart: Option<SmartItem>,
    reactions: Option<ReleaseReactions>,
}

#[derive(Debug, Serialize)]
pub struct TranslatedItem {
    lang: String,
    status: String, // ready | missing | disabled | error
    title: Option<String>,
    summary: Option<String>,
    error_code: Option<String>,
    error_summary: Option<String>,
    error_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_translate: Option<bool>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SmartItem {
    lang: String,
    status: String, // ready | missing | disabled | error | insufficient
    title: Option<String>,
    summary: Option<String>,
    error_code: Option<String>,
    error_summary: Option<String>,
    error_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_translate: Option<bool>,
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
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: Option<i64>,
    release_tag_name: Option<String>,
    release_previous_tag_name: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    reason: Option<String>,
    subject_type: Option<String>,
    html_url: Option<String>,
    unread: Option<i64>,
    actor_login: Option<String>,
    actor_avatar_url: Option<String>,
    actor_html_url: Option<String>,
    release_body: Option<String>,
    react_plus1: Option<i64>,
    react_laugh: Option<i64>,
    react_heart: Option<i64>,
    react_hooray: Option<i64>,
    react_rocket: Option<i64>,
    react_eyes: Option<i64>,
    trans_source_hash: Option<String>,
    trans_status: Option<String>,
    trans_title: Option<String>,
    trans_summary: Option<String>,
    trans_error_text: Option<String>,
    trans_work_status: Option<String>,
    detail_trans_source_hash: Option<String>,
    detail_trans_status: Option<String>,
    detail_trans_title: Option<String>,
    detail_trans_summary: Option<String>,
    detail_trans_error_text: Option<String>,
    detail_trans_work_status: Option<String>,
    smart_source_hash: Option<String>,
    smart_status: Option<String>,
    smart_title: Option<String>,
    smart_summary: Option<String>,
    smart_error_text: Option<String>,
    smart_work_status: Option<String>,
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

    let kind_rank = feed_kind_rank(kind.as_str())
        .ok_or_else(|| ApiError::bad_request("invalid cursor kind"))?;

    Ok((sort_ts, kind_rank, id_key))
}

fn feed_kind_rank(kind: &str) -> Option<i64> {
    match kind {
        "release" => Some(3),
        "repo_star_received" => Some(2),
        "follower_received" => Some(1),
        "notification" => Some(0),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct FeedTypeSelection {
    releases: bool,
    stars: bool,
    followers: bool,
}

impl FeedTypeSelection {
    fn all() -> Self {
        Self {
            releases: true,
            stars: true,
            followers: true,
        }
    }
}

fn parse_feed_types(types: Option<&str>) -> Result<FeedTypeSelection, ApiError> {
    let Some(types) = types else {
        return Ok(FeedTypeSelection::all());
    };

    let mut selection = FeedTypeSelection {
        releases: false,
        stars: false,
        followers: false,
    };
    let mut saw_any = false;
    for part in types.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        saw_any = true;
        match part {
            "releases" | "release" => selection.releases = true,
            "stars" | "star" => selection.stars = true,
            "followers" | "follower" => selection.followers = true,
            "notifications" | "notification" | "inbox" => {
                return Err(ApiError::bad_request(
                    "feed does not include inbox items; use /api/notifications for inbox",
                ));
            }
            _ => return Err(ApiError::bad_request(format!("invalid types: {part}"))),
        }
    }

    if !saw_any {
        return Ok(FeedTypeSelection::all());
    }

    Ok(selection)
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

fn normalize_visual_url(raw: Option<String>) -> Option<String> {
    raw.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn repo_visual_from_parts(
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: bool,
) -> Option<RepoVisual> {
    let owner_avatar_url = normalize_visual_url(owner_avatar_url);
    let open_graph_image_url = normalize_visual_url(open_graph_image_url);
    if owner_avatar_url.is_none() && open_graph_image_url.is_none() && !uses_custom_open_graph_image
    {
        return None;
    }

    Some(RepoVisual {
        owner_avatar_url,
        open_graph_image_url,
        uses_custom_open_graph_image,
    })
}

pub(crate) fn release_detail_source_hash(
    repo_full_name: &str,
    original_title: &str,
    original_body: &str,
) -> String {
    let normalized_body = original_body.replace("\r\n", "\n");
    let source = format!(
        "v=1\nkind=release_detail\nrepo={}\ntitle={}\nbody={}\n",
        repo_full_name.trim(),
        original_title.trim(),
        normalized_body.trim(),
    );
    ai::sha256_hex(&source)
}

pub(crate) const RELEASE_FEED_BODY_MAX_CHARS: usize = 3_000;

pub(crate) fn release_feed_body(body: Option<&str>) -> Option<String> {
    let normalized = body?.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(trimmed, RELEASE_FEED_BODY_MAX_CHARS).into_owned())
}

pub(crate) fn release_feed_translation_source_hash(
    repo_full_name: &str,
    title: &str,
    body: Option<&str>,
) -> String {
    ai::sha256_hex(&format!(
        "v=5\nkind=release\nrepo={}\ntitle={}\nbody={}\n",
        repo_full_name.trim(),
        title.trim(),
        body.unwrap_or("").trim(),
    ))
}

pub(crate) fn release_feed_body_is_over_limit(body: Option<&str>) -> bool {
    let Some(body) = body else {
        return false;
    };
    body.replace("\r\n", "\n").trim().chars().count() > RELEASE_FEED_BODY_MAX_CHARS
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

fn github_private_repo_scope_required_error() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "reauth_required",
        "private repository compare requires repo scope; re-login via GitHub OAuth",
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

#[cfg(test)]
pub(crate) fn release_excerpt(body: Option<&str>) -> Option<String> {
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
    kind_rank: i64,
    id_key: String,
}

fn parse_feed_cursor(cursor: &str) -> Result<StreamCursor, ApiError> {
    let (sort_ts, kind_rank, id_key) = parse_cursor(cursor)?;
    Ok(StreamCursor {
        sort_ts,
        kind_rank,
        id_key,
    })
}

async fn fetch_feed_items(
    state: &AppState,
    user_id: &str,
    cursor: Option<&StreamCursor>,
    types: FeedTypeSelection,
    limit: i64,
) -> Result<Vec<FeedRow>, ApiError> {
    let sql = r#"
        WITH items AS (
          SELECT
            'release' AS kind,
            3 AS kind_rank,
            sort_ts,
            ts,
            id_key,
            entity_id,
            release_id,
            release_node_id,
            repo_full_name,
            owner_avatar_url,
            open_graph_image_url,
            uses_custom_open_graph_image,
            release_tag_name,
            release_previous_tag_name,
            title,
            subtitle,
            reason,
            subject_type,
            html_url,
            unread,
            actor_login,
            actor_avatar_url,
            actor_html_url,
            release_body,
            react_plus1,
            react_laugh,
            react_heart,
            react_hooray,
            react_rocket,
            react_eyes
          FROM (
            SELECT
              COALESCE(r.published_at, r.created_at, r.updated_at) AS sort_ts,
              COALESCE(r.published_at, r.created_at, r.updated_at) AS ts,
              printf('%020d', r.release_id) AS id_key,
              CAST(r.release_id AS TEXT) AS entity_id,
              r.release_id AS release_id,
              r.node_id AS release_node_id,
              sr.full_name AS repo_full_name,
              sr.owner_avatar_url AS owner_avatar_url,
              sr.open_graph_image_url AS open_graph_image_url,
              sr.uses_custom_open_graph_image AS uses_custom_open_graph_image,
              r.tag_name AS release_tag_name,
              LAG(r.tag_name) OVER (
                PARTITION BY r.repo_id
                ORDER BY COALESCE(r.published_at, r.created_at, r.updated_at) ASC, r.release_id ASC
              ) AS release_previous_tag_name,
              COALESCE(NULLIF(TRIM(r.name), ''), r.tag_name) AS title,
              NULL AS subtitle,
              NULL AS reason,
              NULL AS subject_type,
              r.html_url AS html_url,
              NULL AS unread,
              NULL AS actor_login,
              NULL AS actor_avatar_url,
              NULL AS actor_html_url,
              r.body AS release_body,
              r.react_plus1 AS react_plus1,
              r.react_laugh AS react_laugh,
              r.react_heart AS react_heart,
              r.react_hooray AS react_hooray,
              r.react_rocket AS react_rocket,
              r.react_eyes AS react_eyes
            FROM repo_releases r
            JOIN user_release_visible_repos sr
              ON sr.user_id = ? AND sr.repo_id = r.repo_id
          )
          UNION ALL
          SELECT
            e.kind AS kind,
            CASE e.kind
              WHEN 'repo_star_received' THEN 2
              WHEN 'follower_received' THEN 1
              ELSE 0
            END AS kind_rank,
            e.occurred_at AS sort_ts,
            e.occurred_at AS ts,
            e.id AS id_key,
            e.id AS entity_id,
            NULL AS release_id,
            NULL AS release_node_id,
            e.repo_full_name AS repo_full_name,
            COALESCE(e.repo_owner_avatar_url, ob.owner_avatar_url) AS owner_avatar_url,
            COALESCE(e.repo_open_graph_image_url, ob.open_graph_image_url) AS open_graph_image_url,
            COALESCE(
              e.repo_uses_custom_open_graph_image,
              ob.uses_custom_open_graph_image
            ) AS uses_custom_open_graph_image,
            NULL AS release_tag_name,
            NULL AS release_previous_tag_name,
            NULL AS title,
            NULL AS subtitle,
            NULL AS reason,
            NULL AS subject_type,
            COALESCE(e.actor_html_url, 'https://github.com/' || e.actor_login) AS html_url,
            NULL AS unread,
            e.actor_login AS actor_login,
            e.actor_avatar_url AS actor_avatar_url,
            e.actor_html_url AS actor_html_url,
            NULL AS release_body,
            NULL AS react_plus1,
            NULL AS react_laugh,
            NULL AS react_heart,
            NULL AS react_hooray,
            NULL AS react_rocket,
            NULL AS react_eyes
          FROM social_activity_events e
          LEFT JOIN owned_repo_star_baselines ob
            ON ob.user_id = e.user_id AND ob.repo_id = e.repo_id
          WHERE e.user_id = ?
        )
        SELECT
          i.kind, i.sort_ts, i.ts, i.id_key, i.entity_id, i.release_id, i.release_node_id,
          i.repo_full_name, i.owner_avatar_url, i.open_graph_image_url, i.uses_custom_open_graph_image,
          i.release_tag_name, i.release_previous_tag_name,
          i.title, i.subtitle, i.reason, i.subject_type, i.html_url, i.unread,
          i.actor_login, i.actor_avatar_url, i.actor_html_url,
          i.release_body, i.react_plus1, i.react_laugh, i.react_heart, i.react_hooray, i.react_rocket, i.react_eyes,
          t.source_hash AS trans_source_hash,
          t.status AS trans_status,
          t.title AS trans_title,
          t.summary AS trans_summary,
          t.error_text AS trans_error_text,
          tw.status AS trans_work_status,
          dt.source_hash AS detail_trans_source_hash,
          dt.status AS detail_trans_status,
          dt.title AS detail_trans_title,
          dt.summary AS detail_trans_summary,
          dt.error_text AS detail_trans_error_text,
          dtw.status AS detail_trans_work_status,
          s.source_hash AS smart_source_hash,
          s.status AS smart_status,
          s.title AS smart_title,
          s.summary AS smart_summary,
          s.error_text AS smart_error_text,
          sw.status AS smart_work_status
        FROM items i
        LEFT JOIN ai_translations t
          ON t.user_id = ? AND t.entity_type = 'release' AND t.entity_id = i.entity_id AND t.lang = 'zh-CN' AND t.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN translation_work_items tw
          ON tw.id = t.active_work_item_id
        LEFT JOIN ai_translations dt
          ON dt.user_id = ? AND dt.entity_type = 'release_detail' AND dt.entity_id = i.entity_id AND dt.lang = 'zh-CN' AND dt.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN translation_work_items dtw
          ON dtw.id = dt.active_work_item_id
        LEFT JOIN ai_translations s
          ON s.user_id = ? AND s.entity_type = 'release_smart' AND s.entity_id = i.entity_id AND s.lang = 'zh-CN' AND s.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN translation_work_items sw
          ON sw.id = s.active_work_item_id
        WHERE (
          (? = 1 AND i.kind = 'release')
          OR (? = 1 AND i.kind = 'repo_star_received')
          OR (? = 1 AND i.kind = 'follower_received')
        )
          AND (
            ? = 0
            OR i.sort_ts < ?
            OR (i.sort_ts = ? AND i.kind_rank < ?)
            OR (i.sort_ts = ? AND i.kind_rank = ? AND i.id_key < ?)
          )
        ORDER BY i.sort_ts DESC, i.kind_rank DESC, i.id_key DESC
        LIMIT ?
    "#;

    let has_cursor = cursor.is_some();
    let cursor = cursor.cloned();

    let qy = sqlx::query_as::<_, FeedRow>(sql)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id);
    qy.bind(if types.releases { 1_i64 } else { 0_i64 })
        .bind(if types.stars { 1_i64 } else { 0_i64 })
        .bind(if types.followers { 1_i64 } else { 0_i64 })
        .bind(if has_cursor { 1_i64 } else { 0_i64 })
        .bind(cursor.as_ref().map(|c| c.sort_ts.as_str()))
        .bind(cursor.as_ref().map(|c| c.sort_ts.as_str()))
        .bind(cursor.as_ref().map(|c| c.kind_rank))
        .bind(cursor.as_ref().map(|c| c.sort_ts.as_str()))
        .bind(cursor.as_ref().map(|c| c.kind_rank))
        .bind(cursor.as_ref().map(|c| c.id_key.as_str()))
        .bind(limit)
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)
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
    release_id: i64,
    counts: &ReleaseReactionCounts,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE repo_releases
        SET react_plus1 = ?,
            react_laugh = ?,
            react_heart = ?,
            react_hooray = ?,
            react_rocket = ?,
            react_eyes = ?,
            updated_at = ?
        WHERE release_id = ?
        "#,
    )
    .bind(counts.plus1)
    .bind(counts.laugh)
    .bind(counts.heart)
    .bind(counts.hooray)
    .bind(counts.rocket)
    .bind(counts.eyes)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(release_id)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

const SMART_NO_VALUABLE_VERSION_INFO: &str = "no_valuable_version_info";
const RELEASE_FEED_MARKDOWN_MISMATCH_ERROR: &str =
    "release translation failed to preserve markdown structure";
const RELEASE_DETAIL_MARKDOWN_MISMATCH_ERROR: &str =
    "release detail translation failed to preserve markdown structure";

fn translation_error_metadata(
    error_text: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    let classified = crate::translations::classify_translation_error(error_text);
    (
        classified.as_ref().map(|value| value.code.to_owned()),
        classified.as_ref().map(|value| value.summary.to_owned()),
        classified.map(|value| value.detail),
    )
}

fn translated_item(
    status: &str,
    title: Option<String>,
    summary: Option<String>,
    auto_translate: Option<bool>,
    error_text: Option<&str>,
) -> TranslatedItem {
    let (error_code, error_summary, error_detail) = translation_error_metadata(error_text);
    TranslatedItem {
        lang: "zh-CN".to_owned(),
        status: status.to_owned(),
        title,
        summary,
        error_code,
        error_summary,
        error_detail,
        auto_translate,
    }
}

fn smart_item(
    status: &str,
    title: Option<String>,
    summary: Option<String>,
    auto_translate: Option<bool>,
    error_text: Option<&str>,
) -> SmartItem {
    let (error_code, error_summary, error_detail) = translation_error_metadata(error_text);
    SmartItem {
        lang: "zh-CN".to_owned(),
        status: status.to_owned(),
        title,
        summary,
        error_code,
        error_summary,
        error_detail,
        auto_translate,
    }
}

fn translated_terminal_item(status: &str, error_text: Option<&str>) -> Option<TranslatedItem> {
    match status {
        "disabled" => Some(translated_item("disabled", None, None, None, None)),
        "missing" | "error" => Some(translated_item(
            status,
            None,
            None,
            Some(false),
            if status == "error" { error_text } else { None },
        )),
        _ => None,
    }
}

fn translated_ready_item(
    raw_title: Option<String>,
    raw_summary: Option<String>,
    auto_translate: Option<bool>,
) -> Option<TranslatedItem> {
    let (title, summary) = normalize_translation_fields(raw_title, raw_summary);
    if title.is_none() && summary.is_none() {
        return None;
    }
    Some(translated_item(
        "ready",
        title,
        summary,
        auto_translate,
        None,
    ))
}

fn translated_release_feed_ready_item(
    raw_title: Option<String>,
    raw_summary: Option<String>,
    body: Option<&str>,
    auto_translate: Option<bool>,
) -> Option<TranslatedItem> {
    let (mut title, mut summary) = normalize_translation_fields(raw_title, raw_summary);
    let mut status = "ready";
    let mut error_text = None;
    if title.is_none() && summary.is_none() {
        status = "missing";
    }
    if status == "ready"
        && let (Some(src), Some(s)) = (body, summary.as_deref())
        && !markdown_structure_preserved(src, s)
    {
        status = "error";
        title = None;
        summary = None;
        error_text = Some(RELEASE_FEED_MARKDOWN_MISMATCH_ERROR);
    }

    Some(translated_item(
        status,
        title,
        summary,
        if status == "error" {
            Some(false)
        } else {
            auto_translate
        },
        error_text,
    ))
}

fn translated_missing_item(auto_translate: bool) -> TranslatedItem {
    translated_item(
        "missing",
        None,
        None,
        if auto_translate { None } else { Some(false) },
        None,
    )
}

fn smart_terminal_item(status: &str, error_text: Option<&str>) -> Option<SmartItem> {
    match status {
        "disabled" => Some(smart_item("disabled", None, None, None, None)),
        "missing" if error_text == Some(SMART_NO_VALUABLE_VERSION_INFO) => {
            Some(smart_item("insufficient", None, None, Some(false), None))
        }
        "error" if smart_error_is_retryable(error_text) => Some(smart_missing_item(Some(true))),
        "missing" | "error" => Some(smart_item(status, None, None, Some(false), error_text)),
        _ => None,
    }
}

fn smart_ready_item(
    raw_title: Option<String>,
    raw_summary: Option<String>,
    auto_translate: Option<bool>,
) -> Option<SmartItem> {
    let (title, summary) = normalize_translation_fields(raw_title, raw_summary);
    summary.as_ref()?;
    Some(smart_item("ready", title, summary, auto_translate, None))
}

fn smart_missing_item(auto_translate: Option<bool>) -> SmartItem {
    smart_item("missing", None, None, auto_translate, None)
}

fn smart_error_is_retryable(error_text: Option<&str>) -> bool {
    let Some(raw) = error_text else {
        return false;
    };
    let normalized = raw.trim().to_ascii_lowercase();
    normalized.contains("runtime_lease_expired")
        || normalized.contains("repo scope required; re-login via github oauth")
        || normalized.contains("database is locked")
        || normalized.contains("busy")
        || normalized.contains("timed out")
        || normalized.contains("timeout")
        || normalized.contains("temporarily unavailable")
        || normalized.contains("connection reset")
        || normalized.contains("connection refused")
}

fn feed_item_from_row(
    r: FeedRow,
    ai_enabled: bool,
    live_reactions: Option<&LiveReleaseReactions>,
) -> FeedItem {
    let actor = r.actor_login.as_ref().map(|login| FeedActor {
        login: login.clone(),
        avatar_url: r.actor_avatar_url.clone(),
        html_url: r.actor_html_url.clone(),
    });
    let repo_visual = repo_visual_from_parts(
        r.owner_avatar_url.clone(),
        r.open_graph_image_url.clone(),
        r.uses_custom_open_graph_image.unwrap_or(0) != 0,
    );

    if r.kind != "release" {
        return FeedItem {
            kind: r.kind,
            ts: r.ts,
            id: r.entity_id,
            repo_full_name: r.repo_full_name,
            repo_visual,
            title: r.title,
            body: None,
            body_truncated: false,
            subtitle: r.subtitle,
            reason: r.reason,
            subject_type: r.subject_type,
            html_url: r.html_url,
            unread: r.unread,
            actor,
            translated: None,
            smart: None,
            reactions: None,
        };
    }

    let (body, body_truncated) = match r.kind.as_str() {
        "release" => {
            let body = release_feed_body(r.release_body.as_deref());
            let body_truncated = release_feed_body_is_over_limit(r.release_body.as_deref());
            (body, body_truncated)
        }
        _ => (None, false),
    };

    let smart_current_hash = match r.kind.as_str() {
        "release" => crate::translations::release_smart_feed_source_hash(
            r.entity_id.as_str(),
            r.repo_full_name.as_deref().unwrap_or(""),
            r.title.as_deref().unwrap_or(""),
            body.as_deref(),
            r.release_tag_name.as_deref().unwrap_or(""),
            r.release_previous_tag_name.as_deref(),
        ),
        _ => String::new(),
    };

    let translated = if !ai_enabled {
        Some(translated_item("disabled", None, None, None, None))
    } else {
        let current_hash = release_feed_translation_source_hash(
            r.repo_full_name.as_deref().unwrap_or(""),
            r.title.as_deref().unwrap_or(""),
            body.as_deref(),
        );
        let detail_current_hash = release_detail_source_hash(
            r.repo_full_name.as_deref().unwrap_or(""),
            r.title.as_deref().unwrap_or(""),
            r.release_body.as_deref().unwrap_or(""),
        );
        let refresh_in_flight = r.trans_source_hash.as_deref() != Some(current_hash.as_str())
            && r.trans_status.as_deref() == Some("ready")
            && matches!(
                r.trans_work_status.as_deref(),
                Some("queued" | "batched" | "running")
            );
        let detail_refresh_in_flight = r.detail_trans_source_hash.as_deref()
            != Some(detail_current_hash.as_str())
            && r.detail_trans_status.as_deref() == Some("ready")
            && matches!(
                r.detail_trans_work_status.as_deref(),
                Some("queued" | "batched" | "running")
            );
        if r.detail_trans_source_hash.as_deref() == Some(detail_current_hash.as_str()) {
            if let Some(status) = r.detail_trans_status.as_deref()
                && status != "ready"
            {
                if !body_truncated
                    && matches!(status, "missing" | "error")
                    && r.trans_source_hash.as_deref() == Some(current_hash.as_str())
                    && r.trans_status.as_deref() == Some("ready")
                {
                    translated_release_feed_ready_item(
                        r.trans_title.clone(),
                        r.trans_summary.clone(),
                        body.as_deref(),
                        None,
                    )
                } else {
                    translated_terminal_item(status, r.detail_trans_error_text.as_deref())
                }
            } else {
                let (title, summary) = normalize_translation_fields(
                    r.detail_trans_title.clone(),
                    r.detail_trans_summary.clone(),
                );
                if !release_detail_translation_ready(r.release_body.as_deref(), summary.as_deref())
                {
                    if !body_truncated
                        && r.trans_source_hash.as_deref() == Some(current_hash.as_str())
                        && r.trans_status.as_deref() == Some("ready")
                    {
                        translated_release_feed_ready_item(
                            r.trans_title.clone(),
                            r.trans_summary.clone(),
                            body.as_deref(),
                            None,
                        )
                    } else {
                        Some(translated_item(
                            "error",
                            None,
                            None,
                            Some(false),
                            Some(RELEASE_DETAIL_MARKDOWN_MISMATCH_ERROR),
                        ))
                    }
                } else {
                    Some(translated_item("ready", title, summary, None, None))
                }
            }
        } else if detail_refresh_in_flight {
            translated_ready_item(
                r.detail_trans_title.clone(),
                r.detail_trans_summary.clone(),
                Some(true),
            )
            .or_else(|| Some(translated_missing_item(false)))
        } else if r.trans_source_hash.as_deref() == Some(current_hash.as_str()) {
            if let Some(status) = r.trans_status.as_deref()
                && status != "ready"
            {
                if matches!(status, "disabled" | "error") {
                    translated_terminal_item(status, r.trans_error_text.as_deref())
                } else {
                    Some(translated_missing_item(true))
                }
            } else {
                translated_release_feed_ready_item(
                    r.trans_title.clone(),
                    r.trans_summary.clone(),
                    body.as_deref(),
                    None,
                )
            }
        } else if refresh_in_flight {
            translated_ready_item(r.trans_title.clone(), r.trans_summary.clone(), Some(true))
                .or_else(|| Some(translated_missing_item(false)))
        } else {
            Some(translated_missing_item(true))
        }
    };

    let smart = if !ai_enabled {
        Some(smart_item("disabled", None, None, None, None))
    } else {
        let refresh_in_flight = r.smart_source_hash.as_deref() != Some(smart_current_hash.as_str())
            && r.smart_status.as_deref() == Some("ready")
            && matches!(
                r.smart_work_status.as_deref(),
                Some("queued" | "batched" | "running")
            );
        if r.smart_source_hash.as_deref() == Some(smart_current_hash.as_str()) {
            if let Some(status) = r.smart_status.as_deref()
                && status != "ready"
            {
                smart_terminal_item(status, r.smart_error_text.as_deref())
            } else {
                let ready = smart_ready_item(r.smart_title.clone(), r.smart_summary.clone(), None);
                if ready.is_none() {
                    Some(smart_missing_item(None))
                } else {
                    ready
                }
            }
        } else if refresh_in_flight {
            smart_ready_item(r.smart_title.clone(), r.smart_summary.clone(), Some(true))
                .or_else(|| Some(smart_missing_item(Some(false))))
        } else {
            Some(smart_missing_item(None))
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
        repo_visual,
        title: r.title,
        body,
        body_truncated,
        subtitle: r.subtitle,
        reason: r.reason,
        subject_type: r.subject_type,
        html_url: r.html_url,
        unread: r.unread,
        actor: None,
        translated,
        smart,
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
    let types = parse_feed_types(q.types.as_deref())?;

    let limit = q.limit.unwrap_or(30).clamp(1, 100);
    let cursor = q.cursor.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let feed_cursor = match cursor {
        Some(c) => Some(parse_feed_cursor(c)?),
        None => None,
    };

    let rows =
        fetch_feed_items(state.as_ref(), &user_id, feed_cursor.as_ref(), types, limit).await?;
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
    let reaction_pat = load_reaction_pat_token(state.as_ref(), &user_id)
        .await
        .ok()
        .flatten();
    if !node_ids.is_empty()
        && let Some(pat) = reaction_pat
        && let Ok(live) = fetch_live_release_reactions(state.as_ref(), &pat, &node_ids).await
    {
        for (node_id, reaction) in &live {
            if let Some(release_id) = release_by_node.get(node_id) {
                let _ =
                    persist_release_reaction_counts(state.as_ref(), *release_id, &reaction.counts)
                        .await;
            }
        }
        live_reactions_by_node = live;
    }

    let mut items = Vec::with_capacity(rows.len());
    let mut next_cursor: Option<String> = None;
    for (idx, r) in rows.into_iter().enumerate() {
        if idx == limit.saturating_sub(1) as usize {
            next_cursor = Some(format!("{}|{}|{}", r.sort_ts, r.kind, r.id_key));
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

    let token = match load_reaction_pat_token(state.as_ref(), &user_id).await {
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
                &user_id,
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
        SELECT rr.release_id, rr.node_id
        FROM repo_releases rr
        JOIN user_release_visible_repos sr
          ON sr.user_id = ? AND sr.repo_id = rr.repo_id
        WHERE rr.release_id = ?
        "#,
    )
    .bind(&user_id)
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
                    &user_id,
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
                    &user_id,
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
        persist_reaction_pat_check_result(state.as_ref(), &user_id, "valid", Some("PAT is valid"))
            .await;
    persist_release_reaction_counts(state.as_ref(), row.release_id, &updated.counts).await?;

    Ok(Json(ToggleReleaseReactionResponse {
        release_id: row.release_id.to_string(),
        reactions: ReleaseReactions {
            counts: updated.counts,
            viewer: updated.viewer,
            status: "ready".to_owned(),
        },
    }))
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TranslateReleaseRequest {
    release_id: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TranslateReleasesBatchRequest {
    release_ids: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TranslateReleaseDetailRequest {
    release_id: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TranslateReleaseDetailBatchRequest {
    release_ids: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TranslateNotificationRequest {
    thread_id: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TranslateNotificationsBatchRequest {
    thread_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslateResponse {
    pub lang: String,
    pub status: String, // ready | disabled | missing | error
    pub title: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslateBatchResponse {
    pub items: Vec<TranslateBatchItem>,
}

#[derive(Debug, Serialize, Clone)]
pub struct TranslateBatchItem {
    pub id: String,
    pub lang: String,
    pub status: String, // ready | disabled | missing | error | processing(stream)
    pub title: Option<String>,
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
struct TranslateBatchStreamEvent {
    event: &'static str, // item | done | error
    #[serde(skip_serializing_if = "Option::is_none")]
    item: Option<TranslateBatchItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[allow(dead_code)]
async fn send_batch_stream_event(
    tx: &mpsc::Sender<Result<Bytes, Infallible>>,
    mut event: TranslateBatchStreamEvent,
) -> bool {
    if let Some(item) = event.item.take() {
        event.item = Some(translate_batch_item_for_public(item));
    }
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

fn translate_batch_item_for_public(mut item: TranslateBatchItem) -> TranslateBatchItem {
    if item.status == "error" {
        item.error =
            crate::translations::translation_error_summary(item.error.as_deref()).or(item.error);
    }
    item
}

fn translate_batch_items_for_public(items: Vec<TranslateBatchItem>) -> Vec<TranslateBatchItem> {
    items
        .into_iter()
        .map(translate_batch_item_for_public)
        .collect()
}

#[allow(dead_code)]
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
                crate::translations::translation_error_summary(item.error.as_deref())
                    .or(item.error)
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
    let Some((_, body_with_closing)) = rest.split_once('\n') else {
        return trimmed;
    };
    let Some(closing_start) = body_with_closing.rfind("\n```") else {
        return trimmed;
    };
    let closing = &body_with_closing[closing_start + 1..];
    if closing.trim() != "```" {
        return trimmed;
    }
    &body_with_closing[..closing_start]
}

fn count_leading_newlines(value: &str) -> usize {
    value.chars().take_while(|ch| *ch == '\n').count()
}

fn count_trailing_newlines(value: &str) -> usize {
    value.chars().rev().take_while(|ch| *ch == '\n').count()
}

fn preserve_chunk_edge_newlines(source_chunk: &str, translated_chunk: String) -> String {
    let source_leading = count_leading_newlines(source_chunk);
    let source_trailing = count_trailing_newlines(source_chunk);
    let translated_leading = count_leading_newlines(&translated_chunk);
    let translated_trailing = count_trailing_newlines(&translated_chunk);
    let core_start = translated_leading.min(translated_chunk.len());
    let core_end = translated_chunk
        .len()
        .saturating_sub(translated_trailing)
        .max(core_start);
    let core = &translated_chunk[core_start..core_end];

    format!(
        "{}{}{}",
        "\n".repeat(source_leading),
        core,
        "\n".repeat(source_trailing)
    )
}

fn normalize_markdown_translation_output(source: &str, raw: String) -> String {
    let normalized = if markdown_structure_preserved(source, raw.as_str()) {
        raw
    } else {
        let stripped = strip_markdown_code_fence(raw.as_str());
        if stripped != raw.as_str() && markdown_structure_preserved(source, stripped) {
            stripped.to_owned()
        } else {
            raw
        }
    };
    preserve_chunk_edge_newlines(source, normalized)
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

const RELEASE_DETAIL_CHUNK_PROMPT_OVERHEAD_TOKENS: u32 = 320;

#[derive(Debug, Clone, Copy)]
struct ReleaseDetailChunkBudget {
    max_chars: usize,
    input_budget: u32,
    max_output_tokens: u32,
    model_input_limit: u32,
    fallback_source: &'static str,
}

async fn release_detail_chunk_budget(state: &AppState) -> ReleaseDetailChunkBudget {
    let budget_info = ai::compute_input_budget_with_source(state, 0).await;
    let io_budget = budget_info
        .input_budget
        .saturating_sub(RELEASE_DETAIL_CHUNK_PROMPT_OVERHEAD_TOKENS)
        .max(2);
    let input_budget = (io_budget / 2).max(1);
    let max_output_tokens = io_budget.saturating_sub(input_budget).max(1);

    ReleaseDetailChunkBudget {
        max_chars: usize::try_from(input_budget.saturating_mul(4)).unwrap_or(usize::MAX),
        input_budget,
        max_output_tokens,
        model_input_limit: budget_info.model_input_limit,
        fallback_source: budget_info.fallback_source,
    }
}

#[cfg(test)]
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
    user_id: &str,
    requested_at: &str,
    t: TranslationUpsert<'_>,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO ai_translations (
          id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary,
          error_text, active_work_item_id, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, 'ready', ?, ?, NULL, NULL, ?, ?)
        ON CONFLICT(user_id, entity_type, entity_id, lang) DO UPDATE SET
          source_hash = excluded.source_hash,
          status = excluded.status,
          title = excluded.title,
          summary = excluded.summary,
          error_text = excluded.error_text,
          active_work_item_id = excluded.active_work_item_id,
          updated_at = excluded.updated_at
        WHERE ai_translations.source_hash = excluded.source_hash
           OR ai_translations.updated_at <= ?
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(user_id)
    .bind(t.entity_type)
    .bind(t.entity_id)
    .bind(t.lang)
    .bind(t.source_hash)
    .bind(t.title)
    .bind(t.summary)
    .bind(&now)
    .bind(&now)
    .bind(requested_at)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn mark_translation_requested(
    state: &AppState,
    user_id: &str,
    requested_at: &str,
    t: TranslationUpsert<'_>,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        INSERT INTO ai_translations (
          id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary,
          error_text, active_work_item_id, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, 'running', NULL, NULL, NULL, NULL, ?, ?)
        ON CONFLICT(user_id, entity_type, entity_id, lang) DO UPDATE SET
          source_hash = excluded.source_hash,
          status = excluded.status,
          title = NULL,
          summary = NULL,
          error_text = NULL,
          active_work_item_id = NULL,
          updated_at = excluded.updated_at
        WHERE ai_translations.source_hash = excluded.source_hash
          AND ai_translations.status NOT IN ('ready', 'disabled', 'missing')
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(user_id)
    .bind(t.entity_type)
    .bind(t.entity_id)
    .bind(t.lang)
    .bind(t.source_hash)
    .bind(requested_at)
    .bind(requested_at)
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
    body: String,
    source_hash: String,
    legacy_source_hash: Option<String>,
}

#[derive(Debug, Clone)]
struct ReleaseBatchTerminalState {
    status: String,
    error: Option<String>,
}

const RELEASE_BATCH_MAX_TOKENS: u32 = 1_400;
const RELEASE_BATCH_OVERHEAD_TOKENS: u32 = 260;
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
    status: String,
    title: Option<String>,
    summary: Option<String>,
    error_text: Option<String>,
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

#[cfg(test)]
fn release_cache_entry_reusable(cache: &TranslationCacheRow, body: &str) -> bool {
    if cache.summary.as_deref().is_some_and(looks_like_json_blob) {
        return false;
    }
    let summary_is_usable = cache
        .summary
        .as_deref()
        .is_some_and(|s| markdown_structure_preserved(body, s));
    let title_only_cache = cache.summary.is_none() && cache.title.is_some();
    summary_is_usable || title_only_cache
}

fn build_release_batch_prompt(batch: &[ReleaseBatchCandidate]) -> String {
    let mut prompt = String::from(
        "你会收到多条 GitHub Release，请逐条翻译标题与 Markdown 正文。\n输出严格 JSON（不要 markdown code block）：\n{\"items\":[{\"release_id\":123,\"title_zh\":\"...\",\"summary_md\":\"...\"}]}\n要求：\n1) 每个输入 release_id 必须在输出里出现；\n2) title_zh 要自然简洁；\n3) summary_md 必须保留原 Markdown 结构与代码/列表/标题层级，不要补充 URL 或额外事实；\n4) 若正文为空，可返回空字符串作为 summary_md。\n",
    );
    for item in batch {
        prompt.push_str(
            "
---
",
        );
        prompt.push_str(&format!(
            "release_id: {}
repo: {}
title: {}
body_markdown:
{}
",
            item.release_id, item.full_name, item.title, item.body,
        ));
    }
    prompt
}

fn estimate_release_batch_candidate_tokens(item: &ReleaseBatchCandidate) -> u32 {
    ai::estimate_text_tokens(item.full_name.as_str())
        .saturating_add(ai::estimate_text_tokens(item.title.as_str()))
        .saturating_add(ai::estimate_text_tokens(item.body.as_str()))
        .saturating_add(48)
}

fn release_candidate_can_batch(
    item: &ReleaseBatchCandidate,
    chunk_char_budget: usize,
    batch_input_budget: u32,
) -> bool {
    split_markdown_chunks(item.body.as_str(), chunk_char_budget).len() <= 1
        && estimate_release_batch_candidate_tokens(item)
            .saturating_add(RELEASE_BATCH_OVERHEAD_TOKENS)
            <= batch_input_budget
}

async fn translate_pending_release_batch_candidates(
    state: &AppState,
    user_id: &str,
    pending: &[ReleaseBatchCandidate],
) -> Result<Vec<TranslateBatchItem>, ApiError> {
    if pending.is_empty() {
        return Ok(Vec::new());
    }

    let batch_budget = ai::compute_input_budget_with_source(state, RELEASE_BATCH_MAX_TOKENS).await;
    let chunk_budget = release_detail_chunk_budget(state).await;
    let mut batchable = Vec::new();
    let mut fallback = HashMap::<i64, ReleaseBatchCandidate>::new();
    for candidate in pending {
        if release_candidate_can_batch(candidate, chunk_budget.max_chars, batch_budget.input_budget)
        {
            batchable.push(candidate.clone());
        } else {
            fallback.insert(candidate.release_id, candidate.clone());
        }
    }

    let estimated = batchable
        .iter()
        .map(estimate_release_batch_candidate_tokens)
        .collect::<Vec<_>>();
    let groups = ai::pack_batch_indices(
        &estimated,
        batch_budget.input_budget,
        RELEASE_BATCH_OVERHEAD_TOKENS,
    );
    if !batchable.is_empty() {
        let split_count = groups.len().saturating_sub(1);
        let saved_calls = batchable.len().saturating_sub(groups.len());
        let estimated_tokens = estimated.iter().copied().sum::<u32>();
        tracing::info!(
            batch_size = batchable.len(),
            estimated_tokens,
            split_count,
            saved_calls,
            fallback_source = batch_budget.fallback_source,
            input_budget = batch_budget.input_budget,
            model_input_limit = batch_budget.model_input_limit,
            "release detail batch plan"
        );
    }

    let mut translated = HashMap::<i64, (Option<String>, Option<String>)>::new();
    let mut non_retryable_error_text: Option<String> = None;
    let mut abort_remaining_batches = false;
    for batch_indices in groups {
        if abort_remaining_batches {
            break;
        }
        let batch = batch_indices
            .iter()
            .map(|idx| batchable[*idx].clone())
            .collect::<Vec<_>>();
        let prompt = build_release_batch_prompt(&batch);
        let raw = ai::chat_completion(
            state,
            "你是一个批量翻译助手，负责把 GitHub Release 标题与 Markdown 正文翻译成自然中文。",
            &prompt,
            RELEASE_BATCH_MAX_TOKENS,
        )
        .await;

        match raw {
            Ok(raw) => {
                if let Some(payload) = parse_batch_release_translation_payload(&raw) {
                    for item in payload.items {
                        let Some(candidate) = batch
                            .iter()
                            .find(|candidate| candidate.release_id == item.release_id)
                        else {
                            continue;
                        };
                        let (title, summary) =
                            normalize_translation_fields(item.title_zh, item.summary_md);
                        let summary = summary.map(|value| {
                            normalize_markdown_translation_output(candidate.body.as_str(), value)
                        });
                        let markdown_ok = candidate.body.trim().is_empty()
                            || summary.as_deref().is_some_and(|value| {
                                markdown_structure_preserved(candidate.body.as_str(), value)
                            });
                        if (title.is_some() || summary.is_some())
                            && release_detail_translation_ready(
                                Some(candidate.body.as_str()),
                                summary.as_deref(),
                            )
                            && markdown_ok
                        {
                            translated.insert(candidate.release_id, (title, summary));
                        }
                    }
                } else {
                    tracing::warn!(
                        "release detail batch translation response parse failed; fallback to single"
                    );
                }
            }
            Err(err) => {
                if ai_error_is_non_retryable(&err) {
                    abort_remaining_batches = true;
                    non_retryable_error_text = Some(err.to_string());
                    tracing::warn!(
                        ?err,
                        "release detail batch translation upstream error is non-retryable; skipping remaining batch calls"
                    );
                } else {
                    tracing::warn!(
                        ?err,
                        "release detail batch translation failed; fallback to single"
                    );
                }
            }
        }

        for candidate in &batch {
            if !translated.contains_key(&candidate.release_id) {
                fallback.insert(candidate.release_id, candidate.clone());
            }
        }
    }

    let requested_at = chrono::Utc::now().to_rfc3339();
    let mut items = Vec::with_capacity(pending.len());
    for candidate in pending {
        if let Some((title, summary)) = translated.get(&candidate.release_id).cloned() {
            upsert_translation(
                state,
                user_id,
                requested_at.as_str(),
                TranslationUpsert {
                    entity_type: "release_detail",
                    entity_id: &candidate.entity_id,
                    lang: "zh-CN",
                    source_hash: &candidate.source_hash,
                    title: title.as_deref(),
                    summary: summary.as_deref(),
                },
            )
            .await?;
            items.push(TranslateBatchItem {
                id: candidate.release_id.to_string(),
                lang: "zh-CN".to_owned(),
                status: "ready".to_owned(),
                title,
                summary,
                error: None,
            });
            continue;
        }

        if let Some(error_text) = non_retryable_error_text.as_ref() {
            upsert_translation_terminal_status(
                state,
                user_id,
                requested_at.as_str(),
                TranslationUpsert {
                    entity_type: "release_detail",
                    entity_id: &candidate.entity_id,
                    lang: "zh-CN",
                    source_hash: &candidate.source_hash,
                    title: None,
                    summary: None,
                },
                "error",
                Some(error_text.as_str()),
            )
            .await?;
            items.push(TranslateBatchItem {
                id: candidate.release_id.to_string(),
                lang: "zh-CN".to_owned(),
                status: "error".to_owned(),
                title: None,
                summary: None,
                error: Some(error_text.clone()),
            });
            continue;
        }

        match translate_release_detail_internal(state, user_id, candidate.release_id).await {
            Ok(translated) => items.push(TranslateBatchItem {
                id: candidate.release_id.to_string(),
                lang: translated.lang,
                status: translated.status,
                title: translated.title,
                summary: translated.summary,
                error: None,
            }),
            Err(err) if err.code() == "not_found" => items.push(TranslateBatchItem {
                id: candidate.release_id.to_string(),
                lang: "zh-CN".to_owned(),
                status: "missing".to_owned(),
                title: None,
                summary: None,
                error: Some("release not found".to_owned()),
            }),
            Err(err) => {
                let error_text = err.to_string();
                tracing::warn!(
                    release_id = candidate.release_id,
                    error_code = err.code(),
                    "release detail translation failed inside batch"
                );
                items.push(TranslateBatchItem {
                    id: candidate.release_id.to_string(),
                    lang: "zh-CN".to_owned(),
                    status: "error".to_owned(),
                    title: None,
                    summary: None,
                    error: Some(error_text),
                });
            }
        }
    }

    Ok(items)
}

#[derive(Debug)]
struct PreparedReleaseBatch {
    detail_pending_candidates: Vec<ReleaseBatchCandidate>,
    translated: HashMap<i64, (Option<String>, Option<String>)>,
    terminal: HashMap<i64, ReleaseBatchTerminalState>,
    missing: HashSet<i64>,
}

async fn prepare_release_batch(
    state: &AppState,
    user_id: &str,
    release_ids: &[i64],
) -> Result<PreparedReleaseBatch, ApiError> {
    if state.config.ai.is_none() {
        return Ok(PreparedReleaseBatch {
            detail_pending_candidates: Vec::new(),
            translated: HashMap::new(),
            terminal: HashMap::new(),
            missing: HashSet::new(),
        });
    }

    let mut source_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"
        SELECT r.release_id, sr.full_name, r.tag_name, r.name, r.body
        FROM repo_releases r
        JOIN user_release_visible_repos sr
          ON sr.user_id = "#,
    );
    source_query.push_bind(user_id);
    source_query.push(" AND sr.repo_id = r.repo_id AND r.release_id IN (");
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
    let mut terminal = HashMap::<i64, ReleaseBatchTerminalState>::new();
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
        let body = row
            .body
            .as_deref()
            .map(|value| value.replace("\r\n", "\n"))
            .map(|value| value.trim().to_owned())
            .unwrap_or_default();
        let legacy_body = release_feed_body(Some(body.as_str()));
        let legacy_source_hash =
            (!release_feed_body_is_over_limit(Some(body.as_str()))).then(|| {
                release_feed_translation_source_hash(
                    row.full_name.as_str(),
                    title.as_str(),
                    legacy_body.as_deref(),
                )
            });
        let candidate = ReleaseBatchCandidate {
            release_id: *release_id,
            entity_id: release_id.to_string(),
            full_name: row.full_name.trim().to_owned(),
            title: title.trim().to_owned(),
            body: body.clone(),
            source_hash: release_detail_source_hash(
                row.full_name.as_str(),
                title.as_str(),
                body.as_str(),
            ),
            legacy_source_hash,
        };
        candidates.push(candidate);
    }

    let mut detail_cache_by_entity: HashMap<String, TranslationCacheRow> = HashMap::new();
    if !candidates.is_empty() {
        let mut cache_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            SELECT entity_id, source_hash, status, title, summary, error_text
            FROM ai_translations
            WHERE user_id = "#,
        );
        cache_query.push_bind(user_id);
        cache_query.push(" AND entity_type = 'release_detail' AND lang = 'zh-CN' AND status IN ('ready', 'disabled', 'missing', 'error') AND entity_id IN (");
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
            detail_cache_by_entity.insert(row.entity_id.clone(), row);
        }
    }

    let mut legacy_cache_by_entity: HashMap<String, TranslationCacheRow> = HashMap::new();
    if candidates
        .iter()
        .any(|candidate| candidate.legacy_source_hash.is_some())
    {
        let mut cache_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            SELECT entity_id, source_hash, status, title, summary, error_text
            FROM ai_translations
            WHERE user_id = "#,
        );
        cache_query.push_bind(user_id);
        cache_query.push(" AND entity_type = 'release' AND lang = 'zh-CN' AND status IN ('ready', 'disabled', 'missing', 'error') AND entity_id IN (");
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
            legacy_cache_by_entity.insert(row.entity_id.clone(), row);
        }
    }

    let mut detail_pending_candidates = Vec::new();
    let mut translated = HashMap::<i64, (Option<String>, Option<String>)>::new();

    for item in &candidates {
        if let Some(cache) = detail_cache_by_entity.get(&item.entity_id)
            && cache.source_hash == item.source_hash
        {
            if matches!(cache.status.as_str(), "disabled" | "missing" | "error") {
                terminal.insert(
                    item.release_id,
                    ReleaseBatchTerminalState {
                        status: cache.status.clone(),
                        error: cache.error_text.clone(),
                    },
                );
                continue;
            }
            let (title, summary) =
                normalize_translation_fields(cache.title.clone(), cache.summary.clone());
            if release_detail_translation_ready(Some(item.body.as_str()), summary.as_deref()) {
                translated.insert(item.release_id, (title, summary));
                continue;
            }
        }
        if let Some(legacy_source_hash) = item.legacy_source_hash.as_deref()
            && let Some(cache) = legacy_cache_by_entity.get(&item.entity_id)
            && cache.source_hash == legacy_source_hash
        {
            if cache.status == "disabled" {
                terminal.insert(
                    item.release_id,
                    ReleaseBatchTerminalState {
                        status: cache.status.clone(),
                        error: cache.error_text.clone(),
                    },
                );
                continue;
            }
            if cache.status == "ready" {
                let (title, summary) =
                    normalize_translation_fields(cache.title.clone(), cache.summary.clone());
                if summary
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
                {
                    translated.insert(item.release_id, (title, summary));
                    continue;
                }
            }
        }
        detail_pending_candidates.push(item.clone());
    }

    Ok(PreparedReleaseBatch {
        detail_pending_candidates,
        translated,
        terminal,
        missing,
    })
}

fn build_release_batch_item(
    release_id: i64,
    missing: &HashSet<i64>,
    terminal: &HashMap<i64, ReleaseBatchTerminalState>,
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

    if let Some(terminal_state) = terminal.get(&release_id) {
        return TranslateBatchItem {
            id: release_id.to_string(),
            lang: "zh-CN".to_owned(),
            status: terminal_state.status.clone(),
            title: None,
            summary: None,
            error: terminal_state.error.clone().or_else(|| {
                (terminal_state.status == "missing")
                    .then_some("translation result missing".to_owned())
            }),
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

async fn upsert_translation_terminal_status(
    state: &AppState,
    user_id: &str,
    requested_at: &str,
    t: TranslationUpsert<'_>,
    status: &str,
    error_text: Option<&str>,
) -> Result<(), ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO ai_translations (
          id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary,
          error_text, active_work_item_id, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?, NULL, ?, ?)
        ON CONFLICT(user_id, entity_type, entity_id, lang) DO UPDATE SET
          source_hash = excluded.source_hash,
          status = excluded.status,
          title = excluded.title,
          summary = excluded.summary,
          error_text = excluded.error_text,
          active_work_item_id = excluded.active_work_item_id,
          updated_at = excluded.updated_at
        WHERE ai_translations.source_hash = excluded.source_hash
           OR ai_translations.updated_at <= ?
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(user_id)
    .bind(t.entity_type)
    .bind(t.entity_id)
    .bind(t.lang)
    .bind(t.source_hash)
    .bind(status)
    .bind(error_text)
    .bind(&now)
    .bind(&now)
    .bind(requested_at)
    .execute(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn translate_releases_batch_internal(
    state: &AppState,
    user_id: &str,
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
    if !prepared.detail_pending_candidates.is_empty() {
        for item in translate_pending_release_batch_candidates(
            state,
            user_id,
            &prepared.detail_pending_candidates,
        )
        .await?
        {
            let Ok(release_id) = item.id.parse::<i64>() else {
                continue;
            };
            match item.status.as_str() {
                "ready" => {
                    prepared
                        .translated
                        .insert(release_id, (item.title, item.summary));
                }
                "disabled" | "missing" | "error" => {
                    prepared.terminal.insert(
                        release_id,
                        ReleaseBatchTerminalState {
                            status: item.status,
                            error: item.error,
                        },
                    );
                }
                _ => {}
            }
        }
    }
    Ok(release_ids
        .iter()
        .map(|release_id| {
            build_release_batch_item(
                *release_id,
                &prepared.missing,
                &prepared.terminal,
                &prepared.translated,
            )
        })
        .collect())
}

pub async fn translate_releases_batch_for_user(
    state: &AppState,
    user_id: &str,
    release_ids: &[i64],
) -> Result<TranslateBatchResponse, ApiError> {
    let items = translate_releases_batch_internal(state, user_id, release_ids).await?;
    Ok(TranslateBatchResponse { items })
}

#[derive(Debug, Clone)]
struct ReleaseSmartBatchCandidate {
    release_id: i64,
    entity_id: String,
    full_name: String,
    tag_name: String,
    previous_tag_name: Option<String>,
    title: String,
    body: String,
    source_hash: String,
}

#[derive(Debug)]
struct PreparedReleaseSmartBatch {
    candidates: Vec<ReleaseSmartBatchCandidate>,
    pending: Vec<ReleaseSmartBatchCandidate>,
    smart: HashMap<i64, (Option<String>, Option<String>)>,
    terminal: HashMap<i64, ReleaseBatchTerminalState>,
    missing: HashSet<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct ReleaseSmartBatchSourceRow {
    release_id: i64,
    full_name: String,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    previous_tag_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseSmartSummaryPayload {
    valuable: bool,
    title_zh: Option<String>,
    summary_bullets: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubCompareResponse {
    status: Option<String>,
    ahead_by: Option<i64>,
    behind_by: Option<i64>,
    total_commits: Option<i64>,
    commits: Vec<GitHubCompareCommit>,
    files: Vec<GitHubCompareFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubCompareCommit {
    sha: String,
    commit: GitHubCompareCommitDetail,
}

#[derive(Debug, Deserialize)]
struct GitHubCompareCommitDetail {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubCompareFile {
    filename: String,
    status: Option<String>,
    additions: Option<i64>,
    deletions: Option<i64>,
    changes: Option<i64>,
    patch: Option<String>,
}

fn value_as_bool(raw: &serde_json::Value) -> Option<bool> {
    raw.as_bool().or_else(|| match raw.as_str()?.trim() {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    })
}

fn sanitize_smart_bullet_text(raw: &str) -> String {
    let normalized = raw.replace(['\r', '\n'], " ");
    let mut out = Vec::new();
    for token in normalized.split_whitespace() {
        let candidate = token.trim_matches(|c: char| {
            matches!(
                c,
                ')' | '(' | '[' | ']' | '<' | '>' | ',' | ';' | '"' | '\'' | '.'
            )
        });
        if candidate.starts_with("https://") || candidate.starts_with("http://") {
            continue;
        }
        out.push(token.to_owned());
    }
    out.join(" ").trim().to_owned()
}

fn render_smart_summary_markdown(bullets: &[String]) -> Option<String> {
    let lines = bullets
        .iter()
        .map(|bullet| sanitize_smart_bullet_text(bullet))
        .map(|bullet| bullet.trim().to_owned())
        .filter(|bullet| !bullet.is_empty())
        .take(4)
        .map(|bullet| format!("- {bullet}"))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn parse_release_smart_summary_payload(raw: &str) -> Option<ReleaseSmartSummaryPayload> {
    let value = parse_json_value_relaxed(raw)?;
    let obj = value.as_object()?;
    let valuable = obj.get("valuable").and_then(value_as_bool)?;
    let title_zh = object_field_as_string(obj, &["title_zh", "title", "title_cn"]);
    let summary_bullets = obj
        .get("summary_bullets")
        .or_else(|| obj.get("bullets"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(value_as_id_string)
                .map(|bullet| sanitize_smart_bullet_text(bullet.as_str()))
                .filter(|bullet| !bullet.is_empty())
                .take(4)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(ReleaseSmartSummaryPayload {
        valuable,
        title_zh,
        summary_bullets,
    })
}

fn release_smart_body_prompt(item: &ReleaseSmartBatchCandidate) -> String {
    format!(
        "Repo: {repo}\nRelease title: {title}\nRelease tag: {tag}\nSource: release body\n\nRelease body:\n{body}\n\n请判断这段 GitHub Release 正文是否足以生成“方便了解项目版本变化”的中文要点卡片。\n输出严格 JSON（不要 markdown code block）：\n{{\"valuable\":true,\"title_zh\":\"...\",\"summary_bullets\":[\"...\",\"...\"]}}\n\n硬性要求：\n1) 只能根据输入证据，不得脑补；\n2) valuable=true 时，summary_bullets 必须是 1-4 条简洁中文要点，聚焦真实版本变化；\n3) valuable=false 时，summary_bullets 必须是空数组，title_zh 可为 null；\n4) 若正文只有模板、链接、空话、营销句、版本占位或“查看 commits”等无实质变更说明，必须返回 valuable=false；\n5) 不输出 URL，不写长段落，不逐句直译原文。",
        repo = item.full_name,
        title = item.title,
        tag = item.tag_name,
        body = item.body,
    )
}

fn release_smart_diff_prompt(
    item: &ReleaseSmartBatchCandidate,
    compare_range: &str,
    digest: &str,
) -> String {
    format!(
        "Repo: {repo}\nRelease title: {title}\nRelease tag: {tag}\nCompare range: {compare_range}\nSource: compare digest\n\n说明：release body 已经被判定为不足以支撑版本变化摘要，请只根据下列 compare 摘要判断是否能提炼出对人类有用的版本变化。\n\nCompare digest:\n{digest}\n\n输出严格 JSON（不要 markdown code block）：\n{{\"valuable\":true,\"title_zh\":\"...\",\"summary_bullets\":[\"...\",\"...\"]}}\n\n硬性要求：\n1) 只能依据给定 compare digest，总结 1-4 条中文要点；\n2) 优先总结功能、修复、性能、兼容性、运维/构建变更等可读变化；\n3) 若 digest 只体现 lockfile、minified、generated、版本号或噪声文件，且无法说明实际版本变化，必须返回 valuable=false；\n4) valuable=false 时 summary_bullets 必须为空数组；\n5) 不输出 URL，不得臆测未提供的行为影响。",
        repo = item.full_name,
        title = item.title,
        tag = item.tag_name,
        compare_range = compare_range,
        digest = digest,
    )
}

fn compare_file_is_noise(file: &GitHubCompareFile) -> bool {
    let name = file.filename.to_ascii_lowercase();
    let lockfiles = [
        "pnpm-lock.yaml",
        "package-lock.json",
        "yarn.lock",
        "cargo.lock",
        "composer.lock",
        "gemfile.lock",
        "go.sum",
        "poetry.lock",
        "pipfile.lock",
        "bun.lock",
        "bun.lockb",
        "uv.lock",
    ];
    if lockfiles.iter().any(|suffix| name.ends_with(suffix)) {
        return true;
    }
    if name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || name.ends_with(".map")
        || name.contains("/dist/")
        || name.contains("/build/")
        || name.contains("/generated/")
        || name.contains(".generated.")
    {
        return true;
    }
    let binary_exts = [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".pdf", ".zip", ".gz", ".tar", ".jar",
        ".wasm", ".woff", ".woff2", ".ttf", ".otf", ".mp4", ".mov",
    ];
    binary_exts.iter().any(|suffix| name.ends_with(suffix))
        || file.patch.is_none() && name.ends_with(".svg")
}

fn summarize_patch_excerpt(patch: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in patch.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("@@") || trimmed.is_empty() {
            continue;
        }
        if !(trimmed.starts_with('+') || trimmed.starts_with('-')) {
            continue;
        }
        if trimmed.starts_with("+++") || trimmed.starts_with("---") {
            continue;
        }
        lines.push(trimmed.to_owned());
        if lines.len() >= 12 {
            break;
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(truncate_chars(&lines.join("\n"), 800).into_owned())
    }
}

fn build_compare_digest(compare: &GitHubCompareResponse) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(status) = compare.status.as_deref() {
        lines.push(format!("compare_status: {status}"));
    }
    if let Some(ahead_by) = compare.ahead_by {
        lines.push(format!("ahead_by: {ahead_by}"));
    }
    if let Some(behind_by) = compare.behind_by {
        lines.push(format!("behind_by: {behind_by}"));
    }
    if let Some(total_commits) = compare.total_commits {
        lines.push(format!("total_commits: {total_commits}"));
    }

    let commit_lines = compare
        .commits
        .iter()
        .filter_map(|commit| {
            let subject = commit.commit.message.lines().next()?.trim();
            if subject.is_empty() {
                return None;
            }
            Some(format!(
                "- {}: {}",
                &commit.sha.chars().take(7).collect::<String>(),
                truncate_chars(subject, 140).into_owned()
            ))
        })
        .take(12)
        .collect::<Vec<_>>();
    if !commit_lines.is_empty() {
        lines.push("commit_subjects:".to_owned());
        lines.extend(commit_lines);
    }

    let mut file_lines = Vec::new();
    let mut excerpt_lines = Vec::new();
    for file in compare
        .files
        .iter()
        .filter(|file| !compare_file_is_noise(file))
        .take(8)
    {
        file_lines.push(format!(
            "- {} [{}] (+{} / -{} / Δ{})",
            file.filename,
            file.status.as_deref().unwrap_or("modified"),
            file.additions.unwrap_or(0),
            file.deletions.unwrap_or(0),
            file.changes.unwrap_or(0),
        ));
        if let Some(patch) = file.patch.as_deref().and_then(summarize_patch_excerpt) {
            excerpt_lines.push(format!("### {}\n{}", file.filename, patch));
        }
    }
    if !file_lines.is_empty() {
        lines.push("changed_files:".to_owned());
        lines.extend(file_lines);
    }
    if !excerpt_lines.is_empty() {
        lines.push("patch_excerpts:".to_owned());
        lines.extend(excerpt_lines);
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn github_rest_compare_http_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> ApiError {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return github_reauth_required_error();
    }
    if status == reqwest::StatusCode::FORBIDDEN {
        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .map(str::trim);
        if remaining == Some("0") || is_rate_limit_message(body) {
            return github_rate_limited_error();
        }
        if is_reauth_message(body) {
            return github_reauth_required_error();
        }
        if is_access_restricted_message(body) {
            return github_access_restricted_error();
        }
    }
    ApiError::internal(format!("github compare returned {status}: {body}"))
}

async fn fetch_release_compare_digest_request(
    state: &AppState,
    repo_full_name: &str,
    base_tag: &str,
    head_tag: &str,
    access_token: Option<&str>,
) -> Result<Option<String>, ApiError> {
    let compare_ref = format!(
        "{}...{}",
        urlencoding::encode(base_tag),
        urlencoding::encode(head_tag)
    );
    let url = format!("https://api.github.com/repos/{repo_full_name}/compare/{compare_ref}");
    let mut request = state
        .http
        .get(url)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = access_token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(ApiError::internal)?;
    let status = response.status();
    if !status.is_success() {
        let headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();
        return Err(github_rest_compare_http_error(status, &headers, &body));
    }
    let payload = response
        .json::<GitHubCompareResponse>()
        .await
        .map_err(ApiError::internal)?;
    Ok(build_compare_digest(&payload))
}

async fn fetch_release_compare_digest(
    state: &AppState,
    user_id: &str,
    repo_full_name: &str,
    base_tag: &str,
    head_tag: &str,
) -> Result<Option<String>, ApiError> {
    let connections = state
        .load_github_connections(user_id)
        .await
        .map_err(|err| ApiError::internal(format!("load github connections failed: {err}")))?;

    let mut last_auth_err: Option<ApiError> = None;
    for connection in connections {
        match fetch_release_compare_digest_request(
            state,
            repo_full_name,
            base_tag,
            head_tag,
            Some(connection.access_token.as_str()),
        )
        .await
        {
            Ok(digest) => return Ok(digest),
            Err(err) if should_retry_public_compare_without_auth(&err) => {
                last_auth_err = Some(err);
            }
            Err(err) => return Err(err),
        }
    }

    if let Some(auth_err) = last_auth_err {
        match fetch_release_compare_digest_request(state, repo_full_name, base_tag, head_tag, None)
            .await
        {
            Ok(digest) => Ok(digest),
            Err(public_err) => Err(map_public_compare_fallback_error(auth_err, public_err)),
        }
    } else {
        fetch_release_compare_digest_request(state, repo_full_name, base_tag, head_tag, None).await
    }
}

fn should_retry_public_compare_without_auth(err: &ApiError) -> bool {
    matches!(err.code(), "reauth_required" | "forbidden")
}

fn map_public_compare_fallback_error(auth_err: ApiError, public_err: ApiError) -> ApiError {
    if public_err.code() == "rate_limited" || public_err.code() == "forbidden" {
        return public_err;
    }
    if auth_err.code() == "reauth_required" {
        return github_private_repo_scope_required_error();
    }
    auth_err
}

async fn summarize_release_smart_candidate_with_ai(
    state: &AppState,
    user_id: &str,
    item: &ReleaseSmartBatchCandidate,
) -> Result<Option<(Option<String>, Option<String>)>, ApiError> {
    let body_prompt = release_smart_body_prompt(item);
    let raw = ai::chat_completion(
        state,
        "你是一个严谨的版本变化整理助手，专门把 GitHub Release 证据整理成便于人类快速理解的中文要点卡片。只能根据给定证据输出，不得脑补。",
        &body_prompt,
        700,
    )
    .await
    .map_err(ApiError::internal)?;
    let parsed = parse_release_smart_summary_payload(&raw)
        .ok_or_else(|| ApiError::internal("release smart body summary json decode failed"))?;
    if parsed.valuable {
        let summary = render_smart_summary_markdown(&parsed.summary_bullets)
            .ok_or_else(|| ApiError::internal("release smart body summary bullets missing"))?;
        return Ok(Some((parsed.title_zh, Some(summary))));
    }

    let Some(previous_tag_name) = item
        .previous_tag_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let compare_range = format!("{previous_tag_name}...{}", item.tag_name);
    let digest = fetch_release_compare_digest(
        state,
        user_id,
        item.full_name.as_str(),
        previous_tag_name,
        item.tag_name.as_str(),
    )
    .await?;
    let Some(digest) = digest else {
        return Ok(None);
    };

    let diff_prompt = release_smart_diff_prompt(item, &compare_range, &digest);
    let raw = ai::chat_completion(
        state,
        "你是一个严谨的版本变化整理助手，专门把 GitHub compare 摘要整理成便于人类快速理解的中文版本变化要点。只能根据给定证据输出，不得脑补。",
        &diff_prompt,
        700,
    )
    .await
    .map_err(ApiError::internal)?;
    let parsed = parse_release_smart_summary_payload(&raw)
        .ok_or_else(|| ApiError::internal("release smart diff summary json decode failed"))?;
    if !parsed.valuable {
        return Ok(None);
    }
    let summary = render_smart_summary_markdown(&parsed.summary_bullets)
        .ok_or_else(|| ApiError::internal("release smart diff summary bullets missing"))?;
    Ok(Some((parsed.title_zh, Some(summary))))
}

async fn prepare_release_smart_batch(
    state: &AppState,
    user_id: &str,
    release_ids: &[i64],
) -> Result<PreparedReleaseSmartBatch, ApiError> {
    if state.config.ai.is_none() {
        return Ok(PreparedReleaseSmartBatch {
            candidates: Vec::new(),
            pending: Vec::new(),
            smart: HashMap::new(),
            terminal: HashMap::new(),
            missing: HashSet::new(),
        });
    }

    let mut source_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"
        SELECT release_id, full_name, tag_name, name, body, previous_tag_name
        FROM (
          SELECT
            r.release_id AS release_id,
            sr.full_name AS full_name,
            r.tag_name AS tag_name,
            r.name AS name,
            r.body AS body,
            LAG(r.tag_name) OVER (
              PARTITION BY r.repo_id
              ORDER BY COALESCE(r.published_at, r.created_at, r.updated_at) ASC, r.release_id ASC
            ) AS previous_tag_name
          FROM repo_releases r
          JOIN user_release_visible_repos sr
            ON sr.user_id = "#,
    );
    source_query.push_bind(user_id);
    source_query.push(" AND sr.repo_id = r.repo_id");
    source_query.push(") WHERE release_id IN (");
    {
        let mut separated = source_query.separated(", ");
        for release_id in release_ids {
            separated.push_bind(release_id);
        }
    }
    source_query.push(")");

    let source_rows = source_query
        .build_query_as::<ReleaseSmartBatchSourceRow>()
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
        let entity_id = release_id.to_string();
        let title = row
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&row.tag_name)
            .to_owned();
        let body = release_feed_body(row.body.as_deref()).unwrap_or_default();
        let source_hash = crate::translations::release_smart_feed_source_hash(
            entity_id.as_str(),
            row.full_name.as_str(),
            title.as_str(),
            Some(body.as_str()),
            row.tag_name.as_str(),
            row.previous_tag_name.as_deref(),
        );
        candidates.push(ReleaseSmartBatchCandidate {
            release_id: *release_id,
            entity_id,
            full_name: row.full_name.clone(),
            tag_name: row.tag_name.clone(),
            previous_tag_name: row.previous_tag_name.clone(),
            title,
            body,
            source_hash,
        });
    }

    let mut cache_by_entity = HashMap::<String, TranslationCacheRow>::new();
    if !candidates.is_empty() {
        let mut cache_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            SELECT entity_id, source_hash, status, title, summary, error_text
            FROM ai_translations
            WHERE user_id = "#,
        );
        cache_query.push_bind(user_id);
        cache_query.push(" AND entity_type = 'release_smart' AND lang = 'zh-CN' AND status IN ('ready', 'disabled', 'missing', 'error') AND entity_id IN (");
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
    let mut smart = HashMap::<i64, (Option<String>, Option<String>)>::new();
    let mut terminal = HashMap::<i64, ReleaseBatchTerminalState>::new();

    for item in &candidates {
        if let Some(cache) = cache_by_entity.get(&item.entity_id)
            && cache.source_hash == item.source_hash
        {
            if cache.status == "disabled"
                || (cache.status == "missing"
                    && cache.error_text.as_deref() == Some(SMART_NO_VALUABLE_VERSION_INFO))
            {
                terminal.insert(
                    item.release_id,
                    ReleaseBatchTerminalState {
                        status: cache.status.clone(),
                        error: cache.error_text.clone(),
                    },
                );
                continue;
            }
            if cache.status == "error" {
                if smart_error_is_retryable(cache.error_text.as_deref()) {
                    pending.push(item.clone());
                    continue;
                }
                terminal.insert(
                    item.release_id,
                    ReleaseBatchTerminalState {
                        status: cache.status.clone(),
                        error: cache.error_text.clone(),
                    },
                );
                continue;
            }
            if cache.status == "ready" {
                let ready = smart_ready_item(cache.title.clone(), cache.summary.clone(), None);
                if let Some(ready) = ready {
                    smart.insert(item.release_id, (ready.title, ready.summary));
                    continue;
                }
            }
        }
        pending.push(item.clone());
    }

    Ok(PreparedReleaseSmartBatch {
        candidates,
        pending,
        smart,
        terminal,
        missing,
    })
}

async fn mark_release_smart_requested(
    state: &AppState,
    user_id: &str,
    requested_at: &str,
    candidates: &[ReleaseSmartBatchCandidate],
) -> Result<(), ApiError> {
    for item in candidates {
        mark_translation_requested(
            state,
            user_id,
            requested_at,
            TranslationUpsert {
                entity_type: "release_smart",
                entity_id: &item.entity_id,
                lang: "zh-CN",
                source_hash: &item.source_hash,
                title: None,
                summary: None,
            },
        )
        .await?;
    }
    Ok(())
}

async fn upsert_release_smart_results(
    state: &AppState,
    user_id: &str,
    requested_at: &str,
    candidates: &[ReleaseSmartBatchCandidate],
    smart: &HashMap<i64, (Option<String>, Option<String>)>,
) -> Result<(), ApiError> {
    for item in candidates {
        if let Some((title, summary)) = smart.get(&item.release_id) {
            upsert_translation(
                state,
                user_id,
                requested_at,
                TranslationUpsert {
                    entity_type: "release_smart",
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

async fn upsert_release_smart_terminal_states(
    state: &AppState,
    user_id: &str,
    requested_at: &str,
    candidates: &[ReleaseSmartBatchCandidate],
    terminal: &HashMap<i64, ReleaseBatchTerminalState>,
) -> Result<(), ApiError> {
    for item in candidates {
        let Some(terminal_state) = terminal.get(&item.release_id) else {
            continue;
        };
        upsert_translation_terminal_status(
            state,
            user_id,
            requested_at,
            TranslationUpsert {
                entity_type: "release_smart",
                entity_id: &item.entity_id,
                lang: "zh-CN",
                source_hash: &item.source_hash,
                title: None,
                summary: None,
            },
            terminal_state.status.as_str(),
            terminal_state.error.as_deref(),
        )
        .await?;
    }
    Ok(())
}

fn build_release_smart_batch_item(
    release_id: i64,
    missing: &HashSet<i64>,
    terminal: &HashMap<i64, ReleaseBatchTerminalState>,
    smart: &HashMap<i64, (Option<String>, Option<String>)>,
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

    if let Some(terminal_state) = terminal.get(&release_id) {
        return TranslateBatchItem {
            id: release_id.to_string(),
            lang: "zh-CN".to_owned(),
            status: terminal_state.status.clone(),
            title: None,
            summary: None,
            error: terminal_state.error.clone().or_else(|| {
                (terminal_state.status == "missing")
                    .then_some("translation result missing".to_owned())
            }),
        };
    }

    if let Some((title, summary)) = smart.get(&release_id) {
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
        error: Some("release smart summary failed".to_owned()),
    }
}

async fn summarize_releases_smart_batch_internal(
    state: &AppState,
    user_id: &str,
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

    let requested_at = chrono::Utc::now().to_rfc3339();
    let mut prepared = prepare_release_smart_batch(state, user_id, release_ids).await?;
    mark_release_smart_requested(state, user_id, requested_at.as_str(), &prepared.pending).await?;

    for item in &prepared.pending {
        match summarize_release_smart_candidate_with_ai(state, user_id, item).await {
            Ok(Some(result)) => {
                prepared.smart.insert(item.release_id, result);
            }
            Ok(None) => {
                prepared.terminal.insert(
                    item.release_id,
                    ReleaseBatchTerminalState {
                        status: "missing".to_owned(),
                        error: Some(SMART_NO_VALUABLE_VERSION_INFO.to_owned()),
                    },
                );
            }
            Err(err) => {
                prepared.terminal.insert(
                    item.release_id,
                    ReleaseBatchTerminalState {
                        status: "error".to_owned(),
                        error: Some(err.to_string()),
                    },
                );
            }
        }
    }

    upsert_release_smart_results(
        state,
        user_id,
        requested_at.as_str(),
        &prepared.candidates,
        &prepared.smart,
    )
    .await?;
    upsert_release_smart_terminal_states(
        state,
        user_id,
        requested_at.as_str(),
        &prepared.candidates,
        &prepared.terminal,
    )
    .await?;

    Ok(release_ids
        .iter()
        .map(|release_id| {
            build_release_smart_batch_item(
                *release_id,
                &prepared.missing,
                &prepared.terminal,
                &prepared.smart,
            )
        })
        .collect())
}

pub async fn summarize_releases_smart_batch_for_user(
    state: &AppState,
    user_id: &str,
    release_ids: &[i64],
) -> Result<TranslateBatchResponse, ApiError> {
    let items = summarize_releases_smart_batch_internal(state, user_id, release_ids).await?;
    Ok(TranslateBatchResponse { items })
}

#[allow(dead_code)]
async fn translate_releases_batch_stream_worker(
    state: Arc<AppState>,
    user_id: String,
    release_ids: Vec<i64>,
    task_id: String,
    tx: mpsc::Sender<Result<Bytes, Infallible>>,
) {
    let heartbeat = jobs::spawn_task_lease_heartbeat(state.clone(), task_id.clone());
    let mut ready_count = 0usize;
    let mut disabled_count = 0usize;
    let mut missing_count = 0usize;
    let mut error_count = 0usize;

    let context = ai::LlmCallContext {
        source: "api.translate_releases_batch_stream".to_owned(),
        requested_by: Some(user_id.clone()),
        parent_task_id: Some(task_id.clone()),
        parent_task_type: Some(jobs::TASK_TRANSLATE_RELEASE_BATCH.to_owned()),
        parent_translation_batch_id: None,
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
                        "item_error": item.error.clone(),
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

        let mut prepared = prepare_release_batch(state.as_ref(), &user_id, &release_ids).await?;
        let detail_pending_ids = prepared
            .detail_pending_candidates
            .iter()
            .map(|candidate| candidate.release_id)
            .collect::<HashSet<_>>();

        for release_id in &release_ids {
            if detail_pending_ids.contains(release_id) {
                continue;
            }
            let item = build_release_batch_item(
                *release_id,
                &prepared.missing,
                &prepared.terminal,
                &prepared.translated,
            );
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
                    "item_error": item.error.clone(),
                }),
            )
            .await
            .map_err(ApiError::internal)?;
        }

        if !prepared.detail_pending_candidates.is_empty() {
            for candidate in &prepared.detail_pending_candidates {
                if !send_batch_stream_event(
                    &tx,
                    TranslateBatchStreamEvent {
                        event: "item",
                        item: Some(TranslateBatchItem {
                            id: candidate.release_id.to_string(),
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

            for item in translate_pending_release_batch_candidates(
                state.as_ref(),
                &user_id,
                &prepared.detail_pending_candidates,
            )
            .await?
            {
                if let Ok(release_id) = item.id.parse::<i64>() {
                    match item.status.as_str() {
                        "ready" => {
                            prepared
                                .translated
                                .insert(release_id, (item.title.clone(), item.summary.clone()));
                        }
                        "disabled" | "missing" | "error" => {
                            prepared.terminal.insert(
                                release_id,
                                ReleaseBatchTerminalState {
                                    status: item.status.clone(),
                                    error: item.error.clone(),
                                },
                            );
                        }
                        _ => {}
                    }
                }
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
                        "item_error": item.error.clone(),
                    }),
                )
                .await
                .map_err(ApiError::internal)?;
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
            heartbeat.stop().await;
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
            heartbeat.stop().await;
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

#[allow(dead_code)]
pub async fn translate_releases_batch(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateReleasesBatchRequest>,
) -> Result<Json<TranslateBatchResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let release_ids = parse_unique_release_ids(&req.release_ids, 60)?;
    let items = run_with_api_llm_context(
        "api.translate_releases_batch",
        Some(user_id.clone()),
        translate_releases_batch_internal(state.as_ref(), user_id.as_str(), &release_ids),
    )
    .await?;
    Ok(Json(TranslateBatchResponse {
        items: translate_batch_items_for_public(items),
    }))
}

#[allow(dead_code)]
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
                "user_id": user_id.clone(),
                "release_ids": release_ids.clone(),
            }),
            source: "api.translate_releases_batch_stream".to_owned(),
            requested_by: Some(user_id.clone()),
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
    user_id: &str,
    release_id_raw: &str,
) -> Result<TranslateResponse, ApiError> {
    let release_id = parse_release_id_param(release_id_raw)?;
    let mut items = translate_releases_batch_internal(state, user_id, &[release_id]).await?;
    let Some(item) = items.pop() else {
        return Err(ApiError::internal("missing translation result"));
    };
    translate_response_from_batch_item(item)
}

#[allow(dead_code)]
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
            Some(user_id.clone()),
            translate_release_for_user(state.as_ref(), user_id.as_str(), release_id.as_str()),
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
                "user_id": user_id.clone(),
                "release_id": release_id,
            }),
            source: "api.translate_release".to_owned(),
            requested_by: Some(user_id.clone()),
            parent_task_id: None,
        },
    )
    .await
}

async fn translate_release_detail_chunk(
    state: &AppState,
    budget: ReleaseDetailChunkBudget,
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
        budget.max_output_tokens,
    )
    .await
    .map_err(ApiError::internal)?;
    let translated = normalize_markdown_translation_output(chunk, translated);
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
        budget.max_output_tokens,
    )
    .await
    .map_err(ApiError::internal)?;
    let retry = normalize_markdown_translation_output(chunk, retry);
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
    budget: ReleaseDetailChunkBudget,
    repo_full_name: &str,
    original_title: &str,
    chunks: &[String],
) -> Result<Vec<String>, ApiError> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    const CHUNK_BATCH_OVERHEAD_TOKENS: u32 = 320;
    let input_budget = budget.input_budget;
    let estimated = chunks
        .iter()
        .map(|chunk| ai::estimate_text_tokens(chunk).saturating_add(48))
        .collect::<Vec<_>>();
    let grouped = ai::pack_batch_indices(&estimated, input_budget, CHUNK_BATCH_OVERHEAD_TOKENS);
    let split_count = grouped.len().saturating_sub(1);
    let saved_calls = chunks.len().saturating_sub(grouped.len());
    let estimated_tokens = estimated.iter().copied().sum::<u32>();
    tracing::info!(
        batch_size = chunks.len(),
        estimated_tokens,
        split_count,
        saved_calls,
        fallback_source = budget.fallback_source,
        input_budget = budget.input_budget,
        model_input_limit = budget.model_input_limit,
        max_output_tokens = budget.max_output_tokens,
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
            budget.max_output_tokens,
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
                        let source = &chunks[item.chunk_index - 1];
                        parsed.insert(
                            item.chunk_index - 1,
                            normalize_markdown_translation_output(source, item.summary_md),
                        );
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
            let mut out = parsed.remove(&idx);

            if out
                .as_deref()
                .is_none_or(|candidate| !markdown_structure_preserved(source, candidate))
            {
                out = Some(
                    translate_release_detail_chunk(
                        state,
                        budget,
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
    user_id: &str,
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
        starred_repo_id: Option<i64>,
        html_url: String,
        tag_name: String,
        name: Option<String>,
        body: Option<String>,
    }

    let row = sqlx::query_as::<_, ReleaseDetailSourceRow>(
        r#"
        SELECT r.repo_id, sr.repo_id AS starred_repo_id, r.html_url, r.tag_name, r.name, r.body
        FROM repo_releases r
        LEFT JOIN user_release_visible_repos sr
          ON sr.user_id = ? AND sr.repo_id = r.repo_id
        WHERE r.release_id = ?
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

    if row.starred_repo_id.is_none()
        && !user_has_brief_access_to_release(state, user_id, release_id).await?
    {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "release not found",
        ));
    }

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
    let requested_at = chrono::Utc::now().to_rfc3339();

    #[derive(Debug, sqlx::FromRow)]
    struct TranslationRow {
        source_hash: String,
        status: String,
        title: Option<String>,
        summary: Option<String>,
        error_text: Option<String>,
    }
    let cached = sqlx::query_as::<_, TranslationRow>(
        r#"
        SELECT source_hash, status, title, summary, error_text
        FROM ai_translations
        WHERE user_id = ?
          AND entity_type = 'release_detail'
          AND entity_id = ?
          AND lang = 'zh-CN'
          AND status IN ('ready', 'disabled', 'missing', 'error')
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
    {
        match cached.status.as_str() {
            "ready"
                if release_detail_translation_ready(
                    Some(original_body.as_str()),
                    cached.summary.as_deref(),
                ) =>
            {
                return Ok(TranslateResponse {
                    lang: "zh-CN".to_owned(),
                    status: "ready".to_owned(),
                    title: cached.title,
                    summary: cached.summary,
                });
            }
            "disabled" => {
                return Ok(TranslateResponse {
                    lang: "zh-CN".to_owned(),
                    status: cached.status,
                    title: None,
                    summary: None,
                });
            }
            "missing" => {
                return Err(ApiError::new(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    cached
                        .error_text
                        .unwrap_or_else(|| "release not found".to_owned()),
                ));
            }
            "error" => {
                return Err(ApiError::internal(
                    cached
                        .error_text
                        .unwrap_or_else(|| "release detail translation failed".to_owned()),
                ));
            }
            _ => {}
        }
    }

    mark_translation_requested(
        state,
        user_id,
        requested_at.as_str(),
        TranslationUpsert {
            entity_type: "release_detail",
            entity_id: &entity_id,
            lang: "zh-CN",
            source_hash: &source_hash,
            title: None,
            summary: None,
        },
    )
    .await?;

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
        let chunk_budget = release_detail_chunk_budget(state).await;
        tracing::info!(
            chunk_char_budget = chunk_budget.max_chars,
            chunk_input_budget = chunk_budget.input_budget,
            chunk_output_budget = chunk_budget.max_output_tokens,
            fallback_source = chunk_budget.fallback_source,
            model_input_limit = chunk_budget.model_input_limit,
            "release detail chunk budget resolved"
        );
        let chunks = split_markdown_chunks(&original_body, chunk_budget.max_chars);
        let translated_chunks = translate_release_detail_chunks_batched(
            state,
            chunk_budget,
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
        requested_at.as_str(),
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

#[allow(dead_code)]
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
            Some(user_id.clone()),
            translate_release_detail_for_user(
                state.as_ref(),
                user_id.as_str(),
                release_id.as_str(),
            ),
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
                "user_id": user_id.clone(),
                "release_id": release_id,
            }),
            source: "api.translate_release_detail".to_owned(),
            requested_by: Some(user_id.clone()),
            parent_task_id: None,
        },
    )
    .await
}

pub async fn translate_release_detail_for_user(
    state: &AppState,
    user_id: &str,
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
    user_id: &str,
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
                let error_text = err.to_string();
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
                    error: Some(error_text),
                });
            }
        }
    }
    Ok(items)
}

#[allow(dead_code)]
pub async fn translate_release_detail_batch(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateReleaseDetailBatchRequest>,
) -> Result<Json<TranslateBatchResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let release_ids = parse_unique_release_ids(&req.release_ids, 20)?;
    let items = run_with_api_llm_context(
        "api.translate_release_detail_batch",
        Some(user_id.clone()),
        translate_release_detail_batch_internal(state.as_ref(), user_id.as_str(), &release_ids),
    )
    .await?;
    Ok(Json(TranslateBatchResponse {
        items: translate_batch_items_for_public(items),
    }))
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
    user_id: &str,
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

    let requested_at = chrono::Utc::now().to_rfc3339();
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
            SELECT entity_id, source_hash, status, title, summary
            FROM ai_translations
            WHERE user_id = "#,
        );
        cache_query.push_bind(user_id);
        cache_query.push(" AND entity_type = 'notification' AND lang = 'zh-CN' AND status IN ('ready', 'disabled', 'missing') AND entity_id IN (");
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
    let mut terminal = HashMap::<String, String>::new();
    let mut pending = Vec::new();
    for item in &candidates {
        if let Some(cache) = cache_by_id.get(&item.thread_id)
            && cache.source_hash == item.source_hash
        {
            if matches!(cache.status.as_str(), "disabled" | "missing") {
                terminal.insert(item.thread_id.clone(), cache.status.clone());
                continue;
            }
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

    for item in &pending {
        mark_translation_requested(
            state,
            user_id,
            requested_at.as_str(),
            TranslationUpsert {
                entity_type: "notification",
                entity_id: &item.thread_id,
                lang: "zh-CN",
                source_hash: &item.source_hash,
                title: None,
                summary: None,
            },
        )
        .await?;
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
                requested_at.as_str(),
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
        if let Some(status) = terminal.get(thread_id) {
            out.push(TranslateBatchItem {
                id: thread_id.clone(),
                lang: "zh-CN".to_owned(),
                status: status.clone(),
                title: None,
                summary: None,
                error: if status == "missing" {
                    Some("translation result missing".to_owned())
                } else {
                    None
                },
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

#[allow(dead_code)]
pub async fn translate_notifications_batch(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateNotificationsBatchRequest>,
) -> Result<Json<TranslateBatchResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let thread_ids = parse_unique_thread_ids(&req.thread_ids, 60)?;
    let items = run_with_api_llm_context(
        "api.translate_notifications_batch",
        Some(user_id.clone()),
        translate_notifications_batch_internal(state.as_ref(), user_id.as_str(), &thread_ids),
    )
    .await?;
    Ok(Json(TranslateBatchResponse {
        items: translate_batch_items_for_public(items),
    }))
}

#[allow(dead_code)]
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
            Some(user_id.clone()),
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
                "user_id": user_id.clone(),
                "thread_id": thread_id,
            }),
            source: "api.translate_notification".to_owned(),
            requested_by: Some(user_id.clone()),
            parent_task_id: None,
        },
    )
    .await
}

pub async fn translate_notification_for_user(
    state: &AppState,
    user_id: String,
    thread_id_raw: &str,
) -> Result<TranslateResponse, ApiError> {
    let thread_id = thread_id_raw.trim().to_owned();
    if thread_id.is_empty() {
        return Err(ApiError::bad_request("thread_id is required"));
    }

    let mut items =
        translate_notifications_batch_internal(state, &user_id, std::slice::from_ref(&thread_id))
            .await?;
    let Some(item) = items.pop() else {
        return Err(ApiError::internal("missing translation result"));
    };
    translate_response_from_batch_item(item)
}

async fn require_user_id(session: &Session) -> Result<String, ApiError> {
    let Some(user_id) = session
        .get::<String>("user_id")
        .await
        .map_err(ApiError::internal)?
    else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "not logged in",
        ));
    };
    touch_authenticated_session(session).await?;
    parse_local_id_param(user_id, "user_id")
}

async fn touch_authenticated_session(session: &Session) -> Result<(), ApiError> {
    let now = chrono::Utc::now().timestamp();
    let last_touched_at = session
        .get::<i64>(SESSION_ACTIVITY_TOUCHED_AT)
        .await
        .map_err(ApiError::internal)?;

    if last_touched_at
        .is_some_and(|value| now.saturating_sub(value) < SESSION_ACTIVITY_TOUCH_INTERVAL_SECS)
    {
        return Ok(());
    }

    session
        .insert(SESSION_ACTIVITY_TOUCHED_AT, now)
        .await
        .map_err(ApiError::internal)
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
    last_active_at: Option<String>,
}

pub(crate) async fn require_active_user_id(
    state: &AppState,
    session: &Session,
) -> Result<String, ApiError> {
    let user_id = require_user_id(session).await?;
    let row = sqlx::query_as::<_, SessionAccessRow>(
        r#"
        SELECT is_disabled, last_active_at
        FROM users
        WHERE id = ?
        "#,
    )
    .bind(&user_id)
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

    if load_pending_access_sync_reason(session).await?.is_none()
        && last_active_is_stale(row.last_active_at.as_deref())
    {
        let reason = if row.last_active_at.is_some() {
            "inactive_over_1h"
        } else {
            "first_visit"
        };
        mark_pending_access_sync_reason(session, reason).await?;
    }

    touch_user_last_active_at(state, &user_id).await?;

    Ok(user_id)
}

pub(crate) async fn require_admin_user_id(
    state: &AppState,
    session: &Session,
) -> Result<String, ApiError> {
    let user_id = require_active_user_id(state, session).await?;
    let is_admin =
        sqlx::query_scalar::<_, i64>(r#"SELECT is_admin FROM users WHERE id = ? LIMIT 1"#)
            .bind(&user_id)
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
pub(crate) async fn ensure_owned_repo_visual_columns(
    pool: &sqlx::SqlitePool,
) -> Result<(), sqlx::Error> {
    for statement in [
        r#"ALTER TABLE owned_repo_star_baselines ADD COLUMN owner_avatar_url TEXT"#,
        r#"ALTER TABLE owned_repo_star_baselines ADD COLUMN open_graph_image_url TEXT"#,
        r#"ALTER TABLE owned_repo_star_baselines ADD COLUMN uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0"#,
    ] {
        match sqlx::query(statement).execute(pool).await {
            Ok(_) => {}
            Err(sqlx::Error::Database(err))
                if err.message().contains("duplicate column name")
                    || err.message().contains("no such table") => {}
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, TimeZone};

    use super::{
        ADMIN_DASHBOARD_PREAGGREGATE_DAYS, ADMIN_SYNC_SUBSCRIPTION_EVENT_LIMIT,
        ADMIN_TASK_DETAIL_EVENT_LIMIT, AdminDashboardQuery, AdminLlmCallListScope,
        AdminLlmCallsQuery, AdminLlmRuntimeConfigUpdateRequest, AdminRealtimeTaskDetailItem,
        AdminRealtimeTasksQuery, AdminSyncSubscriptionEventItem, AdminTaskEventItem,
        AdminUserPatchRequest, AdminUserUpdateGuard, AdminUsersQuery, FeedQuery, FeedRow,
        GitHubCompareCommit, GitHubCompareCommitDetail, GitHubCompareFile, GitHubCompareResponse,
        GraphQlError, LLM_CALL_ORDER_BY_CREATED_DESC, RELEASE_FEED_BODY_MAX_CHARS, ReturnModeQuery,
        SMART_NO_VALUABLE_VERSION_INFO, TranslateBatchItem, TranslationCacheRow, TranslationUpsert,
        admin_dashboard, admin_download_realtime_task_log, admin_get_llm_call_detail,
        admin_get_llm_scheduler_status, admin_get_realtime_task_detail, admin_list_llm_calls,
        admin_list_realtime_tasks, admin_list_users, admin_patch_llm_runtime_config,
        admin_patch_user, admin_users_offset, ai_error_is_non_retryable,
        brief_contains_release_link, build_compare_digest, build_task_diagnostics,
        ensure_account_enabled, execute_sync_all_sync_with, extract_brief_release_ids,
        extract_translation_fields, feed_item_from_row, get_release_detail,
        github_access_restricted_error, github_graphql_errors_to_api_error,
        github_graphql_http_error, github_rate_limited_error, github_reauth_required_error,
        guard_admin_user_update, has_repo_scope, last_active_is_stale, list_feed, list_releases,
        llm_call_order_by_clause, load_pending_access_sync_reason, looks_like_json_blob,
        map_job_action_error, map_public_compare_fallback_error, mark_translation_requested,
        markdown_structure_preserved, me, normalize_markdown_translation_output,
        normalize_translation_fields, parse_batch_notification_translation_payload,
        parse_batch_release_detail_translation_payload, parse_batch_release_translation_payload,
        parse_feed_types, parse_positive_admin_concurrency, parse_release_id_param,
        parse_release_smart_summary_payload, parse_repo_full_name_from_release_url,
        parse_translation_json, parse_unique_release_ids, parse_unique_thread_ids,
        prepare_release_batch, preserve_chunk_edge_newlines, refresh_admin_dashboard_rollups,
        release_cache_entry_reusable, release_detail_source_hash, release_detail_translation_ready,
        release_excerpt, release_feed_body, release_reactions_status, require_active_user_id,
        resolve_release_full_name, should_retry_public_compare_without_auth,
        smart_error_is_retryable, split_markdown_chunks, sync_all, sync_notifications,
        sync_releases, sync_starred, translate_release_detail_for_user,
        translate_releases_batch_for_user, translate_response_from_batch_item, upsert_translation,
    };
    use crate::ai;
    use crate::error::ApiError;
    use std::{
        fs,
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use crate::{
        config::{AiConfig, AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        jobs,
        state::{AppState, build_oauth_client},
        sync,
    };
    use axum::{
        Json, Router,
        body::to_bytes,
        extract::{Path, Query, State},
        http::{StatusCode, header},
        response::{IntoResponse, Response},
        routing::post,
    };
    use reqwest::header::{HeaderMap, HeaderValue};
    use serde_json::{Value, json};
    use sqlx::{
        Row, SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use tower_sessions::{MemoryStore, Session};
    use url::Url;

    fn test_user_id(id: i64) -> String {
        crate::local_id::test_local_id(&format!("user-{id}"))
    }

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
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: None,
            release_tag_name: None,
            release_previous_tag_name: None,
            title: None,
            subtitle: None,
            reason: None,
            subject_type: None,
            html_url: None,
            unread: None,
            actor_login: None,
            actor_avatar_url: None,
            actor_html_url: None,
            release_body: None,
            react_plus1: None,
            react_laugh: None,
            react_heart: None,
            react_hooray: None,
            react_rocket: None,
            react_eyes: None,
            trans_source_hash: None,
            trans_status: None,
            trans_title: None,
            trans_summary: None,
            trans_error_text: None,
            trans_work_status: None,
            detail_trans_source_hash: None,
            detail_trans_status: None,
            detail_trans_title: None,
            detail_trans_summary: None,
            detail_trans_error_text: None,
            detail_trans_work_status: None,
            smart_source_hash: None,
            smart_status: None,
            smart_title: None,
            smart_summary: None,
            smart_error_text: None,
            smart_work_status: None,
        }
    }

    #[test]
    fn parse_positive_admin_concurrency_rejects_values_above_max_permits() {
        let overflow = i64::try_from(tokio::sync::Semaphore::MAX_PERMITS)
            .expect("max permits fits in i64")
            + 1;
        let err = parse_positive_admin_concurrency(overflow, "max_concurrency")
            .expect_err("overflow concurrency should fail");

        assert!(
            err.to_string().contains(&format!(
                "max_concurrency must be a positive integer <= {}",
                tokio::sync::Semaphore::MAX_PERMITS
            )),
            "unexpected error: {err}"
        );
    }

    fn test_task_detail_item(
        task_type: &str,
        status: &str,
        payload_json: &str,
        result_json: Option<&str>,
        error_message: Option<&str>,
    ) -> AdminRealtimeTaskDetailItem {
        AdminRealtimeTaskDetailItem {
            id: "task-test".to_owned(),
            task_type: task_type.to_owned(),
            status: status.to_owned(),
            source: "tests".to_owned(),
            requested_by: Some(test_user_id(1)),
            parent_task_id: None,
            cancel_requested: false,
            error_message: error_message.map(str::to_owned),
            payload_json: payload_json.to_owned(),
            result_json: result_json.map(str::to_owned),
            log_file_path: None,
            created_at: "2026-02-27T00:00:00Z".to_owned(),
            started_at: Some("2026-02-27T00:00:01Z".to_owned()),
            finished_at: Some("2026-02-27T00:00:02Z".to_owned()),
            updated_at: "2026-02-27T00:00:02Z".to_owned(),
        }
    }

    fn test_task_event(id: i64, event_type: &str, payload_json: &str) -> AdminTaskEventItem {
        AdminTaskEventItem {
            id: id.to_string(),
            event_type: event_type.to_owned(),
            payload_json: payload_json.to_owned(),
            created_at: format!("2026-02-27T00:00:{:02}Z", (id % 60)),
        }
    }

    async fn setup_pool() -> SqlitePool {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create sqlite memory db");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        super::ensure_owned_repo_visual_columns(&pool)
            .await
            .expect("ensure owned repo visual columns");

        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, created_at, updated_at)
            VALUES (?, 30215105, 'IvanLi-CN', ?, ?)
            "#,
        )
        .bind(test_user_id(1))
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed user");

        seed_github_connection(&pool, test_user_id(1).as_str(), 30215105, "IvanLi-CN", now).await;

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
            task_log_dir: std::env::temp_dir().join("octo-rill-task-logs-tests"),
            job_worker_concurrency: 4,
            encryption_key: encryption_key.clone(),
            github: GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse github redirect"),
            },
            linuxdo: None,
            ai: None,
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: crate::briefs::DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned(),
        };
        let github_oauth = build_oauth_client(&config).expect("build oauth client");
        let webauthn = crate::state::build_webauthn(&config).expect("build webauthn");
        Arc::new(AppState {
            llm_scheduler: Arc::new(crate::ai::LlmScheduler::new(config.ai_max_concurrency)),
            translation_scheduler: Arc::new(
                crate::translations::TranslationSchedulerController::new(
                    crate::translations::TranslationRuntimeConfig::default(),
                ),
            ),
            config,
            pool,
            http: reqwest::Client::new(),
            github_oauth,
            linuxdo_oauth: None,
            webauthn,
            encryption_key,
            runtime_owner_id: "api-test-runtime-owner".to_owned(),
        })
    }

    fn setup_state_with_ai_base_url(pool: SqlitePool, base_url: Url) -> Arc<AppState> {
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
            task_log_dir: std::env::temp_dir().join("octo-rill-task-logs-tests"),
            job_worker_concurrency: 4,
            encryption_key: encryption_key.clone(),
            github: GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse github redirect"),
            },
            linuxdo: None,
            ai: Some(AiConfig {
                base_url,
                model: "test-model".to_owned(),
                api_key: "test-key".to_owned(),
            }),
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: crate::briefs::DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned(),
        };
        let github_oauth = build_oauth_client(&config).expect("build oauth client");
        let webauthn = crate::state::build_webauthn(&config).expect("build webauthn");
        Arc::new(AppState {
            llm_scheduler: Arc::new(crate::ai::LlmScheduler::new(config.ai_max_concurrency)),
            translation_scheduler: Arc::new(
                crate::translations::TranslationSchedulerController::new(
                    crate::translations::TranslationRuntimeConfig::default(),
                ),
            ),
            config,
            pool,
            http: reqwest::Client::new(),
            github_oauth,
            linuxdo_oauth: None,
            webauthn,
            encryption_key,
            runtime_owner_id: "api-test-runtime-owner".to_owned(),
        })
    }

    fn setup_state_with_ai(pool: SqlitePool) -> Arc<AppState> {
        setup_state_with_ai_base_url(
            pool,
            Url::parse("https://api.example.test/v1/").expect("parse ai base url"),
        )
    }

    async fn spawn_test_ai_server(app: Router) -> Url {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test ai server");
        let addr = listener.local_addr().expect("resolve test ai server addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test ai app");
        });
        Url::parse(&format!("http://{addr}/")).expect("parse test ai base url")
    }

    async fn setup_session(user_id: i64) -> Session {
        let store = Arc::new(MemoryStore::default());
        let session = Session::new(None, store, None);
        session
            .insert("user_id", test_user_id(user_id))
            .await
            .expect("insert session user id");
        session
    }

    async fn seed_github_connection(
        pool: &SqlitePool,
        user_id: &str,
        github_user_id: i64,
        login: &str,
        linked_at: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO github_connections (
              id,
              user_id,
              github_user_id,
              login,
              access_token_ciphertext,
              access_token_nonce,
              scopes,
              linked_at,
              updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id)
        .bind(github_user_id)
        .bind(login)
        .bind(vec![0_u8])
        .bind(vec![0_u8])
        .bind("read:user")
        .bind(linked_at)
        .bind(linked_at)
        .execute(pool)
        .await
        .expect("seed github connection");
    }

    async fn seed_user(pool: &SqlitePool, id: i64, login: &str, is_admin: i64, is_disabled: i64) {
        let created_at = format!("2026-02-23T00:00:{id:02}Z");
        let local_id = test_user_id(id);
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, is_admin, is_disabled, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&local_id)
        .bind(30000000_i64 + id)
        .bind(login)
        .bind(is_admin)
        .bind(is_disabled)
        .bind(created_at.as_str())
        .bind(created_at.as_str())
        .execute(pool)
        .await
        .expect("seed test user");

        seed_github_connection(
            pool,
            local_id.as_str(),
            30000000_i64 + id,
            login,
            &created_at,
        )
        .await;
    }

    async fn set_last_active_at(pool: &SqlitePool, user_id: &str, last_active_at: Option<&str>) {
        sqlx::query(
            r#"
            UPDATE users
            SET last_active_at = ?
            WHERE id = ?
            "#,
        )
        .bind(last_active_at)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("set last_active_at");
    }

    #[tokio::test]
    async fn stale_visit_marker_survives_until_me_bootstraps_access_sync() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let session = setup_session(1).await;
        let user_id = test_user_id(1);

        set_last_active_at(&pool, user_id.as_str(), Some("2026-02-22T21:30:00Z")).await;

        let resolved = require_active_user_id(state.as_ref(), &session)
            .await
            .expect("require active user");
        assert_eq!(resolved, user_id);
        assert_eq!(
            load_pending_access_sync_reason(&session)
                .await
                .expect("load pending reason")
                .as_deref(),
            Some("inactive_over_1h")
        );
        let touched_last_active_at = sqlx::query_scalar::<_, Option<String>>(
            r#"
            SELECT last_active_at
            FROM users
            WHERE id = ?
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load touched last_active_at");
        assert!(touched_last_active_at.is_some());
        assert_ne!(
            touched_last_active_at.as_deref(),
            Some("2026-02-22T21:30:00Z")
        );
        assert!(!last_active_is_stale(touched_last_active_at.as_deref()));

        let Json(resp) = me(State(state.clone()), session)
            .await
            .expect("bootstrap me");
        assert_eq!(resp.access_sync.reason, "inactive_over_1h");
        assert_eq!(
            resp.access_sync.task_type.as_deref(),
            Some(jobs::TASK_SYNC_ACCESS_REFRESH)
        );
        assert!(resp.access_sync.task_id.is_some());

        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE requested_by = ?
              AND task_type = ?
              AND status = 'queued'
            "#,
        )
        .bind(user_id.as_str())
        .bind(jobs::TASK_SYNC_ACCESS_REFRESH)
        .fetch_one(&pool)
        .await
        .expect("count queued access sync tasks");
        assert_eq!(queued, 1);
    }

    #[tokio::test]
    async fn sync_all_task_id_reuses_inflight_access_refresh_task() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id(1);

        let response = sync_all(
            State(state.clone()),
            setup_session(1).await,
            Query(ReturnModeQuery {
                return_mode: Some("task_id".to_owned()),
            }),
        )
        .await
        .expect("enqueue first sync_all");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read first response body");
        let first: serde_json::Value =
            serde_json::from_slice(&body).expect("parse first task response");
        let first_task_id = first
            .get("task_id")
            .and_then(|value| value.as_str())
            .expect("first task id")
            .to_owned();

        let response = sync_all(
            State(state.clone()),
            setup_session(1).await,
            Query(ReturnModeQuery {
                return_mode: Some("task_id".to_owned()),
            }),
        )
        .await
        .expect("enqueue second sync_all");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read second response body");
        let second: serde_json::Value =
            serde_json::from_slice(&body).expect("parse second task response");
        let second_task_id = second
            .get("task_id")
            .and_then(|value| value.as_str())
            .expect("second task id");
        assert_eq!(second_task_id, first_task_id);

        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE requested_by = ?
              AND task_type = ?
              AND status IN ('queued', 'running')
            "#,
        )
        .bind(user_id.as_str())
        .bind(jobs::TASK_SYNC_ACCESS_REFRESH)
        .fetch_one(&pool)
        .await
        .expect("count access refresh tasks");
        assert_eq!(queued, 1);
    }

    #[tokio::test]
    async fn sync_all_sync_mode_includes_notifications_and_social_error() {
        let pool = setup_pool().await;
        let state = setup_state(pool);

        let body = execute_sync_all_sync_with(
            state.as_ref(),
            test_user_id(1).as_str(),
            |_state, _user_id| Box::pin(async { Ok(sync::SyncStarredResult { repos: 2 }) }),
            |_state, _user_id| {
                Box::pin(async {
                    Ok(sync::SyncReleasesResult {
                        repos: 3,
                        releases: 5,
                    })
                })
            },
            |_state, _user_id| {
                Box::pin(async {
                    (
                        sync::SyncSocialActivityResult::default(),
                        Some("social boom".to_owned()),
                    )
                })
            },
            |_state, _user_id| {
                Box::pin(async {
                    Ok(sync::SyncNotificationsResult {
                        notifications: 7,
                        since: Some("2026-04-11T11:00:00Z".to_owned()),
                    })
                })
            },
        )
        .await
        .expect("sync all sync body");

        assert_eq!(body["starred"]["repos"], json!(2));
        assert_eq!(body["releases"]["repos"], json!(3));
        assert_eq!(body["releases"]["releases"], json!(5));
        assert_eq!(body["social"]["events"], json!(0));
        assert_eq!(body["notifications"]["notifications"], json!(7));
        assert_eq!(
            body["notifications"]["since"],
            json!("2026-04-11T11:00:00Z")
        );
        assert_eq!(body["social_error"], json!("social boom"));
    }

    async fn task_id_from_response(response: Response) -> String {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read task response body");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("parse task response body");
        payload
            .get("task_id")
            .and_then(|value| value.as_str())
            .expect("task id")
            .to_owned()
    }

    #[tokio::test]
    async fn sync_starred_task_id_reuses_inflight_task() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id(1);

        let first_task_id = task_id_from_response(
            sync_starred(
                State(state.clone()),
                setup_session(1).await,
                Query(ReturnModeQuery {
                    return_mode: Some("task_id".to_owned()),
                }),
            )
            .await
            .expect("enqueue first sync_starred"),
        )
        .await;

        let second_task_id = task_id_from_response(
            sync_starred(
                State(state.clone()),
                setup_session(1).await,
                Query(ReturnModeQuery {
                    return_mode: Some("task_id".to_owned()),
                }),
            )
            .await
            .expect("enqueue second sync_starred"),
        )
        .await;

        assert_eq!(second_task_id, first_task_id);

        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE requested_by = ?
              AND task_type = ?
              AND status IN ('queued', 'running')
            "#,
        )
        .bind(user_id.as_str())
        .bind(jobs::TASK_SYNC_STARRED)
        .fetch_one(&pool)
        .await
        .expect("count sync starred tasks");
        assert_eq!(queued, 1);
    }

    #[tokio::test]
    async fn sync_releases_task_id_reuses_inflight_task() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id(1);

        let first_task_id = task_id_from_response(
            sync_releases(
                State(state.clone()),
                setup_session(1).await,
                Query(ReturnModeQuery {
                    return_mode: Some("task_id".to_owned()),
                }),
            )
            .await
            .expect("enqueue first sync_releases"),
        )
        .await;

        let second_task_id = task_id_from_response(
            sync_releases(
                State(state.clone()),
                setup_session(1).await,
                Query(ReturnModeQuery {
                    return_mode: Some("task_id".to_owned()),
                }),
            )
            .await
            .expect("enqueue second sync_releases"),
        )
        .await;

        assert_eq!(second_task_id, first_task_id);

        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE requested_by = ?
              AND task_type = ?
              AND status IN ('queued', 'running')
            "#,
        )
        .bind(user_id.as_str())
        .bind(jobs::TASK_SYNC_RELEASES)
        .fetch_one(&pool)
        .await
        .expect("count sync releases tasks");
        assert_eq!(queued, 1);
    }

    #[tokio::test]
    async fn sync_notifications_task_id_reuses_inflight_task() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id(1);

        let first_task_id = task_id_from_response(
            sync_notifications(
                State(state.clone()),
                setup_session(1).await,
                Query(ReturnModeQuery {
                    return_mode: Some("task_id".to_owned()),
                }),
            )
            .await
            .expect("enqueue first sync_notifications"),
        )
        .await;

        let second_task_id = task_id_from_response(
            sync_notifications(
                State(state.clone()),
                setup_session(1).await,
                Query(ReturnModeQuery {
                    return_mode: Some("task_id".to_owned()),
                }),
            )
            .await
            .expect("enqueue second sync_notifications"),
        )
        .await;

        assert_eq!(second_task_id, first_task_id);

        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE requested_by = ?
              AND task_type = ?
              AND status IN ('queued', 'running')
            "#,
        )
        .bind(user_id.as_str())
        .bind(jobs::TASK_SYNC_NOTIFICATIONS)
        .fetch_one(&pool)
        .await
        .expect("count sync notifications tasks");
        assert_eq!(queued, 1);
    }

    #[tokio::test]
    async fn me_reuses_inflight_access_refresh_task() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let session = setup_session(1).await;
        let user_id = test_user_id(1);

        set_last_active_at(&pool, user_id.as_str(), Some("2026-02-22T21:30:00Z")).await;
        seed_access_refresh_task(
            &pool,
            "task-access-running",
            user_id.as_str(),
            jobs::STATUS_RUNNING,
        )
        .await;

        let Json(resp) = me(State(state), session).await.expect("bootstrap me");
        assert_eq!(resp.access_sync.reason, "reused_inflight");
        assert_eq!(
            resp.access_sync.task_type.as_deref(),
            Some(jobs::TASK_SYNC_ACCESS_REFRESH)
        );
        assert_eq!(
            resp.access_sync.task_id.as_deref(),
            Some("task-access-running")
        );
        assert_eq!(
            resp.access_sync.event_path.as_deref(),
            Some("/api/tasks/task-access-running/events")
        );

        let inflight = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE requested_by = ?
              AND task_type = ?
              AND status IN ('queued', 'running')
            "#,
        )
        .bind(user_id.as_str())
        .bind(jobs::TASK_SYNC_ACCESS_REFRESH)
        .fetch_one(&pool)
        .await
        .expect("count inflight access refresh tasks");
        assert_eq!(inflight, 1);
    }

    async fn seed_repo_release(pool: &SqlitePool, repo_id: i64, release_id: i64) {
        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO repo_releases (
              id,
              repo_id,
              release_id,
              node_id,
              tag_name,
              name,
              body,
              html_url,
              published_at,
              created_at,
              is_prerelease,
              is_draft,
              updated_at,
              react_plus1,
              react_laugh,
              react_heart,
              react_hooray,
              react_rocket,
              react_eyes
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?, 0, 0, 0, 0, 0, 0)
            "#,
        )
        .bind(format!("repo-release-{repo_id}-{release_id}"))
        .bind(repo_id)
        .bind(release_id)
        .bind(format!("node-{release_id}"))
        .bind("v1.2.3")
        .bind("Release v1.2.3")
        .bind("- item")
        .bind("https://github.com/openai/codex/releases/tag/v1.2.3")
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed shared repo release");
    }

    async fn seed_star(pool: &SqlitePool, repo_id: i64) {
        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO starred_repos (
              id, user_id, repo_id, full_name, owner_login, name,
              description, html_url, stargazed_at, is_private, updated_at,
              owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)
            "#,
        )
        .bind(format!("star-{repo_id}"))
        .bind(test_user_id(1))
        .bind(repo_id)
        .bind("openai/codex")
        .bind("openai")
        .bind("codex")
        .bind("octo rill test")
        .bind("https://github.com/openai/codex")
        .bind(now)
        .bind(now)
        .bind("https://avatars.githubusercontent.com/u/14957082")
        .bind("https://repository-images.githubusercontent.com/14957082/codex")
        .bind(1_i64)
        .execute(pool)
        .await
        .expect("seed starred");
    }

    async fn seed_owned_repo_baseline(pool: &SqlitePool, repo_id: i64, full_name: &str) {
        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO owned_repo_star_baselines (
              id,
              user_id,
              repo_id,
              repo_full_name,
              initialized_at,
              updated_at,
              members_snapshot_initialized,
              owner_avatar_url,
              open_graph_image_url,
              uses_custom_open_graph_image
            )
            VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
            "#,
        )
        .bind(format!("owned-repo-{repo_id}"))
        .bind(test_user_id(1))
        .bind(repo_id)
        .bind(full_name)
        .bind(now)
        .bind(now)
        .bind("https://avatars.githubusercontent.com/u/30215105")
        .bind("https://repository-images.githubusercontent.com/30215105/octo-rill")
        .bind(1_i64)
        .execute(pool)
        .await
        .expect("seed owned repo baseline");
    }

    async fn set_include_own_releases(pool: &SqlitePool, enabled: bool) {
        sqlx::query(
            r#"
            UPDATE users
            SET include_own_releases = ?, updated_at = '2026-02-23T00:00:00Z'
            WHERE id = ?
            "#,
        )
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(test_user_id(1))
        .execute(pool)
        .await
        .expect("update include_own_releases");
    }

    struct SeedSocialEventArgs<'a> {
        kind: &'a str,
        event_id: &'a str,
        repo_id: Option<i64>,
        repo_full_name: Option<&'a str>,
        repo_owner_avatar_url: Option<&'a str>,
        repo_open_graph_image_url: Option<&'a str>,
        repo_uses_custom_open_graph_image: Option<bool>,
        actor_login: &'a str,
        occurred_at: &'a str,
    }

    async fn seed_social_event(pool: &SqlitePool, user_id: &str, args: SeedSocialEventArgs<'_>) {
        sqlx::query(
            r#"
            INSERT INTO social_activity_events (
              id,
              user_id,
              kind,
              repo_id,
              repo_full_name,
              repo_owner_avatar_url,
              repo_open_graph_image_url,
              repo_uses_custom_open_graph_image,
              actor_github_user_id,
              actor_login,
              actor_avatar_url,
              actor_html_url,
              occurred_at,
              detected_at,
              created_at,
              updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(args.event_id)
        .bind(user_id)
        .bind(args.kind)
        .bind(args.repo_id)
        .bind(args.repo_full_name)
        .bind(args.repo_owner_avatar_url)
        .bind(args.repo_open_graph_image_url)
        .bind(
            args.repo_uses_custom_open_graph_image
                .map(|uses_custom| if uses_custom { 1_i64 } else { 0_i64 }),
        )
        .bind(90_000_i64 + i64::from(args.actor_login.bytes().map(i16::from).sum::<i16>()))
        .bind(args.actor_login)
        .bind(format!("https://avatars.example/{}.png", args.actor_login))
        .bind(format!("https://github.com/{}", args.actor_login))
        .bind(args.occurred_at)
        .bind(args.occurred_at)
        .bind(args.occurred_at)
        .bind(args.occurred_at)
        .execute(pool)
        .await
        .expect("seed social activity event");
    }

    async fn seed_brief(pool: &SqlitePool, user_id: &str, date: &str, content_markdown: &str) {
        let created_at = format!("{date}T08:00:00Z");
        sqlx::query(
            r#"
            INSERT INTO briefs (id, user_id, date, content_markdown, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(format!("brief-{date}"))
        .bind(user_id)
        .bind(date)
        .bind(content_markdown)
        .bind(&created_at)
        .bind(created_at.clone())
        .execute(pool)
        .await
        .expect("seed brief");
    }

    async fn seed_release_detail_translation(
        pool: &SqlitePool,
        user_id: &str,
        entity_id: &str,
        source_hash: &str,
        title: Option<&str>,
        summary: Option<&str>,
    ) {
        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary, error_text, active_work_item_id, created_at, updated_at
            )
            VALUES (?, ?, 'release_detail', ?, 'zh-CN', ?, 'ready', ?, ?, NULL, NULL, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id)
        .bind(entity_id)
        .bind(source_hash)
        .bind(title)
        .bind(summary)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed release detail translation");
    }

    async fn seed_release_translation(
        pool: &SqlitePool,
        user_id: &str,
        entity_id: &str,
        source_hash: &str,
        title: Option<&str>,
        summary: Option<&str>,
    ) {
        let now = "2026-02-23T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary, error_text, active_work_item_id, created_at, updated_at
            )
            VALUES (?, ?, 'release', ?, 'zh-CN', ?, 'ready', ?, ?, NULL, NULL, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id)
        .bind(entity_id)
        .bind(source_hash)
        .bind(title)
        .bind(summary)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed release translation");
    }

    async fn seed_access_refresh_task(
        pool: &SqlitePool,
        task_id: &str,
        requested_by: &str,
        status: &str,
    ) {
        let now = "2026-02-23T00:00:00Z";
        let started_at = match status {
            jobs::STATUS_RUNNING | jobs::STATUS_SUCCEEDED | jobs::STATUS_FAILED => Some(now),
            _ => None,
        };
        let finished_at = match status {
            jobs::STATUS_SUCCEEDED | jobs::STATUS_FAILED | jobs::STATUS_CANCELED => Some(now),
            _ => None,
        };
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at
            )
            VALUES (?, ?, ?, 'tests', ?, NULL, '{}', NULL, NULL, 0, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id)
        .bind(jobs::TASK_SYNC_ACCESS_REFRESH)
        .bind(status)
        .bind(requested_by)
        .bind(now)
        .bind(started_at)
        .bind(finished_at)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed access refresh task");
    }

    async fn seed_admin_dashboard_task(
        pool: &SqlitePool,
        task_id: &str,
        task_type: &str,
        status: &str,
        requested_by: &str,
        created_at: &str,
    ) {
        let started_at = match status {
            jobs::STATUS_RUNNING | jobs::STATUS_SUCCEEDED | jobs::STATUS_FAILED => Some(created_at),
            _ => None,
        };
        let finished_at = match status {
            jobs::STATUS_SUCCEEDED | jobs::STATUS_FAILED | jobs::STATUS_CANCELED => {
                Some(created_at)
            }
            _ => None,
        };
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at
            )
            VALUES (?, ?, ?, 'tests', ?, NULL, '{}', NULL, NULL, 0, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id)
        .bind(task_type)
        .bind(status)
        .bind(requested_by)
        .bind(created_at)
        .bind(started_at)
        .bind(finished_at)
        .bind(created_at)
        .execute(pool)
        .await
        .expect("seed admin dashboard task");
    }

    async fn seed_llm_call(
        pool: &SqlitePool,
        call_id: &str,
        status: &str,
        source: &str,
        requested_by: Option<String>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();
        seed_llm_call_with_created_at(pool, call_id, status, source, requested_by, &now).await;
    }

    async fn seed_llm_call_with_created_at(
        pool: &SqlitePool,
        call_id: &str,
        status: &str,
        source: &str,
        requested_by: Option<String>,
        created_at: &str,
    ) {
        let started_at = match status {
            "queued" => None,
            _ => Some(created_at),
        };
        let finished_at = match status {
            "queued" | "running" => None,
            _ => Some(created_at),
        };
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
        .bind(created_at)
        .bind(started_at)
        .bind(finished_at)
        .bind(created_at)
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
            acting_user_id: test_user_id(7),
            target_user_id: test_user_id(7),
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
            acting_user_id: test_user_id(1),
            target_user_id: test_user_id(2),
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
            acting_user_id: test_user_id(1),
            target_user_id: test_user_id(2),
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
        sqlx::query(r#"UPDATE users SET is_disabled = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
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
            .get::<String>("user_id")
            .await
            .expect("read session user id");
        assert!(remaining.is_none(), "disabled session should be cleared");
    }

    #[tokio::test]
    async fn admin_patch_user_rejects_demoting_last_admin_via_handler() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        let state = setup_state(pool);
        let session = setup_session(1).await;

        let err = admin_patch_user(
            State(state),
            session,
            Path(test_user_id(1)),
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
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: None,
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
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            "call-failed",
            "failed",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
        )
        .await;
        seed_llm_call(
            &pool,
            "call-ok",
            "succeeded",
            "api.translate_release",
            Some(test_user_id(1)),
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
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: Some("status_grouped".to_owned()),
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
    async fn admin_list_realtime_tasks_keeps_newest_created_first() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        for (task_id, status, created_at) in [
            ("task-old-running", "running", "2026-02-26T01:00:00Z"),
            ("task-new-failed", "failed", "2026-02-26T05:00:00Z"),
            ("task-mid-succeeded", "succeeded", "2026-02-26T03:00:00Z"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO job_tasks (
                  id,
                  task_type,
                  status,
                  source,
                  requested_by,
                  parent_task_id,
                  payload_json,
                  result_json,
                  error_message,
                  cancel_requested,
                  created_at,
                  started_at,
                  finished_at,
                  updated_at
                )
                VALUES (?, 'sync.releases', ?, 'tests', ?, NULL, '{}', '{}', NULL, 0, ?, ?, ?, ?)
                "#,
            )
            .bind(task_id)
            .bind(status)
            .bind(test_user_id(1))
            .bind(created_at)
            .bind(created_at)
            .bind(created_at)
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("seed realtime task");
        }

        let state = setup_state(pool);
        let session = setup_session(1).await;

        let resp = admin_list_realtime_tasks(
            State(state),
            session,
            Query(AdminRealtimeTasksQuery {
                status: Some("all".to_owned()),
                task_type: None,
                exclude_task_type: None,
                task_group: None,
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("admin realtime task list should succeed")
        .0;

        let ids = resp
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["task-new-failed", "task-mid-succeeded", "task-old-running"]
        );
    }

    #[tokio::test]
    async fn refresh_admin_dashboard_rollups_backfills_recent_rows() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_user(&pool, 2, "operator", 0, 0).await;

        let time_zone = chrono_tz::Asia::Shanghai;
        let now_local = chrono::Utc::now().with_timezone(&time_zone);
        let today_local = now_local.date_naive();
        let task_time = time_zone
            .with_ymd_and_hms(
                today_local.year(),
                today_local.month(),
                today_local.day(),
                9,
                0,
                0,
            )
            .single()
            .expect("build local dashboard task time")
            .with_timezone(&chrono::Utc)
            .to_rfc3339();

        set_last_active_at(&pool, test_user_id(1).as_str(), Some(task_time.as_str())).await;
        set_last_active_at(&pool, test_user_id(2).as_str(), Some(task_time.as_str())).await;

        seed_admin_dashboard_task(
            &pool,
            "dashboard-translate-queued",
            jobs::TASK_TRANSLATE_RELEASE_BATCH,
            jobs::STATUS_QUEUED,
            test_user_id(1).as_str(),
            task_time.as_str(),
        )
        .await;
        seed_admin_dashboard_task(
            &pool,
            "dashboard-summary-running",
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH,
            jobs::STATUS_RUNNING,
            test_user_id(1).as_str(),
            task_time.as_str(),
        )
        .await;
        seed_admin_dashboard_task(
            &pool,
            "dashboard-summary-failed",
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH,
            jobs::STATUS_FAILED,
            test_user_id(1).as_str(),
            task_time.as_str(),
        )
        .await;
        seed_admin_dashboard_task(
            &pool,
            "dashboard-brief-succeeded",
            jobs::TASK_BRIEF_DAILY_SLOT,
            jobs::STATUS_SUCCEEDED,
            test_user_id(2).as_str(),
            task_time.as_str(),
        )
        .await;

        let state = setup_state(pool.clone());
        refresh_admin_dashboard_rollups(state.as_ref(), ADMIN_DASHBOARD_PREAGGREGATE_DAYS)
            .await
            .expect("dashboard rollup refresh should succeed");

        let today_rollup_rows = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM admin_dashboard_daily_rollups
            WHERE time_zone = 'Asia/Shanghai'
              AND rollup_date = ?
            "#,
        )
        .bind(today_local.format("%Y-%m-%d").to_string())
        .fetch_one(&pool)
        .await
        .expect("count persisted dashboard rollups");
        assert_eq!(today_rollup_rows, 3);
    }

    #[tokio::test]
    async fn admin_dashboard_uses_today_live_overlay_and_window_switch() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_user(&pool, 2, "operator", 0, 0).await;

        let time_zone = chrono_tz::Asia::Shanghai;
        let now_local = chrono::Utc::now().with_timezone(&time_zone);
        let today_local = now_local.date_naive();
        let yesterday_local = today_local
            .pred_opt()
            .expect("build previous dashboard day");
        let yesterday_time = time_zone
            .with_ymd_and_hms(
                yesterday_local.year(),
                yesterday_local.month(),
                yesterday_local.day(),
                9,
                0,
                0,
            )
            .single()
            .expect("build yesterday task time")
            .with_timezone(&chrono::Utc)
            .to_rfc3339();
        let today_window_start = time_zone
            .with_ymd_and_hms(
                today_local.year(),
                today_local.month(),
                today_local.day(),
                0,
                0,
                0,
            )
            .single()
            .expect("build today window start");
        let safe_today_local = if now_local > today_window_start + chrono::Duration::minutes(5) {
            now_local - chrono::Duration::minutes(5)
        } else {
            today_window_start + chrono::Duration::seconds(1)
        };
        let today_time = safe_today_local.with_timezone(&chrono::Utc).to_rfc3339();

        set_last_active_at(&pool, test_user_id(1).as_str(), Some(today_time.as_str())).await;
        set_last_active_at(&pool, test_user_id(2).as_str(), Some(today_time.as_str())).await;

        seed_admin_dashboard_task(
            &pool,
            "dashboard-yesterday-translate",
            jobs::TASK_TRANSLATE_RELEASE_BATCH,
            jobs::STATUS_SUCCEEDED,
            test_user_id(1).as_str(),
            yesterday_time.as_str(),
        )
        .await;
        seed_admin_dashboard_task(
            &pool,
            "dashboard-today-translate",
            jobs::TASK_TRANSLATE_RELEASE_BATCH,
            jobs::STATUS_QUEUED,
            test_user_id(1).as_str(),
            today_time.as_str(),
        )
        .await;
        seed_admin_dashboard_task(
            &pool,
            "dashboard-today-summary",
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH,
            jobs::STATUS_RUNNING,
            test_user_id(1).as_str(),
            today_time.as_str(),
        )
        .await;
        seed_admin_dashboard_task(
            &pool,
            "dashboard-today-brief",
            jobs::TASK_BRIEF_DAILY_SLOT,
            jobs::STATUS_SUCCEEDED,
            test_user_id(2).as_str(),
            today_time.as_str(),
        )
        .await;

        sqlx::query(
            r#"
            INSERT INTO admin_dashboard_daily_rollups (
              rollup_date,
              time_zone,
              task_type,
              total_users,
              active_users,
              queued_count,
              running_count,
              succeeded_count,
              failed_count,
              canceled_count,
              updated_at
            )
            VALUES (?, 'Asia/Shanghai', ?, 2, 0, 9, 0, 0, 0, 0, ?)
            "#,
        )
        .bind(today_local.format("%Y-%m-%d").to_string())
        .bind(jobs::TASK_TRANSLATE_RELEASE_BATCH)
        .bind(today_time.as_str())
        .execute(&pool)
        .await
        .expect("seed stale today rollup");

        let state = setup_state(pool.clone());
        refresh_admin_dashboard_rollups(state.as_ref(), ADMIN_DASHBOARD_PREAGGREGATE_DAYS)
            .await
            .expect("dashboard rollup refresh should succeed");
        let session = setup_session(1).await;
        let Json(resp) = admin_dashboard(
            State(state),
            session,
            Query(AdminDashboardQuery {
                window: Some("30d".to_owned()),
            }),
        )
        .await
        .expect("admin dashboard should succeed");

        assert_eq!(resp.time_zone, "Asia/Shanghai");
        assert_eq!(resp.summary.total_users, 2);
        assert_eq!(resp.summary.active_users_today, 2);
        assert_eq!(resp.summary.ongoing_tasks_total, 2);
        assert_eq!(resp.summary.ongoing_by_task.translations, 1);
        assert_eq!(resp.summary.ongoing_by_task.summaries, 1);
        assert_eq!(resp.summary.ongoing_by_task.briefs, 0);
        assert_eq!(
            resp.today_live.date,
            today_local.format("%Y-%m-%d").to_string()
        );
        assert_eq!(resp.status_breakdown.queued_total, 1);
        assert_eq!(resp.status_breakdown.running_total, 1);
        assert_eq!(resp.status_breakdown.succeeded_total, 1);
        assert_eq!(resp.status_breakdown.failed_total, 0);
        assert_eq!(resp.status_breakdown.total, 3);
        assert_eq!(resp.status_breakdown.items.len(), 3);
        assert_eq!(resp.window_meta.selected_window, "30d");
        assert_eq!(resp.window_meta.point_count, 30);
        assert_eq!(resp.trend_points.len(), 30);

        let today_point = resp
            .trend_points
            .iter()
            .find(|point| point.date == today_local.format("%Y-%m-%d").to_string())
            .expect("today trend point should exist");
        assert_eq!(today_point.translations_total, 1);
        assert_eq!(today_point.summaries_total, 1);
        assert_eq!(today_point.briefs_total, 1);
    }

    #[tokio::test]
    async fn admin_list_llm_calls_sort_uses_admin_sort_index() {
        let pool = setup_pool().await;
        let plan_rows = sqlx::query(
            r#"
            EXPLAIN QUERY PLAN
            SELECT
              id,
              status,
              created_at
            FROM llm_calls
            ORDER BY
              CASE
                WHEN status = 'running' THEN 0
                WHEN status = 'queued' THEN 1
                ELSE 2
              END,
              julianday(created_at) DESC,
              created_at DESC,
              id DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(20_i64)
        .bind(0_i64)
        .fetch_all(&pool)
        .await
        .expect("load query plan");

        let details = plan_rows
            .iter()
            .map(|row| row.get::<String, _>(3))
            .collect::<Vec<_>>();
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("idx_llm_calls_admin_sort"))
        );
        assert!(
            !details
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY"))
        );
    }

    #[tokio::test]
    async fn admin_list_llm_calls_grouped_source_filter_uses_source_admin_sort_index() {
        let pool = setup_pool().await;
        let plan_rows = sqlx::query(
            r#"
            EXPLAIN QUERY PLAN
            SELECT
              id,
              status,
              created_at
            FROM llm_calls
            WHERE source = ?
            ORDER BY
              CASE
                WHEN status = 'running' THEN 0
                WHEN status = 'queued' THEN 1
                ELSE 2
              END,
              julianday(created_at) DESC,
              created_at DESC,
              id DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind("api.translate_releases_batch")
        .bind(20_i64)
        .bind(0_i64)
        .fetch_all(&pool)
        .await
        .expect("load query plan");

        let details = plan_rows
            .iter()
            .map(|row| row.get::<String, _>(3))
            .collect::<Vec<_>>();
        assert!(
            details
                .iter()
                .any(|detail| detail.contains("idx_llm_calls_source_admin_sort"))
        );
        assert!(
            !details
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY"))
        );
    }

    #[test]
    fn llm_call_order_by_clause_uses_created_desc_when_status_is_fixed() {
        let scope = AdminLlmCallListScope {
            status: Some("running"),
            source: "",
            requested_by: None,
            parent_task_id: "",
            started_from: None,
            started_to: None,
        };

        assert_eq!(
            llm_call_order_by_clause(&scope, "status_grouped"),
            LLM_CALL_ORDER_BY_CREATED_DESC
        );
    }

    #[tokio::test]
    async fn admin_list_llm_calls_orders_running_then_queued_then_terminal() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call_with_created_at(
            &pool,
            "call-running-old",
            "running",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T02:00:00Z",
        )
        .await;
        seed_llm_call_with_created_at(
            &pool,
            "call-running-new",
            "running",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T03:00:00Z",
        )
        .await;
        seed_llm_call_with_created_at(
            &pool,
            "call-queued",
            "queued",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T05:00:00Z",
        )
        .await;
        seed_llm_call_with_created_at(
            &pool,
            "call-failed",
            "failed",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T06:00:00Z",
        )
        .await;

        let state = setup_state(pool);
        let session = setup_session(1).await;

        let resp = admin_list_llm_calls(
            State(state),
            session,
            Query(AdminLlmCallsQuery {
                status: Some("all".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: Some("status_grouped".to_owned()),
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("admin llm call list should sort by runtime priority")
        .0;

        let ids = resp
            .items
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "call-running-new",
                "call-running-old",
                "call-queued",
                "call-failed",
            ]
        );
    }

    #[tokio::test]
    async fn admin_list_llm_calls_defaults_to_created_desc() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call_with_created_at(
            &pool,
            "call-running-oldest",
            "running",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T02:00:00Z",
        )
        .await;
        seed_llm_call_with_created_at(
            &pool,
            "call-failed-newest",
            "failed",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T06:00:00Z",
        )
        .await;
        seed_llm_call_with_created_at(
            &pool,
            "call-queued-middle",
            "queued",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T04:00:00Z",
        )
        .await;

        let state = setup_state(pool);
        let session = setup_session(1).await;

        let resp = admin_list_llm_calls(
            State(state),
            session,
            Query(AdminLlmCallsQuery {
                status: Some("all".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: None,
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("admin llm call list should keep reverse chronological order by default")
        .0;

        let ids = resp
            .items
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "call-failed-newest",
                "call-queued-middle",
                "call-running-oldest",
            ]
        );
    }

    #[tokio::test]
    async fn admin_list_llm_calls_uses_override_snapshot_fields() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call_with_created_at(
            &pool,
            "call-running-snapshot",
            "running",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T02:00:00Z",
        )
        .await;

        let state = setup_state(pool);
        state
            .llm_scheduler
            .set_admin_override(crate::ai::LlmCallAdminOverride {
                id: "call-running-snapshot".to_owned(),
                status: "succeeded".to_owned(),
                attempt_count: 2,
                scheduler_wait_ms: 180,
                first_token_wait_ms: Some(95),
                duration_ms: Some(450),
                input_tokens: Some(220),
                output_tokens: Some(88),
                cached_input_tokens: Some(12),
                total_tokens: Some(308),
                output_messages_json: Some(
                    r#"[{"role":"assistant","content":"override"}]"#.to_owned(),
                ),
                response_text: Some("override response".to_owned()),
                error_text: None,
                started_at: Some("2026-02-26T02:00:01Z".to_owned()),
                finished_at: Some("2026-02-26T02:00:09Z".to_owned()),
                updated_at: "2026-02-26T02:00:09Z".to_owned(),
            })
            .await;
        let session = setup_session(1).await;

        let resp = admin_list_llm_calls(
            State(state),
            session,
            Query(AdminLlmCallsQuery {
                status: Some("all".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: None,
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("admin llm call list should expose override snapshot fields")
        .0;

        let item = resp
            .items
            .into_iter()
            .next()
            .expect("list item should exist");
        assert_eq!(item.status, "succeeded");
        assert_eq!(item.attempt_count, 2);
        assert_eq!(item.scheduler_wait_ms, 180);
        assert_eq!(item.first_token_wait_ms, Some(95));
        assert_eq!(item.duration_ms, Some(450));
        assert_eq!(item.input_tokens, Some(220));
        assert_eq!(item.output_tokens, Some(88));
        assert_eq!(item.cached_input_tokens, Some(12));
        assert_eq!(item.total_tokens, Some(308));
        assert_eq!(item.started_at.as_deref(), Some("2026-02-26T02:00:01Z"));
        assert_eq!(item.finished_at.as_deref(), Some("2026-02-26T02:00:09Z"));
        assert_eq!(item.updated_at, "2026-02-26T02:00:09Z");
    }

    #[tokio::test]
    async fn admin_list_llm_calls_uses_overrides_for_filter_sort_and_pagination() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call_with_created_at(
            &pool,
            "call-running-old",
            "running",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T02:00:00Z",
        )
        .await;
        seed_llm_call_with_created_at(
            &pool,
            "call-queued-middle",
            "queued",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T04:00:00Z",
        )
        .await;
        seed_llm_call_with_created_at(
            &pool,
            "call-failed-newest",
            "failed",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T06:00:00Z",
        )
        .await;

        let state = setup_state(pool);
        state
            .llm_scheduler
            .set_admin_override(crate::ai::LlmCallAdminOverride {
                id: "call-failed-newest".to_owned(),
                status: "queued".to_owned(),
                attempt_count: 3,
                scheduler_wait_ms: 200,
                first_token_wait_ms: None,
                duration_ms: None,
                input_tokens: None,
                output_tokens: None,
                cached_input_tokens: None,
                total_tokens: None,
                output_messages_json: None,
                response_text: None,
                error_text: None,
                started_at: Some("2026-02-26T06:00:01Z".to_owned()),
                finished_at: None,
                updated_at: "2026-02-26T06:00:05Z".to_owned(),
            })
            .await;

        let first_page = admin_list_llm_calls(
            State(Arc::clone(&state)),
            setup_session(1).await,
            Query(AdminLlmCallsQuery {
                status: Some("queued".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: Some("status_grouped".to_owned()),
                page: Some(1),
                page_size: Some(1),
            }),
        )
        .await
        .expect("page one should include override-backed queued call")
        .0;
        assert_eq!(first_page.total, 2);
        assert_eq!(first_page.items.len(), 1);
        assert_eq!(first_page.items[0].id, "call-failed-newest");
        assert_eq!(first_page.items[0].status, "queued");

        let second_page = admin_list_llm_calls(
            State(state),
            setup_session(1).await,
            Query(AdminLlmCallsQuery {
                status: Some("queued".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: Some("status_grouped".to_owned()),
                page: Some(2),
                page_size: Some(1),
            }),
        )
        .await
        .expect("page two should keep override-aware pagination stable")
        .0;
        assert_eq!(second_page.total, 2);
        assert_eq!(second_page.items.len(), 1);
        assert_eq!(second_page.items[0].id, "call-queued-middle");
    }

    #[tokio::test]
    async fn admin_list_llm_calls_orders_mixed_rfc3339_by_true_created_time() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        for (call_id, created_at) in [
            ("call-zulu", "2026-02-26T05:00:00Z"),
            ("call-subsec", "2026-02-26T05:00:00.500+00:00"),
            ("call-offset", "2026-02-26T05:00:00+00:00"),
        ] {
            seed_llm_call_with_created_at(
                &pool,
                call_id,
                "failed",
                "api.translate_releases_batch",
                Some(test_user_id(1)),
                created_at,
            )
            .await;
        }

        let state = setup_state(pool);
        let default_resp = admin_list_llm_calls(
            State(Arc::clone(&state)),
            setup_session(1).await,
            Query(AdminLlmCallsQuery {
                status: Some("all".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: None,
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("default llm call list should sort by actual created_at time")
        .0;
        let grouped_resp = admin_list_llm_calls(
            State(state),
            setup_session(1).await,
            Query(AdminLlmCallsQuery {
                status: Some("all".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: None,
                started_to: None,
                sort: Some("status_grouped".to_owned()),
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("grouped llm call list should sort by actual created_at time")
        .0;

        let expected = vec!["call-subsec", "call-zulu", "call-offset"];
        assert_eq!(
            default_resp
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            grouped_resp
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[tokio::test]
    async fn admin_list_llm_calls_accepts_zulu_started_filters() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            "call-zulu",
            "succeeded",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
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
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: Some(started_from),
                started_to: Some(started_to),
                sort: None,
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
    async fn admin_list_llm_calls_started_filters_include_override_started_at() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call_with_created_at(
            &pool,
            "call-override-started",
            "running",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
            "2026-02-26T02:00:00Z",
        )
        .await;

        let state = setup_state(pool);
        let override_started_at =
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        state
            .llm_scheduler
            .set_admin_override(crate::ai::LlmCallAdminOverride {
                id: "call-override-started".to_owned(),
                status: "running".to_owned(),
                attempt_count: 1,
                scheduler_wait_ms: 0,
                first_token_wait_ms: None,
                duration_ms: None,
                input_tokens: None,
                output_tokens: None,
                cached_input_tokens: None,
                total_tokens: None,
                output_messages_json: None,
                response_text: None,
                error_text: None,
                started_at: Some(override_started_at.clone()),
                finished_at: None,
                updated_at: override_started_at.clone(),
            })
            .await;

        let started_from = (chrono::DateTime::parse_from_rfc3339(&override_started_at)
            .expect("parse override started_at")
            - chrono::Duration::minutes(1))
        .to_rfc3339();
        let started_to = (chrono::DateTime::parse_from_rfc3339(&override_started_at)
            .expect("parse override started_at")
            + chrono::Duration::minutes(1))
        .to_rfc3339();

        let resp = admin_list_llm_calls(
            State(state),
            setup_session(1).await,
            Query(AdminLlmCallsQuery {
                status: Some("all".to_owned()),
                source: Some("api.translate_releases_batch".to_owned()),
                requested_by: Some(test_user_id(1)),
                parent_task_id: None,
                started_from: Some(started_from),
                started_to: Some(started_to),
                sort: Some("status_grouped".to_owned()),
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("override-backed llm call should respect started_at filters")
        .0;

        assert_eq!(resp.total, 1);
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].id, "call-override-started");
        assert_eq!(
            resp.items[0].started_at.as_deref(),
            Some(override_started_at.as_str())
        );
    }

    #[tokio::test]
    async fn admin_list_llm_calls_filters_parent_task_id() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            "call-parent-a",
            "succeeded",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
        )
        .await;
        seed_llm_call(
            &pool,
            "call-parent-b",
            "succeeded",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
        )
        .await;
        let now = chrono::Utc::now().to_rfc3339();
        let parent_task_a = crate::local_id::test_local_id("task-123");
        let parent_task_b = crate::local_id::test_local_id("task-456");
        for task_id in [&parent_task_a, &parent_task_b] {
            sqlx::query(
                r#"
                INSERT INTO job_tasks (
                  id,
                  task_type,
                  status,
                  source,
                  requested_by,
                  parent_task_id,
                  payload_json,
                  result_json,
                  error_message,
                  cancel_requested,
                  created_at,
                  started_at,
                  finished_at,
                  updated_at
                )
                VALUES (?, 'sync.releases', 'succeeded', 'tests', ?, NULL, '{}', '{}', NULL, 0, ?, ?, ?, ?)
                "#,
            )
            .bind(task_id)
            .bind(test_user_id(1))
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(now.as_str())
            .execute(&pool)
            .await
            .expect("seed parent task");
        }
        sqlx::query(
            r#"
            UPDATE llm_calls
            SET parent_task_id = ?, parent_task_type = ?
            WHERE id = ?
            "#,
        )
        .bind(&parent_task_a)
        .bind("translate.release.batch")
        .bind("call-parent-a")
        .execute(&pool)
        .await
        .expect("set parent task for call-parent-a");
        sqlx::query(
            r#"
            UPDATE llm_calls
            SET parent_task_id = ?, parent_task_type = ?
            WHERE id = ?
            "#,
        )
        .bind(&parent_task_b)
        .bind("sync.releases")
        .bind("call-parent-b")
        .execute(&pool)
        .await
        .expect("set parent task for call-parent-b");

        let state = setup_state(pool);
        let session = setup_session(1).await;

        let resp = admin_list_llm_calls(
            State(state),
            session,
            Query(AdminLlmCallsQuery {
                status: Some("all".to_owned()),
                source: None,
                requested_by: Some(test_user_id(1)),
                parent_task_id: Some(parent_task_a.clone()),
                started_from: None,
                started_to: None,
                sort: None,
                page: Some(1),
                page_size: Some(20),
            }),
        )
        .await
        .expect("admin llm call list should filter by parent_task_id")
        .0;

        assert_eq!(resp.total, 1);
        assert_eq!(resp.items.len(), 1);
        assert_eq!(resp.items[0].id, "call-parent-a");
        assert_eq!(
            resp.items[0].parent_task_id.as_deref(),
            Some(parent_task_a.as_str())
        );
    }

    #[tokio::test]
    async fn admin_get_llm_call_detail_returns_not_found() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        let state = setup_state(pool);
        let session = setup_session(1).await;

        let err = admin_get_llm_call_detail(
            State(state),
            session,
            Path(crate::local_id::test_local_id("missing-call")),
        )
        .await
        .expect_err("missing llm call should fail");
        assert_eq!(err.code(), "not_found");
    }

    #[tokio::test]
    async fn admin_get_llm_call_detail_includes_tokens_and_messages() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            &crate::local_id::test_local_id("call-detail"),
            "succeeded",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
        )
        .await;

        let state = setup_state(pool);
        let session = setup_session(1).await;
        let resp = admin_get_llm_call_detail(
            State(state),
            session,
            Path(crate::local_id::test_local_id("call-detail")),
        )
        .await
        .expect("llm call detail should pass")
        .0;

        assert_eq!(resp.id, crate::local_id::test_local_id("call-detail"));
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
    async fn admin_get_llm_call_detail_uses_admin_override_snapshot() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        let call_id = crate::local_id::test_local_id("call-detail-override");
        seed_llm_call(
            &pool,
            &call_id,
            "running",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
        )
        .await;

        let state = setup_state(pool);
        state
            .llm_scheduler
            .set_admin_override(crate::ai::LlmCallAdminOverride {
                id: call_id.clone(),
                status: "succeeded".to_owned(),
                attempt_count: 4,
                scheduler_wait_ms: 260,
                first_token_wait_ms: Some(75),
                duration_ms: Some(910),
                input_tokens: Some(300),
                output_tokens: Some(120),
                cached_input_tokens: Some(40),
                total_tokens: Some(420),
                output_messages_json: Some(
                    r#"[{"role":"assistant","content":"override detail"}]"#.to_owned(),
                ),
                response_text: Some("override detail response".to_owned()),
                error_text: None,
                started_at: Some("2026-02-26T03:00:01Z".to_owned()),
                finished_at: Some("2026-02-26T03:00:08Z".to_owned()),
                updated_at: "2026-02-26T03:00:08Z".to_owned(),
            })
            .await;
        let session = setup_session(1).await;

        let resp = admin_get_llm_call_detail(State(state), session, Path(call_id))
            .await
            .expect("llm call detail should expose override snapshot")
            .0;

        assert_eq!(resp.status, "succeeded");
        assert_eq!(resp.attempt_count, 4);
        assert_eq!(resp.scheduler_wait_ms, 260);
        assert_eq!(resp.first_token_wait_ms, Some(75));
        assert_eq!(resp.duration_ms, Some(910));
        assert_eq!(resp.input_tokens, Some(300));
        assert_eq!(resp.output_tokens, Some(120));
        assert_eq!(resp.cached_input_tokens, Some(40));
        assert_eq!(resp.total_tokens, Some(420));
        assert_eq!(
            resp.output_messages_json.as_deref(),
            Some(r#"[{"role":"assistant","content":"override detail"}]"#)
        );
        assert_eq!(
            resp.response_text.as_deref(),
            Some("override detail response")
        );
        assert_eq!(resp.started_at.as_deref(), Some("2026-02-26T03:00:01Z"));
        assert_eq!(resp.finished_at.as_deref(), Some("2026-02-26T03:00:08Z"));
        assert_eq!(resp.updated_at, "2026-02-26T03:00:08Z");
    }

    #[tokio::test]
    async fn admin_get_llm_scheduler_status_reads_aggregates() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        seed_llm_call(
            &pool,
            &crate::local_id::test_local_id("call-1"),
            "failed",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
        )
        .await;
        seed_llm_call(
            &pool,
            &crate::local_id::test_local_id("call-2"),
            "succeeded",
            "api.translate_releases_batch",
            Some(test_user_id(1)),
        )
        .await;

        let state = setup_state(pool);
        let session = setup_session(1).await;
        let resp = admin_get_llm_scheduler_status(State(state), session)
            .await
            .expect("status should succeed")
            .0;

        assert_eq!(resp.max_concurrency, 1);
        assert_eq!(resp.available_slots, 1);
        assert_eq!(resp.calls_24h, 2);
        assert_eq!(resp.failed_24h, 1);
        assert!(resp.avg_wait_ms_24h.is_some());
    }

    #[tokio::test]
    async fn admin_get_llm_scheduler_status_syncs_persisted_runtime_settings() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        let now = "2026-03-28T11:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO admin_runtime_settings (
              id,
              llm_max_concurrency,
              ai_model_context_limit,
              translation_general_worker_concurrency,
              translation_dedicated_worker_concurrency,
              created_at,
              updated_at
            )
            VALUES (1, 4, 65536, 3, 1, ?, ?)
            "#,
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert runtime settings");

        let state = setup_state(pool);
        assert_eq!(state.llm_scheduler.max_concurrency(), 1);

        let session = setup_session(1).await;
        let resp = admin_get_llm_scheduler_status(State(state.clone()), session)
            .await
            .expect("status should succeed")
            .0;

        assert_eq!(resp.max_concurrency, 4);
        assert_eq!(resp.ai_model_context_limit, Some(65_536));
        assert_eq!(resp.effective_model_input_limit, 65_536);
        assert_eq!(resp.effective_model_input_limit_source, "admin_override");
        assert_eq!(resp.available_slots, 4);
        assert_eq!(state.llm_scheduler.max_concurrency(), 4);
    }

    #[tokio::test]
    async fn admin_patch_llm_runtime_config_preserves_saved_model_limit_when_field_is_omitted() {
        let pool = setup_pool().await;
        sqlx::query(r#"UPDATE users SET is_admin = 1 WHERE id = ?"#)
            .bind(test_user_id(1))
            .execute(&pool)
            .await
            .expect("promote seeded user to admin");
        let now = "2026-03-28T11:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO admin_runtime_settings (
              id,
              llm_max_concurrency,
              ai_model_context_limit,
              translation_general_worker_concurrency,
              translation_dedicated_worker_concurrency,
              created_at,
              updated_at
            )
            VALUES (1, 4, 65536, 3, 1, ?, ?)
            "#,
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert runtime settings");

        let state = setup_state(pool.clone());
        let session = setup_session(1).await;
        let resp = admin_patch_llm_runtime_config(
            State(state),
            session,
            Json(AdminLlmRuntimeConfigUpdateRequest {
                max_concurrency: 2,
                ai_model_context_limit: None,
            }),
        )
        .await
        .expect("patch runtime settings")
        .0;

        assert_eq!(resp.max_concurrency, 2);
        assert_eq!(resp.ai_model_context_limit, Some(65_536));
        assert_eq!(resp.effective_model_input_limit, 65_536);
        assert_eq!(resp.effective_model_input_limit_source, "admin_override");
    }

    #[tokio::test]
    async fn migration_backfills_earliest_user_as_admin() {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
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
            error: Some(
                "release detail translation failed to preserve markdown structure".to_owned(),
            ),
        };
        let err = translate_response_from_batch_item(item).expect_err("error item should fail");
        assert_eq!(err.code(), "internal_error");
        assert_eq!(err.to_string(), "Markdown 结构校验失败");
    }

    #[test]
    fn translate_response_from_batch_item_maps_upstream_rejection_to_summary() {
        let item = TranslateBatchItem {
            id: "1".to_owned(),
            lang: "zh-CN".to_owned(),
            status: "error".to_owned(),
            title: None,
            summary: None,
            error: Some("ai returned 401: bad key".to_owned()),
        };
        let err = translate_response_from_batch_item(item).expect_err("error item should fail");
        assert_eq!(err.code(), "internal_error");
        assert_eq!(err.to_string(), "上游模型拒绝请求");
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
    fn parse_local_id_param_accepts_trimmed_nanoid() {
        let id = crate::local_id::test_local_id("api-parse-local-id");
        assert_eq!(
            super::parse_local_id_param(format!("  {id}  "), "task_id").expect("local id"),
            id
        );
    }

    #[test]
    fn parse_local_id_param_rejects_malformed_value() {
        let err = super::parse_local_id_param("bad-id".to_owned(), "task_id").expect_err("invalid");
        assert_eq!(err.code(), "bad_request");
        assert_eq!(err.to_string(), "invalid task_id");
    }

    #[test]
    fn task_diagnostics_translate_batch_surfaces_business_failure() {
        let task = test_task_detail_item(
            jobs::TASK_TRANSLATE_RELEASE_BATCH,
            jobs::STATUS_SUCCEEDED,
            r#"{"user_id":1,"release_ids":[291058027,291042015]}"#,
            Some(r#"{"total":2,"ready":0,"missing":0,"disabled":0,"error":2}"#),
            None,
        );
        // Intentionally out of order to verify diagnostics sorts by event id.
        let events = vec![
            test_task_event(
                11,
                "task.progress",
                r#"{"stage":"release","release_id":"291042015","item_status":"error","item_error":"translation failed"}"#,
            ),
            test_task_event(
                10,
                "task.progress",
                r#"{"stage":"release","release_id":"291058027","item_status":"error","item_error":"translation failed"}"#,
            ),
            test_task_event(
                9,
                "task.progress",
                r#"{"stage":"collect","total_releases":2}"#,
            ),
        ];

        let diagnostics = build_task_diagnostics(&task, &events, &[]).expect("diagnostics");
        assert_eq!(diagnostics.business_outcome.code, "failed");
        let translate_diag = diagnostics
            .translate_release_batch
            .expect("translate batch diagnostics");
        assert_eq!(translate_diag.release_total, 2);
        assert_eq!(translate_diag.summary.error, 2);
        assert_eq!(translate_diag.progress.processed, 2);
        assert_eq!(
            translate_diag.progress.last_stage.as_deref(),
            Some("release")
        );
        assert_eq!(translate_diag.items.len(), 2);
        assert_eq!(translate_diag.items[0].release_id, "291058027");
        assert_eq!(
            translate_diag.items[0].item_error.as_deref(),
            Some("translation failed")
        );
    }

    #[test]
    fn task_diagnostics_translate_batch_running_shows_processing() {
        let task = test_task_detail_item(
            jobs::TASK_TRANSLATE_RELEASE_BATCH,
            jobs::STATUS_RUNNING,
            r#"{"user_id":1,"release_ids":[291058027]}"#,
            None,
            None,
        );
        let events = vec![test_task_event(
            1,
            "task.progress",
            r#"{"stage":"collect","total_releases":1}"#,
        )];

        let diagnostics = build_task_diagnostics(&task, &events, &[]).expect("diagnostics");
        assert_eq!(diagnostics.business_outcome.code, "unknown");
        assert_eq!(diagnostics.business_outcome.label, "处理中");
    }

    #[test]
    fn task_diagnostics_daily_slot_aggregates_user_states() {
        let task = test_task_detail_item(
            jobs::TASK_BRIEF_DAILY_SLOT,
            jobs::STATUS_SUCCEEDED,
            r#"{"hour_utc":0}"#,
            Some(r#"{"total":2,"succeeded":1,"failed":1}"#),
            None,
        );
        let events = vec![
            test_task_event(1, "task.progress", r#"{"stage":"collect","total_users":2}"#),
            test_task_event(
                2,
                "task.progress",
                r#"{"stage":"generate","index":1,"total":2,"user_id":1,"key_date":"2026-02-27"}"#,
            ),
            test_task_event(
                3,
                "task.progress",
                r#"{"stage":"user_succeeded","user_id":1,"key_date":"2026-02-27","content_length":1200}"#,
            ),
            test_task_event(
                4,
                "task.progress",
                r#"{"stage":"generate","index":2,"total":2,"user_id":2,"key_date":"2026-02-27"}"#,
            ),
            test_task_event(
                5,
                "task.progress",
                r#"{"stage":"user_failed","user_id":2,"error":"ai timeout"}"#,
            ),
            test_task_event(
                6,
                "task.progress",
                r#"{"stage":"summary","total":2,"succeeded":1,"failed":1,"canceled":false}"#,
            ),
        ];

        let diagnostics = build_task_diagnostics(&task, &events, &[]).expect("diagnostics");
        assert_eq!(diagnostics.business_outcome.code, "partial");
        let slot_diag = diagnostics
            .brief_daily_slot
            .expect("daily slot diagnostics");
        assert_eq!(slot_diag.summary.total_users, 2);
        assert_eq!(slot_diag.summary.progressed_users, 2);
        assert_eq!(slot_diag.summary.succeeded_users, 1);
        assert_eq!(slot_diag.summary.failed_users, 1);
        assert_eq!(slot_diag.users.len(), 2);
        assert_eq!(slot_diag.users[0].user_id, "1");
        assert_eq!(slot_diag.users[0].state, "succeeded");
        assert_eq!(slot_diag.users[1].user_id, "2");
        assert_eq!(slot_diag.users[1].state, "failed");
        assert_eq!(slot_diag.users[1].error.as_deref(), Some("ai timeout"));
    }

    #[test]
    fn task_diagnostics_daily_slot_canceled_shows_canceled_outcome() {
        let task = test_task_detail_item(
            jobs::TASK_BRIEF_DAILY_SLOT,
            jobs::STATUS_CANCELED,
            r#"{"hour_utc":8}"#,
            Some(r#"{"total":4,"succeeded":1,"failed":1,"canceled":true}"#),
            None,
        );
        let events = vec![test_task_event(
            1,
            "task.progress",
            r#"{"stage":"summary","total":4,"succeeded":1,"failed":1,"canceled":true}"#,
        )];

        let diagnostics = build_task_diagnostics(&task, &events, &[]).expect("diagnostics");
        assert_eq!(diagnostics.business_outcome.code, "partial");
        assert_eq!(diagnostics.business_outcome.label, "已取消");
    }

    #[test]
    fn task_diagnostics_sync_subscriptions_warning_events_do_not_force_partial_outcome() {
        let task = test_task_detail_item(
            jobs::TASK_SYNC_SUBSCRIPTIONS,
            jobs::STATUS_SUCCEEDED,
            r#"{"trigger":"schedule","schedule_key":"2026-03-06T14:30"}"#,
            Some(
                r#"{"skipped":false,"skip_reason":null,"star":{"total_users":12,"succeeded_users":12,"failed_users":0,"total_repos":8},"release":{"total_repos":8,"succeeded_repos":8,"failed_repos":0,"candidate_failures":2},"social":{"total_users":12,"succeeded_users":12,"failed_users":0,"repo_stars":18,"followers":4,"events":22},"notifications":{"total_users":12,"succeeded_users":12,"failed_users":0,"notifications":35},"releases_written":42,"critical_events":0}"#,
            ),
            None,
        );
        let subscription_events = vec![AdminSyncSubscriptionEventItem {
            id: "1".to_owned(),
            stage: "release".to_owned(),
            event_type: "rate_limited".to_owned(),
            severity: "warning".to_owned(),
            recoverable: true,
            attempt: 1,
            user_id: Some("7".to_owned()),
            repo_id: Some(9),
            repo_full_name: Some("octo/alpha".to_owned()),
            message: Some("retryable release sync error for octo/alpha with user #7".to_owned()),
            created_at: "2026-03-06T14:30:01Z".to_owned(),
        }];

        let diagnostics =
            build_task_diagnostics(&task, &[], &subscription_events).expect("diagnostics");
        assert_eq!(diagnostics.business_outcome.code, "ok");
        assert_eq!(diagnostics.business_outcome.label, "业务成功");
        let sync_diag = diagnostics
            .sync_subscriptions
            .expect("sync subscriptions diagnostics");
        assert_eq!(sync_diag.critical_events, 0);
        assert_eq!(sync_diag.social.succeeded_users, 12);
        assert_eq!(sync_diag.notifications.notifications, 35);
        assert_eq!(sync_diag.recent_events.len(), 1);
        assert_eq!(sync_diag.recent_events[0].severity, "warning");
    }

    #[test]
    fn task_diagnostics_sync_subscriptions_surfaces_recent_events_and_log_download() {
        let log_dir = std::env::temp_dir().join(format!(
            "octo-rill-sync-diagnostics-{}",
            crate::local_id::generate_local_id()
        ));
        fs::create_dir_all(&log_dir).expect("create log dir");
        let log_path = log_dir.join("task-test.ndjson");
        fs::write(
            &log_path,
            r#"{"event":"started"}
"#,
        )
        .expect("write task log");

        let mut task = test_task_detail_item(
            jobs::TASK_SYNC_SUBSCRIPTIONS,
            jobs::STATUS_SUCCEEDED,
            r#"{"trigger":"schedule","schedule_key":"2026-03-06T14:30"}"#,
            Some(
                r#"{"skipped":false,"skip_reason":null,"star":{"total_users":12,"succeeded_users":10,"failed_users":2,"total_repos":8},"release":{"total_repos":8,"succeeded_repos":7,"failed_repos":1,"candidate_failures":3},"social":{"total_users":10,"succeeded_users":9,"failed_users":1,"repo_stars":48,"followers":19,"events":67},"notifications":{"total_users":10,"succeeded_users":8,"failed_users":2,"notifications":192},"releases_written":42,"critical_events":2}"#,
            ),
            None,
        );
        task.log_file_path = Some(log_path.to_string_lossy().into_owned());
        let subscription_events = vec![
            AdminSyncSubscriptionEventItem {
                id: "7".to_owned(),
                stage: "release".to_owned(),
                event_type: "repo_unreachable".to_owned(),
                severity: "error".to_owned(),
                recoverable: false,
                attempt: 0,
                user_id: None,
                repo_id: Some(9001),
                repo_full_name: Some("octo/private-repo".to_owned()),
                message: Some("all candidates failed for octo/private-repo".to_owned()),
                created_at: "2026-03-06T14:31:40Z".to_owned(),
            },
            AdminSyncSubscriptionEventItem {
                id: "6".to_owned(),
                stage: "star".to_owned(),
                event_type: "network_error".to_owned(),
                severity: "warning".to_owned(),
                recoverable: true,
                attempt: 1,
                user_id: Some("23".to_owned()),
                repo_id: None,
                repo_full_name: None,
                message: Some("failed to refresh starred repositories for user #23".to_owned()),
                created_at: "2026-03-06T14:30:10Z".to_owned(),
            },
        ];

        let diagnostics =
            build_task_diagnostics(&task, &[], &subscription_events).expect("diagnostics");
        assert_eq!(diagnostics.business_outcome.code, "partial");
        let sync_diag = diagnostics
            .sync_subscriptions
            .expect("sync subscriptions diagnostics");
        assert_eq!(sync_diag.trigger.as_deref(), Some("schedule"));
        assert_eq!(sync_diag.schedule_key.as_deref(), Some("2026-03-06T14:30"));
        assert!(sync_diag.log_available);
        assert_eq!(
            sync_diag.log_download_path.as_deref(),
            Some("/api/admin/jobs/realtime/task-test/log")
        );
        assert_eq!(sync_diag.star.succeeded_users, 10);
        assert_eq!(sync_diag.release.failed_repos, 1);
        assert_eq!(sync_diag.social.failed_users, 1);
        assert_eq!(sync_diag.notifications.failed_users, 2);
        assert_eq!(sync_diag.releases_written, 42);
        assert_eq!(sync_diag.critical_events, 2);
        assert_eq!(sync_diag.recent_events.len(), 2);
        assert_eq!(sync_diag.recent_events[0].event_type, "repo_unreachable");

        fs::remove_file(&log_path).ok();
        fs::remove_dir_all(&log_dir).ok();
    }

    fn fixed_local_id(prefix: char, n: usize) -> String {
        const ALPHABET: &[u8] = crate::local_id::LOCAL_ID_ALPHABET;
        let mut value = n;
        let mut suffix = ['2'; 15];
        for idx in (0..15).rev() {
            suffix[idx] = ALPHABET[value % ALPHABET.len()] as char;
            value /= ALPHABET.len();
        }
        let mut id = String::with_capacity(16);
        id.push(prefix);
        for ch in suffix {
            id.push(ch);
        }
        id
    }

    fn indexed_created_at(idx: usize) -> String {
        let hours = 14 + (idx / 3600);
        let minutes = (idx / 60) % 60;
        let seconds = idx % 60;
        format!("2026-03-06T{hours:02}:{minutes:02}:{seconds:02}Z")
    }

    #[tokio::test]
    async fn admin_get_realtime_task_detail_uses_latest_task_events_window() {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "admin", 1, 0).await;
        let state = setup_state(pool.clone());
        let session = setup_session(2).await;
        let task_id = crate::local_id::test_local_id("task-detail-events-window");
        let now = "2026-03-06T14:30:00Z";
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at,
              log_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id.as_str())
        .bind(jobs::TASK_TRANSLATE_RELEASE_BATCH)
        .bind(jobs::STATUS_SUCCEEDED)
        .bind("tests")
        .bind(test_user_id(2))
        .bind(Option::<String>::None)
        .bind(r#"{"user_id":"scope22222222222","release_ids":[1]}"#)
        .bind(r#"{"total":1,"ready":1,"missing":0,"disabled":0,"error":0}"#)
        .bind(Option::<String>::None)
        .bind(0_i64)
        .bind(now)
        .bind(Some(now))
        .bind(Some(now))
        .bind(now)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("insert task row");

        for idx in 0..ADMIN_TASK_DETAIL_EVENT_LIMIT {
            let created_at = indexed_created_at(idx as usize);
            sqlx::query(
                r#"
                INSERT INTO job_task_events (id, task_id, event_type, payload_json, created_at)
                VALUES (?, ?, 'task.progress', ?, ?)
                "#,
            )
            .bind(fixed_local_id('z', idx as usize).as_str())
            .bind(task_id.as_str())
            .bind(format!(
                r#"{{"stage":"release","release_id":"{idx}","item_status":"ready"}}"#
            ))
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("insert early event");
        }

        sqlx::query(
            r#"
            INSERT INTO job_task_events (id, task_id, event_type, payload_json, created_at)
            VALUES (?, ?, 'task.progress', ?, ?)
            "#,
        )
        .bind("2222222222222222")
        .bind(task_id.as_str())
        .bind(r#"{"stage":"release","release_id":"latest","item_status":"ready"}"#)
        .bind("2026-03-06T14:31:00Z")
        .execute(&pool)
        .await
        .expect("insert latest event");

        let response = admin_get_realtime_task_detail(State(state), session, Path(task_id.clone()))
            .await
            .expect("task detail")
            .0;

        assert_eq!(response.event_meta.returned, ADMIN_TASK_DETAIL_EVENT_LIMIT);
        assert!(response.event_meta.truncated);
        assert!(
            response
                .events
                .iter()
                .any(|event| event.payload_json.contains("latest"))
        );
        let diagnostics = response.diagnostics.expect("diagnostics");
        let translate = diagnostics
            .translate_release_batch
            .expect("translate diagnostics");
        assert!(
            translate
                .items
                .iter()
                .any(|item| item.release_id == "latest")
        );
    }

    #[tokio::test]
    async fn admin_get_realtime_task_detail_maps_brief_history_recompute_progress() {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "admin", 1, 0).await;
        let state = setup_state(pool.clone());
        let session = setup_session(2).await;
        let task_id = crate::local_id::test_local_id("task-detail-brief-history");
        let now = "2026-03-06T14:30:00Z";
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at,
              log_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id.as_str())
        .bind(jobs::TASK_BRIEF_HISTORY_RECOMPUTE)
        .bind(jobs::STATUS_RUNNING)
        .bind("tests")
        .bind(test_user_id(2))
        .bind(Option::<String>::None)
        .bind("{}")
        .bind(r#"{"total":3,"processed":2,"succeeded":1,"failed":1,"canceled":false}"#)
        .bind(Option::<String>::None)
        .bind(0_i64)
        .bind(now)
        .bind(Some(now))
        .bind(Option::<String>::None)
        .bind(now)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("insert task row");

        for (event_id, payload_json, created_at) in [
            (
                "1111111111111111",
                r#"{"stage":"recompute","brief_id":"brief_old"}"#,
                "2026-03-06T14:30:01Z",
            ),
            (
                "2222222222222222",
                r#"{"stage":"brief_failed","brief_id":"brief_latest","error":"boom"}"#,
                "2026-03-06T14:30:02Z",
            ),
        ] {
            sqlx::query(
                r#"
                INSERT INTO job_task_events (id, task_id, event_type, payload_json, created_at)
                VALUES (?, ?, 'task.progress', ?, ?)
                "#,
            )
            .bind(event_id)
            .bind(task_id.as_str())
            .bind(payload_json)
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("insert task event");
        }

        let response = admin_get_realtime_task_detail(State(state), session, Path(task_id))
            .await
            .expect("task detail")
            .0;

        let diagnostics = response.diagnostics.expect("diagnostics");
        let brief_history = diagnostics
            .brief_history_recompute
            .expect("brief history diagnostics");
        assert_eq!(brief_history.total, 3);
        assert_eq!(brief_history.processed, 2);
        assert_eq!(brief_history.succeeded, 1);
        assert_eq!(brief_history.failed, 1);
        assert_eq!(
            brief_history.current_brief_id.as_deref(),
            Some("brief_latest")
        );
        assert_eq!(brief_history.last_error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn admin_get_realtime_task_detail_uses_summary_event_for_failed_brief_history_recompute()
    {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "admin", 1, 0).await;
        let state = setup_state(pool.clone());
        let session = setup_session(2).await;
        let task_id = crate::local_id::test_local_id("task-detail-brief-history-failed");
        let now = "2026-03-06T14:30:00Z";
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at,
              log_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id.as_str())
        .bind(jobs::TASK_BRIEF_HISTORY_RECOMPUTE)
        .bind(jobs::STATUS_FAILED)
        .bind("tests")
        .bind(test_user_id(2))
        .bind(Option::<String>::None)
        .bind("{}")
        .bind(Option::<String>::None)
        .bind(Some("all failed"))
        .bind(0_i64)
        .bind(now)
        .bind(Some(now))
        .bind(Some("2026-03-06T14:31:00Z"))
        .bind("2026-03-06T14:31:00Z")
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("insert failed task row");

        for (event_id, payload_json, created_at) in [
            (
                "3333333333333333",
                r#"{"stage":"brief_failed","brief_id":"brief_legacy_01","error":"boom"}"#,
                "2026-03-06T14:30:02Z",
            ),
            (
                "4444444444444444",
                r#"{"stage":"summary","total":4,"processed":4,"succeeded":0,"failed":4,"canceled":false}"#,
                "2026-03-06T14:31:00Z",
            ),
        ] {
            sqlx::query(
                r#"
                INSERT INTO job_task_events (id, task_id, event_type, payload_json, created_at)
                VALUES (?, ?, 'task.progress', ?, ?)
                "#,
            )
            .bind(event_id)
            .bind(task_id.as_str())
            .bind(payload_json)
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("insert task event");
        }

        let response = admin_get_realtime_task_detail(State(state), session, Path(task_id))
            .await
            .expect("task detail")
            .0;

        let diagnostics = response.diagnostics.expect("diagnostics");
        let brief_history = diagnostics
            .brief_history_recompute
            .expect("brief history diagnostics");
        assert_eq!(brief_history.total, 4);
        assert_eq!(brief_history.processed, 4);
        assert_eq!(brief_history.succeeded, 0);
        assert_eq!(brief_history.failed, 4);
        assert_eq!(brief_history.current_brief_id, None);
        assert_eq!(brief_history.last_error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn admin_get_realtime_task_detail_maps_brief_refresh_content_progress() {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "admin", 1, 0).await;
        let state = setup_state(pool.clone());
        let session = setup_session(2).await;
        let task_id = crate::local_id::test_local_id("task-detail-brief-refresh");
        let now = "2026-03-06T14:30:00Z";
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at,
              log_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id.as_str())
        .bind(jobs::TASK_BRIEF_REFRESH_CONTENT)
        .bind(jobs::STATUS_RUNNING)
        .bind("tests")
        .bind(test_user_id(2))
        .bind(Option::<String>::None)
        .bind("{}")
        .bind(r#"{"total":5,"processed":3,"succeeded":2,"failed":1,"canceled":false}"#)
        .bind(Option::<String>::None)
        .bind(0_i64)
        .bind(now)
        .bind(Some(now))
        .bind(Option::<String>::None)
        .bind(now)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("insert task row");

        for (event_id, payload_json, created_at) in [
            (
                "5555555555555555",
                r#"{"stage":"refresh","brief_id":"brief_old"}"#,
                "2026-03-06T14:30:01Z",
            ),
            (
                "6666666666666666",
                r#"{"stage":"brief_failed","brief_id":"brief_latest","error":"boom"}"#,
                "2026-03-06T14:30:02Z",
            ),
        ] {
            sqlx::query(
                r#"
                INSERT INTO job_task_events (id, task_id, event_type, payload_json, created_at)
                VALUES (?, ?, 'task.progress', ?, ?)
                "#,
            )
            .bind(event_id)
            .bind(task_id.as_str())
            .bind(payload_json)
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("insert task event");
        }

        let response = admin_get_realtime_task_detail(State(state), session, Path(task_id))
            .await
            .expect("task detail")
            .0;

        let diagnostics = response.diagnostics.expect("diagnostics");
        let brief_refresh = diagnostics
            .brief_refresh_content
            .expect("brief refresh diagnostics");
        assert_eq!(brief_refresh.total, 5);
        assert_eq!(brief_refresh.processed, 3);
        assert_eq!(brief_refresh.succeeded, 2);
        assert_eq!(brief_refresh.failed, 1);
        assert_eq!(
            brief_refresh.current_brief_id.as_deref(),
            Some("brief_latest")
        );
        assert_eq!(brief_refresh.last_error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn admin_get_realtime_task_detail_reads_brief_refresh_collect_total_while_running() {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "admin", 1, 0).await;
        let state = setup_state(pool.clone());
        let session = setup_session(2).await;
        let task_id = crate::local_id::test_local_id("task-detail-brief-refresh-collect");
        let now = "2026-03-06T14:30:00Z";
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at,
              log_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id.as_str())
        .bind(jobs::TASK_BRIEF_REFRESH_CONTENT)
        .bind(jobs::STATUS_RUNNING)
        .bind("tests")
        .bind(test_user_id(2))
        .bind(Option::<String>::None)
        .bind("{}")
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(0_i64)
        .bind(now)
        .bind(Some(now))
        .bind(Option::<String>::None)
        .bind(now)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("insert task row");

        for (event_id, payload_json, created_at) in [
            (
                "7777777777777777",
                r#"{"stage":"collect","total_briefs":5}"#,
                "2026-03-06T14:30:01Z",
            ),
            (
                "8888888888888888",
                r#"{"stage":"refresh","brief_id":"brief_running","index":2,"total":5}"#,
                "2026-03-06T14:30:02Z",
            ),
            (
                "9999999999999999",
                r#"{"stage":"brief_succeeded","brief_id":"brief_done","index":1,"total":5}"#,
                "2026-03-06T14:30:03Z",
            ),
        ] {
            sqlx::query(
                r#"
                INSERT INTO job_task_events (id, task_id, event_type, payload_json, created_at)
                VALUES (?, ?, 'task.progress', ?, ?)
                "#,
            )
            .bind(event_id)
            .bind(task_id.as_str())
            .bind(payload_json)
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("insert task event");
        }

        let response = admin_get_realtime_task_detail(State(state), session, Path(task_id))
            .await
            .expect("task detail")
            .0;

        let diagnostics = response.diagnostics.expect("diagnostics");
        let brief_refresh = diagnostics
            .brief_refresh_content
            .expect("brief refresh diagnostics");
        assert_eq!(brief_refresh.total, 5);
        assert_eq!(brief_refresh.processed, 2);
        assert_eq!(brief_refresh.succeeded, 1);
        assert_eq!(brief_refresh.failed, 0);
        assert_eq!(
            brief_refresh.current_brief_id.as_deref(),
            Some("brief_done")
        );
        assert_eq!(brief_refresh.last_error, None);
    }

    #[tokio::test]
    async fn admin_get_realtime_task_detail_uses_latest_subscription_events_window() {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "admin", 1, 0).await;
        let state = setup_state(pool.clone());
        let session = setup_session(2).await;
        let task_id = crate::local_id::test_local_id("task-detail-subscription-window");
        let now = "2026-03-06T14:30:00Z";
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at,
              log_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id.as_str())
        .bind(jobs::TASK_SYNC_SUBSCRIPTIONS)
        .bind(jobs::STATUS_SUCCEEDED)
        .bind("tests")
        .bind(test_user_id(2))
        .bind(Option::<String>::None)
        .bind(r#"{"trigger":"schedule","schedule_key":"2026-03-06T14:30"}"#)
        .bind(r#"{"skipped":false,"star":{"total_users":1,"succeeded_users":1,"failed_users":0,"total_repos":1},"release":{"total_repos":1,"succeeded_repos":1,"failed_repos":0,"candidate_failures":0},"social":{"total_users":1,"succeeded_users":1,"failed_users":0,"repo_stars":2,"followers":1,"events":3},"notifications":{"total_users":1,"succeeded_users":1,"failed_users":0,"notifications":4},"releases_written":1,"critical_events":1}"#)
        .bind(Option::<String>::None)
        .bind(0_i64)
        .bind(now)
        .bind(Some(now))
        .bind(Some(now))
        .bind(now)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("insert task row");

        for idx in 0..ADMIN_SYNC_SUBSCRIPTION_EVENT_LIMIT {
            let created_at = indexed_created_at(idx as usize);
            sqlx::query(
                r#"
                INSERT INTO sync_subscription_events (
                  id, task_id, stage, event_type, severity, recoverable, attempt,
                  user_id, repo_id, repo_full_name, payload_json, created_at
                ) VALUES (?, ?, 'release', ?, 'warning', 1, 1, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(fixed_local_id('z', idx as usize).as_str())
            .bind(task_id.as_str())
            .bind(format!("event-{idx}"))
            .bind(test_user_id(2))
            .bind(9000_i64 + idx)
            .bind(format!("octo/repo-{idx}"))
            .bind(format!(r#"{{"message":"event-{idx}"}}"#))
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("insert early subscription event");
        }

        sqlx::query(
            r#"
            INSERT INTO sync_subscription_events (
              id, task_id, stage, event_type, severity, recoverable, attempt,
              user_id, repo_id, repo_full_name, payload_json, created_at
            ) VALUES (?, ?, 'release', 'latest-event', 'error', 0, 0, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("2222222222222222")
        .bind(task_id.as_str())
        .bind(test_user_id(2))
        .bind(9999_i64)
        .bind("octo/latest")
        .bind(r#"{"message":"latest-event"}"#)
        .bind("2026-03-06T14:31:00Z")
        .execute(&pool)
        .await
        .expect("insert latest subscription event");

        let response = admin_get_realtime_task_detail(State(state), session, Path(task_id))
            .await
            .expect("task detail")
            .0;

        let diagnostics = response.diagnostics.expect("diagnostics");
        let sync = diagnostics.sync_subscriptions.expect("sync diagnostics");
        assert_eq!(
            sync.recent_events.len(),
            ADMIN_SYNC_SUBSCRIPTION_EVENT_LIMIT as usize
        );
        assert!(
            sync.recent_events
                .iter()
                .any(|event| event.event_type == "latest-event")
        );
        assert_eq!(sync.social.events, 3);
        assert_eq!(sync.notifications.notifications, 4);
    }

    #[tokio::test]
    async fn admin_download_realtime_task_log_returns_ndjson_attachment() {
        let pool = setup_pool().await;
        seed_user(&pool, 2, "admin", 1, 0).await;
        let state = setup_state(pool.clone());
        let session = setup_session(2).await;
        fs::create_dir_all(&state.config.task_log_dir).expect("create task log dir");
        let log_path = state
            .config
            .task_log_dir
            .join(format!("{}.ndjson", crate::local_id::generate_local_id()));
        let log_body = r#"{"event":"started"}
{"event":"finished"}
"#;
        fs::write(&log_path, log_body).expect("write task log");

        let now = "2026-03-06T14:30:00Z";
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at,
              log_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(crate::local_id::test_local_id("task-log-download"))
        .bind(jobs::TASK_SYNC_SUBSCRIPTIONS)
        .bind(jobs::STATUS_SUCCEEDED)
        .bind("scheduler")
        .bind(test_user_id(2))
        .bind(Option::<String>::None)
        .bind(r#"{"trigger":"schedule","schedule_key":"2026-03-06T14:30"}"#)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(0_i64)
        .bind(now)
        .bind(Some(now))
        .bind(Some(now))
        .bind(now)
        .bind(log_path.to_string_lossy().to_string())
        .execute(&pool)
        .await
        .expect("insert task row");

        let response = admin_download_realtime_task_log(
            State(state.clone()),
            session,
            Path(crate::local_id::test_local_id("task-log-download")),
        )
        .await
        .expect("download task log")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&header::HeaderValue::from_static("application/x-ndjson"))
        );
        let expected_disposition = header::HeaderValue::from_str(&format!(
            r#"attachment; filename="{}.ndjson""#,
            crate::local_id::test_local_id("task-log-download")
        ))
        .expect("content disposition header");
        assert_eq!(
            response.headers().get(header::CONTENT_DISPOSITION),
            Some(&expected_disposition)
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read task log body");
        assert_eq!(std::str::from_utf8(&body).expect("body utf8"), log_body);

        fs::remove_file(&log_path).ok();
    }

    #[test]
    fn task_diagnostics_brief_generate_canceled_shows_canceled_outcome() {
        let task = test_task_detail_item(
            jobs::TASK_BRIEF_GENERATE,
            jobs::STATUS_CANCELED,
            r#"{"user_id":7,"key_date":"2026-02-27"}"#,
            Some(r#"{"content_length":200}"#),
            None,
        );

        let diagnostics = build_task_diagnostics(&task, &[], &[]).expect("diagnostics");
        assert_eq!(diagnostics.business_outcome.code, "partial");
        assert_eq!(diagnostics.business_outcome.label, "已取消");
        let brief_generate = diagnostics
            .brief_generate
            .expect("brief generate diagnostics");
        assert_eq!(brief_generate.target_user_id, Some("7".to_owned()));
        assert_eq!(brief_generate.content_length, Some(200));
    }

    #[test]
    fn release_cache_entry_reusable_accepts_title_only_cache() {
        let cache = TranslationCacheRow {
            entity_id: "1".to_owned(),
            source_hash: "hash".to_owned(),
            status: "ready".to_owned(),
            title: Some("标题".to_owned()),
            summary: None,
            error_text: None,
        };
        assert!(release_cache_entry_reusable(&cache, "body excerpt"));
    }

    #[test]
    fn release_cache_entry_reusable_rejects_json_blob_summary() {
        let cache = TranslationCacheRow {
            entity_id: "1".to_owned(),
            source_hash: "hash".to_owned(),
            status: "ready".to_owned(),
            title: Some("标题".to_owned()),
            summary: Some("{\"title_zh\":\"标题\"}".to_owned()),
            error_text: None,
        };
        assert!(looks_like_json_blob(cache.summary.as_deref().unwrap_or("")));
        assert!(!release_cache_entry_reusable(&cache, "body excerpt"));
    }

    #[test]
    fn feed_item_from_row_keeps_terminal_missing_non_retryable() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.title = Some("Release v1.2.3".to_owned());
        row.release_body = Some("- item".to_owned());
        let body = release_feed_body(row.release_body.as_deref()).expect("release body");
        let source = format!(
            "v=5\nkind=release\nrepo={}\ntitle={}\nbody={}\n",
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            body,
        );
        row.trans_source_hash = Some(ai::sha256_hex(&source));
        row.trans_status = Some("missing".to_owned());

        let item = feed_item_from_row(row, true, None);
        let translated = item.translated.expect("translated item");
        assert_eq!(translated.status, "missing");
        assert_eq!(translated.auto_translate, None);
    }

    #[test]
    fn feed_item_from_row_keeps_terminal_error_non_retryable() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.title = Some("Release v1.2.3".to_owned());
        row.release_body = Some("- item".to_owned());
        let body = release_feed_body(row.release_body.as_deref()).expect("release body");
        let source = format!(
            "v=5\nkind=release\nrepo={}\ntitle={}\nbody={}\n",
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            body,
        );
        row.trans_source_hash = Some(ai::sha256_hex(&source));
        row.trans_status = Some("error".to_owned());
        row.trans_error_text =
            Some("release translation failed to preserve markdown structure".to_owned());

        let item = feed_item_from_row(row, true, None);
        let translated = item.translated.expect("translated item");
        assert_eq!(translated.status, "error");
        assert_eq!(
            translated.error_code.as_deref(),
            Some("markdown_structure_mismatch")
        );
        assert_eq!(
            translated.error_summary.as_deref(),
            Some("Markdown 结构校验失败")
        );
        assert_eq!(
            translated.error_detail.as_deref(),
            Some("release translation failed to preserve markdown structure")
        );
        assert_eq!(translated.auto_translate, Some(false));
    }

    #[test]
    fn feed_item_from_row_uses_release_detail_translation_for_truncated_release() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.title = Some("Release v1.2.4".to_owned());
        row.release_body = Some("a".repeat(RELEASE_FEED_BODY_MAX_CHARS + 1));
        row.detail_trans_source_hash = Some(release_detail_source_hash(
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            row.release_body.as_deref().unwrap_or(""),
        ));
        row.detail_trans_status = Some("ready".to_owned());
        row.detail_trans_title = Some("中文标题".to_owned());
        row.detail_trans_summary = Some("- 分块译文".to_owned());

        let item = feed_item_from_row(row, true, None);
        let translated = item.translated.expect("translated item");
        assert_eq!(translated.status, "ready");
        assert_eq!(translated.title.as_deref(), Some("中文标题"));
        assert_eq!(translated.summary.as_deref(), Some("- 分块译文"));
        assert_eq!(translated.auto_translate, None);
        assert!(item.body_truncated);
    }

    #[test]
    fn feed_item_from_row_falls_back_to_legacy_ready_translation_when_detail_failed() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.title = Some("Release v1.2.5".to_owned());
        row.release_body = Some("- item".to_owned());
        let body = release_feed_body(row.release_body.as_deref()).expect("release body");
        let source = format!(
            "v=5\nkind=release\nrepo={}\ntitle={}\nbody={}\n",
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            body,
        );
        row.trans_source_hash = Some(ai::sha256_hex(&source));
        row.trans_status = Some("ready".to_owned());
        row.trans_title = Some("旧译文标题".to_owned());
        row.trans_summary = Some("- 旧译文".to_owned());
        row.detail_trans_source_hash = Some(release_detail_source_hash(
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            row.release_body.as_deref().unwrap_or(""),
        ));
        row.detail_trans_status = Some("error".to_owned());

        let item = feed_item_from_row(row, true, None);
        let translated = item.translated.expect("translated item");
        assert_eq!(translated.status, "ready");
        assert_eq!(translated.title.as_deref(), Some("旧译文标题"));
        assert_eq!(translated.summary.as_deref(), Some("- 旧译文"));
    }

    #[test]
    fn feed_item_from_row_falls_back_to_legacy_ready_translation_when_detail_ready_is_invalid() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.title = Some("Release v1.2.5".to_owned());
        row.release_body = Some("- item".to_owned());
        let body = release_feed_body(row.release_body.as_deref()).expect("release body");
        let source = format!(
            "v=5\nkind=release\nrepo={}\ntitle={}\nbody={}\n",
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            body,
        );
        row.trans_source_hash = Some(ai::sha256_hex(&source));
        row.trans_status = Some("ready".to_owned());
        row.trans_title = Some("旧译文标题".to_owned());
        row.trans_summary = Some("- 旧译文".to_owned());
        row.detail_trans_source_hash = Some(release_detail_source_hash(
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            row.release_body.as_deref().unwrap_or(""),
        ));
        row.detail_trans_status = Some("ready".to_owned());
        row.detail_trans_title = Some("坏掉的详情译文".to_owned());
        row.detail_trans_summary = None;

        let item = feed_item_from_row(row, true, None);
        let translated = item.translated.expect("translated item");
        assert_eq!(translated.status, "ready");
        assert_eq!(translated.title.as_deref(), Some("旧译文标题"));
        assert_eq!(translated.summary.as_deref(), Some("- 旧译文"));
        assert_eq!(translated.auto_translate, None);
    }

    #[test]
    fn feed_item_from_row_does_not_fall_back_to_legacy_ready_translation_when_truncated_detail_failed()
     {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.title = Some("Release v1.2.6".to_owned());
        row.release_body = Some("a".repeat(RELEASE_FEED_BODY_MAX_CHARS + 1));
        let body = release_feed_body(row.release_body.as_deref()).expect("release body");
        let source = format!(
            "v=5
kind=release
repo={}
title={}
body={}
",
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            body,
        );
        row.trans_source_hash = Some(ai::sha256_hex(&source));
        row.trans_status = Some("ready".to_owned());
        row.trans_title = Some("旧译文标题".to_owned());
        row.trans_summary = Some("- 旧译文".to_owned());
        row.detail_trans_source_hash = Some(release_detail_source_hash(
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            row.release_body.as_deref().unwrap_or(""),
        ));
        row.detail_trans_status = Some("error".to_owned());

        let item = feed_item_from_row(row, true, None);
        let translated = item.translated.expect("translated item");
        assert!(item.body_truncated);
        assert_eq!(translated.status, "error");
        assert_eq!(translated.auto_translate, Some(false));
        assert!(translated.title.is_none());
        assert!(translated.summary.is_none());
    }

    #[test]
    fn feed_item_from_row_marks_invalid_detail_ready_as_terminal_error_when_truncated() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.title = Some("Release v1.2.6".to_owned());
        row.release_body = Some("a".repeat(RELEASE_FEED_BODY_MAX_CHARS + 1));
        row.detail_trans_source_hash = Some(release_detail_source_hash(
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            row.release_body.as_deref().unwrap_or(""),
        ));
        row.detail_trans_status = Some("ready".to_owned());
        row.detail_trans_title = Some("坏掉的详情译文".to_owned());
        row.detail_trans_summary = None;

        let item = feed_item_from_row(row, true, None);
        let translated = item.translated.expect("translated item");
        assert!(item.body_truncated);
        assert_eq!(translated.status, "error");
        assert_eq!(translated.auto_translate, Some(false));
        assert_eq!(
            translated.error_code.as_deref(),
            Some("markdown_structure_mismatch")
        );
        assert_eq!(
            translated.error_summary.as_deref(),
            Some("Markdown 结构校验失败")
        );
        assert_eq!(
            translated.error_detail.as_deref(),
            Some("release detail translation failed to preserve markdown structure")
        );
        assert!(translated.title.is_none());
        assert!(translated.summary.is_none());
    }

    #[test]
    fn feed_item_from_row_keeps_ready_translation_visible_while_refresh_pending() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.title = Some("Release v1.2.4".to_owned());
        row.release_body = Some("- new item".to_owned());
        row.trans_source_hash = Some("older-hash".to_owned());
        row.trans_status = Some("ready".to_owned());
        row.trans_title = Some("旧标题".to_owned());
        row.trans_summary = Some("- 旧摘要".to_owned());
        row.trans_work_status = Some("queued".to_owned());

        let item = feed_item_from_row(row, true, None);
        let translated = item.translated.expect("translated item");
        assert_eq!(translated.status, "ready");
        assert_eq!(translated.title.as_deref(), Some("旧标题"));
        assert_eq!(translated.summary.as_deref(), Some("- 旧摘要"));
        assert_eq!(translated.auto_translate, Some(true));
    }

    #[test]
    fn feed_item_from_row_maps_smart_insufficient_to_terminal_state() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.release_tag_name = Some("v1.2.4".to_owned());
        row.release_previous_tag_name = Some("v1.2.3".to_owned());
        row.title = Some("Release v1.2.4".to_owned());
        row.release_body = Some("See full changelog below.".to_owned());
        row.smart_source_hash = Some(crate::translations::release_smart_feed_source_hash(
            row.entity_id.as_str(),
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            row.release_body.as_deref(),
            row.release_tag_name.as_deref().unwrap_or(""),
            row.release_previous_tag_name.as_deref(),
        ));
        row.smart_status = Some("missing".to_owned());
        row.smart_error_text = Some(SMART_NO_VALUABLE_VERSION_INFO.to_owned());

        let item = feed_item_from_row(row, true, None);
        let smart = item.smart.expect("smart item");
        assert_eq!(smart.status, "insufficient");
        assert_eq!(smart.auto_translate, Some(false));
        assert!(smart.title.is_none());
        assert!(smart.summary.is_none());
    }

    #[test]
    fn smart_error_is_retryable_for_public_scope_upgrade() {
        assert!(smart_error_is_retryable(Some(
            "repo scope required; re-login via GitHub OAuth",
        )));
        assert!(!smart_error_is_retryable(Some(
            "private repository compare requires repo scope; re-login via GitHub OAuth",
        )));
    }

    #[test]
    fn feed_item_from_row_retries_retryable_smart_errors() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.release_tag_name = Some("v1.2.4".to_owned());
        row.release_previous_tag_name = Some("v1.2.3".to_owned());
        row.title = Some("Release v1.2.4".to_owned());
        row.release_body = Some("See full changelog below.".to_owned());
        row.smart_source_hash = Some(crate::translations::release_smart_feed_source_hash(
            row.entity_id.as_str(),
            row.repo_full_name.as_deref().unwrap_or(""),
            row.title.as_deref().unwrap_or(""),
            row.release_body.as_deref(),
            row.release_tag_name.as_deref().unwrap_or(""),
            row.release_previous_tag_name.as_deref(),
        ));
        row.smart_status = Some("error".to_owned());
        row.smart_error_text = Some("runtime_lease_expired".to_owned());

        let item = feed_item_from_row(row, true, None);
        let smart = item.smart.expect("smart item");
        assert_eq!(smart.status, "missing");
        assert_eq!(smart.auto_translate, Some(true));
        assert!(smart.title.is_none());
        assert!(smart.summary.is_none());
    }

    #[test]
    fn feed_item_from_row_exposes_repo_visual_metadata() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());
        row.owner_avatar_url = Some("https://avatars.githubusercontent.com/u/14957082".to_owned());
        row.open_graph_image_url =
            Some("https://repository-images.githubusercontent.com/14957082/codex".to_owned());
        row.uses_custom_open_graph_image = Some(1);

        let item = feed_item_from_row(row, true, None);
        let repo_visual = item.repo_visual.expect("repo visual");
        assert_eq!(
            repo_visual.owner_avatar_url.as_deref(),
            Some("https://avatars.githubusercontent.com/u/14957082")
        );
        assert_eq!(
            repo_visual.open_graph_image_url.as_deref(),
            Some("https://repository-images.githubusercontent.com/14957082/codex")
        );
        assert!(repo_visual.uses_custom_open_graph_image);
    }

    #[test]
    fn feed_item_from_row_omits_repo_visual_when_metadata_missing() {
        let mut row = test_feed_row(Some("R_node"));
        row.repo_full_name = Some("openai/codex".to_owned());

        let item = feed_item_from_row(row, true, None);
        assert!(item.repo_visual.is_none());
    }

    #[test]
    fn feed_item_from_row_maps_social_actor_payload() {
        let mut row = test_feed_row(None);
        row.kind = "repo_star_received".to_owned();
        row.release_id = None;
        row.repo_full_name = Some("openai/codex".to_owned());
        row.entity_id = "star-event-1".to_owned();
        row.id_key = "star-event-1".to_owned();
        row.actor_login = Some("octocat".to_owned());
        row.actor_avatar_url = Some("https://avatars.example/octocat.png".to_owned());
        row.actor_html_url = Some("https://github.com/octocat".to_owned());
        row.html_url = Some("https://github.com/octocat".to_owned());

        let item = feed_item_from_row(row, true, None);
        assert_eq!(item.kind, "repo_star_received");
        assert!(item.translated.is_none());
        assert!(item.smart.is_none());
        assert!(item.reactions.is_none());
        let actor = item.actor.expect("social actor");
        assert_eq!(actor.login, "octocat");
        assert_eq!(
            actor.avatar_url.as_deref(),
            Some("https://avatars.example/octocat.png")
        );
        assert_eq!(
            actor.html_url.as_deref(),
            Some("https://github.com/octocat")
        );
        assert_eq!(item.html_url.as_deref(), Some("https://github.com/octocat"));
    }

    #[test]
    fn parse_feed_types_accepts_social_variants() {
        let selection = parse_feed_types(Some("release,stars,follower")).expect("selection");
        assert!(selection.releases);
        assert!(selection.stars);
        assert!(selection.followers);
    }

    #[test]
    fn parse_release_smart_summary_payload_accepts_relaxed_payload_and_sanitizes_bullets() {
        let raw = r#"
        {
          "valuable": "yes",
          "title_cn": "版本变化速览",
          "bullets": [
            "新增 CLI 子命令",
            "修复日志链接换行\n问题",
            "详情见 https://example.com/changelog"
          ]
        }
        "#;

        let payload = parse_release_smart_summary_payload(raw).expect("smart payload");

        assert!(payload.valuable);
        assert_eq!(payload.title_zh.as_deref(), Some("版本变化速览"));
        assert_eq!(
            payload.summary_bullets,
            vec![
                "新增 CLI 子命令".to_owned(),
                "修复日志链接换行 问题".to_owned(),
                "详情见".to_owned(),
            ]
        );
    }

    #[test]
    fn build_compare_digest_filters_noise_and_keeps_meaningful_patch_excerpt() {
        let digest = build_compare_digest(&GitHubCompareResponse {
            status: Some("ahead".to_owned()),
            ahead_by: Some(2),
            behind_by: Some(0),
            total_commits: Some(2),
            commits: vec![
                GitHubCompareCommit {
                    sha: "abcdef123456".to_owned(),
                    commit: GitHubCompareCommitDetail {
                        message: "feat: add release smart fallback\n\nextra".to_owned(),
                    },
                },
                GitHubCompareCommit {
                    sha: "fedcba654321".to_owned(),
                    commit: GitHubCompareCommitDetail {
                        message: "fix: keep markdown bullets readable".to_owned(),
                    },
                },
            ],
            files: vec![
                GitHubCompareFile {
                    filename: "web/bun.lock".to_owned(),
                    status: Some("modified".to_owned()),
                    additions: Some(10),
                    deletions: Some(2),
                    changes: Some(12),
                    patch: Some("+lockfileVersion: 1".to_owned()),
                },
                GitHubCompareFile {
                    filename: "src/api.rs".to_owned(),
                    status: Some("modified".to_owned()),
                    additions: Some(12),
                    deletions: Some(3),
                    changes: Some(15),
                    patch: Some(
                        "@@ -1,3 +1,6 @@\n-use old\n+use new\n context\n+let digest = build();\n-http://example.com\n+summary.push(\"ok\");\n".to_owned(),
                    ),
                },
                GitHubCompareFile {
                    filename: "dist/app.min.js".to_owned(),
                    status: Some("modified".to_owned()),
                    additions: Some(100),
                    deletions: Some(40),
                    changes: Some(140),
                    patch: Some("+compiled".to_owned()),
                },
            ],
        })
        .expect("compare digest");

        assert!(digest.contains("compare_status: ahead"));
        assert!(digest.contains("commit_subjects:"));
        assert!(digest.contains("feat: add release smart fallback"));
        assert!(digest.contains("changed_files:"));
        assert!(digest.contains("src/api.rs [modified]"));
        assert!(digest.contains("patch_excerpts:"));
        assert!(digest.contains("### src/api.rs"));
        assert!(!digest.contains("web/bun.lock"));
        assert!(!digest.contains("dist/app.min.js"));
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

    #[test]
    fn release_detail_source_hash_normalizes_line_endings_and_whitespace() {
        let hash1 = release_detail_source_hash(
            " acme/repo ",
            " v1.0.0 ",
            "line one
line two
",
        );
        let hash2 = release_detail_source_hash(
            "acme/repo",
            "v1.0.0",
            "line one
line two",
        );
        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn mark_translation_requested_keeps_existing_ready_translation_visible() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 7, "race-user", 0, 0).await;

        let user_id = test_user_id(7);
        let entity_id = "42";
        let ready_hash = "ready-source";
        let pending_hash = "pending-source";

        upsert_translation(
            state.as_ref(),
            user_id.as_str(),
            "2099-03-30T00:00:01Z",
            TranslationUpsert {
                entity_type: "release",
                entity_id,
                lang: "zh-CN",
                source_hash: ready_hash,
                title: Some("旧标题"),
                summary: Some("旧摘要"),
            },
        )
        .await
        .expect("seed ready translation");

        mark_translation_requested(
            state.as_ref(),
            user_id.as_str(),
            "2099-03-30T00:00:02Z",
            TranslationUpsert {
                entity_type: "release",
                entity_id,
                lang: "zh-CN",
                source_hash: pending_hash,
                title: None,
                summary: None,
            },
        )
        .await
        .expect("mark refresh request");

        let row: (String, String, Option<String>, Option<String>) = sqlx::query_as(
            r#"
            SELECT source_hash, status, title, summary
            FROM ai_translations
            WHERE user_id = ? AND entity_type = 'release' AND entity_id = ? AND lang = 'zh-CN'
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .bind(entity_id)
        .fetch_one(&pool)
        .await
        .expect("load preserved ready translation row");
        assert_eq!(row.0, ready_hash);
        assert_eq!(row.1, "ready");
        assert_eq!(row.2.as_deref(), Some("旧标题"));
        assert_eq!(row.3.as_deref(), Some("旧摘要"));

        upsert_translation(
            state.as_ref(),
            user_id.as_str(),
            "2099-03-30T00:00:03Z",
            TranslationUpsert {
                entity_type: "release",
                entity_id,
                lang: "zh-CN",
                source_hash: pending_hash,
                title: Some("新标题"),
                summary: Some("新摘要"),
            },
        )
        .await
        .expect("refresh result should land");

        let row: (String, String, Option<String>, Option<String>) = sqlx::query_as(
            r#"
            SELECT source_hash, status, title, summary
            FROM ai_translations
            WHERE user_id = ? AND entity_type = 'release' AND entity_id = ? AND lang = 'zh-CN'
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .bind(entity_id)
        .fetch_one(&pool)
        .await
        .expect("load completed translation row");
        assert_eq!(row.0, pending_hash);
        assert_eq!(row.1, "ready");
        assert_eq!(row.2.as_deref(), Some("新标题"));
        assert_eq!(row.3.as_deref(), Some("新摘要"));
    }

    #[test]
    fn brief_contains_release_link_matches_exact_id() {
        let markdown = "- [v1.2.3](/?tab=briefs&release=123)";
        assert!(brief_contains_release_link(markdown, 123));
        assert!(!brief_contains_release_link(markdown, 12));
    }

    #[test]
    fn brief_contains_release_link_accepts_query_order_variant() {
        let markdown = "- [v1.2.3](/?release=123&tab=briefs)";
        assert!(brief_contains_release_link(markdown, 123));
    }

    #[test]
    fn extract_brief_release_ids_preserves_order_without_duplicates() {
        let markdown = "\
- [a](/?tab=briefs&release=123)\n\
- [b](/?release=456&tab=briefs)\n\
- [dup](/?tab=briefs&release=123)\n";
        assert_eq!(extract_brief_release_ids(markdown), vec![123, 456]);
    }

    #[tokio::test]
    async fn release_visibility_view_hides_owned_repo_until_opted_in() {
        let pool = setup_pool().await;
        seed_repo_release(&pool, 42, 120).await;
        seed_owned_repo_baseline(&pool, 42, "IvanLi-CN/octo-rill").await;

        let count_without_opt_in: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM repo_releases r
            JOIN user_release_visible_repos vr
              ON vr.user_id = ? AND vr.repo_id = r.repo_id
            "#,
        )
        .bind(test_user_id(1))
        .fetch_one(&pool)
        .await
        .expect("count releases without opt-in");
        assert_eq!(count_without_opt_in, 0);

        set_include_own_releases(&pool, true).await;
        let count_with_opt_in: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM repo_releases r
            JOIN user_release_visible_repos vr
              ON vr.user_id = ? AND vr.repo_id = r.repo_id
            "#,
        )
        .bind(test_user_id(1))
        .fetch_one(&pool)
        .await
        .expect("count releases with opt-in");
        assert_eq!(count_with_opt_in, 1);
    }

    #[tokio::test]
    async fn list_feed_returns_mixed_items_and_supports_social_filters() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        seed_social_event(
            &pool,
            user_id.as_str(),
            SeedSocialEventArgs {
                kind: "repo_star_received",
                event_id: "social-star-1",
                repo_id: Some(42),
                repo_full_name: Some("openai/codex"),
                repo_owner_avatar_url: None,
                repo_open_graph_image_url: None,
                repo_uses_custom_open_graph_image: None,
                actor_login: "octocat",
                occurred_at: "2026-02-23T10:00:00Z",
            },
        )
        .await;
        seed_social_event(
            &pool,
            user_id.as_str(),
            SeedSocialEventArgs {
                kind: "follower_received",
                event_id: "social-follow-1",
                repo_id: None,
                repo_full_name: None,
                repo_owner_avatar_url: None,
                repo_open_graph_image_url: None,
                repo_uses_custom_open_graph_image: None,
                actor_login: "monalisa",
                occurred_at: "2026-02-23T09:00:00Z",
            },
        )
        .await;
        let state = setup_state(pool);

        let Json(feed) = list_feed(
            State(state.clone()),
            setup_session(1).await,
            Query(FeedQuery {
                cursor: None,
                limit: Some(30),
                types: None,
            }),
        )
        .await
        .expect("list mixed feed");

        let kinds = feed
            .items
            .iter()
            .map(|item| item.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec!["repo_star_received", "follower_received", "release"]
        );
        assert_eq!(
            feed.items[0]
                .actor
                .as_ref()
                .expect("social actor")
                .login
                .as_str(),
            "octocat"
        );

        let Json(stars_only) = list_feed(
            State(state),
            setup_session(1).await,
            Query(FeedQuery {
                cursor: None,
                limit: Some(30),
                types: Some("stars".to_owned()),
            }),
        )
        .await
        .expect("list stars feed");

        assert_eq!(stars_only.items.len(), 1);
        assert_eq!(stars_only.items[0].kind, "repo_star_received");
    }

    #[tokio::test]
    async fn list_feed_preserves_repo_visuals_for_historical_social_events_without_baseline() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_social_event(
            &pool,
            user_id.as_str(),
            SeedSocialEventArgs {
                kind: "repo_star_received",
                event_id: "social-star-history-1",
                repo_id: Some(42),
                repo_full_name: Some("openai/codex"),
                repo_owner_avatar_url: Some("https://avatars.githubusercontent.com/u/14957082"),
                repo_open_graph_image_url: Some(
                    "https://repository-images.githubusercontent.com/14957082/codex",
                ),
                repo_uses_custom_open_graph_image: Some(true),
                actor_login: "octocat",
                occurred_at: "2026-02-23T10:00:00Z",
            },
        )
        .await;
        let state = setup_state(pool);

        let Json(feed) = list_feed(
            State(state),
            setup_session(1).await,
            Query(FeedQuery {
                cursor: None,
                limit: Some(30),
                types: Some("stars".to_owned()),
            }),
        )
        .await
        .expect("list stars feed");

        let item = feed.items.first().expect("historical social item");
        assert_eq!(item.kind, "repo_star_received");
        let repo_visual = item.repo_visual.as_ref().expect("repo visual");
        assert_eq!(
            repo_visual.owner_avatar_url.as_deref(),
            Some("https://avatars.githubusercontent.com/u/14957082")
        );
        assert_eq!(
            repo_visual.open_graph_image_url.as_deref(),
            Some("https://repository-images.githubusercontent.com/14957082/codex")
        );
        assert!(repo_visual.uses_custom_open_graph_image);
    }

    #[tokio::test]
    async fn list_releases_reads_shared_repo_cache_for_starred_user() {
        let pool = setup_pool().await;
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        let state = setup_state(pool);

        let Json(items) = list_releases(State(state), setup_session(1).await)
            .await
            .expect("list releases");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].full_name, "openai/codex");
        assert_eq!(items[0].tag_name, "v1.2.3");
        assert_eq!(items[0].name.as_deref(), Some("Release v1.2.3"));
    }

    #[tokio::test]
    async fn list_releases_hides_owned_only_repo_when_opt_in_disabled() {
        let pool = setup_pool().await;
        seed_repo_release(&pool, 42, 120).await;
        seed_owned_repo_baseline(&pool, 42, "IvanLi-CN/octo-rill").await;
        let state = setup_state(pool);

        let Json(items) = list_releases(State(state), setup_session(1).await)
            .await
            .expect("list releases");

        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn list_releases_includes_owned_only_repo_when_opted_in() {
        let pool = setup_pool().await;
        seed_repo_release(&pool, 42, 120).await;
        seed_owned_repo_baseline(&pool, 42, "IvanLi-CN/octo-rill").await;
        set_include_own_releases(&pool, true).await;
        let state = setup_state(pool);

        let Json(items) = list_releases(State(state), setup_session(1).await)
            .await
            .expect("list releases");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].full_name, "IvanLi-CN/octo-rill");
        assert_eq!(items[0].tag_name, "v1.2.3");
    }

    #[tokio::test]
    async fn get_release_detail_reads_shared_repo_cache_for_starred_user() {
        let pool = setup_pool().await;
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        let state = setup_state(pool);

        let Json(detail) =
            get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
                .await
                .expect("get release detail");

        assert_eq!(detail.release_id, "120");
        assert_eq!(detail.repo_full_name.as_deref(), Some("openai/codex"));
        let repo_visual = detail.repo_visual.expect("repo visual");
        assert_eq!(
            repo_visual.owner_avatar_url.as_deref(),
            Some("https://avatars.githubusercontent.com/u/14957082")
        );
        assert_eq!(
            repo_visual.open_graph_image_url.as_deref(),
            Some("https://repository-images.githubusercontent.com/14957082/codex")
        );
        assert!(repo_visual.uses_custom_open_graph_image);
        assert_eq!(detail.tag_name, "v1.2.3");
        assert_eq!(detail.name.as_deref(), Some("Release v1.2.3"));
        assert_eq!(detail.body.as_deref(), Some("- item"));
        assert_eq!(
            detail.html_url,
            "https://github.com/openai/codex/releases/tag/v1.2.3"
        );
    }

    #[tokio::test]
    async fn get_release_detail_rejects_owned_only_repo_when_opt_in_disabled() {
        let pool = setup_pool().await;
        seed_repo_release(&pool, 42, 120).await;
        seed_owned_repo_baseline(&pool, 42, "IvanLi-CN/octo-rill").await;
        let state = setup_state(pool);

        let err = get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
            .await
            .expect_err("owned-only release should stay hidden");

        assert_eq!(err.code(), "not_found");
    }

    #[tokio::test]
    async fn get_release_detail_reads_owned_only_repo_when_opted_in() {
        let pool = setup_pool().await;
        seed_repo_release(&pool, 42, 120).await;
        seed_owned_repo_baseline(&pool, 42, "IvanLi-CN/octo-rill").await;
        set_include_own_releases(&pool, true).await;
        let state = setup_state(pool);

        let Json(detail) =
            get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
                .await
                .expect("get owned release detail");

        assert_eq!(
            detail.repo_full_name.as_deref(),
            Some("IvanLi-CN/octo-rill")
        );
        let repo_visual = detail.repo_visual.expect("repo visual");
        assert_eq!(
            repo_visual.owner_avatar_url.as_deref(),
            Some("https://avatars.githubusercontent.com/u/30215105")
        );
        assert_eq!(
            repo_visual.open_graph_image_url.as_deref(),
            Some("https://repository-images.githubusercontent.com/30215105/octo-rill")
        );
        assert!(repo_visual.uses_custom_open_graph_image);
    }

    #[tokio::test]
    async fn get_release_detail_allows_historical_brief_link_without_current_star() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_brief(
            &pool,
            user_id.as_str(),
            "2026-02-23",
            "- [v1.2.3](/?tab=briefs&release=120)",
        )
        .await;
        let state = setup_state(pool);

        let Json(detail) =
            get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
                .await
                .expect("get release detail from brief link");

        assert_eq!(detail.release_id, "120");
        assert_eq!(detail.repo_full_name.as_deref(), Some("openai/codex"));
        assert!(detail.repo_visual.is_none());
    }

    #[tokio::test]
    async fn get_release_detail_allows_content_refresh_failed_brief_link_without_membership() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date, generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-content-refresh-failed")
        .bind(user_id.as_str())
        .bind("2026-02-23")
        .bind("content_refresh_failed")
        .bind("- [v1.2.3](/?tab=briefs&release=120)")
        .bind("2026-02-23T08:00:00Z")
        .bind("2026-02-23T08:00:00Z")
        .execute(&pool)
        .await
        .expect("insert failed refresh brief");
        let state = setup_state(pool);

        let Json(detail) =
            get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
                .await
                .expect("get release detail from failed refresh brief link");

        assert_eq!(detail.release_id, "120");
        assert_eq!(detail.repo_full_name.as_deref(), Some("openai/codex"));
        assert!(detail.repo_visual.is_none());
    }

    #[tokio::test]
    async fn translate_release_detail_allows_historical_brief_link_without_current_star() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_brief(
            &pool,
            user_id.as_str(),
            "2026-02-23",
            "- [v1.2.3](/?tab=briefs&release=120)",
        )
        .await;
        let state = setup_state_with_ai(pool.clone());
        let source_hash = release_detail_source_hash("openai/codex", "Release v1.2.3", "- item");
        seed_release_detail_translation(
            &pool,
            user_id.as_str(),
            "120",
            source_hash.as_str(),
            Some("版本 v1.2.3"),
            Some("- 条目"),
        )
        .await;

        let translated = translate_release_detail_for_user(state.as_ref(), user_id.as_str(), "120")
            .await
            .expect("translate release detail from brief link");

        assert_eq!(translated.status, "ready");
        assert_eq!(translated.title.as_deref(), Some("版本 v1.2.3"));
        assert_eq!(translated.summary.as_deref(), Some("- 条目"));
    }

    #[tokio::test]
    async fn translate_release_detail_accepts_fenced_markdown_chunk_output() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        sqlx::query(
            r#"
            UPDATE repo_releases
            SET body = ?, name = 'Release v1.2.3', tag_name = 'v1.2.3'
            WHERE release_id = ?
            "#,
        )
        .bind("- keep `code`\n")
        .bind(120_i64)
        .execute(&pool)
        .await
        .expect("update release detail body");

        let base_url = spawn_test_ai_server(Router::new().route(
            "/chat/completions",
            post(move |Json(payload): Json<Value>| async move {
                let system_prompt = payload["messages"][0]["content"]
                    .as_str()
                    .unwrap_or_default();
                let content = if system_prompt.contains("只把 GitHub Release 标题翻译成自然中文")
                {
                    "版本 v1.2.3".to_owned()
                } else {
                    "```markdown\n- 保留 `code`\n```".to_owned()
                };
                let response = serde_json::json!({
                    "choices": [{"message": {"content": content}}],
                    "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}
                });
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    Json(response),
                )
            }),
        ))
        .await;
        let state = setup_state_with_ai_base_url(pool.clone(), base_url);

        let translated = translate_release_detail_for_user(state.as_ref(), user_id.as_str(), "120")
            .await
            .expect("translate release detail");

        assert_eq!(translated.status, "ready");
        assert_eq!(translated.title.as_deref(), Some("版本 v1.2.3"));
        assert_eq!(translated.summary.as_deref(), Some("- 保留 `code`\n"));
    }

    #[tokio::test]
    async fn prepare_release_batch_routes_release_to_detail_translation_path() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        let long_body = "a".repeat(RELEASE_FEED_BODY_MAX_CHARS + 24);
        sqlx::query(
            r#"
            UPDATE repo_releases
            SET body = ?
            WHERE release_id = ?
            "#,
        )
        .bind(long_body.as_str())
        .bind(120_i64)
        .execute(&pool)
        .await
        .expect("update repo release body");
        let state = setup_state_with_ai(pool);

        let prepared = prepare_release_batch(state.as_ref(), user_id.as_str(), &[120])
            .await
            .expect("prepare release batch");

        assert_eq!(
            prepared
                .detail_pending_candidates
                .iter()
                .map(|candidate| candidate.release_id)
                .collect::<Vec<_>>(),
            vec![120]
        );
        assert!(prepared.terminal.is_empty());
    }

    #[tokio::test]
    async fn translate_releases_batch_for_user_batches_short_release_details() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_repo_release(&pool, 42, 121).await;
        seed_star(&pool, 42).await;
        sqlx::query(
            r#"
            UPDATE repo_releases
            SET body = CASE release_id
              WHEN 120 THEN ?
              WHEN 121 THEN ?
              ELSE body
            END,
                name = CASE release_id
              WHEN 120 THEN 'Release v1.2.3'
              WHEN 121 THEN 'Release v1.2.4'
              ELSE name
            END,
                tag_name = CASE release_id
              WHEN 120 THEN 'v1.2.3'
              WHEN 121 THEN 'v1.2.4'
              ELSE tag_name
            END
            WHERE release_id IN (120, 121)
            "#,
        )
        .bind("- first item")
        .bind("- second item")
        .execute(&pool)
        .await
        .expect("update release bodies");

        let call_count = Arc::new(AtomicUsize::new(0));
        let seen_payloads = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
        let route_call_count = Arc::clone(&call_count);
        let route_payloads = Arc::clone(&seen_payloads);
        let base_url = spawn_test_ai_server(Router::new().route(
            "/chat/completions",
            post(move |Json(payload): Json<Value>| {
                let route_call_count = Arc::clone(&route_call_count);
                let route_payloads = Arc::clone(&route_payloads);
                async move {
                    route_call_count.fetch_add(1, Ordering::SeqCst);
                    route_payloads.lock().await.push(payload);
                    let content = serde_json::json!({
                        "items": [
                            {
                                "release_id": 120,
                                "title_zh": "版本 1.2.3",
                                "summary_md": "- 第一条"
                            },
                            {
                                "release_id": 121,
                                "title_zh": "版本 1.2.4",
                                "summary_md": "- 第二条"
                            }
                        ]
                    })
                    .to_string();
                    let response = serde_json::json!({
                        "choices": [{"message": {"content": content}}],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}
                    });
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        Json(response),
                    )
                }
            }),
        ))
        .await;
        let state = setup_state_with_ai_base_url(pool.clone(), base_url);

        let translated =
            translate_releases_batch_for_user(state.as_ref(), user_id.as_str(), &[120, 121])
                .await
                .expect("translate release batch");

        assert_eq!(translated.items.len(), 2);
        assert_eq!(translated.items[0].status, "ready");
        assert_eq!(translated.items[0].title.as_deref(), Some("版本 1.2.3"));
        assert_eq!(translated.items[0].summary.as_deref(), Some("- 第一条"));
        assert_eq!(translated.items[1].status, "ready");
        assert_eq!(translated.items[1].title.as_deref(), Some("版本 1.2.4"));
        assert_eq!(translated.items[1].summary.as_deref(), Some("- 第二条"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let payloads = seen_payloads.lock().await;
        let prompt = payloads[0]["messages"][1]["content"]
            .as_str()
            .expect("user prompt should be present");
        assert!(prompt.contains("release_id: 120"));
        assert!(prompt.contains("release_id: 121"));
    }

    #[tokio::test]
    async fn translate_releases_batch_for_user_falls_back_when_batched_markdown_is_invalid() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        sqlx::query(
            r#"
            UPDATE repo_releases
            SET body = ?, name = 'Release v1.2.3', tag_name = 'v1.2.3'
            WHERE release_id = ?
            "#,
        )
        .bind("## Heading\n- item")
        .bind(120_i64)
        .execute(&pool)
        .await
        .expect("update release body");

        let batch_calls = Arc::new(AtomicUsize::new(0));
        let detail_chunk_calls = Arc::new(AtomicUsize::new(0));
        let title_calls = Arc::new(AtomicUsize::new(0));
        let route_batch_calls = Arc::clone(&batch_calls);
        let route_detail_chunk_calls = Arc::clone(&detail_chunk_calls);
        let route_title_calls = Arc::clone(&title_calls);
        let base_url = spawn_test_ai_server(Router::new().route(
            "/chat/completions",
            post(move |Json(payload): Json<Value>| {
                let route_batch_calls = Arc::clone(&route_batch_calls);
                let route_detail_chunk_calls = Arc::clone(&route_detail_chunk_calls);
                let route_title_calls = Arc::clone(&route_title_calls);
                async move {
                    let system_prompt = payload["messages"][0]["content"]
                        .as_str()
                        .unwrap_or_default();
                    let content = if system_prompt.contains("批量翻译助手") {
                        route_batch_calls.fetch_add(1, Ordering::SeqCst);
                        serde_json::json!({
                            "items": [
                                {
                                    "release_id": 120,
                                    "title_zh": "批量标题",
                                    "summary_md": "Heading item"
                                }
                            ]
                        })
                        .to_string()
                    } else if system_prompt.contains("只把 GitHub Release 标题翻译成自然中文")
                    {
                        route_title_calls.fetch_add(1, Ordering::SeqCst);
                        "版本 v1.2.3".to_owned()
                    } else {
                        route_detail_chunk_calls.fetch_add(1, Ordering::SeqCst);
                        "## 标题\n- 条目".to_owned()
                    };
                    let response = serde_json::json!({
                        "choices": [{"message": {"content": content}}],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}
                    });
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        Json(response),
                    )
                }
            }),
        ))
        .await;
        let state = setup_state_with_ai_base_url(pool.clone(), base_url);

        let translated =
            translate_releases_batch_for_user(state.as_ref(), user_id.as_str(), &[120])
                .await
                .expect("translate release batch");

        assert_eq!(translated.items.len(), 1);
        assert_eq!(translated.items[0].status, "ready");
        assert_eq!(translated.items[0].title.as_deref(), Some("版本 v1.2.3"));
        assert_eq!(
            translated.items[0].summary.as_deref(),
            Some("## 标题\n- 条目")
        );
        assert_eq!(batch_calls.load(Ordering::SeqCst), 1);
        assert!(detail_chunk_calls.load(Ordering::SeqCst) >= 1);
        assert_eq!(title_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn translate_releases_batch_for_user_reuses_cached_release_detail_for_truncated_release()
    {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        let long_body = "a".repeat(RELEASE_FEED_BODY_MAX_CHARS + 24);
        sqlx::query(
            r#"
            UPDATE repo_releases
            SET body = ?
            WHERE release_id = ?
            "#,
        )
        .bind(long_body.as_str())
        .bind(120_i64)
        .execute(&pool)
        .await
        .expect("update repo release body");
        let state = setup_state_with_ai(pool.clone());
        let source_hash =
            release_detail_source_hash("openai/codex", "Release v1.2.3", long_body.as_str());
        seed_release_detail_translation(
            &pool,
            user_id.as_str(),
            "120",
            source_hash.as_str(),
            Some("版本 v1.2.3"),
            Some(
                "- 第一段
- 第二段",
            ),
        )
        .await;

        let translated =
            translate_releases_batch_for_user(state.as_ref(), user_id.as_str(), &[120])
                .await
                .expect("translate cached long release batch");

        assert_eq!(translated.items.len(), 1);
        let item = &translated.items[0];
        assert_eq!(item.id, "120");
        assert_eq!(item.status, "ready");
        assert_eq!(item.title.as_deref(), Some("版本 v1.2.3"));
        assert_eq!(
            item.summary.as_deref(),
            Some(
                "- 第一段
- 第二段"
            )
        );

        let release_cache_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM ai_translations
            WHERE user_id = ?
              AND entity_type = 'release'
              AND entity_id = '120'
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count release feed cache entries");
        assert_eq!(release_cache_count, 0);
    }

    #[tokio::test]
    async fn translate_releases_batch_for_user_reuses_legacy_release_cache_for_short_release() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;

        let legacy_body = release_feed_body(Some("- item")).expect("legacy feed body");
        let legacy_hash = crate::api::release_feed_translation_source_hash(
            "openai/codex",
            "Release v1.2.3",
            Some(&legacy_body),
        );
        seed_release_translation(
            &pool,
            user_id.as_str(),
            "120",
            legacy_hash.as_str(),
            Some("版本 v1.2.3"),
            Some("- 已缓存条目"),
        )
        .await;

        let batch_calls = Arc::new(AtomicUsize::new(0));
        let detail_chunk_calls = Arc::new(AtomicUsize::new(0));
        let title_calls = Arc::new(AtomicUsize::new(0));
        let route_batch_calls = Arc::clone(&batch_calls);
        let route_detail_chunk_calls = Arc::clone(&detail_chunk_calls);
        let route_title_calls = Arc::clone(&title_calls);
        let base_url = spawn_test_ai_server(Router::new().route(
            "/chat/completions",
            post(move |_payload: Json<Value>| {
                let route_batch_calls = Arc::clone(&route_batch_calls);
                let route_detail_chunk_calls = Arc::clone(&route_detail_chunk_calls);
                let route_title_calls = Arc::clone(&route_title_calls);
                async move {
                    route_batch_calls.fetch_add(1, Ordering::SeqCst);
                    route_detail_chunk_calls.fetch_add(1, Ordering::SeqCst);
                    route_title_calls.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        [(header::CONTENT_TYPE, "application/json")],
                        Json(json!({"error": "legacy cache should avoid upstream calls"})),
                    )
                }
            }),
        ))
        .await;
        let state = setup_state_with_ai_base_url(pool, base_url);

        let translated =
            translate_releases_batch_for_user(state.as_ref(), user_id.as_str(), &[120])
                .await
                .expect("reuse legacy short release cache");

        assert_eq!(translated.items.len(), 1);
        let item = &translated.items[0];
        assert_eq!(item.id, "120");
        assert_eq!(item.status, "ready");
        assert_eq!(item.title.as_deref(), Some("版本 v1.2.3"));
        assert_eq!(item.summary.as_deref(), Some("- 已缓存条目"));
        assert_eq!(batch_calls.load(Ordering::SeqCst), 0);
        assert_eq!(detail_chunk_calls.load(Ordering::SeqCst), 0);
        assert_eq!(title_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn translate_releases_batch_for_user_skips_batch_calls_that_exceed_batch_budget() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        sqlx::query(
            r#"
            INSERT INTO admin_runtime_settings (
              id,
              llm_max_concurrency,
              ai_model_context_limit,
              translation_general_worker_concurrency,
              translation_dedicated_worker_concurrency,
              created_at,
              updated_at
            )
            VALUES (1, 1, 2048, 3, 1, '2026-03-28T11:00:00Z', '2026-03-28T11:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("insert runtime settings");
        sqlx::query(
            r#"
            UPDATE repo_releases
            SET body = ?
            WHERE release_id = ?
            "#,
        )
        .bind("a".repeat(1_200))
        .bind(120_i64)
        .execute(&pool)
        .await
        .expect("update release body");

        let batch_calls = Arc::new(AtomicUsize::new(0));
        let detail_chunk_calls = Arc::new(AtomicUsize::new(0));
        let title_calls = Arc::new(AtomicUsize::new(0));
        let route_batch_calls = Arc::clone(&batch_calls);
        let route_detail_chunk_calls = Arc::clone(&detail_chunk_calls);
        let route_title_calls = Arc::clone(&title_calls);
        let base_url = spawn_test_ai_server(Router::new().route(
            "/chat/completions",
            post(move |Json(payload): Json<Value>| {
                let route_batch_calls = Arc::clone(&route_batch_calls);
                let route_detail_chunk_calls = Arc::clone(&route_detail_chunk_calls);
                let route_title_calls = Arc::clone(&route_title_calls);
                async move {
                    let system_prompt = payload["messages"][0]["content"]
                        .as_str()
                        .unwrap_or_default();
                    let user_prompt = payload["messages"][1]["content"]
                        .as_str()
                        .unwrap_or_default();
                    let content = if system_prompt.contains("批量翻译助手")
                        && user_prompt.contains("release_id:")
                    {
                        route_batch_calls.fetch_add(1, Ordering::SeqCst);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            [(header::CONTENT_TYPE, "application/json")],
                            Json(json!({"error": "batch should not be used"})),
                        );
                    } else if system_prompt.contains("只把 GitHub Release 标题翻译成自然中文")
                    {
                        route_title_calls.fetch_add(1, Ordering::SeqCst);
                        "版本 v1.2.3".to_owned()
                    } else {
                        route_detail_chunk_calls.fetch_add(1, Ordering::SeqCst);
                        user_prompt
                            .split("Release notes chunk (Markdown):\n")
                            .nth(1)
                            .and_then(|rest| rest.split("\n\n请把这段 GitHub Release notes").next())
                            .unwrap_or_default()
                            .to_owned()
                    };
                    let response = serde_json::json!({
                        "choices": [{"message": {"content": content}}],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}
                    });
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        Json(response),
                    )
                }
            }),
        ))
        .await;
        let state = setup_state_with_ai_base_url(pool.clone(), base_url);

        let translated =
            translate_releases_batch_for_user(state.as_ref(), user_id.as_str(), &[120])
                .await
                .expect("translate release batch");

        let expected_body = "a".repeat(1_200);
        assert_eq!(translated.items.len(), 1);
        assert_eq!(translated.items[0].status, "ready");
        assert_eq!(translated.items[0].title.as_deref(), Some("版本 v1.2.3"));
        assert_eq!(
            translated.items[0].summary.as_deref(),
            Some(expected_body.as_str())
        );
        assert_eq!(batch_calls.load(Ordering::SeqCst), 0);
        assert!(detail_chunk_calls.load(Ordering::SeqCst) >= 1);
        assert_eq!(title_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn translate_releases_batch_for_user_skips_per_release_retry_after_non_retryable_batch_error()
     {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        sqlx::query(
            r#"
            UPDATE repo_releases
            SET body = ?, name = 'Release v1.2.3', tag_name = 'v1.2.3'
            WHERE release_id = ?
            "#,
        )
        .bind("- item")
        .bind(120_i64)
        .execute(&pool)
        .await
        .expect("update release body");

        let batch_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let route_batch_calls = Arc::clone(&batch_calls);
        let route_fallback_calls = Arc::clone(&fallback_calls);
        let base_url = spawn_test_ai_server(Router::new().route(
            "/chat/completions",
            post(move |Json(payload): Json<Value>| {
                let route_batch_calls = Arc::clone(&route_batch_calls);
                let route_fallback_calls = Arc::clone(&route_fallback_calls);
                async move {
                    let system_prompt = payload["messages"][0]["content"]
                        .as_str()
                        .unwrap_or_default();
                    if system_prompt.contains("批量翻译助手") {
                        route_batch_calls.fetch_add(1, Ordering::SeqCst);
                        return (
                            StatusCode::UNAUTHORIZED,
                            [(header::CONTENT_TYPE, "application/json")],
                            Json(json!({"error": {"message": "bad key"}})),
                        );
                    }
                    route_fallback_calls.fetch_add(1, Ordering::SeqCst);
                    let response = serde_json::json!({
                        "choices": [{"message": {"content": "should not be called"}}],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}
                    });
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        Json(response),
                    )
                }
            }),
        ))
        .await;
        let state = setup_state_with_ai_base_url(pool.clone(), base_url);

        let translated =
            translate_releases_batch_for_user(state.as_ref(), user_id.as_str(), &[120])
                .await
                .expect("translate release batch");
        let replay = translate_releases_batch_for_user(state.as_ref(), user_id.as_str(), &[120])
            .await
            .expect("translate cached release batch failure");

        assert_eq!(translated.items.len(), 1);
        assert_eq!(translated.items[0].status, "error");
        assert_eq!(replay.items.len(), 1);
        assert_eq!(replay.items[0].status, "error");
        assert_eq!(batch_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 0);

        let row = sqlx::query(
            r#"
            SELECT status, error_text
            FROM ai_translations
            WHERE user_id = ?
              AND entity_type = 'release_detail'
              AND entity_id = ?
              AND lang = 'zh-CN'
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .bind("120")
        .fetch_one(&pool)
        .await
        .expect("load persisted terminal detail translation");
        assert_eq!(row.get::<String, _>("status"), "error");
        assert!(
            row.get::<Option<String>, _>("error_text")
                .as_deref()
                .is_some_and(|value| value.contains("401") || value.contains("bad key"))
        );
    }

    #[tokio::test]
    async fn get_release_detail_preserves_terminal_missing_state() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        let state = setup_state_with_ai(pool.clone());
        let source_hash = release_detail_source_hash("openai/codex", "Release v1.2.3", "- item");

        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary, error_text, active_work_item_id, created_at, updated_at
            )
            VALUES (?, ?, 'release_detail', ?, 'zh-CN', ?, 'missing', NULL, NULL, NULL, NULL, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("120")
        .bind(source_hash.as_str())
        .bind("2026-02-23T00:00:00Z")
        .bind("2026-02-23T00:00:00Z")
        .execute(&pool)
        .await
        .expect("seed terminal missing detail translation");

        let Json(detail) =
            get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
                .await
                .expect("get release detail");

        let translated = detail.translated.expect("translated detail");
        assert_eq!(translated.status, "missing");
        assert_eq!(translated.auto_translate, Some(false));
    }

    #[tokio::test]
    async fn get_release_detail_keeps_ready_translation_visible_while_refresh_pending() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        let state = setup_state_with_ai(pool.clone());
        let work_item_id = "refresh-work-item";
        let stale_source_hash =
            release_detail_source_hash("openai/codex", "Release v1.2.3", "- older");

        sqlx::query(
            r#"
            INSERT INTO translation_work_items (
              id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version,
              model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate,
              deadline_at, status, cache_hit, created_at, updated_at
            )
            VALUES (?, ?, ?, 'release_detail', 'full', ?, 'zh-CN', 4, 'default', ?, '[]', '[]', 0, ?, 'queued', 0, ?, ?)
            "#,
        )
        .bind(work_item_id)
        .bind("dedupe-refresh-work-item")
        .bind(user_id.as_str())
        .bind("120")
        .bind(stale_source_hash.as_str())
        .bind("2026-02-23T00:00:05Z")
        .bind("2026-02-23T00:00:00Z")
        .bind("2026-02-23T00:00:00Z")
        .execute(&pool)
        .await
        .expect("seed queued detail refresh work item");

        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary, error_text, active_work_item_id, created_at, updated_at
            )
            VALUES (?, ?, 'release_detail', ?, 'zh-CN', ?, 'ready', ?, ?, NULL, ?, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("120")
        .bind(stale_source_hash.as_str())
        .bind("旧标题")
        .bind("- 旧摘要")
        .bind(work_item_id)
        .bind("2026-02-23T00:00:00Z")
        .bind("2026-02-23T00:00:00Z")
        .execute(&pool)
        .await
        .expect("seed stale ready detail translation");

        let Json(detail) =
            get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
                .await
                .expect("get release detail");

        let translated = detail.translated.expect("translated detail");
        assert_eq!(translated.status, "ready");
        assert_eq!(translated.title.as_deref(), Some("旧标题"));
        assert_eq!(translated.summary.as_deref(), Some("- 旧摘要"));
    }

    #[tokio::test]
    async fn get_release_detail_treats_invalid_fresh_ready_as_error() {
        let pool = setup_pool().await;
        let user_id = test_user_id(1);
        seed_repo_release(&pool, 42, 120).await;
        seed_star(&pool, 42).await;
        let state = setup_state_with_ai(pool.clone());
        let source_hash = release_detail_source_hash("openai/codex", "Release v1.2.3", "- item");

        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary, error_text, active_work_item_id, created_at, updated_at
            )
            VALUES (?, ?, 'release_detail', ?, 'zh-CN', ?, 'ready', ?, NULL, NULL, NULL, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("120")
        .bind(source_hash.as_str())
        .bind("空摘要标题")
        .bind("2026-02-23T00:00:00Z")
        .bind("2026-02-23T00:00:00Z")
        .execute(&pool)
        .await
        .expect("seed invalid ready detail translation");

        let Json(detail) =
            get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
                .await
                .expect("get release detail");

        let translated = detail.translated.expect("translated detail");
        assert_eq!(translated.status, "error");
        assert_eq!(
            translated.error_code.as_deref(),
            Some("markdown_structure_mismatch")
        );
        assert_eq!(
            translated.error_summary.as_deref(),
            Some("Markdown 结构校验失败")
        );
        assert_eq!(
            translated.error_detail.as_deref(),
            Some("release detail translation failed to preserve markdown structure")
        );
        assert!(translated.title.is_none());
        assert!(translated.summary.is_none());
    }

    #[tokio::test]
    async fn get_release_detail_rejects_unstarred_release_without_brief_link() {
        let pool = setup_pool().await;
        seed_repo_release(&pool, 42, 120).await;
        let state = setup_state(pool);

        let err = get_release_detail(State(state), setup_session(1).await, Path("120".to_owned()))
            .await
            .expect_err("release detail should stay hidden");

        assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
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
    fn public_compare_fallback_retries_on_reauth_required() {
        assert!(should_retry_public_compare_without_auth(
            &github_reauth_required_error(),
        ));
    }

    #[test]
    fn public_compare_fallback_retries_on_access_restricted() {
        assert!(should_retry_public_compare_without_auth(
            &github_access_restricted_error(),
        ));
    }

    #[test]
    fn public_compare_fallback_skips_other_terminal_errors() {
        assert!(!should_retry_public_compare_without_auth(
            &github_rate_limited_error(),
        ));
    }

    #[test]
    fn public_compare_fallback_preserves_access_restricted_error_on_private_repo_failure() {
        let auth_err = github_access_restricted_error();
        let public_err = ApiError::new(StatusCode::NOT_FOUND, "not_found", "compare not found");
        let mapped = map_public_compare_fallback_error(auth_err, public_err);
        assert_eq!(mapped.code(), "forbidden");
    }

    #[test]
    fn public_compare_fallback_maps_reauth_failure_to_private_scope_required() {
        let auth_err = github_reauth_required_error();
        let public_err = ApiError::new(StatusCode::NOT_FOUND, "not_found", "compare not found");
        let mapped = map_public_compare_fallback_error(auth_err, public_err);
        assert_eq!(mapped.code(), "reauth_required");
        assert!(
            mapped
                .to_string()
                .contains("private repository compare requires repo scope")
        );
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
    fn preserve_chunk_edge_newlines_keeps_chunk_boundaries() {
        assert_eq!(
            preserve_chunk_edge_newlines("line\n", "译文".to_owned()),
            "译文\n"
        );
        assert_eq!(
            preserve_chunk_edge_newlines("line", "译文".to_owned()),
            "译文"
        );
        assert_eq!(
            preserve_chunk_edge_newlines("line\n", "译文\n".to_owned()),
            "译文\n"
        );
        assert_eq!(
            preserve_chunk_edge_newlines("\nline\n\n", "line\n".to_owned()),
            "\nline\n\n"
        );
    }

    #[test]
    fn normalize_markdown_translation_output_strips_outer_fence_only() {
        let normalized = normalize_markdown_translation_output(
            "- item\n",
            "```markdown\n- 条目\n```\n\n".to_owned(),
        );
        assert_eq!(normalized, "- 条目\n");
    }

    #[test]
    fn normalize_markdown_translation_output_keeps_fenced_code_only_chunks() {
        let normalized = normalize_markdown_translation_output(
            "```rust\nfn main() {}\n```\n",
            "```rust\nfn main() {}\n```\n\n".to_owned(),
        );
        assert_eq!(normalized, "```rust\nfn main() {}\n```\n");
    }

    #[test]
    fn normalize_markdown_translation_output_strips_outer_fence_for_multi_block_chunks() {
        let source = "```rust\nfn main() {}\n```\n\n说明文字\n\n```json\n{}\n```\n";
        let normalized = normalize_markdown_translation_output(
            source,
            "```markdown\n```rust\nfn main() {}\n```\n\n说明文字\n\n```json\n{}\n```\n```\n"
                .to_owned(),
        );
        assert_eq!(normalized, source);
    }

    #[test]
    fn normalize_markdown_translation_output_strips_outer_wrapper_for_code_only_chunks() {
        let source = "```rust\nfn main() {}\n```\n";
        let normalized = normalize_markdown_translation_output(
            source,
            "```markdown\n```rust\nfn main() {}\n```\n```\n".to_owned(),
        );
        assert_eq!(normalized, source);
    }

    #[test]
    fn normalize_markdown_translation_output_keeps_valid_multi_block_fences() {
        let source = "```rust\nfn main() {}\n```\n\n说明文字\n\n```json\n{}\n```\n";
        let normalized = normalize_markdown_translation_output(source, source.to_owned());
        assert_eq!(normalized, source);
    }

    #[test]
    fn normalize_markdown_translation_output_preserves_leading_indentation() {
        let source = "    缩进代码块\n";
        let normalized = normalize_markdown_translation_output(source, source.to_owned());
        assert_eq!(normalized, source);
    }

    #[test]
    fn normalize_markdown_translation_output_preserves_indentation_when_unwrapping_outer_fence() {
        let source = "    缩进代码块  \n";
        let normalized = normalize_markdown_translation_output(
            source,
            "```markdown\n    缩进代码块  \n```\n".to_owned(),
        );
        assert_eq!(normalized, source);
    }

    #[test]
    fn normalize_markdown_translation_output_preserves_blank_line_padding() {
        let source = "\n说明文字\n\n";
        let normalized = normalize_markdown_translation_output(
            source,
            "```markdown\n\n说明文字\n\n```\n".to_owned(),
        );
        assert_eq!(normalized, source);
    }

    #[test]
    fn normalize_markdown_translation_output_strips_wrapped_fence_with_surrounding_spaces() {
        let source = "- 条目\n";
        let normalized =
            normalize_markdown_translation_output(source, " ```markdown\n- 条目\n``` ".to_owned());
        assert_eq!(normalized, source);
    }

    #[tokio::test]
    async fn persist_daily_brief_profile_rejects_missing_scheduler_slots() {
        let pool = setup_pool().await;
        sqlx::query(
            r#"
            UPDATE daily_brief_hour_slots
            SET enabled = 0, updated_at = '2026-04-14T00:00:00Z'
            WHERE hour_utc = 12
            "#,
        )
        .execute(&pool)
        .await
        .expect("disable scheduler slot");
        let state = setup_state(pool);

        let err = super::persist_daily_brief_profile(
            state.as_ref(),
            test_user_id(1).as_str(),
            super::DailyBriefProfilePatchRequest {
                daily_brief_local_time: "08:00".to_owned(),
                daily_brief_time_zone: "America/New_York".to_owned(),
                include_own_releases: None,
            },
        )
        .await
        .expect_err("profile update should fail when required slot is disabled");

        assert_eq!(err.code(), "bad_request");
        assert!(err.to_string().contains("12:00Z"));
    }

    #[tokio::test]
    async fn persist_daily_brief_profile_accepts_supported_scheduler_slots() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        let profile = super::persist_daily_brief_profile(
            state.as_ref(),
            test_user_id(1).as_str(),
            super::DailyBriefProfilePatchRequest {
                daily_brief_local_time: "08:00".to_owned(),
                daily_brief_time_zone: "America/New_York".to_owned(),
                include_own_releases: None,
            },
        )
        .await
        .expect("profile update should succeed");

        assert_eq!(profile.daily_brief_local_time, "08:00");
        assert_eq!(profile.daily_brief_time_zone, "America/New_York");
        assert!(!profile.include_own_releases);

        let row = sqlx::query_as::<_, (Option<String>, Option<String>, i64)>(
            r#"
            SELECT daily_brief_local_time, daily_brief_time_zone, include_own_releases
            FROM users
            WHERE id = ?
            "#,
        )
        .bind(test_user_id(1))
        .fetch_one(&pool)
        .await
        .expect("load persisted profile");

        assert_eq!(row.0.as_deref(), Some("08:00"));
        assert_eq!(row.1.as_deref(), Some("America/New_York"));
        assert_eq!(row.2, 0);
    }

    #[tokio::test]
    async fn load_daily_brief_profile_defaults_include_own_releases_to_false() {
        let pool = setup_pool().await;
        let state = setup_state(pool);

        let profile = super::load_daily_brief_profile(state.as_ref(), test_user_id(1).as_str())
            .await
            .expect("load profile");

        assert!(!profile.include_own_releases);
    }

    #[tokio::test]
    async fn persist_daily_brief_profile_updates_include_own_releases_when_present() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        let profile = super::persist_daily_brief_profile(
            state.as_ref(),
            test_user_id(1).as_str(),
            super::DailyBriefProfilePatchRequest {
                daily_brief_local_time: "09:00".to_owned(),
                daily_brief_time_zone: "Asia/Shanghai".to_owned(),
                include_own_releases: Some(true),
            },
        )
        .await
        .expect("profile update should succeed");

        assert!(profile.include_own_releases);

        let include_own_releases = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT include_own_releases
            FROM users
            WHERE id = ?
            "#,
        )
        .bind(test_user_id(1))
        .fetch_one(&pool)
        .await
        .expect("load include_own_releases");

        assert_eq!(include_own_releases, 1);
    }

    #[tokio::test]
    async fn persist_daily_brief_profile_preserves_include_own_releases_when_omitted() {
        let pool = setup_pool().await;
        set_include_own_releases(&pool, true).await;
        let state = setup_state(pool.clone());

        let profile = super::persist_daily_brief_profile(
            state.as_ref(),
            test_user_id(1).as_str(),
            super::DailyBriefProfilePatchRequest {
                daily_brief_local_time: "10:00".to_owned(),
                daily_brief_time_zone: "Asia/Tokyo".to_owned(),
                include_own_releases: None,
            },
        )
        .await
        .expect("profile update should preserve include_own_releases");

        assert!(profile.include_own_releases);
    }
}
