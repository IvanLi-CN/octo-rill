use std::collections::HashSet;

use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Row, Sqlite};

use crate::{error::ApiError, sqlite_write::SqliteWritePriority, state::AppState};

pub const SEARCH_LIMIT: i64 = 20;
pub const SEARCH_RATE_LIMIT: i64 = 50;
pub const SEARCH_RATE_WINDOW_SECONDS: i64 = 300;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedSearchQuery {
    pub terms: Vec<String>,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub resource_type: Option<String>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub unread: bool,
}

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub q: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub items: Vec<SearchResult>,
    pub remaining_requests: i64,
    pub reset_at: Option<String>,
    pub index_status: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: String,
    #[serde(rename = "type")]
    pub result_type: String,
    pub title: String,
    pub excerpt: Option<String>,
    pub repository: Option<SearchRepository>,
    pub updated_at: Option<String>,
    pub source_time: Option<String>,
    pub unread: bool,
    pub is_following: Option<bool>,
    pub matched_lane: String,
    pub matched_lanes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<SearchTarget>,
}

#[derive(Debug, Serialize)]
pub struct SearchRepository {
    pub owner: String,
    pub name: String,
    pub full_name: String,
}

#[derive(Debug, Serialize)]
pub struct SearchTarget {
    pub href: String,
    pub lane: Option<String>,
}

#[derive(Debug, FromRow)]
struct SearchDocumentRow {
    id: String,
    resource_type: String,
    title: Option<String>,
    body: Option<String>,
    repo_full_name: Option<String>,
    translated_text: Option<String>,
    smart_text: Option<String>,
    source_time: Option<String>,
    unread: Option<i64>,
    is_following: Option<i64>,
    target_path: Option<String>,
    target_url: Option<String>,
}

pub fn parse_query(raw: &str) -> Result<ParsedSearchQuery, ApiError> {
    let input = raw.trim();
    if input.is_empty() {
        return Err(invalid_query("搜索词不能为空"));
    }
    if input.chars().count() > 256 {
        return Err(invalid_query("搜索请求过长"));
    }

    let tokens = tokenize(input).map_err(invalid_query)?;
    let mut parsed = ParsedSearchQuery::default();
    let mut seen_filters = HashSet::new();
    for (token, quoted) in tokens {
        if !quoted && let Some((key, value)) = token.split_once(':') {
            let key = key.to_ascii_lowercase();
            let value = value.trim();
            if value.is_empty() {
                return Err(invalid_query("过滤器缺少值"));
            }
            if !seen_filters.insert(key.clone()) {
                return Err(invalid_query("过滤器不能重复"));
            }
            match key.as_str() {
                "owner" => {
                    if value.contains('/') || !valid_name(value) {
                        return Err(invalid_query("owner 过滤器无效"));
                    }
                    parsed.owner = Some(value.to_owned());
                }
                "repo" => {
                    let normalized = value.trim_matches('/');
                    let parts = normalized.split('/').collect::<Vec<_>>();
                    if parts.len() > 2 || parts.iter().any(|part| !valid_name(part)) {
                        return Err(invalid_query("repo 过滤器无效"));
                    }
                    parsed.repo = Some(normalized.to_owned());
                }
                "type" => {
                    let value = value.to_ascii_lowercase();
                    if !matches!(
                        value.as_str(),
                        "release" | "announcement" | "brief" | "notification" | "repository"
                    ) {
                        return Err(invalid_query("type 过滤器无效"));
                    }
                    parsed.resource_type = Some(value);
                }
                "after" => parsed.after = Some(parse_date(value)?),
                "before" => parsed.before = Some(parse_date(value)?),
                "is" if value.eq_ignore_ascii_case("unread") => parsed.unread = true,
                _ => return Err(invalid_query("不支持的过滤器")),
            }
            continue;
        }
        {
            parsed.terms.push(token);
        }
    }
    if parsed.terms.len() > 16 {
        return Err(invalid_query("搜索词过多"));
    }
    if let (Some(after), Some(before)) = (&parsed.after, &parsed.before)
        && after > before
    {
        return Err(invalid_query("after 不能晚于 before"));
    }
    if parsed.unread
        && parsed
            .resource_type
            .as_deref()
            .is_some_and(|value| value != "notification")
    {
        return Err(invalid_query("is:unread 只适用于通知"));
    }
    if let (Some(owner), Some(repo)) = (&parsed.owner, &parsed.repo)
        && let Some(repo_owner) = repo.split_once('/').map(|parts| parts.0)
        && !owner.eq_ignore_ascii_case(repo_owner)
    {
        return Err(invalid_query("owner 与 repo 过滤器冲突"));
    }
    if parsed.terms.is_empty()
        && parsed.owner.is_none()
        && parsed.repo.is_none()
        && parsed.resource_type.is_none()
        && parsed.after.is_none()
        && parsed.before.is_none()
        && !parsed.unread
    {
        return Err(invalid_query("请提供搜索词或过滤器"));
    }
    Ok(parsed)
}

fn tokenize(input: &str) -> Result<Vec<(String, bool)>, String> {
    let mut result = Vec::new();
    let mut chars = input.chars().peekable();
    while chars.peek().is_some() {
        while matches!(chars.peek(), Some(ch) if ch.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        if chars.next_if_eq(&'"').is_some() {
            let mut phrase = String::new();
            let mut closed = false;
            for ch in chars.by_ref() {
                if ch == '"' {
                    closed = true;
                    break;
                }
                phrase.push(ch);
            }
            if !closed || phrase.trim().is_empty() {
                return Err("引号短语无效".to_owned());
            }
            result.push((phrase, true));
            if matches!(chars.peek(), Some(ch) if !ch.is_whitespace()) {
                return Err("引号短语后需要空格".to_owned());
            }
            continue;
        }
        let mut token = String::new();
        while let Some(ch) = chars.peek().copied() {
            if ch.is_whitespace() {
                break;
            }
            if ch == '"' {
                return Err("引号必须成对出现".to_owned());
            }
            token.push(ch);
            chars.next();
        }
        if !token.is_empty() {
            result.push((token, false));
        }
    }
    Ok(result)
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~'))
}

fn parse_date(value: &str) -> Result<String, ApiError> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| invalid_query("日期过滤器必须使用 YYYY-MM-DD"))?;
    Ok(format!("{date}T00:00:00Z"))
}

fn invalid_query(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_search_query", message)
}

pub async fn consume_quota(state: &AppState, user_id: &str) -> Result<(i64, String), ApiError> {
    let now = Utc::now().timestamp();
    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_rate_limit",
            SqliteWritePriority::Foreground,
        )
        .await
        .map_err(ApiError::internal)?;

    let row = sqlx::query(
        "SELECT window_started_at, request_count FROM search_rate_limits WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let (start, count) = row
        .map(|row| {
            (
                row.get::<i64, _>("window_started_at"),
                row.get::<i64, _>("request_count"),
            )
        })
        .unwrap_or((now, 0));
    let expired = now >= start.saturating_add(SEARCH_RATE_WINDOW_SECONDS);
    let (next_start, next_count) = if expired {
        (now, 1)
    } else {
        (start, count + 1)
    };
    let reset_epoch = next_start.saturating_add(SEARCH_RATE_WINDOW_SECONDS);

    if !expired && count >= SEARCH_RATE_LIMIT {
        drop(tx);
        drop(permit);
        let retry_after = reset_epoch.saturating_sub(now).max(1) as u64;
        let reset_at = timestamp_to_rfc3339(reset_epoch);
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "search_rate_limited",
            "搜索请求过于频繁",
        )
        .with_retry_after(retry_after)
        .with_details(serde_json::json!({
            "retry_after_seconds": retry_after,
            "reset_at": reset_at,
        })));
    }

    sqlx::query(
        r#"INSERT INTO search_rate_limits (user_id, window_started_at, request_count, updated_at)
           VALUES (?, ?, ?, CURRENT_TIMESTAMP)
           ON CONFLICT(user_id) DO UPDATE SET
             window_started_at = excluded.window_started_at,
             request_count = excluded.request_count,
             updated_at = excluded.updated_at"#,
    )
    .bind(user_id)
    .bind(next_start)
    .bind(next_count)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    drop(permit);

    Ok((
        SEARCH_RATE_LIMIT - next_count,
        timestamp_to_rfc3339(reset_epoch),
    ))
}

fn timestamp_to_rfc3339(epoch: i64) -> String {
    DateTime::<Utc>::from_timestamp(epoch, 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

pub async fn query(
    state: &AppState,
    user_id: &str,
    parsed: &ParsedSearchQuery,
) -> Result<Vec<SearchResult>, ApiError> {
    let use_fts = parsed.terms.iter().any(|term| term.chars().count() >= 3);
    let fts_match_query = parsed
        .terms
        .iter()
        .filter(|term| term.chars().count() >= 3)
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut builder = QueryBuilder::<Sqlite>::new(
        "WITH current_release_metadata AS (SELECT repo_id, full_name, owner_login, ROW_NUMBER() OVER (PARTITION BY repo_id ORDER BY updated_at DESC, full_name ASC) AS metadata_rank FROM user_release_visible_repos WHERE user_id = ",
    );
    builder.push_bind(user_id);
    builder.push(
        "), ranked AS (SELECT d.id, d.resource_type, d.title, d.body, CASE WHEN d.resource_type = 'release' THEN rm.full_name ELSE d.repo_full_name END AS repo_full_name, COALESCE(ul.translated_text, d.translated_text) AS translated_text, COALESCE(ul.smart_text, d.smart_text) AS smart_text, d.source_time, d.updated_at, d.unread, CASE WHEN d.resource_type = 'repository' THEN EXISTS (SELECT 1 FROM user_repo_associations ura WHERE ura.user_id = d.user_id AND lower(ura.repo_full_name_lower) = lower(d.resource_id) AND ura.is_following != 0) ELSE NULL END AS is_following, CASE WHEN d.resource_type = 'release' AND rm.full_name IS NOT NULL AND rr.tag_name IS NOT NULL THEN '/' || rm.full_name || '/releases/tag/' || rr.tag_name ELSE CASE WHEN d.resource_type = 'release' THEN NULL ELSE d.target_path END END AS target_path, CASE WHEN d.resource_type = 'release' AND (rm.full_name IS NULL OR rr.tag_name IS NULL) THEN NULL ELSE d.target_url END AS target_url, ROW_NUMBER() OVER (PARTITION BY CASE WHEN d.resource_type = 'announcement' THEN COALESCE(d.resource_id, '') ELSE d.id END ORDER BY CASE WHEN d.resource_type = 'announcement' AND d.user_id = ",
    );
    builder.push_bind(user_id);
    builder.push(
        " THEN 0 ELSE 1 END, COALESCE(d.source_time, d.updated_at) DESC, d.id DESC) AS search_rank FROM search_documents d LEFT JOIN current_release_metadata rm ON rm.repo_id = d.repo_id AND rm.metadata_rank = 1 LEFT JOIN repo_releases rr ON rr.release_id = CAST(d.resource_id AS INTEGER) LEFT JOIN search_document_user_lanes ul ON ul.document_id = d.id AND ul.user_id = ",
    );
    builder.push_bind(user_id);
    builder.push(" WHERE ((d.resource_type IN ('release', 'announcement') AND EXISTS (SELECT 1 FROM user_release_visible_repos vr WHERE vr.user_id = ");
    builder.push_bind(user_id);
    builder.push(" AND vr.repo_id = d.repo_id)) OR (d.resource_type NOT IN ('release', 'announcement') AND d.user_id = ");
    builder.push_bind(user_id);
    builder.push(")");
    builder.push(")");

    if let Some(resource_type) = &parsed.resource_type {
        builder.push(" AND d.resource_type = ");
        builder.push_bind(resource_type);
    }
    if let Some(owner) = &parsed.owner {
        builder.push(" AND lower(COALESCE(CASE WHEN d.resource_type = 'release' THEN rm.owner_login ELSE d.owner_login END, '')) = lower(");
        builder.push_bind(owner);
        builder.push(")");
    }
    if let Some(repo) = &parsed.repo {
        builder.push(" AND (lower(COALESCE(CASE WHEN d.resource_type = 'release' THEN rm.full_name ELSE d.repo_full_name END, '')) = lower(");
        builder.push_bind(repo);
        builder.push(
            ") OR lower(COALESCE(CASE WHEN d.resource_type = 'release' THEN rm.full_name ELSE d.repo_full_name END, '')) LIKE '%/' || lower(",
        );
        builder.push_bind(escape_like(repo));
        builder.push(") ESCAPE char(92))");
    }
    if let Some(after) = &parsed.after {
        builder.push(" AND COALESCE(d.source_time, '') >= ");
        builder.push_bind(after);
    }
    if let Some(before) = &parsed.before {
        builder.push(" AND COALESCE(d.source_time, '') <= ");
        builder.push_bind(before);
    }
    if parsed.unread {
        builder.push(" AND COALESCE(d.unread, 0) != 0");
    }

    if use_fts {
        // Keep the backfill readiness gate in the same snapshot as the FTS
        // match. Realtime projection rows are eventually consistent and must
        // not globally force every search through the unindexed fallback.
        builder.push(" AND (");
        builder.push("NOT EXISTS (SELECT 1 FROM search_projection_backfill_state s WHERE s.id = 1 AND s.status = 'ready' AND NOT EXISTS (SELECT 1 FROM search_metadata_backfill_queue)) OR ");
        builder.push("d.id IN (SELECT m.document_id FROM search_documents_fts_v2 AS f JOIN search_fts_document_rows AS m ON m.fts_rowid = f.rowid WHERE search_documents_fts_v2 MATCH ");
        builder.push_bind(fts_match_query.clone());
        builder.push(") OR d.id IN (SELECT m.document_id FROM search_document_user_lanes_fts_v2 AS uf JOIN search_fts_user_lane_rows AS m ON m.fts_rowid = uf.rowid WHERE m.user_id = ");
        builder.push_bind(user_id);
        builder.push(" AND search_document_user_lanes_fts_v2 MATCH ");
        builder.push_bind(fts_match_query);
        builder.push(") OR ");
        builder.push("(");
        let fts_terms = parsed.terms.iter().filter(|term| term.chars().count() >= 3);
        for (index, term) in fts_terms.enumerate() {
            if index > 0 {
                builder.push(" OR ");
            }
            let pattern = format!("%{}%", escape_like(term));
            builder.push("lower(COALESCE(ul.translated_text, '')) LIKE lower(");
            builder.push_bind(pattern.clone());
            builder.push(") ESCAPE char(92) OR lower(COALESCE(ul.smart_text, '')) LIKE lower(");
            builder.push_bind(pattern);
            builder.push(") ESCAPE char(92)");
        }
        builder.push("))");
    }
    for term in &parsed.terms {
        let pattern = format!("%{}%", escape_like(term));
        builder.push(" AND (lower(COALESCE(d.title, '')) LIKE lower(");
        builder.push_bind(pattern.clone());
        builder.push(") ESCAPE char(92)");
        builder.push(" OR lower(COALESCE(d.body, '')) LIKE lower(");
        builder.push_bind(pattern.clone());
        builder.push(") ESCAPE char(92)");
        builder.push(" OR lower(COALESCE(CASE WHEN d.resource_type = 'release' THEN rm.full_name ELSE d.repo_full_name END, '')) LIKE lower(");
        builder.push_bind(pattern.clone());
        builder.push(") ESCAPE char(92)");
        builder.push(" OR lower(COALESCE(d.translated_text, '')) LIKE lower(");
        builder.push_bind(pattern.clone());
        builder.push(") ESCAPE char(92)");
        builder.push(" OR lower(COALESCE(d.smart_text, '')) LIKE lower(");
        builder.push_bind(pattern);
        builder.push(") ESCAPE char(92)");
        builder.push(" OR lower(COALESCE(ul.translated_text, '')) LIKE lower(");
        builder.push_bind(format!("%{}%", escape_like(term)));
        builder.push(") ESCAPE char(92)");
        builder.push(" OR lower(COALESCE(ul.smart_text, '')) LIKE lower(");
        builder.push_bind(format!("%{}%", escape_like(term)));
        builder.push(") ESCAPE char(92))");
    }
    builder.push(
        ") SELECT id, resource_type, title, body, repo_full_name, translated_text, smart_text, source_time, unread, is_following, target_path, target_url FROM ranked WHERE search_rank = 1 ORDER BY COALESCE(source_time, updated_at) DESC, id DESC LIMIT ",
    );
    builder.push_bind(SEARCH_LIMIT);

    let rows = builder
        .build_query_as::<SearchDocumentRow>()
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|row| to_result(row, &parsed.terms))
        .collect())
}

pub async fn index_status(state: &AppState) -> Result<String, ApiError> {
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM search_projection_backfill_state WHERE id = 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let metadata_pending =
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM search_metadata_backfill_queue)")
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?
            != 0;
    let projection_pending =
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM search_projection_outbox)")
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::internal)?
            != 0;
    let projection_failed = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM search_projection_outbox_failed)",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?
        != 0;
    Ok(match status.as_deref() {
        _ if projection_failed => "failed".to_owned(),
        Some("ready") if !metadata_pending && !projection_pending => "ready".to_owned(),
        Some("paused_low_disk") => "paused_low_disk".to_owned(),
        Some("failed") => "failed".to_owned(),
        _ => "building".to_owned(),
    })
}

fn to_result(row: SearchDocumentRow, terms: &[String]) -> SearchResult {
    let original = format!(
        "{} {} {}",
        row.title.as_deref().unwrap_or_default(),
        row.body.as_deref().unwrap_or_default(),
        row.repo_full_name.as_deref().unwrap_or_default()
    );
    let translated = row.translated_text.clone().unwrap_or_default();
    let smart = row.smart_text.clone().unwrap_or_default();
    let lane_texts = [
        ("original", original.as_str()),
        ("translated", translated.as_str()),
        ("smart", smart.as_str()),
    ];
    let mut lane_scores = lane_texts
        .iter()
        .map(|(lane, text)| {
            let score = terms
                .iter()
                .filter(|term| text.to_lowercase().contains(&term.to_lowercase()))
                .count();
            (*lane, score)
        })
        .collect::<Vec<_>>();
    if terms.is_empty() {
        lane_scores[0].1 = 1;
    }
    let matched_lanes = lane_scores
        .iter()
        .filter(|(_, score)| *score > 0)
        .map(|(lane, _)| (*lane).to_owned())
        .collect::<Vec<_>>();
    lane_scores.sort_by(|(lane_a, score_a), (lane_b, score_b)| {
        score_b.cmp(score_a).then_with(|| {
            ["original", "translated", "smart"]
                .iter()
                .position(|lane| lane == lane_a)
                .cmp(
                    &["original", "translated", "smart"]
                        .iter()
                        .position(|lane| lane == lane_b),
                )
        })
    });
    let matched_lane = lane_scores
        .first()
        .filter(|(_, score)| *score > 0)
        .map(|(lane, _)| (*lane).to_owned())
        .unwrap_or_else(|| "original".to_owned());
    let content = match matched_lane.as_str() {
        "translated" => translated.clone(),
        "smart" => smart.clone(),
        _ => original.clone(),
    };
    let repository = row.repo_full_name.as_ref().and_then(|full_name| {
        let (owner, name) = full_name.split_once('/')?;
        Some(SearchRepository {
            owner: owner.to_owned(),
            name: name.to_owned(),
            full_name: full_name.to_owned(),
        })
    });
    let target = row
        .target_path
        .clone()
        .or(row.target_url.clone())
        .map(|target| canonicalize_target_path(&row.resource_type, &target))
        .map(|href| SearchTarget {
            href,
            lane: Some(matched_lane.clone()),
        });
    let source_time = row.source_time.clone();
    SearchResult {
        id: row.id,
        result_type: row.resource_type,
        title: row.title.unwrap_or_else(|| content.clone()),
        excerpt: Some(content.chars().take(240).collect()),
        repository,
        updated_at: source_time.clone(),
        source_time,
        unread: row.unread.unwrap_or_default() != 0,
        is_following: row.is_following.map(|value| value != 0),
        matched_lane: matched_lane.clone(),
        matched_lanes,
        target,
    }
}

fn canonicalize_target_path(resource_type: &str, target: &str) -> String {
    if resource_type != "release" {
        return target.to_owned();
    }
    let Some((prefix, tag)) = target.rsplit_once("/releases/tag/") else {
        return target.to_owned();
    };
    if tag.is_empty() {
        return target.to_owned();
    }
    let encoded_tag = urlencoding::decode(tag)
        .map(|decoded| urlencoding::encode(decoded.as_ref()).into_owned())
        .unwrap_or_else(|_| urlencoding::encode(tag).into_owned());
    format!("{prefix}/releases/tag/{encoded_tag}")
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::SocketAddr, sync::Arc};

    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    };
    use url::Url;

    use crate::{
        ai::LlmScheduler,
        api_keys::ApiKeyLastUsedTouchQueue,
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        observability::LoggingThresholds,
        sqlite_write::SqliteWriteCoordinator,
        state::{AppState, build_oauth_client, build_webauthn},
        translations::{TranslationRuntimeConfig, TranslationSchedulerController},
    };

    #[test]
    fn parses_phrases_and_filters() {
        let parsed = parse_query(r#""release notes" owner:octo repo:octo/rill type:notification after:2026-01-01 is:unread"#).unwrap();
        assert_eq!(parsed.terms, vec!["release notes"]);
        assert_eq!(parsed.owner.as_deref(), Some("octo"));
        assert_eq!(parsed.repo.as_deref(), Some("octo/rill"));
        assert!(parsed.unread);
    }

    #[test]
    fn rejects_unknown_duplicate_and_conflicting_filters() {
        assert_eq!(
            parse_query("wat:value").unwrap_err().code(),
            "invalid_search_query"
        );
        assert_eq!(
            parse_query("owner:a owner:b").unwrap_err().code(),
            "invalid_search_query"
        );
        assert_eq!(
            parse_query("after:2026-02-01 before:2026-01-01")
                .unwrap_err()
                .code(),
            "invalid_search_query"
        );
    }

    #[test]
    fn accepts_two_character_terms_for_like_fallback() {
        let parsed = parse_query("中文").unwrap();
        assert_eq!(parsed.terms, vec!["中文"]);
    }

    async fn setup_pool() -> SqlitePool {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-search-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .after_release(|connection, _metadata| {
                Box::pin(async move {
                    crate::sqlite_write::cleanup_sqlite_pool_connection(connection).await?;
                    Ok(true)
                })
            })
            .connect_with(options)
            .await
            .expect("create search sqlite db");
        crate::database_migrations::run(&pool)
            .await
            .expect("run search migrations");
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, created_at, updated_at)
            VALUES (?, 39999999, 'search-user', ?, ?)
            "#,
        )
        .bind("search-user")
        .bind("2026-02-23T00:00:00Z")
        .bind("2026-02-23T00:00:00Z")
        .execute(&pool)
        .await
        .expect("seed search user");
        pool
    }

    #[tokio::test]
    async fn migration_is_schema_only() {
        let pool = setup_pool().await;
        let projection_rows = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents")
            .fetch_one(&pool)
            .await
            .expect("count search projection rows");
        let fts_rows = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents_fts")
            .fetch_one(&pool)
            .await
            .expect("count search FTS rows");
        let quota_rows = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_rate_limits")
            .fetch_one(&pool)
            .await
            .expect("count search quota rows");
        assert_eq!(projection_rows, 0);
        assert_eq!(fts_rows, 0);
        assert_eq!(quota_rows, 0);
        let state = sqlx::query_as::<_, (String, i64)>(
            "SELECT status, cursor FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read initial projection state");
        assert_eq!(state, ("pending".to_owned(), 0));
        let trigger_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'search_%'",
        )
        .fetch_one(&pool)
        .await
        .expect("count search triggers");
        assert!(trigger_count > 0);
    }

    fn setup_state(pool: SqlitePool) -> Arc<AppState> {
        let encryption_key =
            EncryptionKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                .expect("build search test encryption key");
        let config = AppConfig {
            bind_addr: "127.0.0.1:58090".parse::<SocketAddr>().unwrap(),
            public_base_url: Url::parse("http://127.0.0.1:58090").unwrap(),
            database_url: "sqlite::memory:".to_owned(),
            sqlite_pool_max_connections: 4,
            static_dir: None,
            task_log_dir: std::env::temp_dir().join("octo-rill-search-test-logs"),
            job_worker_concurrency: 1,
            encryption_key: encryption_key.clone(),
            github: GitHubOAuthConfig {
                client_id: "search-test-client".to_owned(),
                client_secret: "search-test-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback").unwrap(),
            },
            linuxdo: None,
            ai: None,
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: crate::briefs::DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned(),
            logging: LoggingThresholds::default(),
        };
        Arc::new(AppState {
            llm_scheduler: Arc::new(LlmScheduler::new(1)),
            translation_scheduler: Arc::new(TranslationSchedulerController::new(
                TranslationRuntimeConfig::default(),
            )),
            github_oauth: build_oauth_client(&config).unwrap(),
            webauthn: build_webauthn(&config).unwrap(),
            config,
            pool,
            sqlite_writer: SqliteWriteCoordinator::new(),
            api_key_last_used_touches: ApiKeyLastUsedTouchQueue::new(),
            http: reqwest::Client::new(),
            github_rest_http: reqwest::Client::new(),
            github_rest_api_base: Url::parse("https://api.github.com/").unwrap(),
            github_graphql_url: Url::parse("https://api.github.com/graphql").unwrap(),
            linuxdo_oauth: None,
            encryption_key,
            runtime_owner_id: "search-test-runtime".to_owned(),
        })
    }

    async fn drain_search_projection_outbox(state: &AppState) {
        for _ in 0..64 {
            let processed = crate::search_index::run_projection_for_test(state)
                .await
                .expect("run search projection outbox item");
            let pending =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_projection_outbox")
                    .fetch_one(&state.pool)
                    .await
                    .expect("count search projection outbox items");
            if pending == 0 {
                return;
            }
            if !processed {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
        panic!("search projection outbox did not drain within test budget");
    }

    async fn seed_repo_association(pool: &SqlitePool) {
        sqlx::query(
            r#"
            INSERT INTO user_repo_associations (
              id, user_id, repo_id, repo_full_name, repo_full_name_lower,
              owner_login, repo_name, html_url, description, is_private,
              first_source, first_associated_at, last_seen_at, is_following,
              follow_state_source, has_personal_owned_source, has_github_star_source,
              has_manual_feed_source, created_at, updated_at
            )
            VALUES ('search-association', 'search-user', 42, 'octo/rill', 'octo/rill',
                    'octo', 'rill', 'https://github.com/octo/rill', 'local search fixture', 0,
                    'github_star', '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z', 1,
                    'system_default', 0, 1, 0, '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(pool)
        .await
        .expect("seed search association");
    }

    async fn seed_release(pool: &SqlitePool) {
        sqlx::query(
            r#"
            INSERT INTO repo_releases (
              id, repo_id, release_id, node_id, tag_name, name, body, html_url,
              published_at, created_at, is_prerelease, is_draft, updated_at,
              react_plus1, react_laugh, react_heart, react_hooray, react_rocket, react_eyes
            )
            VALUES ('search-release', 42, 4201, 'node-search', 'v1.0.0',
                    'Original title', 'legacy body', 'https://github.com/octo/rill/releases/tag/v1.0.0',
                    '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z', 0, 0,
                    '2026-02-22T00:00:00Z', 0, 0, 0, 0, 0, 0)
            "#,
        )
        .execute(pool)
        .await
        .expect("seed search release");
    }

    #[tokio::test]
    async fn migration_and_projection() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-late-release-translation', 'search-user', 'release_detail',
                      '4202', 'zh-CN', 'hash-late-release', 'ready',
                      'late source translation', 'late source summary',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed translation before canonical source");
        sqlx::query(
            r#"
            INSERT INTO repo_releases (
              id, repo_id, release_id, node_id, tag_name, name, body, html_url,
              published_at, created_at, is_prerelease, is_draft, updated_at,
              react_plus1, react_laugh, react_heart, react_hooray, react_rocket, react_eyes
            ) VALUES ('search-release-late', 42, 4202, 'node-search-late', 'v2.0.0',
                      'Late source title', 'late source body',
                      'https://github.com/octo/rill/releases/tag/v2.0.0',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z', 0, 0,
                      '2026-02-22T00:00:00Z', 0, 0, 0, 0, 0, 0)
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed canonical source after translation");
        let late_source = query(
            &setup_state(pool.clone()),
            "search-user",
            &parse_query("late source translation").unwrap(),
        )
        .await
        .expect("query translation restored on late source insert");
        assert_eq!(late_source.len(), 1);
        assert_eq!(late_source[0].id, "release:4202");
        assert_eq!(late_source[0].matched_lane, "translated");
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, created_at, updated_at)
            VALUES ('search-user-2', 39999998, 'search-user-2', '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed second search user");
        sqlx::query(
            r#"
            INSERT INTO user_repo_associations (
              id, user_id, repo_id, repo_full_name, repo_full_name_lower,
              owner_login, repo_name, html_url, description, is_private,
              first_source, first_associated_at, last_seen_at, is_following,
              follow_state_source, has_personal_owned_source, has_github_star_source,
              has_manual_feed_source, created_at, updated_at
            ) VALUES ('search-association-2', 'search-user-2', 42, 'octo/rill', 'octo/rill',
                      'octo', 'rill', 'https://github.com/octo/rill', 'second user', 0,
                      'github_star', '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z', 1,
                      'system_default', 0, 1, 0, '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed second user association");

        sqlx::query(
            r#"
            INSERT INTO repo_release_work_items (
              id, repo_id, repo_full_name, status, request_origin, priority,
              has_new_repo_watchers, deadline_at, last_release_count,
              last_candidate_failures, last_success_at, error_text,
              created_at, started_at, finished_at, updated_at
            ) VALUES ('search-work', 42, 'octo/rill', 'succeeded', 'test', 0,
                      0, '2026-02-23T00:00:00Z', 1, 0, NULL, NULL,
                      '2026-02-23T00:00:00Z', NULL, NULL, '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed release work item");
        let release_projection = sqlx::query_as::<_, (Option<String>,)>(
            "SELECT target_path FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release target");
        assert_eq!(
            release_projection.0.as_deref(),
            Some("/octo/rill/releases/tag/v1.0.0")
        );

        sqlx::query(
            r#"
            INSERT INTO social_activity_events (
              id, user_id, kind, repo_id, repo_full_name, discussion_number,
              title, body, html_url, actor_github_user_id, actor_login,
              occurred_at, detected_at, created_at, updated_at
            ) VALUES ('search-announcement', 'search-user', 'announcement', 42,
                      'octo/rill', 7, 'Original announcement', 'announcement body',
                      'https://github.com/octo/rill/discussions/7', 99, 'octocat',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed search announcement");

        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, error_text, active_work_item_id, created_at, updated_at
            )
            VALUES ('search-translation', 'search-user', 'release_detail', '4201', 'zh-CN',
                    'hash-search', 'ready', 'translated needle', 'translated summary', NULL, NULL,
                    '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed search translation");
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash,
              title, summary, created_at, updated_at
            ) VALUES ('search-announcement-translation', 'search-user',
                      'announcement_detail', 'octo/rill#7', 'zh-CN', 'hash-announcement',
                      '公告翻译命中', '公告翻译正文', '2026-02-22T00:00:00Z',
                      '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed announcement translation");

        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-notification-translation', 'search-user', 'notification',
                      'search-thread', 'zh-CN', 'hash-notification', 'ready',
                      '通知翻译命中', '通知翻译正文', '2026-02-22T00:00:00Z',
                      '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed notification translation");
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-release-translation-user-2', 'search-user-2', 'release_detail',
                      '4201', 'zh-CN', 'hash-user-2', 'ready',
                      '仅第二用户可见', '第二用户 lane', '2026-02-22T00:00:00Z',
                      '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed second user translation");
        sqlx::query(
            "INSERT INTO notifications (id, user_id, thread_id, subject_title, reason, updated_at, unread) VALUES ('search-notification', 'search-user', 'search-thread', 'notification needle', 'mention', '2026-02-22T00:00:00Z', 1)",
        )
		.execute(&pool)
        .await
        .expect("seed search notification");
        sqlx::query(
            "INSERT INTO notifications (id, user_id, thread_id, subject_title, reason, updated_at, unread) VALUES ('search-notification-user-2', 'search-user-2', 'search-thread', 'user-two-notification', 'mention', '2026-02-22T00:00:00Z', 1)",
        )
        .execute(&pool)
        .await
        .expect("seed same-thread notification for second user");

        let state = setup_state(pool.clone());
        drain_search_projection_outbox(state.as_ref()).await;
        let parsed = parse_query("needle").expect("parse trigram query");
        let results = query(&state, "search-user", &parsed)
            .await
            .expect("query translated release");
        assert_eq!(results.len(), 2);
        let release = results
            .iter()
            .find(|item| item.id == "release:4201")
            .expect("translated release result");
        assert_eq!(release.matched_lane, "translated");
        assert_eq!(release.matched_lanes, vec!["translated"]);

        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-release-translation-older', 'search-user', 'release_detail_old',
                      '4201', 'zh-CN', 'hash-older', 'ready',
                      'older translated needle', 'older translation', '2026-02-21T00:00:00Z',
                      '2026-02-21T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed older release translation");
        assert!(
            query(
                &state,
                "search-user",
                &parse_query("older translated").unwrap()
            )
            .await
            .expect("query latest release translation")
            .is_empty()
        );
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-release-smart', 'search-user', 'release_smart',
                      '4201', 'zh-CN', 'hash-smart', 'ready',
                      'polished translated needle', 'polished translation', '2026-02-23T00:00:00Z',
                      '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed release smart translation");
        let both_lanes = query(&state, "search-user", &parse_query("polished").unwrap())
            .await
            .expect("query smart release translation");
        assert_eq!(both_lanes.len(), 1);
        assert_eq!(both_lanes[0].matched_lane, "smart");
        sqlx::query("DELETE FROM ai_translations WHERE id = 'search-release-smart'")
            .execute(&pool)
            .await
            .expect("delete release smart translation");
        assert_eq!(
            query(&state, "search-user", &parse_query("translated").unwrap())
                .await
                .expect("query remaining translated lane")
                .len(),
            1
        );

        let announcement = query(&state, "search-user", &parse_query("公告翻译").unwrap())
            .await
            .expect("query translated announcement");
        assert_eq!(announcement.len(), 1);
        assert_eq!(announcement[0].id, "announcement:search-announcement");
        assert_eq!(announcement[0].matched_lane, "translated");
        assert_eq!(
            announcement[0].target.as_ref().unwrap().href,
            "/octo/rill/discussions/7"
        );

        let notification = query(&state, "search-user", &parse_query("通知翻译").unwrap())
            .await
            .expect("query translated notification");
        assert_eq!(notification.len(), 1);
        assert_eq!(notification[0].id, "notification:search-user:search-thread");
        assert_eq!(notification[0].matched_lane, "translated");

        let cross_lane = query(
            &state,
            "search-user",
            &parse_query("legacy translated").unwrap(),
        )
        .await
        .expect("query terms across lanes");
        assert_eq!(cross_lane.len(), 1);
        assert_eq!(cross_lane[0].matched_lanes, vec!["original", "translated"]);

        let user_two = query(&state, "search-user-2", &parse_query("仅第二用户").unwrap())
            .await
            .expect("query second user lane");
        assert_eq!(user_two.len(), 1);
        assert_eq!(user_two[0].matched_lane, "translated");
        let user_two_notification = query(
            &state,
            "search-user-2",
            &parse_query("user-two-notification").unwrap(),
        )
        .await
        .expect("query second user's notification");
        assert_eq!(user_two_notification.len(), 1);
        assert_eq!(
            user_two_notification[0].id,
            "notification:search-user-2:search-thread"
        );
        let user_one_cannot_see_user_two =
            query(&state, "search-user", &parse_query("仅第二用户").unwrap())
                .await
                .expect("isolate second user lane");
        assert!(user_one_cannot_see_user_two.is_empty());

        sqlx::query(
            "INSERT INTO briefs (id, user_id, date, content_markdown, created_at, updated_at) VALUES ('search-brief', 'search-user', '2026-02-22', 'brief needle', '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("seed search brief");
        let broad = query(&state, "search-user", &parse_query("needle").unwrap())
            .await
            .expect("query all projected types");
        assert_eq!(broad.len(), 3);
        assert!(broad.iter().any(|item| item.result_type == "release"));
        assert!(broad.iter().any(|item| item.result_type == "brief"));
        assert!(broad.iter().any(|item| item.result_type == "notification"));

        assert!(
            query(&state, "search-user", &parse_query("%").unwrap())
                .await
                .expect("escape percent wildcard")
                .is_empty()
        );
        assert!(
            query(&state, "search-user", &parse_query("_").unwrap())
                .await
                .expect("escape underscore wildcard")
                .is_empty()
        );

        sqlx::query(
            r#"
            INSERT INTO starred_repos (
              id, user_id, repo_id, full_name, owner_login, name, description,
              html_url, stargazed_at, is_private, updated_at
            ) VALUES ('search-starred-only', 'search-user', 77, 'starred/only',
                      'starred', 'only', 'starred-only repository',
                      'https://github.com/starred/only', '2026-02-22T00:00:00Z', 0,
                      '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed starred-only repository");
        let starred_only = query(
            &state,
            "search-user",
            &parse_query("type:repository starred-only").unwrap(),
        )
        .await
        .expect("query starred-only repository");
        assert_eq!(starred_only.len(), 1);
        assert_eq!(starred_only[0].id, "repository:search-user:starred/only");
        sqlx::query(
			"UPDATE starred_repos SET full_name = 'starred/renamed', name = 'renamed' WHERE id = 'search-starred-only'",
		)
		.execute(&pool)
		.await
		.expect("rename starred-only repository");
        assert!(
            query(&state, "search-user", &parse_query("starred/only").unwrap())
                .await
                .expect("query stale starred name")
                .is_empty()
        );
        assert_eq!(
            query(
                &state,
                "search-user",
                &parse_query("starred/renamed").unwrap()
            )
            .await
            .expect("query renamed starred repository")
            .len(),
            1
        );

        sqlx::raw_sql(include_str!(
            "../migrations/legacy/0081_command_palette_search.sql"
        ))
        .execute(&pool)
        .await
        .expect("rerun search migration idempotently");
        let document_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_documents WHERE user_id = 'search-user' OR user_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("count search documents after rerun");
        assert_eq!(document_count, 7);

        sqlx::query("UPDATE repo_releases SET body = 'updated body' WHERE release_id = 4201")
            .execute(&pool)
            .await
            .expect("update search release");
        let updated = query(&state, "search-user", &parse_query("legacy").unwrap())
            .await
            .expect("query updated projection");
        assert!(updated.is_empty());
        sqlx::query("DELETE FROM repo_releases WHERE release_id = 4201")
            .execute(&pool)
            .await
            .expect("delete search release");
        let after_delete = query(&state, "search-user", &parse_query("needle").unwrap())
            .await
            .expect("query after projection delete");
        assert_eq!(after_delete.len(), 2);
    }

    #[tokio::test]
    async fn projection_backfill_failed_state_is_terminal() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        sqlx::query("DELETE FROM search_documents WHERE id = 'release:4201'")
            .execute(&pool)
            .await
            .expect("clear release projection before failed-state check");
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'releases', cursor = 0, status = 'failed', last_error = 'previous failure' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("seed failed backfill state");
        let state = setup_state(pool.clone());

        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("failed backfill state should be terminal");

        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE id = 'release:4201'",
            )
            .fetch_one(&pool)
            .await
            .expect("count release projection after failed-state check"),
            0
        );
        let backfill_state = sqlx::query_as::<_, (String, i64, String, Option<String>)>(
            "SELECT phase, cursor, status, last_error FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read failed backfill state");
        assert_eq!(
            backfill_state,
            (
                "releases".to_owned(),
                0,
                "failed".to_owned(),
                Some("previous failure".to_owned())
            )
        );

        assert!(
            crate::search_index::resume_failed_for_test(state.as_ref())
                .await
                .expect("resume failed search projection")
        );
        let resumed_state = sqlx::query_as::<_, (String, i64, String, Option<String>)>(
            "SELECT phase, cursor, status, last_error FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read resumed backfill state");
        assert_eq!(
            resumed_state,
            ("releases".to_owned(), 0, "building".to_owned(), None)
        );
        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("run resumed search projection batch");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE id = 'release:4201'",
            )
            .fetch_one(&pool)
            .await
            .expect("count release projection after explicit resume"),
            1
        );
        assert!(
            sqlx::query_scalar::<_, i64>(
                "SELECT cursor FROM search_projection_backfill_state WHERE id = 1",
            )
            .fetch_one(&pool)
            .await
            .expect("read cursor after explicit resume")
                > 0
        );
    }

    #[tokio::test]
    async fn projection_backfill_failure_rolls_back_before_marking_failed() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        sqlx::query("DELETE FROM search_documents WHERE id = 'release:4201'")
            .execute(&pool)
            .await
            .expect("clear release projection before failure injection");
        sqlx::query(
            "CREATE TRIGGER search_test_fail_projection BEFORE INSERT ON search_documents WHEN NEW.id = 'release:4201' BEGIN SELECT RAISE(ABORT, 'forced search projection failure'); END",
        )
        .execute(&pool)
        .await
        .expect("create search projection failure trigger");
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'releases', cursor = 0, status = 'building', last_error = NULL WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("seed building backfill state");
        let state = setup_state(pool.clone());

        let error = crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect_err("forced search projection failure");
        assert!(
            error
                .to_string()
                .contains("forced search projection failure")
        );

        let backfill_state = sqlx::query_as::<_, (String, i64, String, Option<String>)>(
            "SELECT phase, cursor, status, last_error FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read terminal failure state");
        assert_eq!(backfill_state.0, "releases");
        assert_eq!(backfill_state.1, 0);
        assert_eq!(backfill_state.2, "failed");
        assert!(
            backfill_state
                .3
                .as_deref()
                .is_some_and(|value| value.contains("forced search projection failure"))
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE id = 'release:4201'",
            )
            .fetch_one(&pool)
            .await
            .expect("count rolled-back release projection"),
            0
        );

        let (_permit, mut tx) = state
            .sqlite_writer
            .begin_immediate_with_priority(
                &state.pool,
                "search_projection_foreground_regression",
                crate::sqlite_write::SqliteWritePriority::Foreground,
            )
            .await
            .expect("acquire foreground writer after failed backfill");
        sqlx::query("UPDATE users SET updated_at = updated_at WHERE id = 'search-user'")
            .execute(&mut *tx)
            .await
            .expect("foreground write after failed backfill");
        tx.commit()
            .await
            .expect("commit foreground write after failed backfill");
    }

    #[tokio::test]
    async fn projection_backfill_resumes_in_bounded_batches() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO social_activity_events (
              id, user_id, kind, repo_id, repo_full_name, title, body, html_url,
              actor_github_user_id, actor_login, occurred_at, detected_at,
              created_at, updated_at
            ) VALUES ('search-announcement-backfill', 'search-user', 'announcement', 42,
                      'octo/rill', 'Backfill announcement', 'announcement body',
                      'https://github.com/octo/rill/discussions/7', 99, 'octocat',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed announcement for backfill");
        let state = setup_state(pool.clone());

        for _ in 0..=crate::search_index::PHASE_COUNT * 2 + 2 {
            crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
                .await
                .expect("run search projection batch");
        }

        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read backfill status");
        assert_eq!(status, "ready");
        let results = query(&state, "search-user", &parse_query("legacy").unwrap())
            .await
            .expect("query backfilled release");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "release:4201");
        let announcements = query(
            &state,
            "search-user",
            &parse_query("Backfill announcement").unwrap(),
        )
        .await
        .expect("query backfilled announcement");
        assert_eq!(announcements.len(), 1);
        assert_eq!(
            announcements[0].id,
            "announcement:search-announcement-backfill"
        );
    }

    #[tokio::test]
    async fn projection_backfill_releases_stays_within_batch_boundary() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        for index in 0..26_i64 {
            let release_id = 4201 + index;
            sqlx::query(
                r#"
                INSERT INTO repo_releases (
                  id, repo_id, release_id, node_id, tag_name, name, body, html_url,
                  published_at, created_at, is_prerelease, is_draft, updated_at,
                  react_plus1, react_laugh, react_heart, react_hooray, react_rocket, react_eyes
                ) VALUES (?, 42, ?, ?, ?, ?, ?, ?, '2026-02-22T00:00:00Z',
                          '2026-02-22T00:00:00Z', 0, 0, '2026-02-22T00:00:00Z',
                          0, 0, 0, 0, 0, 0)
                "#,
            )
            .bind(format!("search-release-{release_id}"))
            .bind(release_id)
            .bind(format!("node-search-{release_id}"))
            .bind(format!("v{index}.0.0"))
            .bind(format!("Release {index}"))
            .bind(format!("release body {index}"))
            .bind(format!(
                "https://github.com/octo/rill/releases/tag/v{index}.0.0"
            ))
            .execute(&pool)
            .await
            .expect("seed bounded release batch");
        }
        sqlx::query("DELETE FROM search_documents WHERE resource_type = 'release'")
            .execute(&pool)
            .await
            .expect("clear release projection before bounded backfill");
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'releases', cursor = 0, status = 'building' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("seed release backfill cursor");
        let state = setup_state(pool.clone());

        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("run first bounded release batch");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE resource_type = 'release'",
            )
            .fetch_one(&pool)
            .await
            .expect("count first bounded release batch"),
            25
        );
        let first_cursor = sqlx::query_scalar::<_, i64>(
            "SELECT cursor FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read first release batch cursor");
        assert_eq!(first_cursor, 25);

        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("run second bounded release batch");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE resource_type = 'release'",
            )
            .fetch_one(&pool)
            .await
            .expect("count second bounded release batch"),
            26
        );
        let second_cursor = sqlx::query_scalar::<_, i64>(
            "SELECT cursor FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read second release batch cursor");
        assert_eq!(second_cursor, 26);
    }

    #[tokio::test]
    async fn search_projection_lookup_plans_use_backfill_indexes() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        let visible_plan = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT repo_id, full_name, owner_login FROM user_release_visible_repos WHERE repo_id = 42",
        )
        .fetch_all(&pool)
        .await
        .expect("explain visible repository lookup");
        assert!(visible_plan.iter().any(|row| {
            row.get::<String, _>("detail")
                .contains("idx_user_repo_associations_repo_id")
        }));

        let release_plan = sqlx::query(
            r#"
            EXPLAIN QUERY PLAN
            WITH batch_releases AS (
              SELECT release_id, repo_id
              FROM repo_releases
              WHERE rowid IN (1)
            ), visible_repo_candidates AS (
              SELECT ura.repo_id, ura.repo_full_name AS full_name, ura.owner_login,
                     1 AS source_priority, ura.user_id
              FROM user_repo_associations ura
              JOIN batch_releases br ON br.repo_id = ura.repo_id
              JOIN users u ON u.id = ura.user_id
              WHERE ura.is_following != 0
                AND ura.repo_id IS NOT NULL
                AND (
                  ura.has_personal_owned_source = 0
                  OR ura.has_github_star_source != 0
                  OR u.include_own_releases != 0
                )
            ), visible_repo_metadata AS (
              SELECT repo_id, full_name, owner_login
              FROM (
                SELECT repo_id, full_name, owner_login,
                       ROW_NUMBER() OVER (
                         PARTITION BY repo_id
                         ORDER BY source_priority, user_id
                       ) AS metadata_rank
                FROM visible_repo_candidates
              )
              WHERE metadata_rank = 1
            )
            SELECT br.release_id, vr.full_name, vr.owner_login
            FROM batch_releases br
            LEFT JOIN visible_repo_metadata vr ON vr.repo_id = br.repo_id
            "#,
        )
        .fetch_all(&pool)
        .await
        .expect("explain release projection lookup");
        let release_plan_details = release_plan
            .iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect::<Vec<_>>();
        assert!(
            release_plan.iter().any(|row| {
                row.get::<String, _>("detail")
                    .contains("idx_user_repo_associations_repo_id")
            }),
            "release plan: {release_plan_details:?}"
        );

        let resource_plan = sqlx::query(
            "EXPLAIN QUERY PLAN UPDATE search_documents SET translated_text = NULL WHERE resource_type = 'release' AND resource_id = '4201'",
        )
        .fetch_all(&pool)
        .await
        .expect("explain resource projection lookup");
        assert!(
            resource_plan.iter().any(|row| {
                row.get::<String, _>("detail")
                    .contains("idx_search_documents_resource_key")
            }),
            "resource plan: {:?}",
            resource_plan
                .iter()
                .map(|row| row.get::<String, _>("detail"))
                .collect::<Vec<_>>()
        );

        let outbox_poll_plan = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT projection_key FROM search_projection_outbox WHERE available_at <= CURRENT_TIMESTAMP AND (lease_expires_at IS NULL OR lease_expires_at <= CURRENT_TIMESTAMP) ORDER BY available_at ASC, created_at ASC, projection_key ASC LIMIT 1",
        )
        .fetch_all(&pool)
        .await
        .expect("explain projection outbox poll");
        let outbox_poll_plan_details = outbox_poll_plan
            .iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect::<Vec<_>>();
        assert!(
            outbox_poll_plan_details
                .iter()
                .any(|detail| detail.contains("idx_search_projection_outbox_poll")),
            "outbox poll plan: {outbox_poll_plan_details:?}"
        );
        assert!(
            outbox_poll_plan_details
                .iter()
                .all(|detail| !detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "outbox poll should not sort a backlog in a temp B-tree: {outbox_poll_plan_details:?}"
        );

        let failed_outbox_poll_plan = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT projection_key FROM search_projection_outbox_failed WHERE failed_at <= datetime('now', '-30 seconds') ORDER BY failed_at ASC, projection_key ASC LIMIT 1",
        )
        .fetch_all(&pool)
        .await
        .expect("explain failed projection outbox poll");
        let failed_outbox_poll_plan_details = failed_outbox_poll_plan
            .iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect::<Vec<_>>();
        assert!(
            failed_outbox_poll_plan_details
                .iter()
                .any(|detail| detail.contains("idx_search_projection_outbox_failed_poll")),
            "failed outbox poll plan: {failed_outbox_poll_plan_details:?}"
        );
        assert!(
            failed_outbox_poll_plan_details
                .iter()
                .all(|detail| !detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "failed outbox poll should not sort a backlog in a temp B-tree: {failed_outbox_poll_plan_details:?}"
        );

        let translation_plan = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT user_id FROM ai_translations WHERE lang = 'zh-CN' AND entity_id = '4201' AND status IN ('ready', 'disabled', 'missing') AND lower(entity_type) LIKE 'release%'",
        )
        .fetch_all(&pool)
        .await
        .expect("explain translation lookup");
        assert!(translation_plan.iter().any(|row| {
            row.get::<String, _>("detail")
                .contains("idx_ai_translations_search_entity")
        }));
    }

    #[tokio::test]
    async fn projection_backfill_recovers_persisted_cursor_after_restart() {
        let pool = setup_pool().await;
        for index in 0..205 {
            sqlx::query(
                "INSERT INTO search_documents (id,resource_type,resource_id,title,body,created_at,updated_at) VALUES (?,'release',?,'restart title','restart body','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z')",
            )
            .bind(format!("release:restart-{index}"))
            .bind(format!("restart-{index}"))
            .execute(&pool)
            .await
            .expect("seed restart projection");
        }
        sqlx::query("DELETE FROM search_documents_fts_v2")
            .execute(&pool)
            .await
            .expect("clear FTS cache before restart recovery");
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'fts_documents', cursor = 0, status = 'building' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("seed persisted FTS cursor state");

        let first_state = setup_state(pool.clone());
        crate::search_index::run_batch_for_test(first_state.as_ref(), u64::MAX)
            .await
            .expect("run first FTS recovery batch");
        drop(first_state);
        let first_cursor = sqlx::query_scalar::<_, i64>(
            "SELECT cursor FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read persisted FTS cursor");
        assert_eq!(first_cursor, 25);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents_fts_v2")
                .fetch_one(&pool)
                .await
                .expect("count first recovered FTS batch"),
            25
        );

        let restarted_state = setup_state(pool.clone());
        crate::search_index::run_batch_for_test(restarted_state.as_ref(), u64::MAX)
            .await
            .expect("resume FTS recovery after restart");
        let second_cursor = sqlx::query_scalar::<_, i64>(
            "SELECT cursor FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read resumed FTS cursor");
        assert_eq!(second_cursor, 50);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents_fts_v2")
                .fetch_one(&pool)
                .await
                .expect("count resumed FTS batches"),
            50
        );
    }

    #[tokio::test]
    async fn projection_backfill_pauses_below_disk_watermark() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        let state = setup_state(pool.clone());
        let before = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents")
            .fetch_one(&pool)
            .await
            .expect("count existing search documents");
        let fts_before =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents_fts_v2")
                .fetch_one(&pool)
                .await
                .expect("count existing global FTS rows");
        crate::search_index::run_batch_for_test(state.as_ref(), 0)
            .await
            .expect("record low disk status");
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read paused backfill status");
        assert_eq!(status, "paused_low_disk");
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents")
            .fetch_one(&pool)
            .await
            .expect("count search documents");
        assert_eq!(count, before);
        let fts_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents_fts_v2")
                .fetch_one(&pool)
                .await
                .expect("count global FTS rows after pause");
        assert_eq!(fts_count, fts_before);
        let queued =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_metadata_backfill_queue")
                .fetch_one(&pool)
                .await
                .expect("count metadata queue after pause");
        assert!(queued > 0);

        sqlx::query(
            "UPDATE search_projection_backfill_state SET updated_at = '2000-01-01T00:00:00Z' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("set stable low-disk status timestamp");
        crate::search_index::run_batch_for_test(state.as_ref(), 0)
            .await
            .expect("keep backfill paused while disk remains low");
        let paused_state = sqlx::query_as::<_, (String, String)>(
            "SELECT status, updated_at FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read repeated low-disk state");
        assert_eq!(
            paused_state,
            (
                "paused_low_disk".to_owned(),
                "2000-01-01T00:00:00Z".to_owned()
            )
        );
    }

    #[tokio::test]
    async fn search_reports_index_status() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        assert_eq!(index_status(state.as_ref()).await.unwrap(), "building");
        sqlx::query(
            "UPDATE search_projection_backfill_state SET status = 'paused_low_disk' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("pause search index");
        assert_eq!(
            index_status(state.as_ref()).await.unwrap(),
            "paused_low_disk"
        );
        sqlx::query(
            "UPDATE search_projection_backfill_state SET status = 'failed', last_error = 'projection failed' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("fail search index");
        assert_eq!(index_status(state.as_ref()).await.unwrap(), "failed");
        sqlx::query("UPDATE search_projection_backfill_state SET status = 'ready' WHERE id = 1")
            .execute(&pool)
            .await
            .expect("ready search index");
        assert_eq!(index_status(state.as_ref()).await.unwrap(), "ready");
        sqlx::query(
            "INSERT INTO search_projection_outbox (projection_key, operation, resource_type, resource_id, available_at, created_at, updated_at) VALUES ('content:pending-search-status', 'upsert', 'content', 'pending-search-status', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed pending realtime projection");
        assert_eq!(index_status(state.as_ref()).await.unwrap(), "building");
        sqlx::query(
            "DELETE FROM search_projection_outbox WHERE projection_key = 'content:pending-search-status'",
        )
        .execute(&pool)
        .await
        .expect("clear pending realtime projection");
        assert_eq!(index_status(state.as_ref()).await.unwrap(), "ready");
    }

    #[tokio::test]
    async fn realtime_projection_backlog_does_not_enable_unindexed_fallback() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        let state = setup_state(pool.clone());

        sqlx::query(
            "UPDATE search_projection_backfill_state SET status = 'ready', phase = 'metadata', cursor = 0 WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("mark search projection ready");
        sqlx::query("DELETE FROM search_metadata_backfill_queue")
            .execute(&pool)
            .await
            .expect("clear metadata backlog");
        sqlx::query(
            "UPDATE search_documents SET title = 'source ftsneedle', body = 'source body' WHERE id = 'release:4201'",
        )
        .execute(&pool)
        .await
        .expect("seed source projection text");
        sqlx::query(
            "UPDATE search_documents_fts_v2 SET title = '', body = '', repo_full_name = '', translated_text = '', smart_text = '' WHERE doc_id = 'release:4201'",
        )
        .execute(&pool)
        .await
        .expect("seed indexed-only search term");
        sqlx::query(
            "INSERT INTO search_projection_outbox (projection_key, operation, resource_type, resource_id, available_at, created_at, updated_at) VALUES ('notification:search-user:pending', 'upsert', 'notification', 'pending', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed active projection backlog");
        sqlx::query(
            "INSERT INTO search_projection_outbox_failed (projection_key, operation, resource_type, resource_id, attempt_count, last_error, created_at, updated_at, failed_at) VALUES ('notification:search-user:failed', 'upsert', 'notification', 'failed', 8, 'terminal failure', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed terminal projection failure");

        let results = query(&state, "search-user", &parse_query("ftsneedle").unwrap())
            .await
            .expect("query while projection backlog exists");
        assert!(
            results.is_empty(),
            "a projection backlog must not globally enable the unindexed LIKE fallback"
        );
    }

    #[tokio::test]
    async fn realtime_projection_outbox_coalesces_and_converges() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        sqlx::query(
            "INSERT INTO users (id, github_user_id, login, created_at, updated_at) VALUES ('search-user-2', 39999998, 'search-user-2', '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("seed notification migration user");
        sqlx::query(
            "INSERT INTO notifications (id, user_id, thread_id, subject_title, reason, updated_at, unread) VALUES ('outbox-notification', 'search-user', 'outbox-thread', 'first title', 'mention', '2026-02-22T00:00:00Z', 1)",
        )
        .execute(&pool)
        .await
        .expect("seed outbox notification");
        sqlx::query(
            "UPDATE notifications SET subject_title = 'latest title', updated_at = '2026-02-22T00:00:01Z' WHERE id = 'outbox-notification'",
        )
        .execute(&pool)
        .await
        .expect("update outbox notification");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'notification:search-user:outbox-thread'",
            )
            .fetch_one(&pool)
            .await
            .expect("count coalesced outbox item"),
            1
        );

        drain_search_projection_outbox(state.as_ref()).await;
        sqlx::query(
            "UPDATE notifications SET subject_title = 'latest title after worker', updated_at = '2026-02-22T00:00:02Z' WHERE id = 'outbox-notification'",
        )
        .execute(&pool)
        .await
        .expect("update projected notification");
        drain_search_projection_outbox(state.as_ref()).await;
        let projected_title: String = sqlx::query_scalar(
            "SELECT title FROM search_documents WHERE id = 'notification:search-user:outbox-thread'",
        )
        .fetch_one(&pool)
        .await
        .expect("read projected notification");
        assert_eq!(projected_title, "latest title after worker");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents_fts WHERE doc_id = 'notification:search-user:outbox-thread'",
            )
            .fetch_one(&pool)
            .await
            .expect("count projected FTS row"),
            1
        );

        sqlx::query(
            "UPDATE notifications SET user_id = 'search-user-2', thread_id = 'outbox-thread-migrated' WHERE id = 'outbox-notification'",
        )
        .execute(&pool)
        .await
        .expect("move projected notification identity");
        drain_search_projection_outbox(state.as_ref()).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE id = 'notification:search-user:outbox-thread'",
            )
            .fetch_one(&pool)
            .await
            .expect("count old notification projection after identity move"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE id = 'notification:search-user-2:outbox-thread-migrated'",
            )
            .fetch_one(&pool)
            .await
            .expect("count new notification projection after identity move"),
            1
        );

        sqlx::query("DELETE FROM notifications WHERE id = 'outbox-notification'")
            .execute(&pool)
            .await
            .expect("delete outbox notification");
        drain_search_projection_outbox(state.as_ref()).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE id = 'notification:search-user-2:outbox-thread-migrated'",
            )
            .fetch_one(&pool)
            .await
            .expect("count deleted search document"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents_fts WHERE doc_id = 'notification:search-user-2:outbox-thread-migrated'",
            )
            .fetch_one(&pool)
            .await
            .expect("count deleted FTS row"),
            0
        );
    }

    #[tokio::test]
    async fn realtime_projection_outbox_claim_has_single_effective_owner() {
        let pool = setup_pool().await;
        sqlx::query(
            "INSERT INTO notifications (id, user_id, thread_id, subject_title, reason, updated_at, unread) VALUES ('outbox-race-notification', 'search-user', 'outbox-race-thread', 'race title', 'mention', CURRENT_TIMESTAMP, 1)",
        )
        .execute(&pool)
        .await
        .expect("seed racing notification");
        let state = setup_state(pool.clone());
        let (left, right) = tokio::join!(
            crate::search_index::run_projection_for_test(state.as_ref()),
            crate::search_index::run_projection_for_test(state.as_ref())
        );
        let committed = [
            left.expect("first projection runner"),
            right.expect("second projection runner"),
        ]
        .into_iter()
        .filter(|processed| *processed)
        .count();
        assert_eq!(
            committed, 1,
            "exactly one runner should apply the leased item"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'notification:search-user:outbox-race-thread'",
            )
            .fetch_one(&pool)
            .await
            .expect("count drained race item"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents_fts WHERE doc_id = 'notification:search-user:outbox-race-thread'",
            )
            .fetch_one(&pool)
            .await
            .expect("count race FTS rows"),
            1
        );
    }

    #[tokio::test]
    async fn realtime_projection_pressure_preserves_foreground_writer() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        sqlx::query(
            "INSERT INTO notifications (id, user_id, thread_id, subject_title, reason, updated_at, unread) VALUES ('projection-pressure-notification', 'search-user', 'projection-pressure-thread', 'pressure title', 'mention', CURRENT_TIMESTAMP, 1)",
        )
        .execute(&pool)
        .await
        .expect("seed projection pressure notification");

        let external_lock = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .expect("hold external sqlite writer lock");
        let projection_state = state.clone();
        let projection_task = tokio::spawn(async move {
            crate::search_index::run_projection_for_test(projection_state.as_ref()).await
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let status = state.sqlite_writer.runtime_status();
                if status.active {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("projection worker should enter the shared writer lane");

        let foreground_pool = pool.clone();
        let foreground_state = state.clone();
        let foreground_task = tokio::spawn(async move {
            foreground_state
                .sqlite_writer
                .write_foreground("projection_pressure_foreground", |_| {
                    let foreground_pool = foreground_pool.clone();
                    async move {
                        sqlx::query(
                            "UPDATE users SET updated_at = CURRENT_TIMESTAMP WHERE id = 'search-user'",
                        )
                        .execute(&foreground_pool)
                        .await?;
                        Ok::<_, anyhow::Error>(())
                    }
                })
                .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if state.sqlite_writer.runtime_status().waiting_foreground > 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("foreground writer should wait behind the external sqlite lock");

        external_lock
            .rollback()
            .await
            .expect("release external sqlite writer lock");
        foreground_task
            .await
            .expect("join foreground projection pressure task")
            .expect("foreground writer should succeed after lock release");
        let first_projection = projection_task
            .await
            .expect("join projection pressure task");
        if let Err(error) = &first_projection {
            assert!(
                error
                    .to_string()
                    .contains("background writer admission denied")
                    || error
                        .to_string()
                        .contains("retryable sqlite write deadline exceeded"),
                "projection pressure must defer through the coordinator: {error:#}"
            );
        }
        if !matches!(first_projection, Ok(true)) {
            let reset = sqlx::query(
                "UPDATE search_projection_outbox
                 SET available_at = CURRENT_TIMESTAMP,
                     lease_owner = NULL,
                     lease_expires_at = NULL
                 WHERE projection_key = 'notification:search-user:projection-pressure-thread'",
            )
            .execute(&pool)
            .await
            .expect("make deferred projection due for retry");
            assert_eq!(reset.rows_affected(), 1);
            tokio::time::sleep(std::time::Duration::from_millis(75)).await;
            assert!(
                crate::search_index::run_projection_for_test(state.as_ref())
                    .await
                    .expect("retry projection after foreground pressure")
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents_fts WHERE doc_id = 'notification:search-user:projection-pressure-thread'",
            )
            .fetch_one(&pool)
            .await
            .expect("count committed pressure projection FTS row"),
            1
        );
    }

    #[tokio::test]
    async fn realtime_projection_outbox_takes_over_expired_lease() {
        let pool = setup_pool().await;
        sqlx::query(
            "INSERT INTO notifications (id, user_id, thread_id, subject_title, reason, updated_at, unread) VALUES ('outbox-expired-notification', 'search-user', 'outbox-expired-thread', 'expired title', 'mention', CURRENT_TIMESTAMP, 1)",
        )
        .execute(&pool)
        .await
        .expect("seed expired-lease notification");
        sqlx::query(
            "UPDATE search_projection_outbox SET lease_owner = 'stale-owner', lease_expires_at = '2000-01-01T00:00:00Z', available_at = CURRENT_TIMESTAMP WHERE projection_key = 'notification:search-user:outbox-expired-thread'",
        )
        .execute(&pool)
        .await
        .expect("expire projection lease");
        let state = setup_state(pool.clone());
        assert!(
            crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .expect("expired lease should be reclaimable")
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE id = 'notification:search-user:outbox-expired-thread'",
            )
            .fetch_one(&pool)
            .await
            .expect("count reclaimed projection"),
            1
        );
    }

    #[tokio::test]
    async fn realtime_projection_outbox_failure_is_retryable() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        sqlx::query(
            "INSERT INTO search_projection_outbox (projection_key, operation, resource_type, resource_id, available_at, created_at, updated_at) VALUES ('content:release:missing', 'upsert', 'content', 'missing', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed malformed projection item");
        assert!(
            crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .is_err()
        );
        let retry = sqlx::query_as::<_, (i64, Option<String>, Option<String>)>(
            "SELECT attempt_count, last_error, lease_owner FROM search_projection_outbox WHERE projection_key = 'content:release:missing'",
        )
        .fetch_one(&pool)
        .await
        .expect("read projection retry state");
        assert_eq!(retry.0, 1);
        assert!(retry.1.is_some());
        assert!(retry.2.is_none());
    }

    #[tokio::test]
    async fn realtime_projection_source_update_resets_coalesced_retry_budget() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        sqlx::query(
            r#"
            INSERT INTO content_work_items (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, source_hash, protocol_version, model_profile,
              source_snapshot_json, configuration_fingerprint, status,
              created_at, updated_at
            ) VALUES ('search-retry-reset-work', 'release', 'retry-reset', 'translation', 'summary',
                      'zh-CN', 'search-retry-reset-hash', 'content-processing.v1', 'test-model',
                      '{}', 'test-config', 'ready', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed retry-reset content work item");
        sqlx::query(
            r#"
            INSERT INTO content_result_projections (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, protocol_version, model_profile, source_hash, work_item_id,
              active_work_item_id, payload_json, published_at, updated_at
            ) VALUES ('search-retry-reset-projection', 'release', 'retry-reset', 'translation', 'summary',
                      'zh-CN', 'content-processing.v1', 'test-model', 'search-retry-reset-hash',
                      'search-retry-reset-work', 'search-retry-reset-work', '{"title":"initial"}',
                      CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed retry-reset content projection");
        sqlx::query(
            "UPDATE search_projection_outbox SET attempt_count = 7, fanout_cursor = 'stale-cursor', last_error = 'old writer deadline', updated_at = CURRENT_TIMESTAMP WHERE projection_key = 'content:release:retry-reset'",
        )
        .execute(&pool)
        .await
        .expect("seed coalesced retry projection state");
        sqlx::query(
            "UPDATE content_result_projections SET payload_json = '{\"title\":\"updated\"}', updated_at = CURRENT_TIMESTAMP WHERE id = 'search-retry-reset-projection'",
        )
        .execute(&pool)
        .await
        .expect("apply source update to projection");
        assert_eq!(
            sqlx::query_as::<_, (i64, Option<String>)>(
                "SELECT attempt_count, fanout_cursor FROM search_projection_outbox WHERE projection_key = 'content:release:retry-reset'",
            )
            .fetch_one(&pool)
            .await
            .expect("read reset source retry budget"),
            (0, None)
        );
        assert!(
            !crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .expect("source update with a missing base document should defer")
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT attempt_count FROM search_projection_outbox WHERE projection_key = 'content:release:retry-reset'",
            )
            .fetch_one(&pool)
            .await
            .expect("read post-update retry budget"),
            1
        );
    }

    #[tokio::test]
    async fn realtime_projection_announcement_lanes_commit_in_bounded_chunks() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        for index in 0..101 {
            let user_id = format!("fanout-user-{index:03}");
            sqlx::query(
                "INSERT INTO users (id, github_user_id, login, created_at, updated_at) VALUES (?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .bind(&user_id)
            .bind(50000000_i64 + index)
            .bind(&user_id)
            .execute(&pool)
            .await
            .expect("seed fanout user");
            sqlx::query(
                "INSERT INTO ai_translations (id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary, created_at, updated_at) VALUES (?, ?, 'announcement', 'fanout-event', 'zh-CN', ?, 'ready', ?, 'fanout summary', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .bind(format!("fanout-translation-{index:03}"))
            .bind(&user_id)
            .bind(format!("fanout-hash-{index:03}"))
            .bind(format!("fanout title {index:03}"))
            .execute(&pool)
            .await
            .expect("seed fanout translation");
        }
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, created_at, updated_at)
            VALUES ('fanout-owner', 59999999, 'fanout-owner', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed announcement owner");
        sqlx::query(
            r#"
            INSERT INTO social_activity_events (
              id, user_id, kind, repo_id, repo_full_name, title, body, html_url,
              actor_github_user_id, actor_login, occurred_at, detected_at, created_at, updated_at
            ) VALUES (
              'fanout-event', 'fanout-owner', 'announcement', 42, 'octo/rill',
              'fanout announcement', 'fanout body', 'https://example.test/fanout',
              59999999, 'fanout-owner', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP,
              CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed announcement source");
        sqlx::query(
            "INSERT INTO search_projection_outbox (projection_key, operation, resource_type, resource_id, available_at, created_at, updated_at) VALUES ('announcement:fanout-event', 'upsert', 'announcement', 'fanout-event', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) ON CONFLICT(projection_key) DO UPDATE SET available_at = CURRENT_TIMESTAMP, last_error = NULL, fanout_cursor = NULL, updated_at = CURRENT_TIMESTAMP",
        )
        .execute(&pool)
        .await
        .expect("seed announcement projection");

        assert!(
            crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .expect("run first announcement fanout chunk")
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_document_user_lanes WHERE document_id = 'announcement:fanout-event'",
            )
            .fetch_one(&pool)
            .await
            .expect("count first fanout chunk"),
            100
        );
        assert!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT fanout_cursor FROM search_projection_outbox WHERE projection_key = 'announcement:fanout-event'",
            )
            .fetch_one(&pool)
            .await
            .expect("read announcement fanout cursor")
            .is_some()
        );
        drain_search_projection_outbox(state.as_ref()).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_document_user_lanes WHERE document_id = 'announcement:fanout-event'",
            )
            .fetch_one(&pool)
            .await
            .expect("count completed fanout lanes"),
            101
        );
    }

    #[tokio::test]
    async fn realtime_projection_outbox_moves_terminal_failures_to_recovery_queue() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        sqlx::query(
            "INSERT INTO search_projection_outbox (projection_key, operation, resource_type, resource_id, canonical_resource_type, available_at, created_at, updated_at) VALUES ('content:release:terminal-missing', 'upsert', 'content', 'terminal-missing', 'release', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed terminal projection item");
        for attempt in 1..=24 {
            match crate::search_index::run_projection_for_test(state.as_ref()).await {
                Ok(processed) => assert!(
                    !processed,
                    "terminal recovery attempt {attempt} should commit deferred retry state"
                ),
                Err(error) => assert!(
                    crate::sqlite_write::is_sqlite_write_deadline_error(error.as_ref())
                        || crate::sqlite_write::is_sqlite_background_admission_error(
                            error.as_ref(),
                        ),
                    "terminal recovery attempt {attempt} should only surface retryable SQLite pressure: {error:#}"
                ),
            }
            let active_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'content:release:terminal-missing'",
            )
            .fetch_one(&pool)
            .await
            .expect("count active terminal projection during retry");
            let failed_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox_failed WHERE projection_key = 'content:release:terminal-missing'",
            )
            .fetch_one(&pool)
            .await
            .expect("count failed terminal projection during retry");
            if active_count == 0 && failed_count == 1 {
                break;
            }
            assert_eq!(
                active_count, 1,
                "terminal projection should remain retryable"
            );
            assert_eq!(
                failed_count, 0,
                "terminal projection should not recover early"
            );
            sqlx::query(
                "UPDATE search_projection_outbox SET available_at = CURRENT_TIMESTAMP WHERE projection_key = 'content:release:terminal-missing'",
            )
            .execute(&pool)
            .await
            .expect("make the next projection retry available");
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'content:release:terminal-missing'",
            )
            .fetch_one(&pool)
            .await
            .expect("count active terminal projection"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox_failed WHERE projection_key = 'content:release:terminal-missing'",
            )
            .fetch_one(&pool)
            .await
            .expect("count terminal projection failure"),
            1
        );
        assert_eq!(index_status(state.as_ref()).await.unwrap(), "failed");

        sqlx::query(
            "INSERT INTO search_documents (id, resource_type, resource_id, title, body, created_at, updated_at) VALUES ('release:terminal-missing', 'release', 'terminal-missing', 'recovered source', 'source body', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("insert recovered source document");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'content:release:terminal-missing'",
            )
            .fetch_one(&pool)
            .await
            .expect("count reactivated projection"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox_failed WHERE projection_key = 'content:release:terminal-missing'",
            )
            .fetch_one(&pool)
            .await
            .expect("count cleared terminal projection failure"),
            0
        );
    }

    #[tokio::test]
    async fn realtime_projection_terminal_failure_is_retried_after_backpressure_clears() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        sqlx::query(
            "INSERT INTO search_projection_outbox_failed (projection_key, operation, resource_type, resource_id, canonical_resource_type, attempt_count, last_error, created_at, updated_at, failed_at) VALUES ('content:release:terminal-retry', 'upsert', 'content', 'terminal-retry', 'release', 8, 'writer deadline', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2000-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("seed stale terminal projection failure");

        assert!(
            !crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .expect("reactivated missing source document should be deferred")
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'content:release:terminal-retry'",
            )
            .fetch_one(&pool)
            .await
            .expect("count reactivated retry projection"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox_failed WHERE projection_key = 'content:release:terminal-retry'",
            )
            .fetch_one(&pool)
            .await
            .expect("count terminal recovery source"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT attempt_count FROM search_projection_outbox WHERE projection_key = 'content:release:terminal-retry'",
            )
            .fetch_one(&pool)
            .await
            .expect("read reset retry budget"),
            1
        );
    }

    #[tokio::test]
    async fn realtime_projection_actual_apply_error_rolls_back_before_recovery() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        sqlx::query(
            r#"
            INSERT INTO content_work_items (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, source_hash, protocol_version, model_profile,
              source_snapshot_json, configuration_fingerprint, status,
              created_at, updated_at
            ) VALUES ('search-actual-error-work', 'release', 'actual-error', 'translation', 'summary',
                      'zh-CN', 'search-actual-error-hash', 'content-processing.v1', 'test-model',
                      '{}', 'test-config', 'ready', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed actual-error work item");
        sqlx::query(
            r#"
            INSERT INTO content_result_projections (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, protocol_version, model_profile, source_hash, work_item_id,
              active_work_item_id, payload_json, published_at, updated_at
            ) VALUES ('search-actual-error-projection', 'release', 'actual-error', 'translation', 'summary',
                      'zh-CN', 'content-processing.v1', 'test-model', 'search-actual-error-hash',
                      'search-actual-error-work', 'search-actual-error-work', '{"title":"should not publish"}',
                      CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed actual-error projection");
        sqlx::query(
            "INSERT INTO search_documents (id, resource_type, resource_id, title, body, created_at, updated_at) VALUES ('release:actual-error', 'release', 'actual-error', 'source title', 'source body', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed actual-error search document");
        sqlx::query(
            "CREATE TRIGGER force_content_projection_failure BEFORE UPDATE OF translated_text ON search_documents WHEN NEW.id = 'release:actual-error' BEGIN SELECT RAISE(ABORT, 'forced projection apply failure'); END",
        )
        .execute(&pool)
        .await
        .expect("create projection failure trigger");
        sqlx::query(
            "INSERT INTO search_projection_outbox (projection_key, operation, resource_type, resource_id, canonical_resource_type, available_at, created_at, updated_at) VALUES ('content:release:actual-error', 'upsert', 'content', 'actual-error', 'release', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) ON CONFLICT(projection_key) DO UPDATE SET operation = excluded.operation, resource_type = excluded.resource_type, resource_id = excluded.resource_id, canonical_resource_type = excluded.canonical_resource_type, available_at = CURRENT_TIMESTAMP, lease_owner = NULL, lease_expires_at = NULL, last_error = NULL, updated_at = CURRENT_TIMESTAMP",
        )
        .execute(&pool)
        .await
        .expect("seed actual-error projection item");

        assert!(
            crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .is_err(),
            "actual projection SQL errors must keep the source error"
        );
        let (attempt_count, last_error, lease_owner, lease_expires_at) = sqlx::query_as::<
            _,
            (i64, Option<String>, Option<String>, Option<String>),
        >(
            "SELECT attempt_count, last_error, lease_owner, lease_expires_at FROM search_projection_outbox WHERE projection_key = 'content:release:actual-error'",
        )
        .fetch_one(&pool)
        .await
        .expect("load recovered actual-error projection");
        assert_eq!(attempt_count, 1);
        assert!(
            last_error
                .as_deref()
                .is_some_and(|error| error.contains("forced projection apply failure"))
        );
        assert!(lease_owner.is_none());
        assert!(lease_expires_at.is_none());
    }

    #[tokio::test]
    async fn realtime_projection_unsupported_delete_rolls_back_before_recovery() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let mut connection = pool.acquire().await.expect("acquire fixture connection");
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut *connection)
            .await
            .expect("allow invalid resource type for rollback fixture");
        sqlx::query(
            "INSERT INTO search_projection_outbox (projection_key, operation, resource_type, resource_id, available_at, created_at, updated_at) VALUES ('unsupported:delete', 'delete', 'unsupported', 'unsupported', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&mut *connection)
        .await
        .expect("seed unsupported delete projection");
        drop(connection);

        assert!(
            crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .is_err(),
            "unsupported projection deletes must keep the source error"
        );
        let (attempt_count, last_error, lease_owner, lease_expires_at) = sqlx::query_as::<
            _,
            (i64, Option<String>, Option<String>, Option<String>),
        >(
            "SELECT attempt_count, last_error, lease_owner, lease_expires_at FROM search_projection_outbox WHERE projection_key = 'unsupported:delete'",
        )
        .fetch_one(&pool)
        .await
        .expect("load recovered unsupported projection");
        assert_eq!(attempt_count, 1);
        assert!(last_error.as_deref().is_some_and(|error| {
            error.contains("unsupported search projection resource type unsupported")
        }));
        assert!(lease_owner.is_none());
        assert!(lease_expires_at.is_none());
    }

    #[tokio::test]
    async fn realtime_projection_terminal_recovery_failure_does_not_block_active_work() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        sqlx::query(
            "CREATE TRIGGER force_terminal_recovery_failure
             BEFORE INSERT ON search_projection_outbox
             WHEN NEW.projection_key = 'content:release:terminal-blocked'
             BEGIN
               SELECT RAISE(ABORT, 'forced terminal recovery failure');
             END",
        )
        .execute(&pool)
        .await
        .expect("create terminal recovery failure trigger");
        sqlx::query(
            "INSERT INTO search_projection_outbox_failed (
               projection_key, operation, resource_type, resource_id,
               canonical_resource_type, attempt_count, last_error,
               created_at, updated_at, failed_at
             ) VALUES (
               'content:release:terminal-blocked', 'upsert', 'content',
               'terminal-blocked', 'release', 8, 'writer deadline',
               CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, '2000-01-01T00:00:00Z'
             )",
        )
        .execute(&pool)
        .await
        .expect("seed terminal projection failure");
        sqlx::query(
            "INSERT INTO search_projection_outbox (
               projection_key, operation, resource_type, resource_id,
               available_at, created_at, updated_at
             ) VALUES (
               'notification:search-user:active-delete', 'delete', 'notification',
               'active-delete', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
             )",
        )
        .execute(&pool)
        .await
        .expect("seed active projection work");

        assert!(
            crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .expect("active projection should continue after recovery failure")
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'notification:search-user:active-delete'",
            )
            .fetch_one(&pool)
            .await
            .expect("count active projection work"),
            0
        );
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        assert!(
            crate::search_index::reactivate_terminal_projection_for_test(state.as_ref())
                .await
                .is_err(),
            "the forced trigger must exercise the terminal recovery error path"
        );
        assert!(
            !crate::search_index::run_projection_for_test(state.as_ref())
                .await
                .expect("terminal recovery failure should be deferred")
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox_failed WHERE projection_key = 'content:release:terminal-blocked'",
            )
            .fetch_one(&pool)
            .await
            .expect("count deferred terminal projection"),
            1
        );
    }

    #[tokio::test]
    async fn content_projection_retries_until_search_document_exists() {
        let pool = setup_pool().await;
        sqlx::query(
            r#"
            INSERT INTO content_work_items (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, source_hash, protocol_version, model_profile,
              source_snapshot_json, configuration_fingerprint, status,
              created_at, updated_at
            ) VALUES ('search-content-first-work', 'release', 'content-first', 'translation', 'summary',
                      'zh-CN', 'search-content-first-hash', 'content-processing.v1', 'test-model',
                      '{}', 'test-config', 'ready', '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed content-first work item");
        sqlx::query(
            r#"
            INSERT INTO content_result_projections (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, protocol_version, model_profile, source_hash, work_item_id,
              active_work_item_id, payload_json, published_at, updated_at
            ) VALUES ('search-content-first-projection', 'release', 'content-first', 'translation', 'summary',
                      'zh-CN', 'content-processing.v1', 'test-model', 'search-content-first-hash',
                      'search-content-first-work', 'search-content-first-work', '{"title":"content-first text"}',
                      '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed content-first projection");
        let state = setup_state(pool.clone());
        for retry in 1..=8 {
            match crate::search_index::run_projection_for_test(state.as_ref()).await {
                Ok(processed) => {
                    assert!(
                        !processed,
                        "content projection should commit deferred retry state"
                    );
                    break;
                }
                Err(error) => {
                    assert!(
                        crate::sqlite_write::is_sqlite_write_deadline_error(error.as_ref())
                            || crate::sqlite_write::is_sqlite_background_admission_error(
                                error.as_ref(),
                            ),
                        "content projection should only retry typed SQLite pressure: {error:#}"
                    );
                    assert!(
                        retry < 8,
                        "content projection pressure retries should be bounded"
                    );
                    sqlx::query(
                        "UPDATE search_projection_outbox SET available_at = CURRENT_TIMESTAMP WHERE projection_key = 'content:release:content-first'",
                    )
                    .execute(&pool)
                    .await
                    .expect("make deferred content projection available after pressure");
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
            }
        }
        let (last_error, lease_owner, lease_expires_at, attempt_count, retry_scheduled) =
            sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>, i64, i64)>(
                "SELECT last_error, lease_owner, lease_expires_at, attempt_count, available_at > CURRENT_TIMESTAMP FROM search_projection_outbox WHERE projection_key = 'content:release:content-first'",
            )
            .fetch_one(&pool)
            .await
            .expect("read deferred content projection state");
        assert_eq!(last_error.as_deref(), Some("missing_search_document"));
        assert!(lease_owner.is_none());
        assert!(lease_expires_at.is_none());
        assert_eq!(attempt_count, 1);
        assert_eq!(retry_scheduled, 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'content:release:content-first'",
            )
            .fetch_one(&pool)
            .await
            .expect("count deferred content projection"),
            1
        );
        sqlx::query(
            "INSERT INTO search_documents (id, resource_type, resource_id, title, body, created_at, updated_at) VALUES ('release:content-first', 'release', 'content-first', 'source title', 'source body', '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("materialize source search document");
        sqlx::query(
            "UPDATE search_projection_outbox SET available_at = CURRENT_TIMESTAMP WHERE projection_key = 'content:release:content-first'",
        )
        .execute(&pool)
        .await
        .expect("make deferred content projection available");
        drain_search_projection_outbox(state.as_ref()).await;
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT translated_text FROM search_documents WHERE id = 'release:content-first'",
            )
            .fetch_one(&pool)
            .await
            .expect("read converged content projection")
            .as_deref(),
            Some("content-first text")
        );
    }

    #[tokio::test]
    async fn content_projection_eligibility_change_refreshes_the_previous_search_key() {
        let pool = setup_pool().await;
        sqlx::query(
            r#"
            INSERT INTO content_work_items (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, source_hash, protocol_version, model_profile,
              source_snapshot_json, configuration_fingerprint, status,
              created_at, updated_at
            ) VALUES ('search-content-transition-work', 'release', 'content-transition', 'translation', 'summary',
                      'zh-CN', 'search-content-transition-hash', 'content-processing.v1', 'test-model',
                      '{}', 'test-config', 'ready', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed transition work item");
        sqlx::query(
            r#"
            INSERT INTO content_result_projections (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, protocol_version, model_profile, source_hash, work_item_id,
              active_work_item_id, payload_json, published_at, updated_at
            ) VALUES ('search-content-transition-projection', 'release', 'content-transition', 'translation', 'summary',
                      'zh-CN', 'content-processing.v1', 'test-model', 'search-content-transition-hash',
                      'search-content-transition-work', 'search-content-transition-work', '{"title":"transition text"}',
                      CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed transition projection");
        sqlx::query(
            "INSERT INTO search_documents (id, resource_type, resource_id, title, body, created_at, updated_at) VALUES ('release:content-transition', 'release', 'content-transition', 'source title', 'source body', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("seed transition source document");
        let state = setup_state(pool.clone());
        drain_search_projection_outbox(state.as_ref()).await;
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT translated_text FROM search_documents WHERE id = 'release:content-transition'",
            )
            .fetch_one(&pool)
            .await
            .expect("read initial transition projection")
            .as_deref(),
            Some("transition text")
        );

        sqlx::query(
            "UPDATE content_result_projections SET target_lang = 'en', updated_at = CURRENT_TIMESTAMP WHERE id = 'search-content-transition-projection'",
        )
        .execute(&pool)
        .await
        .expect("make content projection ineligible");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_projection_outbox WHERE projection_key = 'content:release:content-transition'",
            )
            .fetch_one(&pool)
            .await
            .expect("count old-key refresh item"),
            1
        );
        drain_search_projection_outbox(state.as_ref()).await;
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT translated_text FROM search_documents WHERE id = 'release:content-transition'",
            )
            .fetch_one(&pool)
            .await
            .expect("read cleared transition projection")
            .as_deref(),
            None
        );
    }

    #[tokio::test]
    async fn announcement_key_change_rebuilds_cached_lanes() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        sqlx::query(
            "INSERT INTO users (id, github_user_id, login, created_at, updated_at) VALUES ('search-announcement-user-2', 39999997, 'search-announcement-user-2', '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("seed second announcement lane user");
        sqlx::query(
            r#"
            INSERT INTO social_activity_events (
              id, user_id, kind, repo_id, repo_full_name, discussion_number,
              title, body, html_url, actor_github_user_id, actor_login,
              occurred_at, detected_at, created_at, updated_at
            ) VALUES ('search-announcement-key', 'search-user', 'announcement', 42,
                      'octo/rill', 1, 'Key migration announcement', 'announcement body',
                      'https://github.com/octo/rill/discussions/1', 99, 'octocat',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed announcement without discussion key");
        let state = setup_state(pool.clone());
        drain_search_projection_outbox(state.as_ref()).await;
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-announcement-key-translation', 'search-user',
                      'announcement_detail', 'octo/rill#1', 'zh-CN',
                      'hash-announcement-key', 'ready', '讨论键迁移命中', '公告正文',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed announcement translation before key");
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-announcement-key-smart', 'search-user',
                      'announcement_smart', 'octo/rill#1', 'zh-CN',
                      'hash-announcement-key-smart', 'ready', '智能摘要命中', '智能公告正文',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed smart announcement translation before key");
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-announcement-key-translation-2', 'search-announcement-user-2',
                      'announcement_detail', 'octo/rill#1', 'zh-CN',
                      'hash-announcement-key-2', 'ready', '第二用户命中', '公告正文二',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed second announcement lane translation before key");
        sqlx::query(
            "UPDATE social_activity_events SET discussion_number = 9, html_url = 'https://github.com/octo/rill/discussions/9' WHERE id = 'search-announcement-key'",
        )
        .execute(&pool)
        .await
        .expect("promote announcement to discussion key");

        drain_search_projection_outbox(state.as_ref()).await;
        let results = query(
            &state,
            "search-user",
            &parse_query("讨论键迁移").expect("parse migrated announcement query"),
        )
        .await
        .expect("query migrated announcement lane");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matched_lane, "translated");
        assert_eq!(
            results[0].target.as_ref().unwrap().href,
            "/octo/rill/discussions/9"
        );
        let lane_rows = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT user_id, translated_text, smart_text FROM search_document_user_lanes WHERE document_id = 'announcement:search-announcement-key' ORDER BY user_id",
        )
        .fetch_all(&pool)
        .await
        .expect("read rebuilt announcement lane users");
        assert_eq!(
            lane_rows,
            vec![
                (
                    "search-announcement-user-2".to_owned(),
                    Some("第二用户命中 公告正文二".to_owned()),
                    None,
                ),
                (
                    "search-user".to_owned(),
                    Some("讨论键迁移命中 公告正文".to_owned()),
                    Some("智能摘要命中 智能公告正文".to_owned()),
                ),
            ]
        );

        sqlx::query("DELETE FROM social_activity_events WHERE id = 'search-announcement-key'")
            .execute(&pool)
            .await
            .expect("delete projected announcement");
        drain_search_projection_outbox(state.as_ref()).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE id = 'announcement:search-announcement-key'",
            )
            .fetch_one(&pool)
            .await
            .expect("count deleted announcement document"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents_fts WHERE doc_id = 'announcement:search-announcement-key'",
            )
            .fetch_one(&pool)
            .await
            .expect("count deleted announcement FTS row"),
            0
        );
    }

    #[tokio::test]
    async fn late_global_projection_cannot_replace_newer_ready_projection() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        for (id, hash, status) in [
            ("search-work-new", "search-hash-new", "ready"),
            ("search-work-old", "search-hash-old", "ready"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO content_work_items (
                  id, canonical_resource_type, canonical_resource_id, pipeline, variant,
                  target_lang, source_hash, protocol_version, model_profile,
                  source_snapshot_json, configuration_fingerprint, status,
                  created_at, updated_at
                ) VALUES (?, 'release', '4201', 'translation', 'summary', 'zh-CN', ?,
                          'content-processing.v1', 'test-model', '{}', 'test-config', ?,
                          ?, ?)
                "#,
            )
            .bind(id)
            .bind(hash)
            .bind(status)
            .bind(if id.ends_with("new") {
                "2026-02-23T00:00:00Z"
            } else {
                "2026-02-22T00:00:00Z"
            })
            .bind(if id.ends_with("new") {
                "2026-02-23T00:00:00Z"
            } else {
                "2026-02-22T00:00:00Z"
            })
            .execute(&pool)
            .await
            .expect("seed global projection work item");
        }
        for (id, work_id, hash, title, updated_at) in [
            (
                "search-projection-new",
                "search-work-new",
                "search-hash-new",
                "new projection text",
                "2026-02-23T00:00:00Z",
            ),
            (
                "search-projection-old",
                "search-work-old",
                "search-hash-old",
                "old projection text",
                "2026-02-22T00:00:00Z",
            ),
        ] {
            sqlx::query(
                r#"
                INSERT INTO content_result_projections (
                  id, canonical_resource_type, canonical_resource_id, pipeline, variant,
                  target_lang, protocol_version, model_profile, source_hash, work_item_id,
                  active_work_item_id, payload_json, published_at, updated_at
                ) VALUES (?, 'release', '4201', 'translation', 'summary', 'zh-CN',
                          'content-processing.v1', 'test-model', ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(id)
            .bind(hash)
            .bind(work_id)
            .bind(work_id)
            .bind(format!(r#"{{"title":"{}"}}"#, title))
            .bind(updated_at)
            .bind(updated_at)
            .execute(&pool)
            .await
            .expect("seed global projection");
        }
        let state = setup_state(pool.clone());
        drain_search_projection_outbox(state.as_ref()).await;
        let projected = sqlx::query_scalar::<_, Option<String>>(
            "SELECT translated_text FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read global projection text");
        assert_eq!(projected.as_deref(), Some("new projection text"));

        sqlx::query("DELETE FROM content_result_projections WHERE id = 'search-projection-new'")
            .execute(&pool)
            .await
            .expect("delete newest global projection");
        drain_search_projection_outbox(state.as_ref()).await;
        let recovered = sqlx::query_scalar::<_, Option<String>>(
            "SELECT translated_text FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read recovered global projection text");
        assert_eq!(recovered.as_deref(), Some("old projection text"));
    }

    #[tokio::test]
    async fn backfill_prefers_ready_projection_over_newer_running_projection() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO content_work_items (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, source_hash, protocol_version, model_profile,
              source_snapshot_json, configuration_fingerprint, status,
              created_at, updated_at
            ) VALUES
              ('search-work-ready', 'release', '4201', 'translation', 'summary', 'zh-CN',
               'search-hash-ready', 'content-processing.v1', 'test-model', '{}', 'test-config',
               'ready', '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z'),
              ('search-work-running', 'release', '4201', 'translation', 'summary', 'zh-CN',
               'search-hash-running', 'content-processing.v1', 'test-model', '{}', 'test-config',
               'running', '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed ready and running work items");
        sqlx::query(
            r#"
            INSERT INTO content_result_projections (
              id, canonical_resource_type, canonical_resource_id, pipeline, variant,
              target_lang, protocol_version, model_profile, source_hash, work_item_id,
              active_work_item_id, payload_json, published_at, updated_at
            ) VALUES
              ('search-projection-ready', 'release', '4201', 'translation', 'summary', 'zh-CN',
               'content-processing.v1', 'test-model', 'search-hash-ready', 'search-work-ready',
               'search-work-ready', '{"title":"ready projection text"}',
               '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z'),
              ('search-projection-running', 'release', '4201', 'translation', 'summary', 'zh-CN',
               'content-processing.v1', 'test-model', 'search-hash-running', 'search-work-running',
               'search-work-running', '{"title":"running projection text"}',
               '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed ready and running projections");
        sqlx::query(
            "UPDATE search_documents SET translated_text = 'stale projection text' WHERE id = 'release:4201'",
        )
        .execute(&pool)
        .await
        .expect("make search projection stale");

        let state = setup_state(pool.clone());
        for _ in 0..=crate::search_index::PHASE_COUNT * 2 + 2 {
            crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
                .await
                .expect("run search projection batch");
        }

        let projected = sqlx::query_scalar::<_, Option<String>>(
            "SELECT translated_text FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read recovered projection text");
        assert_eq!(projected.as_deref(), Some("ready projection text"));
    }

    #[tokio::test]
    async fn association_repairs_release_metadata() {
        let pool = setup_pool().await;
        seed_release(&pool).await;
        let before_association = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT repo_full_name, target_path FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release before association");
        assert_eq!(before_association.0, None);
        assert_eq!(before_association.1, None);

        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'translations', cursor = 0, status = 'ready' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("mark search projection ready before metadata event");
        seed_repo_association(&pool).await;
        let queued = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_metadata_backfill_queue WHERE repo_id = 42",
        )
        .fetch_one(&pool)
        .await
        .expect("read queued release metadata refresh");
        assert_eq!(queued, 1);
        let after_association = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT repo_full_name, target_path FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release after association");
        assert_eq!(after_association, (None, None));

        let state = setup_state(pool.clone());
        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("run bounded release metadata refresh");
        let after_backfill = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT repo_full_name, target_path FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release after metadata backfill");
        assert_eq!(after_backfill.0.as_deref(), Some("octo/rill"));
        assert_eq!(
            after_backfill.1.as_deref(),
            Some("/octo/rill/releases/tag/v1.0.0")
        );
    }

    #[tokio::test]
    async fn work_item_deletion_repairs_release_metadata() {
        let pool = setup_pool().await;
        seed_release(&pool).await;
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'translations', cursor = 0, status = 'ready' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("mark search projection ready before work item metadata event");
        sqlx::query(
            r#"
            INSERT INTO repo_release_work_items (
              id, repo_id, repo_full_name, status, request_origin, priority,
              has_new_repo_watchers, deadline_at, last_release_count,
              last_candidate_failures, last_success_at, error_text,
              created_at, started_at, finished_at, updated_at
            ) VALUES ('search-work-delete', 42, 'octo/rill', 'succeeded', 'test', 0,
                      0, '2026-02-23T00:00:00Z', 1, 0, NULL, NULL,
                      '2026-02-23T00:00:00Z', NULL, NULL, '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed release work item metadata");

        let state = setup_state(pool.clone());
        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("apply work item metadata");
        let before_delete = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT repo_full_name, target_path FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release metadata before work item deletion");
        assert_eq!(
            before_delete,
            (
                Some("octo/rill".to_owned()),
                Some("/octo/rill/releases/tag/v1.0.0".to_owned())
            )
        );

        sqlx::query("DELETE FROM repo_release_work_items WHERE id = 'search-work-delete'")
            .execute(&pool)
            .await
            .expect("delete release work item metadata source");
        let queued = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_metadata_backfill_queue WHERE repo_id = 42",
        )
        .fetch_one(&pool)
        .await
        .expect("read queued metadata repair after work item deletion");
        assert_eq!(queued, 1);

        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("repair metadata after work item deletion");
        let after_delete = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT repo_full_name, target_path FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release metadata after work item deletion");
        assert_eq!(after_delete, (None, None));
    }

    #[tokio::test]
    async fn metadata_fallback_matches_current_short_repo_name() {
        let pool = setup_pool().await;
        seed_release(&pool).await;
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'translations', cursor = 0, status = 'ready' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("mark search projection ready before metadata fallback");
        seed_repo_association(&pool).await;

        let results = query(
            &setup_state(pool.clone()),
            "search-user",
            &parse_query("repo:rill").expect("parse short repository filter"),
        )
        .await
        .expect("query current metadata short repository filter");
        assert!(
            results.iter().any(|result| result.id == "release:4201"),
            "current metadata should match a short repository filter before fanout completes"
        );
    }

    #[tokio::test]
    async fn release_query_does_not_fallback_to_stale_metadata() {
        let pool = setup_pool().await;
        seed_release(&pool).await;
        sqlx::query(
            "UPDATE search_documents SET repo_full_name = 'stale/legacy', owner_login = 'stale', target_path = '/stale/legacy/releases/tag/old', target_url = 'https://github.com/stale/legacy/releases/tag/old' WHERE id = 'release:4201'",
        )
        .execute(&pool)
        .await
        .expect("seed stale release metadata");

        sqlx::query("DROP VIEW user_release_visible_repos")
            .execute(&pool)
            .await
            .expect("replace release visibility view");
        sqlx::query(
            "CREATE VIEW user_release_visible_repos AS SELECT 'other-user' AS user_id, 42 AS repo_id, 'stale/other' AS full_name, 'stale' AS owner_login, '2026-02-24T00:00:00Z' AS updated_at UNION ALL SELECT 'search-user' AS user_id, 42 AS repo_id, NULL AS full_name, NULL AS owner_login, NULL AS updated_at",
        )
        .execute(&pool)
        .await
        .expect("seed visible release with missing current metadata");

        let state = setup_state(pool.clone());
        let visible = query(
            &state,
            "search-user",
            &parse_query("type:release").expect("parse release type filter"),
        )
        .await
        .expect("query release with missing current metadata");
        assert_eq!(visible.len(), 1);
        assert!(visible[0].repository.is_none());
        assert!(visible[0].target.is_none());

        assert!(
            query(
                &state,
                "search-user",
                &parse_query("stale").expect("parse stale metadata term"),
            )
            .await
            .expect("query stale release metadata")
            .is_empty()
        );
        assert!(
            query(
                &state,
                "search-user",
                &parse_query("repo:legacy").expect("parse stale repository filter"),
            )
            .await
            .expect("query stale repository filter")
            .is_empty()
        );
    }

    #[tokio::test]
    async fn metadata_fanout_resumes_in_release_row_batches() {
        let pool = setup_pool().await;
        for index in 0..250 {
            let document_id = format!("release:metadata-{index}");
            let resource_id = format!("metadata-{index}");
            sqlx::query(
                "INSERT INTO search_documents (id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,created_at,updated_at) VALUES (?,'release',?,42,'stale/repo','stale','metadata title','metadata body','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z')",
            )
            .bind(document_id)
            .bind(resource_id)
            .execute(&pool)
            .await
            .expect("seed release projection rows");
        }
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'translations', cursor = 0, status = 'ready' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("mark search projection ready before metadata fanout");
        seed_repo_association(&pool).await;

        let state = setup_state(pool.clone());
        for expected in [25_i64, 50, 75, 100, 125, 150, 175, 200, 225, 250] {
            crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
                .await
                .expect("run bounded metadata batch");
            let refreshed = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM search_documents WHERE resource_type = 'release' AND repo_id = 42 AND repo_full_name = 'octo/rill'",
            )
            .fetch_one(&pool)
            .await
            .expect("count refreshed release metadata");
            assert_eq!(refreshed, expected);
        }
        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("confirm metadata cursor exhaustion");
        let queued = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_metadata_backfill_queue WHERE repo_id = 42",
        )
        .fetch_one(&pool)
        .await
        .expect("count remaining metadata queue");
        assert_eq!(queued, 0);
        let indexed = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_documents_fts WHERE doc_id LIKE 'release:%' AND repo_full_name = 'octo/rill'",
        )
        .fetch_one(&pool)
        .await
        .expect("count refreshed FTS metadata");
        assert_eq!(indexed, 250);
    }

    #[tokio::test]
    async fn metadata_change_restarts_release_cursor() {
        let pool = setup_pool().await;
        for index in 0..250 {
            let document_id = format!("release:metadata-restart-{index}");
            let resource_id = format!("metadata-restart-{index}");
            sqlx::query(
                "INSERT INTO search_documents (id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,created_at,updated_at) VALUES (?,'release',?,42,'stale/repo','stale','metadata title','metadata body','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z')",
            )
            .bind(document_id)
            .bind(resource_id)
            .execute(&pool)
            .await
            .expect("seed release projection rows for cursor restart");
        }
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'translations', cursor = 0, status = 'ready' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("mark search projection ready before cursor restart");
        seed_repo_association(&pool).await;

        let state = setup_state(pool.clone());
        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("run first metadata cursor batch");
        let first_batch = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_documents WHERE resource_type = 'release' AND repo_id = 42 AND repo_full_name = 'octo/rill'",
        )
        .fetch_one(&pool)
        .await
        .expect("count first metadata cursor batch");
        assert_eq!(first_batch, 25);

        sqlx::query(
            "UPDATE user_repo_associations SET repo_full_name = 'next/rill', repo_full_name_lower = 'next/rill', owner_login = 'next' WHERE id = 'search-association'",
        )
        .execute(&pool)
        .await
        .expect("change repository metadata during queued fanout");
        let cursor = sqlx::query_scalar::<_, i64>(
            "SELECT release_cursor FROM search_metadata_backfill_queue WHERE repo_id = 42",
        )
        .fetch_one(&pool)
        .await
        .expect("read reset metadata cursor");
        assert_eq!(cursor, 0);

        for _ in 0..10 {
            crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
                .await
                .expect("run restarted metadata cursor batch");
        }
        let refreshed = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_documents WHERE resource_type = 'release' AND repo_id = 42 AND repo_full_name = 'next/rill'",
        )
        .fetch_one(&pool)
        .await
        .expect("count refreshed metadata after cursor restart");
        assert_eq!(refreshed, 250);
    }

    #[tokio::test]
    async fn association_non_projection_updates_do_not_rebuild_release_fts() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        let expected_release_document = sqlx::query_as::<
            _,
            (
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                String,
            ),
        >(
            "SELECT repo_full_name, owner_login, title, body, target_path, target_url, updated_at FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release search document sentinel");
        let expected_repository_document = sqlx::query_as::<
            _,
            (
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                String,
            ),
        >(
            "SELECT repo_full_name, owner_login, title, body, target_path, target_url, updated_at FROM search_documents WHERE id = 'repository:search-user:octo/rill'",
        )
        .fetch_one(&pool)
        .await
        .expect("read repository search document sentinel");

        let updates = [
            (
                "source flag",
                "UPDATE user_repo_associations SET has_github_star_source = 0, updated_at = '2026-02-24T00:00:00Z' WHERE id = 'search-association'",
            ),
            (
                "follow state",
                "UPDATE user_repo_associations SET is_following = 0, updated_at = '2026-02-24T00:00:01Z' WHERE id = 'search-association'",
            ),
            (
                "timestamp",
                "UPDATE user_repo_associations SET updated_at = '2026-02-24T00:00:02Z' WHERE id = 'search-association'",
            ),
        ];

        for (label, update) in updates {
            sqlx::query("UPDATE search_documents_fts SET body = 'fts-sentinel' WHERE doc_id IN ('release:4201', 'repository:search-user:octo/rill')")
                .execute(&pool)
                .await
                .expect("seed release FTS sentinel");
            sqlx::query(update)
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("apply {label} association update: {error}"));

            let release_document = sqlx::query_as::<
                _,
                (
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    String,
                ),
            >(
                "SELECT repo_full_name, owner_login, title, body, target_path, target_url, updated_at FROM search_documents WHERE id = 'release:4201'",
            )
            .fetch_one(&pool)
            .await
            .expect("read release search document");
            assert_eq!(
                release_document, expected_release_document,
                "{label} update changed release search document"
            );
            let repository_document = sqlx::query_as::<
                _,
                (
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    String,
                ),
            >(
                "SELECT repo_full_name, owner_login, title, body, target_path, target_url, updated_at FROM search_documents WHERE id = 'repository:search-user:octo/rill'",
            )
            .fetch_one(&pool)
            .await
            .expect("read repository search document");
            assert_eq!(
                repository_document, expected_repository_document,
                "{label} update changed repository search document"
            );
            let release_fts_body = sqlx::query_scalar::<_, String>(
                "SELECT body FROM search_documents_fts WHERE doc_id = 'release:4201'",
            )
            .fetch_one(&pool)
            .await
            .expect("read release FTS sentinel");
            assert_eq!(
                release_fts_body, "fts-sentinel",
                "{label} update rebuilt release FTS"
            );
            let repository_fts_body = sqlx::query_scalar::<_, String>(
                "SELECT body FROM search_documents_fts WHERE doc_id = 'repository:search-user:octo/rill'",
            )
            .fetch_one(&pool)
            .await
            .expect("read repository FTS sentinel");
            assert_eq!(
                repository_fts_body, "fts-sentinel",
                "{label} update rebuilt repository FTS"
            );
        }
    }

    #[tokio::test]
    async fn owned_release_visibility_repairs_cached_release_metadata() {
        let pool = setup_pool().await;
        seed_release(&pool).await;
        let before = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT repo_full_name, target_path FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release before owned baseline");
        assert_eq!(before, (None, None));

        sqlx::query("UPDATE users SET include_own_releases = 1 WHERE id = 'search-user'")
            .execute(&pool)
            .await
            .expect("enable owned release visibility");
        sqlx::query(
            r#"
            INSERT INTO owned_repo_star_baselines (
              id, user_id, repo_id, repo_full_name, initialized_at, updated_at
            ) VALUES ('search-owned-baseline', 'search-user', 42, 'octo/rill',
                      '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("insert owned repository baseline");

        let state = setup_state(pool.clone());
        let visible = query(
            &state,
            "search-user",
            &parse_query("legacy owner:octo repo:octo/rill").unwrap(),
        )
        .await
        .expect("query owned release after baseline");
        assert_eq!(visible.len(), 1);
        assert_eq!(
            visible[0].target.as_ref().unwrap().href,
            "/octo/rill/releases/tag/v1.0.0"
        );

        sqlx::query("UPDATE search_documents SET target_path = NULL WHERE id = 'release:4201'")
            .execute(&pool)
            .await
            .expect("simulate legacy release target");
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'releases', cursor = 0, status = 'building' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("rewind release backfill state");
        for _ in 0..=crate::search_index::PHASE_COUNT * 2 + 2 {
            crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
                .await
                .expect("repair legacy release target");
        }
        let recovered = query(
            &state,
            "search-user",
            &parse_query("legacy owner:octo repo:octo/rill").unwrap(),
        )
        .await
        .expect("query repaired legacy release");
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            recovered[0].target.as_ref().unwrap().href,
            "/octo/rill/releases/tag/v1.0.0"
        );

        sqlx::query(
            "UPDATE owned_repo_star_baselines SET repo_full_name = 'octo/renamed', updated_at = '2026-02-24T00:00:00Z' WHERE id = 'search-owned-baseline'",
        )
        .execute(&pool)
        .await
        .expect("rename owned repository baseline");
        let renamed = query(
            &state,
            "search-user",
            &parse_query("legacy owner:octo repo:octo/renamed").unwrap(),
        )
        .await
        .expect("query renamed owned release");
        assert_eq!(renamed.len(), 1);
        assert_eq!(
            renamed[0].target.as_ref().unwrap().href,
            "/octo/renamed/releases/tag/v1.0.0"
        );
    }

    #[tokio::test]
    async fn association_rename_removes_stale_repository_projection() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;

        sqlx::query(
            "UPDATE user_repo_associations SET repo_full_name = 'octo/renamed', repo_full_name_lower = 'octo/renamed', repo_name = 'renamed', updated_at = '2026-02-24T00:00:00Z' WHERE id = 'search-association'",
        )
        .execute(&pool)
        .await
        .expect("rename repository association");

        let stale_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_documents WHERE id = 'repository:search-user:octo/rill'",
        )
        .fetch_one(&pool)
        .await
        .expect("count stale repository projection");
        assert_eq!(stale_count, 0);

        let renamed = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM search_documents WHERE id = 'repository:search-user:octo/renamed'",
        )
        .fetch_one(&pool)
        .await
        .expect("count renamed repository projection");
        assert_eq!(renamed, 1);
    }

    #[tokio::test]
    async fn repository_rename_deduplicates_star_and_association_projection() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO starred_repos (
              id, user_id, repo_id, full_name, owner_login, name, description,
              html_url, stargazed_at, is_private, updated_at
            ) VALUES ('search-starred-rename', 'search-user', 42, 'octo/rill',
                      'octo', 'rill', 'starred repository',
                      'https://github.com/octo/rill', '2026-02-23T00:00:00Z', 0,
                      '2026-02-23T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed starred rename fixture");

        sqlx::query(
            "UPDATE starred_repos SET full_name = 'octo/renamed', owner_login = 'octo', name = 'renamed', html_url = 'https://github.com/octo/renamed' WHERE id = 'search-starred-rename'",
        )
        .execute(&pool)
        .await
        .expect("rename starred repository");
        sqlx::query(
            r#"
            INSERT INTO user_repo_associations (
              id, user_id, repo_id, repo_full_name, repo_full_name_lower,
              owner_login, repo_name, html_url, description, is_private,
              first_source, first_associated_at, last_seen_at, is_following,
              follow_state_source, has_personal_owned_source, has_github_star_source,
              has_manual_feed_source, created_at, updated_at
            )
            VALUES ('search-association-renamed', 'search-user', 42, 'octo/renamed', 'octo/renamed',
                    'octo', 'renamed', 'https://github.com/octo/renamed', 'renamed repository', 0,
                    'github_star', '2026-02-23T00:00:00Z', '2026-02-24T00:00:00Z', 1,
                    'system_default', 0, 1, 0, '2026-02-23T00:00:00Z', '2026-02-24T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("insert renamed association");

        let projections = sqlx::query_as::<_, (String, String)>(
            "SELECT id, repo_full_name FROM search_documents WHERE user_id = 'search-user' AND resource_type = 'repository' AND repo_id = 42 ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .expect("read renamed repository projections");
        assert_eq!(
            projections,
            vec![(
                "repository:search-user:octo/renamed".to_owned(),
                "octo/renamed".to_owned(),
            )]
        );
    }

    #[tokio::test]
    #[ignore = "manual production-shaped search maintenance benchmark"]
    async fn production_sized_fts_maintenance_benchmark() {
        use std::time::Instant;

        use time::OffsetDateTime;
        use tower_sessions::{
            SessionStore,
            session::{Id, Record},
        };

        let pool = setup_pool().await;
        const ASSOCIATION_COUNT: i64 = 397;
        const RELEASE_COUNT: i64 = 39_700;
        const MAINTENANCE_ROUNDS: usize = 32;

        for index in 0..ASSOCIATION_COUNT {
            let user_id = format!("search-bench-user-{index}");
            sqlx::query(
                "INSERT INTO users (id, github_user_id, login, created_at, updated_at) VALUES (?, ?, ?, '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')",
            )
            .bind(&user_id)
            .bind(50_000_000 + index)
            .bind(&user_id)
            .execute(&pool)
            .await
            .expect("seed benchmark user");
            let repo_name = format!("repo-{index}");
            let full_name = format!("bench/{repo_name}");
            sqlx::query(
                r#"
                INSERT INTO user_repo_associations (
                  id, user_id, repo_id, repo_full_name, repo_full_name_lower,
                  owner_login, repo_name, html_url, description, is_private,
                  first_source, first_associated_at, last_seen_at, is_following,
                  follow_state_source, has_personal_owned_source, has_github_star_source,
                  has_manual_feed_source, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, 'bench', ?, ?, 'benchmark association', 0,
                          'github_star', '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z', 1,
                          'system_default', 0, 1, 0, '2026-02-23T00:00:00Z', '2026-02-23T00:00:00Z')
                "#,
            )
            .bind(format!("search-bench-association-{index}"))
            .bind(&user_id)
            .bind(10_000 + index)
            .bind(&full_name)
            .bind(&full_name)
            .bind(&repo_name)
            .bind(format!("https://github.com/{full_name}"))
            .execute(&pool)
            .await
            .expect("seed benchmark association");
        }

        for index in 0..RELEASE_COUNT {
            let document_id = format!("release:search-bench-{index}");
            let resource_id = format!("search-bench-{index}");
            sqlx::query(
                "INSERT INTO search_documents (id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,created_at,updated_at) VALUES (?,'release',?,?,'bench/stale','bench','benchmark title','benchmark body','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z')",
            )
            .bind(document_id)
            .bind(resource_id)
            .bind(10_000 + index % ASSOCIATION_COUNT)
            .execute(&pool)
            .await
            .expect("seed benchmark release document");
        }
        for index in 0..ASSOCIATION_COUNT {
            sqlx::query(
                "INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,updated_at) VALUES (?,'search-user','benchmark translated lane','2026-02-23T00:00:00Z')",
            )
            .bind(format!("release:search-bench-{index}"))
            .execute(&pool)
            .await
            .expect("seed benchmark user lane");
        }

        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE bench_search_documents_fts_before USING fts5(
              doc_id UNINDEXED, title, body, repo_full_name, translated_text, smart_text,
              tokenize = 'trigram'
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create old global FTS reproduction");
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE bench_search_user_lanes_fts_before USING fts5(
              doc_id UNINDEXED, user_id UNINDEXED, translated_text, smart_text,
              tokenize = 'trigram'
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create old lane FTS reproduction");
        sqlx::query(
            "INSERT INTO bench_search_documents_fts_before SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id LIKE 'release:search-bench-%'",
        )
        .execute(&pool)
        .await
        .expect("populate old global FTS reproduction");
        sqlx::query(
            "INSERT INTO bench_search_user_lanes_fts_before SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE user_id = 'search-user'",
        )
        .execute(&pool)
        .await
        .expect("populate old lane FTS reproduction");

        let old_global_plan = sqlx::query(
            "EXPLAIN QUERY PLAN DELETE FROM bench_search_documents_fts_before WHERE doc_id = 'release:search-bench-396'",
        )
        .fetch_all(&pool)
        .await
        .expect("explain old global FTS maintenance")
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();
        let old_lane_plan = sqlx::query(
            "EXPLAIN QUERY PLAN DELETE FROM bench_search_user_lanes_fts_before WHERE doc_id = 'release:search-bench-396' AND user_id = 'search-user'",
        )
        .fetch_all(&pool)
        .await
        .expect("explain old lane FTS maintenance")
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();
        let new_global_plan = sqlx::query(
            "EXPLAIN QUERY PLAN DELETE FROM search_documents_fts WHERE doc_id = 'release:search-bench-396'",
        )
        .fetch_all(&pool)
        .await
        .expect("explain new global FTS maintenance")
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();
        let new_lane_plan = sqlx::query(
            "EXPLAIN QUERY PLAN DELETE FROM search_document_user_lanes_fts WHERE doc_id = 'release:search-bench-396' AND user_id = 'search-user'",
        )
        .fetch_all(&pool)
        .await
        .expect("explain new lane FTS maintenance")
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();

        let old_global_started = Instant::now();
        for _ in 0..MAINTENANCE_ROUNDS {
            sqlx::query(
                "DELETE FROM bench_search_documents_fts_before WHERE doc_id = 'release:search-bench-396'",
            )
            .execute(&pool)
            .await
            .expect("delete old global FTS row");
            sqlx::query(
                "INSERT INTO bench_search_documents_fts_before VALUES ('release:search-bench-396','benchmark title','benchmark body','bench/stale','','')",
            )
            .execute(&pool)
            .await
            .expect("restore old global FTS row");
        }
        let old_global_elapsed = old_global_started.elapsed();

        let new_global_started = Instant::now();
        for _ in 0..MAINTENANCE_ROUNDS {
            sqlx::query(
                "DELETE FROM search_documents_fts WHERE doc_id = 'release:search-bench-396'",
            )
            .execute(&pool)
            .await
            .expect("delete new global FTS row");
            sqlx::query(
                "INSERT INTO search_documents_fts VALUES ('release:search-bench-396','benchmark title','benchmark body','bench/stale','','')",
            )
            .execute(&pool)
            .await
            .expect("restore new global FTS row");
        }
        let new_global_elapsed = new_global_started.elapsed();

        let old_lane_started = Instant::now();
        for _ in 0..MAINTENANCE_ROUNDS {
            sqlx::query(
                "DELETE FROM bench_search_user_lanes_fts_before WHERE doc_id = 'release:search-bench-396' AND user_id = 'search-user'",
            )
            .execute(&pool)
            .await
            .expect("delete old lane FTS row");
            sqlx::query(
                "INSERT INTO bench_search_user_lanes_fts_before VALUES ('release:search-bench-396','search-user','benchmark translated lane','')",
            )
            .execute(&pool)
            .await
            .expect("restore old lane FTS row");
        }
        let old_lane_elapsed = old_lane_started.elapsed();

        let new_lane_started = Instant::now();
        for _ in 0..MAINTENANCE_ROUNDS {
            sqlx::query(
                "DELETE FROM search_document_user_lanes_fts WHERE doc_id = 'release:search-bench-396' AND user_id = 'search-user'",
            )
            .execute(&pool)
            .await
            .expect("delete new lane FTS row");
            sqlx::query(
                "INSERT INTO search_document_user_lanes_fts VALUES ('release:search-bench-396','search-user','benchmark translated lane','')",
            )
            .execute(&pool)
            .await
            .expect("restore new lane FTS row");
        }
        let new_lane_elapsed = new_lane_started.elapsed();

        let state = setup_state(pool.clone());
        sqlx::query(
            "UPDATE search_projection_backfill_state SET phase = 'metadata', cursor = 0, status = 'building' WHERE id = 1",
        )
        .execute(&pool)
        .await
        .expect("set metadata benchmark phase");
        let metadata_started = Instant::now();
        crate::search_index::run_batch_for_test(state.as_ref(), u64::MAX)
            .await
            .expect("run metadata benchmark batch");
        let metadata_elapsed = metadata_started.elapsed();
        let metadata_remaining =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_metadata_backfill_queue")
                .fetch_one(&pool)
                .await
                .expect("count remaining benchmark metadata work");

        let session_store = crate::session_store::CoordinatedSqliteSessionStore::new(
            tower_sessions_sqlx_store::SqliteStore::new(pool.clone()),
            pool.clone(),
            state.sqlite_writer.clone(),
        );
        session_store
            .migrate()
            .await
            .expect("migrate benchmark session store");
        let parsed = parse_query("benchmark").expect("parse benchmark query");
        let mut get_tasks = Vec::new();
        for _ in 0..32 {
            let state = state.clone();
            let parsed = parsed.clone();
            get_tasks.push(tokio::spawn(async move {
                consume_quota(&state, "search-user")
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                query(&state, "search-user", &parsed)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                Ok::<_, anyhow::Error>(())
            }));
        }
        let mut search_tasks = Vec::new();
        for _ in 0..32 {
            let state = state.clone();
            let parsed = parsed.clone();
            search_tasks.push(tokio::spawn(async move {
                for _ in 0..8 {
                    query(&state, "search-user", &parsed)
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                    tokio::task::yield_now().await;
                }
                Ok::<_, anyhow::Error>(())
            }));
        }
        let mut session_tasks = Vec::new();
        for _ in 0..32 {
            let session_store = session_store.clone();
            session_tasks.push(tokio::spawn(async move {
                let mut record = Record {
                    id: Id::default(),
                    data: Default::default(),
                    expiry_date: OffsetDateTime::now_utc(),
                };
                session_store
                    .create(&mut record)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                Ok::<_, anyhow::Error>(())
            }));
        }
        for task in search_tasks {
            task.await
                .expect("join benchmark search task")
                .expect("benchmark search task");
        }
        for task in get_tasks {
            task.await
                .expect("join benchmark GET task")
                .expect("benchmark GET task");
        }
        for task in session_tasks {
            task.await
                .expect("join benchmark session task")
                .expect("benchmark session task");
        }

        println!(
            "SEARCH_FTS_BENCHMARK associations={ASSOCIATION_COUNT} releases={RELEASE_COUNT} old_global_plan={old_global_plan:?} new_global_plan={new_global_plan:?} old_lane_plan={old_lane_plan:?} new_lane_plan={new_lane_plan:?} old_global_ms={} new_global_ms={} old_lane_ms={} new_lane_ms={} rounds={MAINTENANCE_ROUNDS} metadata_batch_ms={} metadata_remaining={} concurrent_search_tasks=32 concurrent_get_tasks=32 concurrent_session_writes=32",
            old_global_elapsed.as_secs_f64() * 1000.0,
            new_global_elapsed.as_secs_f64() * 1000.0,
            old_lane_elapsed.as_secs_f64() * 1000.0,
            new_lane_elapsed.as_secs_f64() * 1000.0,
            metadata_elapsed.as_secs_f64() * 1000.0,
            metadata_remaining,
        );
    }

    #[tokio::test]
    async fn filtered_results_apply_predicates_before_limit() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        for index in 0..SEARCH_LIMIT {
            let id = format!("repository:search-user:noise-{index}");
            sqlx::query(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_full_name,owner_login,title,body,source_time,created_at,updated_at) VALUES (?,?,'repository',?,?,?,'noise','needle','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z','2026-02-23T00:00:00Z')",
            )
            .bind(&id)
            .bind("search-user")
            .bind(format!("noise/repo-{index}"))
            .bind(format!("noise/repo-{index}"))
            .bind("noise")
            .execute(&pool)
            .await
            .expect("seed over-cap noise document");
            sqlx::query(
                "INSERT INTO search_documents_fts (doc_id,title,body,repo_full_name,translated_text,smart_text) VALUES (?, 'noise', 'needle', ?, '', '')",
            )
            .bind(&id)
            .bind(format!("noise/repo-{index}"))
            .execute(&pool)
            .await
            .expect("index over-cap noise document");
        }
        sqlx::query(
            "INSERT INTO search_documents (id,user_id,resource_type,resource_id,title,body,source_time,created_at,updated_at) VALUES ('brief:filtered-target','search-user','brief','filtered-target','target','needle','2026-02-22T00:00:00Z','2026-02-22T00:00:00Z','2026-02-22T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("seed filtered target");
        sqlx::query(
            "INSERT INTO search_documents_fts (doc_id,title,body,repo_full_name,translated_text,smart_text) VALUES ('brief:filtered-target', 'target', 'needle', '', '', '')",
        )
        .execute(&pool)
        .await
        .expect("index filtered target");

        let results = query(
            &state,
            "search-user",
            &parse_query("needle type:brief").unwrap(),
        )
        .await
        .expect("query filtered over-cap results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "brief:filtered-target");
    }

    #[tokio::test]
    async fn rate_limit_is_atomic_under_concurrency() {
        let pool = setup_pool().await;
        let state = Arc::new(setup_state(pool.clone()));
        let mut handles = Vec::new();
        for _ in 0..55 {
            let state = state.clone();
            handles.push(tokio::spawn(async move {
                consume_quota(state.as_ref(), "search-user").await
            }));
        }
        let mut successes = 0;
        let mut limited = 0;
        for handle in handles {
            match handle.await.expect("quota task") {
                Ok((remaining, _)) => {
                    assert!((0..SEARCH_RATE_LIMIT).contains(&remaining));
                    successes += 1;
                }
                Err(error) => {
                    assert_eq!(error.code(), "search_rate_limited");
                    limited += 1;
                }
            }
        }
        assert_eq!(successes, SEARCH_RATE_LIMIT);
        assert_eq!(limited, 5);
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT request_count FROM search_rate_limits WHERE user_id = 'search-user'",
        )
        .fetch_one(&pool)
        .await
        .expect("read concurrent quota");
        assert_eq!(count, SEARCH_RATE_LIMIT);
    }

    #[tokio::test]
    async fn fts_doc_id_maintenance_uses_indexed_point_updates() {
        let pool = setup_pool().await;

        let global_plan = sqlx::query(
            "EXPLAIN QUERY PLAN DELETE FROM search_documents_fts WHERE doc_id = 'release:4201'",
        )
        .fetch_all(&pool)
        .await
        .expect("explain global FTS maintenance")
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();
        let lane_plan = sqlx::query(
            "EXPLAIN QUERY PLAN DELETE FROM search_document_user_lanes_fts WHERE doc_id = 'release:4201' AND user_id = 'search-user'",
        )
        .fetch_all(&pool)
        .await
        .expect("explain user-lane FTS maintenance")
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>();

        assert!(
            global_plan
                .iter()
                .any(|detail| detail.contains("VIRTUAL TABLE INDEX 0:=")),
            "global FTS maintenance must use an indexed rowid lookup: {global_plan:?}"
        );
        assert!(
            !global_plan.iter().any(|detail| {
                detail.contains("VIRTUAL TABLE INDEX 0:")
                    && !detail.contains("VIRTUAL TABLE INDEX 0:=")
            }),
            "global FTS maintenance must not scan the FTS corpus: {global_plan:?}"
        );
        assert!(
            lane_plan
                .iter()
                .any(|detail| detail.contains("VIRTUAL TABLE INDEX 0:=")),
            "user-lane FTS maintenance must use an indexed rowid lookup: {lane_plan:?}"
        );
        assert!(
            !lane_plan.iter().any(|detail| {
                detail.contains("VIRTUAL TABLE INDEX 0:")
                    && !detail.contains("VIRTUAL TABLE INDEX 0:=")
            }),
            "user-lane FTS maintenance must not scan the FTS corpus: {lane_plan:?}"
        );
    }
}
