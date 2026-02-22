use std::sync::Arc;

use axum::extract::Query;
use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
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
    window_start: Option<String>,
    window_end: Option<String>,
    content_markdown: String,
    created_at: String,
}

pub async fn list_briefs(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<Vec<BriefItem>>, ApiError> {
    let user_id = require_user_id(&session).await?;

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
    date: String,
    window_start: Option<String>,
    window_end: Option<String>,
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
    subtitle: Option<String>,
    reason: Option<String>,
    subject_type: Option<String>,
    html_url: Option<String>,
    unread: Option<i64>,
    translated: Option<TranslatedItem>,
}

#[derive(Debug, Serialize)]
pub struct TranslatedItem {
    lang: String,
    status: String, // ready | missing | disabled
    title: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct FeedRow {
    kind: String,
    sort_ts: String,
    ts: String,
    id_key: String,
    entity_id: String,
    repo_full_name: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    reason: Option<String>,
    subject_type: Option<String>,
    html_url: Option<String>,
    unread: Option<i64>,
    release_body: Option<String>,
    trans_source_hash: Option<String>,
    trans_title: Option<String>,
    trans_summary: Option<String>,
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

fn parse_types(types: Option<String>) -> Result<(bool, bool), ApiError> {
    let Some(types) = types else {
        return Ok((true, true));
    };
    let mut include_releases = false;
    let mut include_notifications = false;
    for part in types.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match part {
            "releases" | "release" => include_releases = true,
            "notifications" | "notification" | "inbox" => include_notifications = true,
            _ => return Err(ApiError::bad_request(format!("invalid types: {part}"))),
        }
    }
    if !include_releases && !include_notifications {
        return Err(ApiError::bad_request("types is empty"));
    }
    Ok((include_releases, include_notifications))
}

fn truncate_chars<'a>(s: &'a str, max_chars: usize) -> std::borrow::Cow<'a, str> {
    if s.chars().count() > max_chars {
        std::borrow::Cow::Owned(s.chars().take(max_chars).collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

pub async fn list_feed(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(q): Query<FeedQuery>,
) -> Result<Json<FeedResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
    let (include_releases, include_notifications) = parse_types(q.types)?;

    let limit = q.limit.unwrap_or(30).clamp(1, 100);
    let cursor = q.cursor.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let (cursor_sort_ts, cursor_kind_rank, cursor_id_key) = match cursor {
        Some(c) => {
            let (sort_ts, kind_rank, id_key) = parse_cursor(c)?;
            (Some(sort_ts), Some(kind_rank), Some(id_key))
        }
        None => (None, None, None),
    };

    let release_sql = r#"
        SELECT
          'release' AS kind,
          1 AS kind_rank,
          COALESCE(r.published_at, r.created_at, r.updated_at) AS sort_ts,
          COALESCE(r.published_at, r.created_at, r.updated_at) AS ts,
          printf('%020d', r.release_id) AS id_key,
          CAST(r.release_id AS TEXT) AS entity_id,
          sr.full_name AS repo_full_name,
          COALESCE(NULLIF(TRIM(r.name), ''), r.tag_name) AS title,
          NULL AS subtitle,
          NULL AS reason,
          NULL AS subject_type,
          r.html_url AS html_url,
          NULL AS unread,
          r.body AS release_body
        FROM releases r
        JOIN starred_repos sr
          ON sr.user_id = r.user_id AND sr.repo_id = r.repo_id
        WHERE r.user_id = ?
    "#;

    let notif_sql = r#"
        SELECT
          'notification' AS kind,
          0 AS kind_rank,
          COALESCE(n.last_seen_at, n.updated_at, '1970-01-01T00:00:00Z') AS sort_ts,
          COALESCE(n.updated_at, n.last_seen_at, '1970-01-01T00:00:00Z') AS ts,
          n.thread_id AS id_key,
          n.thread_id AS entity_id,
          n.repo_full_name AS repo_full_name,
          n.subject_title AS title,
          n.reason AS subtitle,
          n.reason AS reason,
          n.subject_type AS subject_type,
          ('https://github.com/notifications/thread/' || n.thread_id) AS html_url,
          n.unread AS unread,
          NULL AS release_body
        FROM notifications n
        WHERE n.user_id = ?
    "#;

    let mut parts: Vec<&str> = Vec::new();
    if include_releases {
        parts.push(release_sql);
    }
    if include_notifications {
        parts.push(notif_sql);
    }

    let inner = parts.join("\nUNION ALL\n");

    let (sql, has_cursor) = if cursor_sort_ts.is_some() {
        (
            format!(
                r#"
                WITH items AS (
                {inner}
                )
                SELECT
                  i.kind, i.sort_ts, i.ts, i.id_key, i.entity_id,
                  i.repo_full_name, i.title, i.subtitle, i.reason, i.subject_type, i.html_url, i.unread,
                  i.release_body,
                  t.source_hash AS trans_source_hash,
                  t.title AS trans_title,
                  t.summary AS trans_summary
                FROM items i
                LEFT JOIN ai_translations t
                  ON t.user_id = ? AND t.entity_type = i.kind AND t.entity_id = i.entity_id AND t.lang = 'zh-CN'
                WHERE (
                  i.sort_ts < ?
                  OR (i.sort_ts = ? AND i.kind_rank < ?)
                  OR (i.sort_ts = ? AND i.kind_rank = ? AND i.id_key < ?)
                )
                ORDER BY i.sort_ts DESC, i.kind_rank DESC, i.id_key DESC
                LIMIT ?
                "#
            ),
            true,
        )
    } else {
        (
            format!(
                r#"
                WITH items AS (
                {inner}
                )
                SELECT
                  i.kind, i.sort_ts, i.ts, i.id_key, i.entity_id,
                  i.repo_full_name, i.title, i.subtitle, i.reason, i.subject_type, i.html_url, i.unread,
                  i.release_body,
                  t.source_hash AS trans_source_hash,
                  t.title AS trans_title,
                  t.summary AS trans_summary
                FROM items i
                LEFT JOIN ai_translations t
                  ON t.user_id = ? AND t.entity_type = i.kind AND t.entity_id = i.entity_id AND t.lang = 'zh-CN'
                ORDER BY i.sort_ts DESC, i.kind_rank DESC, i.id_key DESC
                LIMIT ?
                "#
            ),
            false,
        )
    };

    let mut qy = sqlx::query_as::<_, FeedRow>(&sql);
    // binds for inner query
    if include_releases {
        qy = qy.bind(user_id);
    }
    if include_notifications {
        qy = qy.bind(user_id);
    }
    // bind for translation join
    qy = qy.bind(user_id);
    // binds for cursor
    if has_cursor {
        let sort_ts = cursor_sort_ts.as_ref().expect("cursor sort_ts");
        let kind_rank = cursor_kind_rank.expect("cursor kind_rank");
        let id_key = cursor_id_key.as_ref().expect("cursor id_key");
        qy = qy
            .bind(sort_ts)
            .bind(sort_ts)
            .bind(kind_rank)
            .bind(sort_ts)
            .bind(kind_rank)
            .bind(id_key);
    }
    qy = qy.bind(limit);

    let rows = qy
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;

    let ai_enabled = state.config.ai.is_some();

    let mut items = Vec::with_capacity(rows.len());
    let mut next_cursor: Option<String> = None;
    for (idx, r) in rows.into_iter().enumerate() {
        if idx == limit.saturating_sub(1) as usize {
            next_cursor = Some(format!("{}|{}|{}", r.sort_ts, r.kind, r.id_key));
        }

        let source = match r.kind.as_str() {
            "release" => format!(
                "kind=release\nrepo={}\ntitle={}\nbody={}\n",
                r.repo_full_name.as_deref().unwrap_or(""),
                r.title.as_deref().unwrap_or(""),
                truncate_chars(r.release_body.as_deref().unwrap_or(""), 6000),
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
                Some(TranslatedItem {
                    lang: "zh-CN".to_owned(),
                    status: "ready".to_owned(),
                    title: r.trans_title,
                    summary: r.trans_summary,
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

        items.push(FeedItem {
            kind: r.kind,
            ts: r.ts,
            id: r.entity_id,
            repo_full_name: r.repo_full_name,
            title: r.title,
            subtitle: r.subtitle,
            reason: r.reason,
            subject_type: r.subject_type,
            html_url: r.html_url,
            unread: r.unread,
            translated,
        });
    }

    // If we returned fewer than limit, there's no next page.
    if items.len() < limit as usize {
        next_cursor = None;
    }

    Ok(Json(FeedResponse { items, next_cursor }))
}

#[derive(Debug, Deserialize)]
pub struct TranslateReleaseRequest {
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
    let user_id = require_user_id(&session).await?;
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
    let body = if body.chars().count() > 6000 {
        body.chars().take(6000).collect::<String>()
    } else {
        body
    };

    let source = format!(
        "kind=release\nrepo={}\ntitle={}\nbody={}\n",
        row.full_name, title, body
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
        return Ok(Json(TranslateResponse {
            lang: "zh-CN".to_owned(),
            status: "ready".to_owned(),
            title: c.title,
            summary: c.summary,
        }));
    }

    let prompt = format!(
        "Repo: {repo}\nOriginal title: {title}\n\nRelease body (truncated):\n{body}\n\n请把这条 Release 翻译并总结为中文，输出严格 JSON（不要 markdown code block）：\n{{\"title_zh\": \"...\", \"summary_md\": \"- ...\\n- ...\"}}\n\n要求：保留版本号/专有名词；summary_md 2-5 条；不包含任何 URL。",
        repo = row.full_name,
        title = title,
        body = body,
    );

    let raw = ai::chat_completion(
        state.as_ref(),
        "你是一个助理，负责把 GitHub Release 标题与正文转写为中文标题与简短要点摘要（Markdown）。不要包含任何 URL。",
        &prompt,
        700,
    )
    .await
    .map_err(ApiError::internal)?;

    let parsed = serde_json::from_str::<TranslationJson>(&raw).ok();
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

pub async fn translate_notification(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslateNotificationRequest>,
) -> Result<Json<TranslateResponse>, ApiError> {
    let user_id = require_user_id(&session).await?;
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
        return Ok(Json(TranslateResponse {
            lang: "zh-CN".to_owned(),
            status: "ready".to_owned(),
            title: c.title,
            summary: c.summary,
        }));
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

    let parsed = serde_json::from_str::<TranslationJson>(&raw).ok();
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
