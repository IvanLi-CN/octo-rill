use std::sync::Arc;

use axum::extract::{Path, Query};
use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use url::Url;

use crate::{ai, sync};
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

pub async fn sync_starred(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<sync::SyncStarredResult>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let res = sync::sync_starred(state.as_ref(), user_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(res))
}

pub async fn sync_releases(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<sync::SyncReleasesResult>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let res = sync::sync_releases(state.as_ref(), user_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(res))
}

pub async fn sync_notifications(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<sync::SyncNotificationsResult>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let res = sync::sync_notifications(state.as_ref(), user_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(res))
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
) -> Result<Json<BriefGenerateResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let content = ai::generate_daily_brief(state.as_ref(), user_id)
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
    }))
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
pub struct TranslateReleaseDetailRequest {
    release_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TranslateNotificationRequest {
    thread_id: String,
}

#[derive(Debug, Serialize)]
pub struct TranslateResponse {
    lang: String,
    status: String, // ready | disabled
    title: Option<String>,
    summary: Option<String>,
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
            if s.is_empty() {
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

pub async fn translate_release(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateReleaseRequest>,
) -> Result<Json<TranslateResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let release_id_raw = req.release_id.trim();
    if release_id_raw.is_empty() {
        return Err(ApiError::bad_request("release_id is required"));
    }
    let release_id: i64 = release_id_raw
        .parse()
        .map_err(|_| ApiError::bad_request("release_id must be an integer string"))?;

    if state.config.ai.is_none() {
        return Ok(Json(TranslateResponse {
            lang: "zh-CN".to_owned(),
            status: "disabled".to_owned(),
            title: None,
            summary: None,
        }));
    }

    #[derive(Debug, sqlx::FromRow)]
    struct ReleaseSourceRow {
        full_name: String,
        tag_name: String,
        name: Option<String>,
        body: Option<String>,
    }

    let row = sqlx::query_as::<_, ReleaseSourceRow>(
        r#"
        SELECT sr.full_name, r.tag_name, r.name, r.body
        FROM releases r
        JOIN starred_repos sr
          ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
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

    let title = row
        .name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&row.tag_name)
        .to_owned();

    let body = row.body.unwrap_or_default();
    let excerpt = release_excerpt(Some(&body));
    let excerpt = excerpt.unwrap_or_default();
    let excerpt = if excerpt.chars().count() > 2000 {
        excerpt.chars().take(2000).collect::<String>()
    } else {
        excerpt
    };

    let source = format!(
        "v=4\nkind=release\nrepo={}\ntitle={}\nexcerpt={}\n",
        row.full_name, title, excerpt
    );
    let source_hash = ai::sha256_hex(&source);
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
        WHERE user_id = ? AND entity_type = 'release' AND entity_id = ? AND lang = 'zh-CN'
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(&entity_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    if let Some(c) = cached
        && c.source_hash == source_hash
    {
        if let Some(raw) = c.summary.as_deref() {
            let t = raw.trim_start();
            if (t.starts_with('{') || t.starts_with("\"{"))
                && let Some((t_title, t_summary)) = extract_translation_from_json_blob(raw)
            {
                let out_title = t_title.or_else(|| c.title.clone());
                let out_summary = t_summary.or_else(|| c.summary.clone());

                // Normalize the cache so subsequent requests don't have to parse the blob.
                upsert_translation(
                    state.as_ref(),
                    user_id,
                    TranslationUpsert {
                        entity_type: "release",
                        entity_id: &entity_id,
                        lang: "zh-CN",
                        source_hash: &source_hash,
                        title: out_title.as_deref(),
                        summary: out_summary.as_deref(),
                    },
                )
                .await?;

                return Ok(Json(TranslateResponse {
                    lang: "zh-CN".to_owned(),
                    status: "ready".to_owned(),
                    title: out_title,
                    summary: out_summary,
                }));
            }
        }

        // If the cache looks normal, return it. If it looked like a JSON blob and we couldn't
        // salvage it, fall through and regenerate.
        let cache_is_json_blob = c
            .summary
            .as_deref()
            .map(|raw| {
                let t = raw.trim_start();
                t.starts_with('{') || t.starts_with("\"{")
            })
            .unwrap_or(false);
        let cache_preserves_structure = c
            .summary
            .as_deref()
            .map(|s| markdown_structure_preserved(&excerpt, s))
            .unwrap_or(true);
        if !cache_is_json_blob && cache_preserves_structure {
            return Ok(Json(TranslateResponse {
                lang: "zh-CN".to_owned(),
                status: "ready".to_owned(),
                title: c.title,
                summary: c.summary,
            }));
        }
    }

    let prompt = release_translation_prompt(&row.full_name, &title, &excerpt, None);

    let raw = ai::chat_completion(
        state.as_ref(),
        "你是一个助理，负责把 GitHub Release 标题与发布说明翻译为中文（严格保留 Markdown 结构与标记，不新增信息）。不要包含任何 URL。",
        &prompt,
        900,
    )
    .await
    .map_err(ApiError::internal)?;

    let (mut out_title, mut out_summary) = extract_translation_fields(&raw);
    if let Some(summary) = out_summary.as_deref()
        && !markdown_structure_preserved(&excerpt, summary)
    {
        let retry_prompt =
            release_translation_prompt(&row.full_name, &title, &excerpt, Some(summary));
        if let Ok(retry_raw) = ai::chat_completion(
            state.as_ref(),
            "你是一个助理，负责把 GitHub Release 标题与发布说明翻译为中文（严格保留 Markdown 结构与标记，不新增信息）。不要包含任何 URL。",
            &retry_prompt,
            900,
        )
        .await
        {
            let (retry_title, retry_summary) = extract_translation_fields(&retry_raw);
            if retry_summary
                .as_deref()
                .is_some_and(|s| markdown_structure_preserved(&excerpt, s))
            {
                out_title = retry_title.or(out_title);
                out_summary = retry_summary;
            }
        }
    }

    upsert_translation(
        state.as_ref(),
        user_id,
        TranslationUpsert {
            entity_type: "release",
            entity_id: &entity_id,
            lang: "zh-CN",
            source_hash: &source_hash,
            title: out_title.as_deref(),
            summary: out_summary.as_deref(),
        },
    )
    .await?;

    Ok(Json(TranslateResponse {
        lang: "zh-CN".to_owned(),
        status: "ready".to_owned(),
        title: out_title,
        summary: out_summary,
    }))
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

pub async fn translate_release_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateReleaseDetailRequest>,
) -> Result<Json<TranslateResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let release_id = parse_release_id_param(&req.release_id)?;

    if state.config.ai.is_none() {
        return Ok(Json(TranslateResponse {
            lang: "zh-CN".to_owned(),
            status: "disabled".to_owned(),
            title: None,
            summary: None,
        }));
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
        return Ok(Json(TranslateResponse {
            lang: "zh-CN".to_owned(),
            status: "ready".to_owned(),
            title: cached.title,
            summary: cached.summary,
        }));
    }

    let translated_title = ai::chat_completion(
        state.as_ref(),
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
        let total_chunks = chunks.len();
        let mut translated_chunks = Vec::with_capacity(total_chunks);
        for (idx, chunk) in chunks.iter().enumerate() {
            let translated = translate_release_detail_chunk(
                state.as_ref(),
                &repo_full_name,
                &original_title,
                chunk,
                idx + 1,
                total_chunks,
            )
            .await?;
            translated_chunks.push(translated);
        }

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
        state.as_ref(),
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

    Ok(Json(TranslateResponse {
        lang: "zh-CN".to_owned(),
        status: "ready".to_owned(),
        title: translated_title,
        summary: translated_summary,
    }))
}

pub async fn translate_notification(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateNotificationRequest>,
) -> Result<Json<TranslateResponse>, ApiError> {
    let user_id = require_active_user_id(state.as_ref(), &session).await?;
    let thread_id = req.thread_id.trim();
    if thread_id.is_empty() {
        return Err(ApiError::bad_request("thread_id is required"));
    }

    if state.config.ai.is_none() {
        return Ok(Json(TranslateResponse {
            lang: "zh-CN".to_owned(),
            status: "disabled".to_owned(),
            title: None,
            summary: None,
        }));
    }

    #[derive(Debug, sqlx::FromRow)]
    struct NotificationSourceRow {
        repo_full_name: Option<String>,
        subject_title: Option<String>,
        reason: Option<String>,
        subject_type: Option<String>,
    }

    let row = sqlx::query_as::<_, NotificationSourceRow>(
        r#"
        SELECT repo_full_name, subject_title, reason, subject_type
        FROM notifications
        WHERE user_id = ? AND thread_id = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(thread_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "notification not found",
        ));
    };

    let source = format!(
        "kind=notification\nrepo={}\ntitle={}\nreason={}\nsubject_type={}\n",
        row.repo_full_name.as_deref().unwrap_or(""),
        row.subject_title.as_deref().unwrap_or(""),
        row.reason.as_deref().unwrap_or(""),
        row.subject_type.as_deref().unwrap_or(""),
    );
    let source_hash = ai::sha256_hex(&source);

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
        WHERE user_id = ? AND entity_type = 'notification' AND entity_id = ? AND lang = 'zh-CN'
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(thread_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    if let Some(c) = cached
        && c.source_hash == source_hash
    {
        if let Some(raw) = c.summary.as_deref() {
            let t = raw.trim_start();
            if (t.starts_with('{') || t.starts_with("\"{"))
                && let Some((t_title, t_summary)) = extract_translation_from_json_blob(raw)
            {
                let out_title = t_title.or_else(|| c.title.clone());
                let out_summary = t_summary.or_else(|| c.summary.clone());

                upsert_translation(
                    state.as_ref(),
                    user_id,
                    TranslationUpsert {
                        entity_type: "notification",
                        entity_id: thread_id,
                        lang: "zh-CN",
                        source_hash: &source_hash,
                        title: out_title.as_deref(),
                        summary: out_summary.as_deref(),
                    },
                )
                .await?;

                return Ok(Json(TranslateResponse {
                    lang: "zh-CN".to_owned(),
                    status: "ready".to_owned(),
                    title: out_title,
                    summary: out_summary,
                }));
            }
        }

        let cache_is_json_blob = c
            .summary
            .as_deref()
            .map(|raw| {
                let t = raw.trim_start();
                t.starts_with('{') || t.starts_with("\"{")
            })
            .unwrap_or(false);
        if !cache_is_json_blob {
            return Ok(Json(TranslateResponse {
                lang: "zh-CN".to_owned(),
                status: "ready".to_owned(),
                title: c.title,
                summary: c.summary,
            }));
        }
    }

    let prompt = format!(
        "Repo: {repo}\nOriginal title: {title}\nReason: {reason}\nType: {subject_type}\n\n请把这条 Inbox 通知翻译/解释为中文，输出严格 JSON（不要 markdown code block）：\n{{\"title_zh\": \"...\", \"summary_md\": \"- ...\"}}\n\n要求：summary_md 1-3 条；给出建议动作；不包含任何 URL。",
        repo = row.repo_full_name.as_deref().unwrap_or("(unknown repo)"),
        title = row.subject_title.as_deref().unwrap_or("(no title)"),
        reason = row.reason.as_deref().unwrap_or(""),
        subject_type = row.subject_type.as_deref().unwrap_or(""),
    );

    let raw = ai::chat_completion(
        state.as_ref(),
        "你是一个助理，负责把 GitHub Notifications 条目转写为中文标题与简短建议（Markdown）。不要包含任何 URL。",
        &prompt,
        500,
    )
    .await
    .map_err(ApiError::internal)?;

    let parsed = parse_translation_json(&raw);
    let out_title = parsed
        .as_ref()
        .and_then(|p| p.title_zh.as_deref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned());
    let out_summary = parsed
        .as_ref()
        .and_then(|p| p.summary_md.as_deref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned())
        .or_else(|| {
            let s = raw.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_owned())
            }
        });

    upsert_translation(
        state.as_ref(),
        user_id,
        TranslationUpsert {
            entity_type: "notification",
            entity_id: thread_id,
            lang: "zh-CN",
            source_hash: &source_hash,
            title: out_title.as_deref(),
            summary: out_summary.as_deref(),
        },
    )
    .await?;

    Ok(Json(TranslateResponse {
        lang: "zh-CN".to_owned(),
        status: "ready".to_owned(),
        title: out_title,
        summary: out_summary,
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
        AdminUserPatchRequest, AdminUserUpdateGuard, AdminUsersQuery, FeedRow, GraphQlError,
        admin_list_users, admin_patch_user, admin_users_offset, ensure_account_enabled,
        github_graphql_errors_to_api_error, github_graphql_http_error, guard_admin_user_update,
        has_repo_scope, markdown_structure_preserved, parse_release_id_param,
        parse_repo_full_name_from_release_url, parse_translation_json,
        preserve_chunk_trailing_newline, release_detail_source_hash,
        release_detail_translation_ready, release_excerpt, release_reactions_status,
        resolve_release_full_name, split_markdown_chunks,
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
