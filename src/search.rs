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
    pub target: SearchTarget,
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
        "WITH ranked AS (SELECT d.id, d.resource_type, d.title, d.body, d.repo_full_name, COALESCE(ul.translated_text, d.translated_text) AS translated_text, COALESCE(ul.smart_text, d.smart_text) AS smart_text, d.source_time, d.updated_at, d.unread, CASE WHEN d.resource_type = 'repository' THEN EXISTS (SELECT 1 FROM user_repo_associations ura WHERE ura.user_id = d.user_id AND lower(ura.repo_full_name_lower) = lower(d.resource_id) AND ura.is_following != 0) ELSE NULL END AS is_following, d.target_path, d.target_url, ROW_NUMBER() OVER (PARTITION BY CASE WHEN d.resource_type = 'announcement' THEN COALESCE(d.resource_id, '') ELSE d.id END ORDER BY CASE WHEN d.resource_type = 'announcement' AND d.user_id = ",
    );
    builder.push_bind(user_id);
    builder.push(
        " THEN 0 ELSE 1 END, COALESCE(d.source_time, d.updated_at) DESC, d.id DESC) AS search_rank FROM search_documents d LEFT JOIN search_document_user_lanes ul ON ul.document_id = d.id AND ul.user_id = ",
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
        builder.push(" AND lower(COALESCE(d.owner_login, '')) = lower(");
        builder.push_bind(owner);
        builder.push(")");
    }
    if let Some(repo) = &parsed.repo {
        builder.push(" AND (lower(COALESCE(d.repo_full_name, '')) = lower(");
        builder.push_bind(repo);
        builder.push(") OR lower(COALESCE(d.repo_full_name, '')) LIKE '%/' || lower(");
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
        builder.push(" AND (");
        builder.push("d.id IN (SELECT f.doc_id FROM search_documents_fts AS f WHERE search_documents_fts MATCH ");
        builder.push_bind(fts_match_query.clone());
        builder.push(") OR d.id IN (SELECT uf.doc_id FROM search_document_user_lanes_fts AS uf WHERE uf.user_id = ");
        builder.push_bind(user_id);
        builder.push(" AND search_document_user_lanes_fts MATCH ");
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
        builder.push(" OR lower(COALESCE(d.repo_full_name, '')) LIKE lower(");
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
    Ok(match status.as_deref() {
        Some("ready") => "ready".to_owned(),
        Some("paused_low_disk") => "paused_low_disk".to_owned(),
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
    let href = row
        .target_path
        .clone()
        .or(row.target_url.clone())
        .map(|target| canonicalize_target_path(&row.resource_type, &target))
        .unwrap_or_else(|| "/".to_owned());
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
        target: SearchTarget {
            href,
            lane: Some(matched_lane),
        },
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
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
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
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
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
            admin_collection_read_gate: Arc::new(tokio::sync::Semaphore::new(1)),
            runtime_owner_id: "search-test-runtime".to_owned(),
        })
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
        assert_eq!(announcement[0].target.href, "/octo/rill/discussions/7");

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
    async fn projection_backfill_pauses_below_disk_watermark() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        seed_release(&pool).await;
        let state = setup_state(pool.clone());
        let before = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM search_documents")
            .fetch_one(&pool)
            .await
            .expect("count existing search documents");
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
        sqlx::query("UPDATE search_projection_backfill_state SET status = 'ready' WHERE id = 1")
            .execute(&pool)
            .await
            .expect("ready search index");
        assert_eq!(index_status(state.as_ref()).await.unwrap(), "ready");
    }

    #[tokio::test]
    async fn announcement_key_change_rebuilds_cached_lanes() {
        let pool = setup_pool().await;
        seed_repo_association(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO social_activity_events (
              id, user_id, kind, repo_id, repo_full_name, discussion_number,
              title, body, html_url, actor_github_user_id, actor_login,
              occurred_at, detected_at, created_at, updated_at
            ) VALUES ('search-announcement-key', 'search-user', 'announcement', 42,
                      'octo/rill', NULL, 'Key migration announcement', 'announcement body',
                      'https://github.com/octo/rill', 99, 'octocat',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed announcement without discussion key");
        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status,
              title, summary, created_at, updated_at
            ) VALUES ('search-announcement-key-translation', 'search-user',
                      'announcement_detail', 'search-announcement-key', 'zh-CN',
                      'hash-announcement-key', 'ready', '讨论键迁移命中', '公告正文',
                      '2026-02-22T00:00:00Z', '2026-02-22T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed announcement translation before key");
        sqlx::query(
            "UPDATE social_activity_events SET discussion_number = 9, html_url = 'https://github.com/octo/rill/discussions/9' WHERE id = 'search-announcement-key'",
        )
        .execute(&pool)
        .await
        .expect("promote announcement to discussion key");

        let state = setup_state(pool.clone());
        let results = query(
            &state,
            "search-user",
            &parse_query("讨论键迁移").expect("parse migrated announcement query"),
        )
        .await
        .expect("query migrated announcement lane");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matched_lane, "translated");
        assert_eq!(results[0].target.href, "/octo/rill/discussions/9");
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
        let projected = sqlx::query_scalar::<_, Option<String>>(
            "SELECT translated_text FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read global projection text");
        assert_eq!(projected.as_deref(), Some("new projection text"));
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

        seed_repo_association(&pool).await;
        let after_association = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT repo_full_name, target_path FROM search_documents WHERE id = 'release:4201'",
        )
        .fetch_one(&pool)
        .await
        .expect("read release after association");
        assert_eq!(after_association.0.as_deref(), Some("octo/rill"));
        assert_eq!(
            after_association.1.as_deref(),
            Some("/octo/rill/releases/tag/v1.0.0")
        );
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
        assert_eq!(visible[0].target.href, "/octo/rill/releases/tag/v1.0.0");

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
        assert_eq!(recovered[0].target.href, "/octo/rill/releases/tag/v1.0.0");

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
        assert_eq!(renamed[0].target.href, "/octo/renamed/releases/tag/v1.0.0");
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
}
