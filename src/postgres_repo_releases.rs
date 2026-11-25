//! PostgreSQL release detail, announcement detail, and public release reads.
//!
//! These endpoints keep the legacy response contract while reading the shared
//! PostgreSQL runtime state: `public.repo_releases`, the
//! `public.user_release_visible_repos` visibility projection,
//! `public.public_repo_release_usage` publication state, and the
//! `public.ai_translations` translation cache. In global content-processing
//! mode the translated/detail/smart payloads come from
//! [`crate::postgres_content_overlay`] instead.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

use axum::{
    Json,
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder};
use tower_sessions::Session;

use crate::{
    error::ApiError,
    postgres_content_overlay,
    postgres_server::PostgresServerState,
    release_links::{
        InternalReleaseRef, ReleaseLocator, build_github_release_url_prefixes,
        parse_internal_release_ref, parse_release_locator_from_github_release_url,
        parse_repo_full_name_from_release_url,
    },
};

const SESSION_KEY_USER_ID: &str = "user_id";
const PUBLIC_RELEASE_HIGHLIGHT_ID_LIMIT: usize = 20;
const PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT: i64 = 30;
const RELEASE_DETAIL_MARKDOWN_MISMATCH_ERROR: &str =
    "release detail translation failed to preserve markdown structure";
const ANNOUNCEMENT_DETAIL_MARKDOWN_MISMATCH_ERROR: &str =
    "announcement detail translation failed to preserve markdown structure";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

fn invalid_api_key() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        "invalid_api_key",
        "invalid API key",
    )
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
    msg.to_ascii_lowercase().contains("rate limit")
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
        .and_then(|value| value.to_str().ok())
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
    if errors
        .iter()
        .any(|error| is_rate_limit_message(&error.message))
    {
        return Some(github_rate_limited_error());
    }
    if errors.iter().any(|error| is_reauth_message(&error.message)) {
        return Some(github_reauth_required_error());
    }
    if errors
        .iter()
        .any(|error| is_access_restricted_message(&error.message))
    {
        return Some(github_access_restricted_error());
    }
    None
}

// ---------------------------------------------------------------------------
// Session and API key authentication
// ---------------------------------------------------------------------------

async fn require_user_id(session: &Session) -> Result<String, ApiError> {
    match session.get::<String>(SESSION_KEY_USER_ID).await {
        Ok(Some(user_id)) => Ok(user_id),
        Ok(None) => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "not logged in",
        )),
        Err(error) => Err(ApiError::internal(error)),
    }
}

async fn require_active_user_id(
    state: &PostgresServerState,
    session: &Session,
) -> Result<String, ApiError> {
    let user_id = require_user_id(session).await?;
    let row = sqlx::query_as::<_, (i64, Option<String>)>(
        "SELECT is_disabled, paused_at FROM public.users WHERE id = $1",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some((is_disabled, paused_at)) = row else {
        let _ = session.clear().await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "session user not found",
        ));
    };
    if is_disabled != 0 {
        let _ = session.clear().await;
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }
    if paused_at.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_paused",
            "account is paused",
        ));
    }
    if let Err(error) = sqlx::query("UPDATE public.users SET last_active_at = $1 WHERE id = $2")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&user_id)
        .execute(&state.runtime.pool)
        .await
    {
        tracing::warn!(%error, user_id = %user_id, "PostgreSQL last_active_at touch failed");
    }
    Ok(user_id)
}

async fn authenticate_api_key(
    state: &PostgresServerState,
    api_key: &str,
) -> Result<String, ApiError> {
    if !api_key.starts_with(crate::api_keys::API_KEY_PREFIX) {
        return Err(invalid_api_key());
    }
    let key_hash = crate::api_keys::hash_api_key(api_key);
    let row = sqlx::query_as::<_, (String, i64, Option<String>)>(
        "SELECT k.user_id, u.is_disabled, u.paused_at FROM public.user_api_keys k JOIN public.users u ON u.id = k.user_id WHERE k.key_hash = $1 AND k.revoked_at IS NULL LIMIT 1",
    )
    .bind(&key_hash)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some((user_id, is_disabled, paused_at)) = row else {
        return Err(invalid_api_key());
    };
    if is_disabled != 0 {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_disabled",
            "account is disabled",
        ));
    }
    if paused_at.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account_paused",
            "account is paused",
        ));
    }
    Ok(user_id)
}

async fn require_business_user_id(
    state: &PostgresServerState,
    session: &Session,
    headers: &HeaderMap,
) -> Result<String, ApiError> {
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let value = value.to_str().map_err(|_| invalid_api_key())?;
        let value = value.trim_start();
        let mut auth_parts = value.splitn(2, char::is_whitespace);
        let scheme = auth_parts.next().unwrap_or("");
        if scheme.eq_ignore_ascii_case("Bearer") {
            let api_key = auth_parts.next().unwrap_or("").trim();
            if !api_key.starts_with(crate::api_keys::API_KEY_PREFIX) {
                return Err(invalid_api_key());
            }
            return authenticate_api_key(state, api_key).await;
        }
        return require_active_user_id(state, session).await;
    }
    require_active_user_id(state, session).await
}

async fn require_public_release_refresh_api_key(
    state: &PostgresServerState,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(invalid_api_key)?
        .to_str()
        .map_err(|_| invalid_api_key())?
        .trim_start();
    let mut auth_parts = value.splitn(2, char::is_whitespace);
    let scheme = auth_parts.next().unwrap_or("");
    let api_key = auth_parts.next().unwrap_or("").trim();
    if !scheme.eq_ignore_ascii_case("Bearer")
        || !api_key.starts_with(crate::api_keys::API_KEY_PREFIX)
    {
        return Err(invalid_api_key());
    }
    authenticate_api_key(state, api_key).await?;
    Ok(())
}

async fn user_has_brief_access_to_release(
    pool: &PgPool,
    user_id: &str,
    release_id: i64,
    locator: Option<&ReleaseLocator>,
) -> Result<bool, ApiError> {
    let membership_exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.brief_release_memberships m JOIN public.briefs b ON b.id = m.brief_id WHERE b.user_id = $1 AND m.release_id = $2",
    )
    .bind(user_id)
    .bind(release_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    if membership_exists > 0 {
        return Ok(true);
    }
    let briefs = sqlx::query_scalar::<_, String>(
        "SELECT content_markdown FROM public.briefs WHERE user_id = $1 AND generation_source IN ('legacy', 'history_recompute_failed', 'content_refresh_failed')",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(briefs
        .iter()
        .any(|markdown| brief_contains_release_link(markdown, release_id, locator)))
}

fn brief_contains_release_link(
    markdown: &str,
    release_id: i64,
    locator: Option<&ReleaseLocator>,
) -> bool {
    extract_brief_release_refs(markdown)
        .into_iter()
        .any(|candidate| match candidate {
            InternalReleaseRef::ReleaseId(candidate_id) => candidate_id == release_id,
            InternalReleaseRef::Locator(candidate_locator) => {
                locator.is_some_and(|expected_locator| candidate_locator.matches(expected_locator))
            }
        })
}

fn extract_brief_release_refs(markdown: &str) -> Vec<InternalReleaseRef> {
    let mut refs = Vec::new();
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
            if let Some(reference) = parse_internal_release_ref(target)
                && seen.insert(reference.clone())
            {
                refs.push(reference);
            }
            i = url_end + 1;
            continue;
        }
        let mut chars = rest.chars();
        let ch = chars.next().expect("rest is non-empty");
        i += ch.len_utf8();
    }
    refs
}

fn resolve_release_full_name(html_url: &str, repo_id: i64) -> String {
    parse_repo_full_name_from_release_url(html_url).unwrap_or_else(|| format!("unknown/{repo_id}"))
}

// ---------------------------------------------------------------------------
// Translation payload helpers
// ---------------------------------------------------------------------------

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
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let summary = parsed
        .summary_md
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    if title.is_none() && summary.is_none() {
        None
    } else {
        Some((title, summary))
    }
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

fn translation_error_metadata(
    error_text: Option<&str>,
) -> (Option<&'static str>, Option<&'static str>, Option<String>) {
    match crate::translation_error::classify_translation_error(error_text) {
        Some(classified) => (
            Some(classified.code),
            Some(classified.summary),
            Some(classified.detail),
        ),
        None => (None, None, None),
    }
}

pub(crate) fn translated_item_json(
    status: &str,
    title: Option<String>,
    summary: Option<String>,
    auto_translate: Option<bool>,
    error_text: Option<&str>,
    request_id: Option<String>,
) -> Value {
    let (error_code, error_summary, error_detail) = translation_error_metadata(error_text);
    let mut item = json!({
        "lang": "zh-CN",
        "status": status,
        "title": title,
        "summary": summary,
        "error_code": error_code,
        "error_summary": error_summary,
        "error_detail": error_detail,
    });
    if let Some(request_id) = request_id {
        item["request_id"] = json!(request_id);
    }
    if let Some(auto_translate) = auto_translate {
        item["auto_translate"] = json!(auto_translate);
    }
    item
}

fn smart_item_json(
    status: &str,
    title: Option<String>,
    summary: Option<String>,
    auto_translate: Option<bool>,
    error_text: Option<&str>,
    request_id: Option<String>,
    reading: Option<Value>,
) -> Value {
    let (error_code, error_summary, error_detail) = translation_error_metadata(error_text);
    let mut item = json!({
        "lang": "zh-CN",
        "status": status,
        "title": title,
        "summary": summary,
        "error_code": error_code,
        "error_summary": error_summary,
        "error_detail": error_detail,
    });
    if let Some(request_id) = request_id {
        item["request_id"] = json!(request_id);
    }
    if let Some(auto_translate) = auto_translate {
        item["auto_translate"] = json!(auto_translate);
    }
    if let Some(reading) = reading {
        item["reading"] = reading;
    }
    item
}

fn translated_terminal_item(
    status: &str,
    error_text: Option<&str>,
    request_id: Option<String>,
) -> Option<Value> {
    let window = request_id.clone();
    match status {
        "disabled" => Some(translated_item_json(
            "disabled", None, None, None, None, window,
        )),
        "missing" | "error" => Some(translated_item_json(
            status,
            None,
            None,
            Some(false),
            if status == "error" { error_text } else { None },
            window,
        )),
        "failed" => Some(translated_item_json(
            "error",
            None,
            None,
            Some(false),
            error_text,
            window,
        )),
        "queued" | "running" | "not_applicable" | "deferred_provider" | "blocked_config"
        | "cancelled" | "superseded" => Some(translated_item_json(
            status,
            None,
            None,
            Some(true),
            error_text,
            window,
        )),
        _ => None,
    }
}

fn translated_ready_item(
    raw_title: Option<String>,
    raw_summary: Option<String>,
    auto_translate: Option<bool>,
    request_id: Option<String>,
) -> Option<Value> {
    let (title, summary) = normalize_translation_fields(raw_title, raw_summary);
    if title.is_none() && summary.is_none() {
        return None;
    }
    Some(translated_item_json(
        "ready",
        title,
        summary,
        auto_translate,
        None,
        request_id,
    ))
}

fn translated_missing_item(auto_translate: bool, request_id: Option<String>) -> Value {
    translated_item_json(
        "missing",
        None,
        None,
        if auto_translate { None } else { Some(false) },
        None,
        request_id,
    )
}

/// Attach the legacy `request_id` / `auto_translate` metadata to a translated
/// item. Both fields are optional in the response contract and stay omitted
/// when unknown.
pub(crate) fn apply_translation_request_metadata(
    item: &mut Value,
    request_id: Option<String>,
    auto_translate: bool,
) {
    if let Some(request_id) = request_id {
        item["request_id"] = json!(request_id);
    }
    if auto_translate {
        item["auto_translate"] = json!(true);
    }
}

/// In-flight state of the legacy `ai_translations` cache for a release.
///
/// Returns `(translation, smart)`. A ready cache row with an active work item
/// also counts: that is the legacy refresh-in-flight projection, which keeps
/// showing the retained text while new work runs.
pub(crate) async fn legacy_release_in_flight(
    pool: &PgPool,
    user_id: Option<&str>,
    entity_id: &str,
) -> Result<(bool, bool), sqlx::Error> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT t.entity_type FROM public.ai_translations t \
         LEFT JOIN public.translation_work_items w ON w.id = t.active_work_item_id \
         WHERE t.entity_id = $1 \
           AND t.entity_type IN ('release_detail', 'release_smart') \
           AND t.lang = 'zh-CN' \
           AND ($2::text IS NULL OR t.user_id = $2) \
           AND (t.status IN ('queued', 'running') \
                OR w.status IN ('queued', 'batched', 'running'))",
    )
    .bind(entity_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok((
        rows.iter()
            .any(|entity_type| entity_type == "release_detail"),
        rows.iter()
            .any(|entity_type| entity_type == "release_smart"),
    ))
}

fn smart_terminal_item(
    status: &str,
    error_text: Option<&str>,
    request_id: Option<String>,
) -> Option<Value> {
    let window = request_id.clone();
    match status {
        "disabled" => Some(smart_item_json(
            "disabled", None, None, None, None, window, None,
        )),
        "insufficient" => Some(smart_item_json(
            "insufficient",
            None,
            None,
            Some(false),
            None,
            window,
            None,
        )),
        "missing"
            if error_text == Some(crate::translation_error::SMART_NO_VALUABLE_VERSION_INFO) =>
        {
            Some(smart_item_json(
                "insufficient",
                None,
                None,
                Some(false),
                None,
                window,
                None,
            ))
        }
        "missing" | "error" | "incompatible" => Some(smart_item_json(
            status,
            None,
            None,
            Some(false),
            error_text,
            window,
            None,
        )),
        "failed" => Some(smart_item_json(
            "error",
            None,
            None,
            Some(false),
            error_text,
            window,
            None,
        )),
        "queued" | "running" | "not_applicable" | "deferred_provider" | "blocked_config"
        | "cancelled" | "superseded" => Some(smart_item_json(
            status,
            None,
            None,
            Some(true),
            error_text,
            window,
            None,
        )),
        _ => None,
    }
}

fn smart_ready_item(
    raw_title: Option<String>,
    raw_summary: Option<String>,
    auto_translate: Option<bool>,
    request_id: Option<String>,
) -> Option<Value> {
    let (title, summary) = normalize_translation_fields(raw_title, raw_summary);
    summary.as_ref()?;
    Some(smart_item_json(
        "ready",
        title,
        summary,
        auto_translate,
        None,
        request_id,
        None,
    ))
}

fn smart_ready_item_with_reading(
    raw_title: Option<String>,
    raw_summary: Option<String>,
    reading: Option<Value>,
    auto_translate: Option<bool>,
    request_id: Option<String>,
) -> Option<Value> {
    let (title, summary) = normalize_translation_fields(raw_title, raw_summary);
    if title.is_none() && summary.is_none() && reading.is_none() {
        return None;
    }
    Some(smart_item_json(
        "ready",
        title,
        summary,
        auto_translate,
        None,
        request_id,
        reading,
    ))
}

fn smart_missing_item(auto_translate: Option<bool>, request_id: Option<String>) -> Value {
    smart_item_json(
        "missing",
        None,
        None,
        auto_translate,
        None,
        request_id,
        None,
    )
}

fn release_reading_state(payload: &Value) -> (Option<Value>, bool) {
    let Some(raw_reading) = payload.get("reading") else {
        return (None, false);
    };
    let Ok(mut reading) = serde_json::from_value::<crate::translation_types::ReleaseReadingResult>(
        raw_reading.clone(),
    ) else {
        return (None, true);
    };
    if reading.normalize_references().is_err() || reading.validate().is_err() {
        return (None, true);
    }
    let changes = reading
        .changes
        .into_iter()
        .map(|change| {
            let references = change
                .references
                .into_iter()
                .filter_map(|reference| {
                    let number =
                        crate::translation_types::release_reading_pr_number(&reference.url);
                    let label = reference
                        .label
                        .or_else(|| number.map(|number| format!("PR#{number}")))?;
                    let mut view = json!({"label": label, "url": reference.url});
                    if let Some(number) = number {
                        view["number"] = json!(number);
                    }
                    Some(view)
                })
                .collect::<Vec<_>>();
            let mut view = json!({"text": change.text});
            if !change.versions.is_empty() {
                view["versions"] = json!(change.versions);
            }
            if !references.is_empty() {
                view["references"] = json!(references);
            }
            view
        })
        .collect::<Vec<_>>();
    let mut view = json!({
        "version": reading.version,
        "outcome": reading.outcome,
        "title": reading.title,
        "changes": changes,
    });
    if !reading.machine_materials.is_empty() {
        view["machine_materials"] = json!(
            reading
                .machine_materials
                .into_iter()
                .map(|material| json!({"label": material.label, "value": material.value}))
                .collect::<Vec<_>>()
        );
    }
    (Some(view), false)
}

fn smart_status_for_reading(
    status: &str,
    reading: Option<&Value>,
    reading_incompatible: bool,
) -> String {
    if status == "ready" && reading_incompatible {
        "incompatible".to_owned()
    } else if status == "ready"
        && reading
            .and_then(|reading| reading.get("outcome"))
            .and_then(Value::as_str)
            == Some("insufficient")
    {
        "insufficient".to_owned()
    } else {
        status.to_owned()
    }
}

fn repo_visual_json(
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: bool,
) -> Option<Value> {
    let normalize = |raw: Option<String>| {
        raw.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        })
    };
    let owner_avatar_url = normalize(owner_avatar_url);
    let open_graph_image_url = normalize(open_graph_image_url);
    if owner_avatar_url.is_none() && open_graph_image_url.is_none() && !uses_custom_open_graph_image
    {
        return None;
    }
    Some(json!({
        "owner_avatar_url": owner_avatar_url,
        "open_graph_image_url": open_graph_image_url,
        "uses_custom_open_graph_image": uses_custom_open_graph_image,
    }))
}

fn release_detail_translation_ready(body: Option<&str>, summary: Option<&str>) -> bool {
    let body_has_content = body.map(str::trim).is_some_and(|s| !s.is_empty());
    if !body_has_content {
        return true;
    }
    summary.map(str::trim).is_some_and(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Authenticated release detail
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
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
    previous_tag_name: Option<String>,
    trans_source_hash: Option<String>,
    trans_status: Option<String>,
    trans_title: Option<String>,
    trans_summary: Option<String>,
    trans_error_text: Option<String>,
    trans_work_status: Option<String>,
    smart_source_hash: Option<String>,
    smart_status: Option<String>,
    smart_title: Option<String>,
    smart_summary: Option<String>,
    smart_error_text: Option<String>,
    smart_work_status: Option<String>,
}

const RELEASE_DETAIL_SELECT: &str = r#"
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
          rp.previous_tag_name,
          t.source_hash AS trans_source_hash,
          t.status AS trans_status,
          t.title AS trans_title,
          t.summary AS trans_summary,
          t.error_text AS trans_error_text,
          tw.status AS trans_work_status,
          s.source_hash AS smart_source_hash,
          s.status AS smart_status,
          s.title AS smart_title,
          s.summary AS smart_summary,
          s.error_text AS smart_error_text,
          sw.status AS smart_work_status
        FROM public.repo_releases r
        LEFT JOIN (
          SELECT
            release_id,
            LAG(tag_name) OVER (
              PARTITION BY repo_id
              ORDER BY COALESCE(published_at, created_at, updated_at) ASC, release_id ASC
            ) AS previous_tag_name
          FROM public.repo_releases
        ) rp
          ON rp.release_id = r.release_id
        LEFT JOIN public.user_release_visible_repos sr
          ON sr.user_id = $1 AND sr.repo_id = r.repo_id
        LEFT JOIN public.ai_translations t
          ON t.user_id = $1
          AND t.entity_type = 'release_detail'
          AND t.entity_id = r.release_id::text
          AND t.lang = 'zh-CN'
          AND t.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN public.translation_work_items tw
          ON tw.id = t.active_work_item_id
        LEFT JOIN public.ai_translations s
          ON s.user_id = $1
          AND s.entity_type = 'release_smart'
          AND s.entity_id = r.release_id::text
          AND s.lang = 'zh-CN'
          AND s.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN public.translation_work_items sw
          ON sw.id = s.active_work_item_id
"#;

async fn fetch_release_detail_row_by_locator(
    pool: &PgPool,
    user_id: &str,
    locator: &ReleaseLocator,
) -> Result<Option<ReleaseDetailRow>, ApiError> {
    let [github_prefix, www_prefix] = build_github_release_url_prefixes(locator);
    let sql = format!(
        "{RELEASE_DETAIL_SELECT} WHERE r.tag_name = $2 AND (strpos(lower(r.html_url), $3) = 1 OR strpos(lower(r.html_url), $4) = 1) ORDER BY r.published_at DESC, r.created_at DESC, r.release_id DESC LIMIT 1"
    );
    sqlx::query_as::<_, ReleaseDetailRow>(&sql)
        .bind(user_id)
        .bind(&locator.tag)
        .bind(github_prefix.to_ascii_lowercase())
        .bind(www_prefix.to_ascii_lowercase())
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)
}

async fn build_release_detail_response(
    state: &PostgresServerState,
    user_id: &str,
    row: ReleaseDetailRow,
) -> Result<Value, ApiError> {
    let locator = parse_release_locator_from_github_release_url(&row.html_url);
    if row.repo_full_name.is_none()
        && !user_has_brief_access_to_release(
            &state.runtime.pool,
            user_id,
            row.release_id,
            locator.as_ref(),
        )
        .await?
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
    let overlay = postgres_content_overlay::OverlayState::load(&state.runtime.pool).await;

    let mut translated: Option<Value>;
    if overlay.is_global() {
        let source = postgres_content_overlay::ReleaseSource {
            release_id: row.release_id,
            repo_full_name: &resolved_full_name,
            repo_identity: &resolved_full_name,
            tag_name: &row.tag_name,
            name: row.name.as_deref(),
            body: row.body.as_deref(),
            previous_tag_name: row.previous_tag_name.as_deref(),
        };
        let audience =
            postgres_content_overlay::release_audience(&state.runtime.pool, user_id, row.repo_id)
                .await
                .map_err(ApiError::internal)?;
        let processing = postgres_content_overlay::release_detail_processing(
            &state.runtime.pool,
            overlay,
            &source,
            audience,
        )
        .await
        .map_err(ApiError::internal)?;
        let translation = processing
            .translation
            .map(|projection| (projection.status, projection.payload));
        let smart_projection = processing
            .smart
            .map(|projection| (projection.status, projection.payload));
        let translation_status = translation.as_ref().map(|(status, _)| status.clone());
        let smart_status = smart_projection.as_ref().map(|(status, _)| status.clone());
        // The legacy detail reader named the request that produced the served
        // payload and flagged queued/running projections as auto-translated.
        let release_key = row.release_id.to_string();
        let translation_request_id = postgres_content_overlay::latest_translation_request_id(
            &state.runtime.pool,
            release_key.as_str(),
            &postgres_content_overlay::RELEASE_TRANSLATION_REQUEST_KINDS,
            None,
        )
        .await
        .map_err(ApiError::internal)?;
        let smart_request_id = postgres_content_overlay::latest_translation_request_id(
            &state.runtime.pool,
            release_key.as_str(),
            &postgres_content_overlay::RELEASE_SMART_REQUEST_KINDS,
            None,
        )
        .await
        .map_err(ApiError::internal)?;
        let translation_in_flight = translation_status
            .as_deref()
            .is_some_and(postgres_content_overlay::is_active_work_status);
        let smart_in_flight = smart_projection
            .as_ref()
            .is_some_and(|(status, _)| postgres_content_overlay::is_active_work_status(status));
        let project_string = |project: &Option<(String, Value)>, key: &str| {
            project
                .as_ref()
                .and_then(|(_, payload)| payload.get(key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        translated = match translation_status.as_deref() {
            Some("ready") => {
                let title = project_string(&translation, "title_zh");
                let summary = project_string(&translation, "body_md")
                    .or_else(|| project_string(&translation, "summary_md"));
                if release_detail_translation_ready(
                    Some(original_body.as_str()),
                    summary.as_deref(),
                ) {
                    Some(translated_item_json(
                        "ready", title, summary, None, None, None,
                    ))
                } else {
                    Some(translated_item_json(
                        "error",
                        None,
                        None,
                        Some(false),
                        Some(RELEASE_DETAIL_MARKDOWN_MISMATCH_ERROR),
                        None,
                    ))
                }
            }
            Some("queued" | "running" | "deferred_provider" | "blocked_config") => {
                let title = project_string(&translation, "title_zh");
                let summary = project_string(&translation, "body_md")
                    .or_else(|| project_string(&translation, "summary_md"));
                translated_ready_item(title, summary, Some(true), None)
                    .or_else(|| Some(translated_missing_item(true, None)))
            }
            Some(status) => translated_terminal_item(status, None, None).or_else(|| {
                Some(translated_item_json(
                    status,
                    None,
                    None,
                    Some(false),
                    None,
                    None,
                ))
            }),
            None => Some(translated_missing_item(true, None)),
        };
        let (smart_reading, smart_reading_incompatible) = smart_projection
            .as_ref()
            .map(|(_, payload)| release_reading_state(payload))
            .unwrap_or((None, false));
        let smart_status = smart_status.map(|status| {
            smart_status_for_reading(
                status.as_str(),
                smart_reading.as_ref(),
                smart_reading_incompatible,
            )
        });
        let mut smart = match smart_status.as_deref() {
            Some("ready") => smart_ready_item_with_reading(
                project_string(&smart_projection, "title_zh"),
                project_string(&smart_projection, "summary_md"),
                smart_reading.clone(),
                None,
                None,
            )
            .or_else(|| Some(smart_missing_item(None, None))),
            Some("queued" | "running" | "deferred_provider" | "blocked_config") => {
                smart_ready_item(
                    project_string(&smart_projection, "title_zh"),
                    project_string(&smart_projection, "summary_md"),
                    Some(true),
                    None,
                )
                .or_else(|| Some(smart_missing_item(Some(true), None)))
            }
            Some(status) => smart_terminal_item(status, None, None).or_else(|| {
                Some(smart_item_json(
                    status,
                    None,
                    None,
                    Some(false),
                    None,
                    None,
                    None,
                ))
            }),
            None => Some(smart_missing_item(None, None)),
        };
        if let (Some(smart), Some(reading)) = (smart.as_mut(), smart_reading)
            && smart["status"] == json!("ready")
        {
            smart["reading"] = reading;
        }
        if let Some(item) = translated.as_mut() {
            apply_translation_request_metadata(item, translation_request_id, translation_in_flight);
        }
        if let Some(item) = smart.as_mut() {
            apply_translation_request_metadata(item, smart_request_id, smart_in_flight);
        }
        return Ok(release_detail_body(
            row,
            resolved_full_name,
            translated,
            smart,
        ));
    }

    let source_hash = crate::content_source_identity::release_detail_source_hash(
        &resolved_full_name,
        original_title.as_str(),
        original_body.as_str(),
    );
    let smart_body = crate::release_content::release_feed_body(row.body.as_deref());
    let smart_source_hash = crate::content_source_identity::release_smart_feed_source_hash(
        row.release_id.to_string().as_str(),
        &resolved_full_name,
        &original_title,
        smart_body.as_deref(),
        &row.tag_name,
        row.previous_tag_name.as_deref(),
    );
    let translation_fresh = row.trans_source_hash.as_deref() == Some(source_hash.as_str());
    let smart_fresh = row.smart_source_hash.as_deref() == Some(smart_source_hash.as_str());
    let refresh_in_flight = !translation_fresh
        && row.trans_status.as_deref() == Some("ready")
        && matches!(
            row.trans_work_status.as_deref(),
            Some("queued" | "batched" | "running")
        );
    let smart_refresh_in_flight = !smart_fresh
        && row.smart_status.as_deref() == Some("ready")
        && matches!(
            row.smart_work_status.as_deref(),
            Some("queued" | "batched" | "running")
        );

    let mut translated = if state.ai_model.is_none() {
        Some(translated_item_json(
            "disabled", None, None, None, None, None,
        ))
    } else {
        match (translation_fresh, row.trans_status.as_deref()) {
            (true, Some("ready"))
                if release_detail_translation_ready(
                    Some(original_body.as_str()),
                    row.trans_summary.as_deref(),
                ) =>
            {
                Some(translated_item_json(
                    "ready",
                    row.trans_title.clone(),
                    row.trans_summary.clone(),
                    None,
                    None,
                    None,
                ))
            }
            (true, Some("ready")) => Some(translated_item_json(
                "error",
                None,
                None,
                None,
                Some(RELEASE_DETAIL_MARKDOWN_MISMATCH_ERROR),
                None,
            )),
            (false, Some("ready")) if refresh_in_flight => translated_ready_item(
                row.trans_title.clone(),
                row.trans_summary.clone(),
                None,
                None,
            )
            .or_else(|| Some(translated_missing_item(false, None))),
            (true, Some(status)) => {
                translated_terminal_item(status, row.trans_error_text.as_deref(), None)
            }
            _ => Some(translated_missing_item(true, None)),
        }
    };

    let mut smart = if state.ai_model.is_none() {
        Some(smart_item_json(
            "disabled", None, None, None, None, None, None,
        ))
    } else if smart_fresh {
        if let Some(status) = row.smart_status.as_deref()
            && status != "ready"
        {
            smart_terminal_item(status, row.smart_error_text.as_deref(), None)
        } else {
            smart_ready_item(
                row.smart_title.clone(),
                row.smart_summary.clone(),
                None,
                None,
            )
            .or_else(|| Some(smart_missing_item(None, None)))
        }
    } else if smart_refresh_in_flight {
        smart_ready_item(
            row.smart_title.clone(),
            row.smart_summary.clone(),
            Some(true),
            None,
        )
        .or_else(|| Some(smart_missing_item(Some(false), None)))
    } else {
        Some(smart_missing_item(None, None))
    };

    let release_key = row.release_id.to_string();
    let translation_request_id = postgres_content_overlay::latest_translation_request_id(
        &state.runtime.pool,
        release_key.as_str(),
        &postgres_content_overlay::RELEASE_TRANSLATION_REQUEST_KINDS,
        Some(user_id),
    )
    .await
    .map_err(ApiError::internal)?;
    let smart_request_id = postgres_content_overlay::latest_translation_request_id(
        &state.runtime.pool,
        release_key.as_str(),
        &postgres_content_overlay::RELEASE_SMART_REQUEST_KINDS,
        Some(user_id),
    )
    .await
    .map_err(ApiError::internal)?;
    let (translation_in_flight, smart_in_flight) =
        legacy_release_in_flight(&state.runtime.pool, Some(user_id), release_key.as_str())
            .await
            .map_err(ApiError::internal)?;
    if let Some(item) = translated.as_mut() {
        apply_translation_request_metadata(
            item,
            translation_request_id,
            translation_in_flight || refresh_in_flight,
        );
    }
    if let Some(item) = smart.as_mut() {
        apply_translation_request_metadata(
            item,
            smart_request_id,
            smart_in_flight || smart_refresh_in_flight,
        );
    }

    Ok(release_detail_body(
        row,
        resolved_full_name,
        translated,
        smart,
    ))
}

fn release_detail_body(
    row: ReleaseDetailRow,
    resolved_full_name: String,
    translated: Option<Value>,
    smart: Option<Value>,
) -> Value {
    let repo_visual = repo_visual_json(
        row.owner_avatar_url,
        row.open_graph_image_url,
        row.uses_custom_open_graph_image.unwrap_or(0) != 0,
    );
    json!({
        "release_id": row.release_id.to_string(),
        "repo_full_name": row.repo_full_name.or(Some(resolved_full_name)),
        "repo_visual": repo_visual,
        "tag_name": row.tag_name,
        "previous_tag_name": row.previous_tag_name,
        "name": row.name,
        "body": row.body,
        "html_url": row.html_url,
        "published_at": row.published_at,
        "is_prerelease": row.is_prerelease,
        "is_draft": row.is_draft,
        "translated": translated,
        "smart": smart,
    })
}

pub(crate) async fn get_release_detail_by_repo_tag(
    State(state): State<PostgresServerState>,
    session: Session,
    headers: HeaderMap,
    Path((owner_raw, repo_raw, tag_raw)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let user_id = require_business_user_id(&state, &session, &headers).await?;
    let locator = ReleaseLocator {
        owner: owner_raw,
        repo: repo_raw,
        tag: tag_raw,
    };
    let row = fetch_release_detail_row_by_locator(&state.runtime.pool, &user_id, &locator)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "release not found"))?;
    let body = build_release_detail_response(&state, user_id.as_str(), row).await?;
    Ok(Json(body).into_response())
}

// ---------------------------------------------------------------------------
// Announcement detail
// ---------------------------------------------------------------------------

#[derive(Debug, FromRow)]
struct AnnouncementDetailDbRow {
    repo_full_name: Option<String>,
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: Option<i64>,
    discussion_number: Option<i64>,
    title: Option<String>,
    body: Option<String>,
    html_url: Option<String>,
    occurred_at: Option<String>,
    actor_login: Option<String>,
    actor_avatar_url: Option<String>,
    actor_html_url: Option<String>,
}

#[derive(Debug, Clone)]
struct AnnouncementDetailSource {
    repo_full_name: String,
    discussion_number: i64,
    repo_visual: Option<Value>,
    title: String,
    body: Option<String>,
    html_url: String,
    occurred_at: Option<String>,
    actor: Option<Value>,
}

fn parse_discussion_number_param(raw: &str) -> Result<i64, ApiError> {
    let discussion_number = raw
        .trim()
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("discussion number must be an integer"))?;
    if discussion_number <= 0 {
        return Err(ApiError::bad_request(
            "discussion number must be a positive integer",
        ));
    }
    Ok(discussion_number)
}

async fn fetch_announcement_detail_source_from_db(
    pool: &PgPool,
    user_id: &str,
    owner: &str,
    repo: &str,
    discussion_number: i64,
) -> Result<Option<AnnouncementDetailSource>, ApiError> {
    let repo_full_name = format!("{owner}/{repo}");
    let row = sqlx::query_as::<_, AnnouncementDetailDbRow>(
        r#"
        SELECT
          e.repo_full_name,
          e.repo_owner_avatar_url AS owner_avatar_url,
          e.repo_open_graph_image_url AS open_graph_image_url,
          e.repo_uses_custom_open_graph_image AS uses_custom_open_graph_image,
          e.discussion_number,
          e.title,
          e.body,
          e.html_url,
          e.occurred_at,
          e.actor_login,
          e.actor_avatar_url,
          e.actor_html_url
        FROM public.social_activity_events e
        WHERE e.user_id = $1
          AND e.kind = 'announcement'
          AND lower(e.repo_full_name) = lower($2)
          AND e.discussion_number = $3
        ORDER BY e.occurred_at DESC, COALESCE(e.title, '') DESC, COALESCE(e.body, '') DESC, e.id DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(repo_full_name.as_str())
    .bind(discussion_number)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(row.map(|row| {
        let resolved_repo_full_name = row.repo_full_name.unwrap_or(repo_full_name);
        let resolved_discussion_number = row
            .discussion_number
            .or_else(|| {
                row.html_url.as_deref().and_then(
                    crate::content_source_identity::parse_discussion_number_from_github_url,
                )
            })
            .unwrap_or(discussion_number);
        let title = row
            .title
            .unwrap_or_else(|| format!("Discussion #{discussion_number}"));
        AnnouncementDetailSource {
            repo_full_name: resolved_repo_full_name,
            discussion_number: resolved_discussion_number,
            repo_visual: repo_visual_json(
                row.owner_avatar_url,
                row.open_graph_image_url,
                row.uses_custom_open_graph_image.unwrap_or(0) != 0,
            ),
            title,
            body: row.body,
            html_url: row.html_url.unwrap_or_else(|| {
                format!("https://github.com/{owner}/{repo}/discussions/{discussion_number}")
            }),
            occurred_at: row.occurred_at,
            actor: row.actor_login.map(|login| {
                json!({
                    "login": login,
                    "avatar_url": row.actor_avatar_url,
                    "html_url": row.actor_html_url,
                })
            }),
        }
    }))
}

fn graphql_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
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
struct GraphQlAnnouncementDetailData {
    repository: Option<GraphQlAnnouncementRepository>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlAnnouncementRepository {
    name_with_owner: Option<String>,
    open_graph_image_url: Option<String>,
    owner: Option<GraphQlAnnouncementRepositoryOwner>,
    discussion: Option<GraphQlAnnouncementNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlAnnouncementRepositoryOwner {
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlAnnouncementNode {
    number: i64,
    title: String,
    body: Option<String>,
    url: String,
    created_at: Option<String>,
    updated_at: Option<String>,
    category: Option<GraphQlAnnouncementCategory>,
    author: Option<GraphQlAnnouncementAuthor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlAnnouncementCategory {
    name: Option<String>,
    slug: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlAnnouncementAuthor {
    login: Option<String>,
    avatar_url: Option<String>,
    url: Option<String>,
}

fn is_announcement_category_match(slug: Option<&str>, name: Option<&str>) -> bool {
    let slug = slug.unwrap_or_default().trim().to_ascii_lowercase();
    let name = name.unwrap_or_default().trim().to_ascii_lowercase();
    matches!(slug.as_str(), "announcement" | "announcements")
        || matches!(name.as_str(), "announcement" | "announcements")
}

async fn fetch_live_announcement_detail_request(
    state: &PostgresServerState,
    access_token: &str,
    owner: &str,
    repo: &str,
    discussion_number: i64,
) -> Result<Option<AnnouncementDetailSource>, ApiError> {
    let query = format!(
        r#"
        query {{
          repository(owner: {owner}, name: {repo}) {{
            nameWithOwner
            openGraphImageUrl
            owner {{
              avatarUrl
            }}
            discussion(number: {discussion_number}) {{
              number
              title
              body
              url
              createdAt
              updatedAt
              category {{
                name
                slug
              }}
              author {{
                login
                avatarUrl
                url
              }}
            }}
          }}
        }}
        "#,
        owner = graphql_string_literal(owner),
        repo = graphql_string_literal(repo),
        discussion_number = discussion_number,
    );

    let response = state
        .http
        .post(crate::github_api::graphql_url())
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&json!({ "query": query }))
        .send()
        .await
        .map_err(ApiError::internal)?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let headers = response.headers().clone();
        let body = response.text().await.map_err(ApiError::internal)?;
        if let Some(err) = github_graphql_http_error(status, &headers, &body) {
            return Err(err);
        }
        return Err(ApiError::internal(format!(
            "github graphql returned {status}: {body}"
        )));
    }

    let response = response.error_for_status().map_err(ApiError::internal)?;
    let body = response.text().await.map_err(ApiError::internal)?;
    let parsed = serde_json::from_str::<GraphQlResponse<GraphQlAnnouncementDetailData>>(&body)
        .map_err(ApiError::internal)?;
    if let Some(errors) = parsed.errors
        && !errors.is_empty()
    {
        if let Some(err) = github_graphql_errors_to_api_error(&errors) {
            return Err(err);
        }
        let message = errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ApiError::internal(format!(
            "github graphql error: {message}"
        )));
    }

    let Some(repository) = parsed.data.and_then(|data| data.repository) else {
        return Ok(None);
    };
    let Some(discussion) = repository.discussion else {
        return Ok(None);
    };
    if !is_announcement_category_match(
        discussion
            .category
            .as_ref()
            .and_then(|category| category.slug.as_deref()),
        discussion
            .category
            .as_ref()
            .and_then(|category| category.name.as_deref()),
    ) {
        return Ok(None);
    }

    let repo_full_name = repository
        .name_with_owner
        .unwrap_or_else(|| format!("{owner}/{repo}"));
    Ok(Some(AnnouncementDetailSource {
        repo_full_name,
        discussion_number: discussion.number,
        repo_visual: repo_visual_json(
            repository.owner.and_then(|owner| owner.avatar_url),
            repository.open_graph_image_url,
            false,
        ),
        title: discussion.title,
        body: discussion.body,
        html_url: discussion.url,
        occurred_at: discussion.updated_at.or(discussion.created_at),
        actor: discussion.author.and_then(|author| {
            let login = author.login?.trim().to_owned();
            if login.is_empty() {
                return None;
            }
            Some(json!({
                "login": login,
                "avatar_url": author.avatar_url,
                "html_url": author.url,
            }))
        }),
    }))
}

async fn fetch_live_announcement_detail_for_user(
    state: &PostgresServerState,
    user_id: &str,
    owner: &str,
    repo: &str,
    discussion_number: i64,
) -> Result<Option<AnnouncementDetailSource>, ApiError> {
    let rows = sqlx::query_as::<_, (String, Vec<u8>, Vec<u8>)>(
        "SELECT id, access_token_ciphertext, access_token_nonce FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC",
    )
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|error| ApiError::internal(format!("load github connections failed: {error}")))?;

    let mut last_auth_error: Option<ApiError> = None;
    for (connection_id, ciphertext, nonce) in rows {
        let access_token = match state.encryption_key.decrypt_str(&ciphertext, &nonce) {
            Ok(token) => token,
            Err(error) => {
                return Err(ApiError::internal(format!(
                    "load github connections failed: decrypt GitHub connection {connection_id}: {error}"
                )));
            }
        };
        match fetch_live_announcement_detail_request(
            state,
            access_token.as_str(),
            owner,
            repo,
            discussion_number,
        )
        .await
        {
            Ok(detail) => return Ok(detail),
            Err(err) if matches!(err.code(), "reauth_required" | "forbidden") => {
                last_auth_error = Some(err);
            }
            Err(err) => return Err(err),
        }
    }

    if let Some(err) = last_auth_error {
        return Err(err);
    }
    Ok(None)
}

#[derive(Debug, Default, FromRow)]
struct AnnouncementTranslationStateRow {
    trans_source_hash: Option<String>,
    trans_status: Option<String>,
    trans_title: Option<String>,
    trans_summary: Option<String>,
    trans_error_text: Option<String>,
    trans_work_status: Option<String>,
    smart_source_hash: Option<String>,
    smart_status: Option<String>,
    smart_title: Option<String>,
    smart_summary: Option<String>,
    smart_error_text: Option<String>,
    smart_work_status: Option<String>,
}

async fn load_announcement_translation_state(
    pool: &PgPool,
    user_id: &str,
    discussion_key: &str,
) -> Result<AnnouncementTranslationStateRow, ApiError> {
    let row = sqlx::query_as::<_, AnnouncementTranslationStateRow>(
        r#"
        SELECT
          t.source_hash AS trans_source_hash,
          t.status AS trans_status,
          t.title AS trans_title,
          t.summary AS trans_summary,
          t.error_text AS trans_error_text,
          tw.status AS trans_work_status,
          s.source_hash AS smart_source_hash,
          s.status AS smart_status,
          s.title AS smart_title,
          s.summary AS smart_summary,
          s.error_text AS smart_error_text,
          sw.status AS smart_work_status
        FROM (SELECT 1) seed
        LEFT JOIN public.ai_translations t
          ON t.user_id = $1
          AND t.entity_type = 'announcement_detail'
          AND t.entity_id = $2
          AND t.lang = 'zh-CN'
          AND t.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN public.translation_work_items tw
          ON tw.id = t.active_work_item_id
        LEFT JOIN public.ai_translations s
          ON s.user_id = $1
          AND s.entity_type = 'announcement_smart'
          AND s.entity_id = $2
          AND s.lang = 'zh-CN'
          AND s.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN public.translation_work_items sw
          ON sw.id = s.active_work_item_id
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(discussion_key)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(row.unwrap_or_default())
}

async fn build_announcement_detail_response(
    state: &PostgresServerState,
    user_id: &str,
    source: AnnouncementDetailSource,
) -> Result<Value, ApiError> {
    let discussion_key = crate::content_source_identity::announcement_discussion_key(
        &source.repo_full_name,
        source.discussion_number,
    );
    let original_body = source.body.clone().unwrap_or_default();
    let overlay = postgres_content_overlay::OverlayState::load(&state.runtime.pool).await;

    let translated: Option<Value>;
    let smart: Option<Value>;
    if overlay.is_global() {
        let announcement = postgres_content_overlay::AnnouncementSource {
            discussion_key: discussion_key.as_str(),
            repo_full_name: &source.repo_full_name,
            discussion_number: source.discussion_number,
            title: source.title.as_str(),
            body: source.body.as_deref(),
        };
        let processing = postgres_content_overlay::announcement_detail_processing(
            &state.runtime.pool,
            overlay,
            &announcement,
        )
        .await
        .map_err(ApiError::internal)?;
        let translation = processing
            .translation
            .map(|projection| (projection.status, projection.payload));
        let smart_projection = processing
            .smart
            .map(|projection| (projection.status, projection.payload));
        let project_string = |project: &Option<(String, Value)>, key: &str| {
            project
                .as_ref()
                .and_then(|(_, payload)| payload.get(key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let project_status =
            |project: &Option<(String, Value)>| project.as_ref().map(|(status, _)| status.clone());
        translated = match project_status(&translation).as_deref() {
            Some("ready") => Some(translated_item_json(
                "ready",
                project_string(&translation, "title_zh"),
                project_string(&translation, "body_md")
                    .or_else(|| project_string(&translation, "summary_md")),
                None,
                None,
                None,
            )),
            Some("queued" | "running" | "deferred_provider")
                if project_string(&translation, "body_md").is_some()
                    || project_string(&translation, "summary_md").is_some() =>
            {
                Some(translated_item_json(
                    "ready",
                    project_string(&translation, "title_zh"),
                    project_string(&translation, "body_md")
                        .or_else(|| project_string(&translation, "summary_md")),
                    Some(true),
                    None,
                    None,
                ))
            }
            Some("failed") => Some(translated_item_json(
                "error",
                None,
                None,
                Some(false),
                Some("global content processing failed"),
                None,
            )),
            Some("queued" | "running" | "deferred_provider" | "blocked_config") => {
                Some(translated_missing_item(true, None))
            }
            Some(status) => translated_terminal_item(status, None, None),
            None => Some(translated_missing_item(true, None)),
        };
        smart = match project_status(&smart_projection).as_deref() {
            Some("ready") => Some(smart_item_json(
                "ready",
                project_string(&smart_projection, "title_zh"),
                project_string(&smart_projection, "summary_md"),
                None,
                None,
                None,
                None,
            )),
            Some("queued" | "running" | "deferred_provider")
                if project_string(&smart_projection, "summary_md").is_some() =>
            {
                Some(smart_item_json(
                    "ready",
                    project_string(&smart_projection, "title_zh"),
                    project_string(&smart_projection, "summary_md"),
                    Some(true),
                    None,
                    None,
                    None,
                ))
            }
            Some("failed") => Some(smart_item_json(
                "error",
                None,
                None,
                Some(false),
                Some("global content processing failed"),
                None,
                None,
            )),
            Some("queued" | "running" | "deferred_provider" | "blocked_config") => {
                Some(smart_missing_item(Some(true), None))
            }
            Some(status) => smart_terminal_item(status, None, None),
            None => Some(smart_missing_item(None, None)),
        };
        return Ok(announcement_detail_body(
            source,
            discussion_key,
            translated,
            smart,
        ));
    }

    let translation_state =
        load_announcement_translation_state(&state.runtime.pool, user_id, discussion_key.as_str())
            .await?;
    let source_hash = crate::content_source_identity::announcement_detail_source_hash(
        &source.repo_full_name,
        Some(source.discussion_number),
        source.title.as_str(),
        original_body.as_str(),
    );
    let smart_source_hash = crate::content_source_identity::announcement_smart_feed_source_hash(
        discussion_key.as_str(),
        source.repo_full_name.as_str(),
        Some(source.discussion_number),
        source.title.as_str(),
        source.body.as_deref(),
    );
    let translation_fresh =
        translation_state.trans_source_hash.as_deref() == Some(source_hash.as_str());
    let smart_fresh =
        translation_state.smart_source_hash.as_deref() == Some(smart_source_hash.as_str());
    let translation_refresh_in_flight = !translation_fresh
        && translation_state.trans_status.as_deref() == Some("ready")
        && matches!(
            translation_state.trans_work_status.as_deref(),
            Some("queued" | "batched" | "running")
        );
    let smart_refresh_in_flight = !smart_fresh
        && translation_state.smart_status.as_deref() == Some("ready")
        && matches!(
            translation_state.smart_work_status.as_deref(),
            Some("queued" | "batched" | "running")
        );

    let translated = if state.ai_model.is_none() {
        Some(translated_item_json(
            "disabled", None, None, None, None, None,
        ))
    } else if translation_fresh {
        if let Some(status) = translation_state.trans_status.as_deref()
            && status != "ready"
        {
            translated_terminal_item(status, translation_state.trans_error_text.as_deref(), None)
        } else {
            let (title, summary) = normalize_translation_fields(
                translation_state.trans_title.clone(),
                translation_state.trans_summary.clone(),
            );
            if !release_detail_translation_ready(Some(original_body.as_str()), summary.as_deref()) {
                Some(translated_item_json(
                    "error",
                    None,
                    None,
                    Some(false),
                    Some(ANNOUNCEMENT_DETAIL_MARKDOWN_MISMATCH_ERROR),
                    None,
                ))
            } else {
                Some(translated_item_json(
                    "ready", title, summary, None, None, None,
                ))
            }
        }
    } else if translation_refresh_in_flight {
        translated_ready_item(
            translation_state.trans_title.clone(),
            translation_state.trans_summary.clone(),
            Some(true),
            None,
        )
        .or_else(|| Some(translated_missing_item(false, None)))
    } else {
        Some(translated_missing_item(true, None))
    };

    let smart = if state.ai_model.is_none() {
        Some(smart_item_json(
            "disabled", None, None, None, None, None, None,
        ))
    } else if smart_fresh {
        if let Some(status) = translation_state.smart_status.as_deref()
            && status != "ready"
        {
            smart_terminal_item(status, translation_state.smart_error_text.as_deref(), None)
        } else {
            smart_ready_item(
                translation_state.smart_title.clone(),
                translation_state.smart_summary.clone(),
                None,
                None,
            )
            .or_else(|| Some(smart_missing_item(None, None)))
        }
    } else if smart_refresh_in_flight {
        smart_ready_item(
            translation_state.smart_title.clone(),
            translation_state.smart_summary.clone(),
            Some(true),
            None,
        )
        .or_else(|| Some(smart_missing_item(Some(false), None)))
    } else {
        Some(smart_missing_item(None, None))
    };

    Ok(announcement_detail_body(
        source,
        discussion_key,
        translated,
        smart,
    ))
}

fn announcement_detail_body(
    source: AnnouncementDetailSource,
    discussion_key: String,
    translated: Option<Value>,
    smart: Option<Value>,
) -> Value {
    let mut body = json!({
        "repo_full_name": source.repo_full_name,
        "discussion_number": source.discussion_number,
        "discussion_key": discussion_key,
        "repo_visual": source.repo_visual,
        "title": source.title,
        "body": source.body,
        "html_url": source.html_url,
        "occurred_at": source.occurred_at,
        "translated": translated,
        "smart": smart,
    });
    if let Some(actor) = source.actor {
        body["actor"] = actor;
    }
    body
}

pub(crate) async fn get_announcement_detail(
    State(state): State<PostgresServerState>,
    session: Session,
    headers: HeaderMap,
    Path((owner_raw, repo_raw, discussion_number_raw)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let user_id = require_business_user_id(&state, &session, &headers).await?;
    let owner = owner_raw.trim();
    let repo = repo_raw.trim();
    if owner.is_empty() || repo.is_empty() {
        return Err(ApiError::bad_request("owner and repo are required"));
    }
    let discussion_number = parse_discussion_number_param(&discussion_number_raw)?;

    let cached_source = fetch_announcement_detail_source_from_db(
        &state.runtime.pool,
        user_id.as_str(),
        owner,
        repo,
        discussion_number,
    )
    .await?;
    let live_source = match fetch_live_announcement_detail_for_user(
        &state,
        user_id.as_str(),
        owner,
        repo,
        discussion_number,
    )
    .await
    {
        Ok(source) => source,
        Err(err) if cached_source.is_some() => {
            tracing::warn!(
                ?err,
                repo = format!("{owner}/{repo}"),
                discussion_number,
                "announcement detail live fetch failed; falling back to cached row"
            );
            None
        }
        Err(err) => return Err(err),
    };

    let source = live_source.or(cached_source).ok_or_else(|| {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", "announcement not found")
    })?;

    let body = build_announcement_detail_response(&state, user_id.as_str(), source).await?;
    Ok(Json(body).into_response())
}

// ---------------------------------------------------------------------------
// Repo public-release publication state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
struct RepoPublicReleaseVisibleRow {
    full_name: String,
    is_private: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct RepoPublicReleaseOwnedRow {
    repo_id: i64,
    full_name: String,
    is_private: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct RepoPublicReleaseUsageStatusRow {
    full_name: String,
    last_sync_status: String,
    access_kind: String,
    published_by_user_id: Option<String>,
    published_at: Option<String>,
}

fn normalize_public_repo_path(owner: String, repo: String) -> Result<(String, String), ApiError> {
    let owner = owner.trim();
    let repo = repo.trim();
    let valid = |value: &str| {
        !value.is_empty()
            && value.len() <= 100
            && value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    };
    if !valid(owner) || !valid(repo) {
        return Err(ApiError::bad_request("invalid repository path"));
    }
    Ok((owner.to_owned(), repo.to_owned()))
}

fn public_release_path_for_repo(full_name: &str) -> String {
    format!("/{full_name}/releases")
}

async fn load_repo_public_release_usage_status(
    pool: &PgPool,
    full_name_lower: &str,
) -> Result<Option<RepoPublicReleaseUsageStatusRow>, ApiError> {
    sqlx::query_as::<_, RepoPublicReleaseUsageStatusRow>(
        "SELECT full_name, last_sync_status, access_kind, published_by_user_id, published_at FROM public.public_repo_release_usage WHERE full_name_lower = $1 LIMIT 1",
    )
    .bind(full_name_lower)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)
}

async fn load_repo_public_release_visible_row(
    pool: &PgPool,
    user_id: &str,
    full_name_lower: &str,
) -> Result<Option<RepoPublicReleaseVisibleRow>, ApiError> {
    sqlx::query_as::<_, RepoPublicReleaseVisibleRow>(
        "SELECT full_name, is_private FROM public.user_release_visible_repos WHERE user_id = $1 AND lower(full_name) = $2 ORDER BY is_private IS NULL ASC, is_private ASC, updated_at DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(full_name_lower)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)
}

async fn load_repo_public_release_owned_row(
    pool: &PgPool,
    user_id: &str,
    full_name_lower: &str,
) -> Result<Option<RepoPublicReleaseOwnedRow>, ApiError> {
    sqlx::query_as::<_, RepoPublicReleaseOwnedRow>(
        "SELECT repo_id, repo_full_name AS full_name, is_private FROM public.owned_repo_star_baselines WHERE user_id = $1 AND lower(repo_full_name) = $2 LIMIT 1",
    )
    .bind(user_id)
    .bind(full_name_lower)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)
}

fn repo_public_release_status_response(
    requested_full_name: String,
    user_id: &str,
    visible: Option<&RepoPublicReleaseVisibleRow>,
    owned: Option<&RepoPublicReleaseOwnedRow>,
    usage: Option<&RepoPublicReleaseUsageStatusRow>,
    cache_cleanup: Option<Value>,
) -> Value {
    let full_name = usage
        .map(|row| row.full_name.clone())
        .or_else(|| owned.map(|row| row.full_name.clone()))
        .or_else(|| visible.map(|row| row.full_name.clone()))
        .unwrap_or(requested_full_name);
    let public_path = public_release_path_for_repo(full_name.as_str());
    let usage_is_private_publish =
        usage.is_some_and(|row| row.access_kind == "private_owner_published");
    let usage_published_by_current_user = usage
        .and_then(|row| row.published_by_user_id.as_deref())
        .is_some_and(|publisher| publisher == user_id);
    let last_sync_status = usage.map(|row| row.last_sync_status.clone());
    let published_at = usage.and_then(|row| row.published_at.clone());

    let mut body = if visible.is_some_and(|row| row.is_private == Some(0))
        || owned.is_some_and(|row| row.is_private == Some(0))
    {
        json!({
            "repo_full_name": full_name,
            "public_path": public_path,
            "visibility": "github_public",
            "publication_state": "github_public",
            "can_publish": false,
            "can_unpublish": false,
            "last_sync_status": last_sync_status,
            "published_at": Value::Null,
            "reason": "github_public_default",
        })
    } else if owned.is_some_and(|row| row.is_private.is_some_and(|is_private| is_private != 0)) {
        if usage_is_private_publish {
            json!({
                "repo_full_name": full_name,
                "public_path": public_path,
                "visibility": "private",
                "publication_state": "private_owner_published",
                "can_publish": false,
                "can_unpublish": usage_published_by_current_user || owned.is_some(),
                "last_sync_status": last_sync_status,
                "published_at": published_at,
                "reason": Value::Null,
            })
        } else {
            json!({
                "repo_full_name": full_name,
                "public_path": public_path,
                "visibility": "private",
                "publication_state": "private_owner_unpublished",
                "can_publish": true,
                "can_unpublish": false,
                "last_sync_status": last_sync_status,
                "published_at": Value::Null,
                "reason": "owner_private_repo_unpublished",
            })
        }
    } else if owned.is_some_and(|row| row.is_private.is_none()) {
        json!({
            "repo_full_name": full_name,
            "public_path": public_path,
            "visibility": "unknown",
            "publication_state": "not_publishable",
            "can_publish": false,
            "can_unpublish": false,
            "last_sync_status": last_sync_status,
            "published_at": Value::Null,
            "reason": "repo_privacy_unknown",
        })
    } else if usage_is_private_publish && usage_published_by_current_user {
        json!({
            "repo_full_name": full_name,
            "public_path": public_path,
            "visibility": "private",
            "publication_state": "private_owner_published",
            "can_publish": false,
            "can_unpublish": true,
            "last_sync_status": last_sync_status,
            "published_at": published_at,
            "reason": "owned_metadata_missing",
        })
    } else {
        json!({
            "repo_full_name": full_name,
            "public_path": public_path,
            "visibility": "unknown",
            "publication_state": "not_publishable",
            "can_publish": false,
            "can_unpublish": false,
            "last_sync_status": last_sync_status,
            "published_at": Value::Null,
            "reason": "not_viewer_owned_private_repo",
        })
    };
    if let Some(cache_cleanup) = cache_cleanup {
        body["cache_cleanup"] = cache_cleanup;
    }
    body
}

pub(crate) async fn get_repo_public_release_status(
    State(state): State<PostgresServerState>,
    session: Session,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(&state, &session).await?;
    let (owner, repo) = normalize_public_repo_path(owner, repo)?;
    let requested_full_name = format!("{owner}/{repo}");
    let full_name_lower = requested_full_name.to_ascii_lowercase();
    let visible = load_repo_public_release_visible_row(
        &state.runtime.pool,
        user_id.as_str(),
        full_name_lower.as_str(),
    )
    .await?;
    let owned = load_repo_public_release_owned_row(
        &state.runtime.pool,
        user_id.as_str(),
        full_name_lower.as_str(),
    )
    .await?;
    let usage =
        load_repo_public_release_usage_status(&state.runtime.pool, full_name_lower.as_str())
            .await?;
    Ok(Json(repo_public_release_status_response(
        requested_full_name,
        user_id.as_str(),
        visible.as_ref(),
        owned.as_ref(),
        usage.as_ref(),
        None,
    ))
    .into_response())
}

pub(crate) async fn publish_repo_public_release(
    State(state): State<PostgresServerState>,
    session: Session,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(&state, &session).await?;
    let (owner, repo) = normalize_public_repo_path(owner, repo)?;
    let requested_full_name = format!("{owner}/{repo}");
    let full_name_lower = requested_full_name.to_ascii_lowercase();
    let owned = load_repo_public_release_owned_row(
        &state.runtime.pool,
        user_id.as_str(),
        full_name_lower.as_str(),
    )
    .await?
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "repo_not_publishable",
            "only viewer-owned private repositories can be published",
        )
    })?;
    if owned.is_private == Some(0) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "public_repo_does_not_require_publish",
            "public GitHub repositories are public by default",
        ));
    }
    if owned.is_private.is_none_or(|is_private| is_private == 0) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "repo_privacy_unknown",
            "repository privacy must be refreshed before it can be published",
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut tx = crate::postgres_write::begin(&state.runtime.pool)
        .await
        .map_err(ApiError::internal)?;
    let release_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.repo_releases WHERE repo_id = $1 AND is_draft = 0",
    )
    .bind(owned.repo_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let initial_status = if release_count > 0 {
        "ready"
    } else {
        "pending"
    };
    sqlx::query(
        r#"
        INSERT INTO public.public_repo_release_usage (
          id,
          repo_id,
          owner_login,
          repo_name,
          full_name,
          full_name_lower,
          first_registered_at,
          last_requested_at,
          last_sync_status,
          last_sync_error,
          access_kind,
          published_by_user_id,
          published_at,
          created_at,
          updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, 'private_owner_published', $10, $11, $12, $13)
        ON CONFLICT(full_name_lower) DO UPDATE SET
          repo_id = EXCLUDED.repo_id,
          owner_login = EXCLUDED.owner_login,
          repo_name = EXCLUDED.repo_name,
          full_name = EXCLUDED.full_name,
          last_sync_status = EXCLUDED.last_sync_status,
          last_sync_error = NULL,
          access_kind = 'private_owner_published',
          published_by_user_id = EXCLUDED.published_by_user_id,
          published_at = COALESCE(public.public_repo_release_usage.published_at, EXCLUDED.published_at),
          updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(owned.repo_id)
    .bind(owner.as_str())
    .bind(repo.as_str())
    .bind(owned.full_name.as_str())
    .bind(full_name_lower.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(initial_status)
    .bind(user_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;

    // A repository with no cached releases reports `pending` until the release
    // sync worker fills the cache; the legacy handler additionally enqueued the
    // public repo release sync, which has no PostgreSQL task executor yet.
    let visible = load_repo_public_release_visible_row(
        &state.runtime.pool,
        user_id.as_str(),
        full_name_lower.as_str(),
    )
    .await?;
    let usage =
        load_repo_public_release_usage_status(&state.runtime.pool, full_name_lower.as_str())
            .await?;
    Ok(Json(repo_public_release_status_response(
        requested_full_name,
        user_id.as_str(),
        visible.as_ref(),
        Some(&owned),
        usage.as_ref(),
        None,
    ))
    .into_response())
}

async fn cleanup_public_release_repo_cache_if_unused_in_transaction(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    repo_id: Option<i64>,
    full_name: String,
) -> Result<Value, ApiError> {
    let Some(repo_id) = repo_id else {
        return Ok(json!({
            "repo_id": repo_id,
            "full_name": full_name,
            "deleted_release_count": 0,
            "deleted_ai_cache_count": 0,
            "skipped_reason": "repo_not_resolved",
        }));
    };

    if !pg_content_mode_is_legacy_tx(tx).await? {
        return Ok(json!({
            "repo_id": repo_id,
            "full_name": full_name,
            "deleted_release_count": 0,
            "deleted_ai_cache_count": 0,
            "skipped_reason": "content_processing_transition",
        }));
    }

    let remaining_public_usage = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.public_repo_release_usage WHERE repo_id = $1",
    )
    .bind(repo_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    if remaining_public_usage > 0 {
        return Ok(json!({
            "repo_id": repo_id,
            "full_name": full_name,
            "deleted_release_count": 0,
            "deleted_ai_cache_count": 0,
            "skipped_reason": "still_used_by_public_endpoint",
        }));
    }

    let user_visible_usage = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.user_release_visible_repos WHERE repo_id = $1",
    )
    .bind(repo_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    if user_visible_usage > 0 {
        return Ok(json!({
            "repo_id": repo_id,
            "full_name": full_name,
            "deleted_release_count": 0,
            "deleted_ai_cache_count": 0,
            "skipped_reason": "still_used_by_user_release_visibility",
        }));
    }

    let brief_usage = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.brief_release_memberships m JOIN public.repo_releases r ON r.release_id = m.release_id WHERE r.repo_id = $1",
    )
    .bind(repo_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    if brief_usage > 0 {
        return Ok(json!({
            "repo_id": repo_id,
            "full_name": full_name,
            "deleted_release_count": 0,
            "deleted_ai_cache_count": 0,
            "skipped_reason": "still_used_by_brief_membership",
        }));
    }

    let release_ids = sqlx::query_scalar::<_, i64>(
        "SELECT release_id FROM public.repo_releases WHERE repo_id = $1",
    )
    .bind(repo_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    let mut deleted_ai_cache_count = 0i64;
    let mut deleted_release_count = 0i64;
    for release_id in release_ids {
        let release_id_text = release_id.to_string();
        deleted_ai_cache_count += sqlx::query(
            "DELETE FROM public.ai_translations WHERE entity_type IN ('release_detail', 'release_smart') AND entity_id = $1",
        )
        .bind(release_id_text.as_str())
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?
        .rows_affected() as i64;
        deleted_release_count +=
            sqlx::query("DELETE FROM public.repo_releases WHERE release_id = $1")
                .bind(release_id)
                .execute(&mut **tx)
                .await
                .map_err(ApiError::internal)?
                .rows_affected() as i64;
    }
    sqlx::query("DELETE FROM public.repo_release_work_items WHERE repo_id = $1")
        .bind(repo_id)
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
    sqlx::query("DELETE FROM public.repo_release_sync_state WHERE repo_id = $1")
        .bind(repo_id)
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
    Ok(json!({
        "repo_id": repo_id,
        "full_name": full_name,
        "deleted_release_count": deleted_release_count,
        "deleted_ai_cache_count": deleted_ai_cache_count,
        "skipped_reason": Value::Null,
    }))
}

async fn pg_content_mode_is_legacy_tx(
    tx: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<bool, ApiError> {
    let mode: Option<String> =
        sqlx::query_scalar("SELECT mode FROM public.content_processing_control WHERE id = 1")
            .fetch_optional(&mut **tx)
            .await
            .map_err(ApiError::internal)?;
    Ok(mode.as_deref().unwrap_or("legacy") == "legacy")
}

pub(crate) async fn unpublish_repo_public_release(
    State(state): State<PostgresServerState>,
    session: Session,
    Path((owner, repo)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let user_id = require_active_user_id(&state, &session).await?;
    let (owner, repo) = normalize_public_repo_path(owner, repo)?;
    let requested_full_name = format!("{owner}/{repo}");
    let full_name_lower = requested_full_name.to_ascii_lowercase();
    let owned = load_repo_public_release_owned_row(
        &state.runtime.pool,
        user_id.as_str(),
        full_name_lower.as_str(),
    )
    .await?;
    if owned
        .as_ref()
        .is_none_or(|row| row.is_private.is_none_or(|is_private| is_private == 0))
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "repo_not_publishable",
            "only viewer-owned private repositories can be unpublished",
        ));
    }
    let mut tx = crate::postgres_write::begin(&state.runtime.pool)
        .await
        .map_err(ApiError::internal)?;
    let deleted_usage = sqlx::query_as::<_, (Option<i64>, String)>(
        "SELECT repo_id, full_name FROM public.public_repo_release_usage WHERE full_name_lower = $1 AND access_kind = 'private_owner_published' LIMIT 1",
    )
    .bind(full_name_lower.as_str())
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let cache_cleanup = if let Some((repo_id, full_name)) = deleted_usage {
        sqlx::query(
            "DELETE FROM public.public_repo_release_usage WHERE full_name_lower = $1 AND access_kind = 'private_owner_published'",
        )
        .bind(full_name_lower.as_str())
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        let cleanup =
            cleanup_public_release_repo_cache_if_unused_in_transaction(&mut tx, repo_id, full_name)
                .await?;
        tx.commit().await.map_err(ApiError::internal)?;
        Some(cleanup)
    } else {
        tx.commit().await.map_err(ApiError::internal)?;
        None
    };

    let visible = load_repo_public_release_visible_row(
        &state.runtime.pool,
        user_id.as_str(),
        full_name_lower.as_str(),
    )
    .await?;
    let usage =
        load_repo_public_release_usage_status(&state.runtime.pool, full_name_lower.as_str())
            .await?;
    Ok(Json(repo_public_release_status_response(
        requested_full_name,
        user_id.as_str(),
        visible.as_ref(),
        owned.as_ref(),
        usage.as_ref(),
        cache_cleanup,
    ))
    .into_response())
}

// ---------------------------------------------------------------------------
// Public release query model
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicReleaseQuery {
    content: Option<String>,
    lang: Option<String>,
    source: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
    highlight_ids: Option<String>,
    highlight_start: Option<String>,
    highlight_end: Option<String>,
    direction: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicReleaseRefreshRequest {
    IfStale,
}

struct PublicReleaseHttpQuery {
    query: PublicReleaseQuery,
    highlight_request: PublicReleaseHighlightRequest,
    focus: Option<String>,
    until_cursor: Option<String>,
    refresh: Option<PublicReleaseRefreshRequest>,
}

struct PublicReleaseListRequest {
    query: PublicReleaseQuery,
    highlight_request: PublicReleaseHighlightRequest,
    focus: Option<String>,
    until_cursor: Option<String>,
    refresh_request: Option<PublicReleaseRefreshRequest>,
}

#[derive(Debug, Clone)]
struct UsageRow {
    repo_id: Option<i64>,
    full_name: String,
    last_requested_at: String,
    last_sync_status: String,
}

#[derive(Debug, Clone, FromRow)]
struct PublicReleaseBaseRow {
    release_id: i64,
    sort_ts: String,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    is_prerelease: i64,
    is_draft: i64,
    previous_tag_name: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct PublicReleaseRepoVisualRow {
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: Option<i64>,
}

#[derive(Debug, Clone)]
struct PublicReleaseTranslationRow {
    source_hash: String,
    status: String,
    title: Option<String>,
    summary: Option<String>,
    error_text: Option<String>,
    reading: Option<Value>,
    reading_incompatible: bool,
    in_flight: bool,
}

#[derive(Debug, Clone)]
struct PublicReleaseRow {
    release_id: i64,
    sort_ts: String,
    repo_full_name: String,
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
    previous_tag_name: Option<String>,
    trans_source_hash: Option<String>,
    trans_status: Option<String>,
    trans_title: Option<String>,
    trans_summary: Option<String>,
    trans_error_text: Option<String>,
    trans_request_id: Option<String>,
    trans_in_flight: bool,
    smart_source_hash: Option<String>,
    smart_status: Option<String>,
    smart_title: Option<String>,
    smart_summary: Option<String>,
    smart_error_text: Option<String>,
    smart_reading: Option<Value>,
    smart_reading_incompatible: bool,
    smart_request_id: Option<String>,
    smart_in_flight: bool,
    global_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicReleaseCursor {
    sort_ts: String,
    release_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicReleaseCursorDirection {
    Older,
    Newer,
}

#[derive(Debug, Clone)]
enum PublicReleaseHighlightSpec {
    Ids(Vec<i64>),
    Range { start_id: i64, end_id: i64 },
}

#[derive(Debug, Clone)]
struct PublicReleaseRangeBounds {
    newer: PublicReleaseCursor,
    older: PublicReleaseCursor,
}

#[derive(Debug, Clone, Default)]
struct PublicReleaseHighlightRequest {
    selectors: Vec<String>,
    start: Option<String>,
    end: Option<String>,
    active: Option<String>,
}

impl PublicReleaseHighlightRequest {
    fn is_empty(&self) -> bool {
        self.selectors.is_empty()
            && self
                .start
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            && self
                .end
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            && self
                .active
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
    }
}

#[derive(Debug, Clone)]
enum PublicReleaseTypedSelector {
    Id(i64),
    Tag(String),
}

#[derive(Debug, Clone, FromRow)]
struct PublicReleaseSelectorRow {
    release_id: i64,
    tag_name: String,
    sort_ts: String,
    ordinal: i64,
}

#[derive(Debug, Clone)]
struct ResolvedPublicReleaseSelector {
    raw: String,
    row: PublicReleaseSelectorRow,
}

#[derive(Debug)]
struct PublicReleaseHighlightResolution {
    spec: Option<PublicReleaseHighlightSpec>,
    response: Option<Value>,
    active_release_id: Option<i64>,
    active_position: Option<PublicReleaseCursor>,
}

fn parse_public_release_typed_selector(raw: &str) -> Result<PublicReleaseTypedSelector, ()> {
    let raw = raw.trim();
    if let Some(value) = raw.strip_prefix("id:") {
        return parse_public_release_id(value)
            .map(PublicReleaseTypedSelector::Id)
            .map_err(|_| ());
    }
    if let Some(value) = raw.strip_prefix("tag:") {
        let value = value.trim();
        if !value.is_empty() && value.len() <= 255 {
            return Ok(PublicReleaseTypedSelector::Tag(value.to_owned()));
        }
    }
    Err(())
}

fn parse_public_release_http_query(
    raw_query: Option<&str>,
) -> Result<PublicReleaseHttpQuery, ApiError> {
    let mut query = PublicReleaseQuery::default();
    let mut highlight = PublicReleaseHighlightRequest::default();
    let mut focus = None;
    let mut until_cursor = None;
    let mut refresh = None;
    for (key, value) in url::form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "content" => query.content = Some(value.into_owned()),
            "lang" => query.lang = Some(value.into_owned()),
            "source" => query.source = Some(value.into_owned()),
            "cursor" => query.cursor = Some(value.into_owned()),
            "until_cursor" => until_cursor = Some(value.into_owned()),
            "limit" => {
                query.limit = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_| ApiError::bad_request("limit must be an integer"))?,
                )
            }
            "direction" => query.direction = Some(value.into_owned()),
            "highlight" => highlight.selectors.push(value.into_owned()),
            "highlight_start" => highlight.start = Some(value.into_owned()),
            "highlight_end" => highlight.end = Some(value.into_owned()),
            "highlight_active" => highlight.active = Some(value.into_owned()),
            "focus" => {
                if focus.is_some() {
                    return Err(ApiError::bad_request("focus must only be provided once"));
                }
                focus = Some(value.into_owned());
            }
            "refresh" => {
                if refresh.is_some() {
                    return Err(ApiError::bad_request("refresh must only be provided once"));
                }
                refresh = Some(match value.as_ref() {
                    "if_stale" => PublicReleaseRefreshRequest::IfStale,
                    _ => return Err(ApiError::bad_request("refresh must be if_stale")),
                });
            }
            _ => {}
        }
    }
    Ok(PublicReleaseHttpQuery {
        query,
        highlight_request: highlight,
        focus,
        until_cursor,
        refresh,
    })
}

fn validate_public_release_query(query: &PublicReleaseQuery) -> Result<(), ApiError> {
    let lang = query.lang.as_deref().unwrap_or("zh-CN");
    if lang != "zh-CN" {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_language",
            "only zh-CN is supported",
        ));
    }
    let content = query.content.as_deref().unwrap_or("all");
    if !matches!(content, "original" | "translated" | "polished" | "all") {
        return Err(ApiError::bad_request(
            "content must be original, translated, polished, or all",
        ));
    }
    parse_public_release_highlight_spec(query)?;
    parse_public_release_cursor_direction(query.direction.as_deref())?;
    Ok(())
}

fn public_source_is_page(query: &PublicReleaseQuery) -> bool {
    query.source.as_deref() == Some("page")
}

fn parse_public_release_cursor(raw: &str) -> Result<PublicReleaseCursor, ApiError> {
    let trimmed = raw.trim();
    let Some((sort_ts, release_id_raw)) = trimmed.rsplit_once('|') else {
        return Err(ApiError::bad_request("invalid cursor"));
    };
    let sort_ts = sort_ts.trim();
    if sort_ts.is_empty() {
        return Err(ApiError::bad_request("invalid cursor"));
    }
    let release_id = release_id_raw
        .trim()
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("invalid cursor"))?;
    Ok(PublicReleaseCursor {
        sort_ts: sort_ts.to_owned(),
        release_id,
    })
}

fn public_release_cursor(row: &PublicReleaseRow) -> String {
    format!("{}|{}", row.sort_ts, row.release_id)
}

fn parse_public_release_id(raw: &str) -> Result<i64, ApiError> {
    let value = raw
        .trim()
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("highlight release ids must be positive integers"))?;
    if value <= 0 {
        return Err(ApiError::bad_request(
            "highlight release ids must be positive integers",
        ));
    }
    Ok(value)
}

fn parse_public_release_highlight_spec(
    query: &PublicReleaseQuery,
) -> Result<Option<PublicReleaseHighlightSpec>, ApiError> {
    let has_ids = query
        .highlight_ids
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_start = query
        .highlight_start
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_end = query
        .highlight_end
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());

    if has_ids && (has_start || has_end) {
        return Err(ApiError::bad_request(
            "highlight_ids cannot be combined with highlight_start or highlight_end",
        ));
    }
    if has_start != has_end {
        return Err(ApiError::bad_request(
            "highlight_start and highlight_end must be provided together",
        ));
    }

    if has_ids {
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        for raw_id in query
            .highlight_ids
            .as_deref()
            .unwrap_or_default()
            .split(',')
        {
            let release_id = parse_public_release_id(raw_id)?;
            if seen.insert(release_id) {
                ids.push(release_id);
            }
        }
        if ids.is_empty() || ids.len() > PUBLIC_RELEASE_HIGHLIGHT_ID_LIMIT {
            return Err(ApiError::bad_request(format!(
                "highlight_ids must contain 1 to {PUBLIC_RELEASE_HIGHLIGHT_ID_LIMIT} ids"
            )));
        }
        return Ok(Some(PublicReleaseHighlightSpec::Ids(ids)));
    }

    if has_start {
        return Ok(Some(PublicReleaseHighlightSpec::Range {
            start_id: parse_public_release_id(
                query.highlight_start.as_deref().unwrap_or_default(),
            )?,
            end_id: parse_public_release_id(query.highlight_end.as_deref().unwrap_or_default())?,
        }));
    }

    Ok(None)
}

fn parse_public_release_cursor_direction(
    raw: Option<&str>,
) -> Result<PublicReleaseCursorDirection, ApiError> {
    match raw.unwrap_or("older") {
        "older" => Ok(PublicReleaseCursorDirection::Older),
        "newer" => Ok(PublicReleaseCursorDirection::Newer),
        _ => Err(ApiError::bad_request("direction must be older or newer")),
    }
}

fn compare_public_release_positions(
    left: &PublicReleaseCursor,
    right: &PublicReleaseCursor,
) -> Ordering {
    right
        .sort_ts
        .cmp(&left.sort_ts)
        .then_with(|| right.release_id.cmp(&left.release_id))
}

// ---------------------------------------------------------------------------
// Public release usage registration
// ---------------------------------------------------------------------------

async fn upsert_public_release_usage(
    pool: &PgPool,
    owner: String,
    repo: String,
    detail: bool,
    page_source: bool,
) -> Result<UsageRow, ApiError> {
    let (owner, repo) = normalize_public_repo_path(owner, repo)?;
    let full_name = format!("{owner}/{repo}");
    let full_name_lower = full_name.to_ascii_lowercase();
    let now = chrono::Utc::now().to_rfc3339();
    let (api_list_inc, api_detail_inc, page_list_inc, page_detail_inc) = match (detail, page_source)
    {
        (false, false) => (1_i64, 0_i64, 0_i64, 0_i64),
        (true, false) => (0_i64, 1_i64, 0_i64, 0_i64),
        (false, true) => (0_i64, 0_i64, 1_i64, 0_i64),
        (true, true) => (0_i64, 0_i64, 0_i64, 1_i64),
    };
    sqlx::query(
        r#"
        INSERT INTO public.public_repo_release_usage (
          id, owner_login, repo_name, full_name, full_name_lower,
          first_registered_at, last_requested_at, last_list_requested_at,
          last_detail_requested_at, api_list_requests, api_detail_requests,
          page_list_requests, page_detail_requests, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
        ON CONFLICT(full_name_lower) DO UPDATE SET
          last_requested_at = EXCLUDED.last_requested_at,
          last_list_requested_at = COALESCE(EXCLUDED.last_list_requested_at, public.public_repo_release_usage.last_list_requested_at),
          last_detail_requested_at = COALESCE(EXCLUDED.last_detail_requested_at, public.public_repo_release_usage.last_detail_requested_at),
          api_list_requests = public.public_repo_release_usage.api_list_requests + EXCLUDED.api_list_requests,
          api_detail_requests = public.public_repo_release_usage.api_detail_requests + EXCLUDED.api_detail_requests,
          page_list_requests = public.public_repo_release_usage.page_list_requests + EXCLUDED.page_list_requests,
          page_detail_requests = public.public_repo_release_usage.page_detail_requests + EXCLUDED.page_detail_requests,
          updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(owner.as_str())
    .bind(repo.as_str())
    .bind(full_name.as_str())
    .bind(full_name_lower.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind((!detail).then_some(now.as_str()))
    .bind(detail.then_some(now.as_str()))
    .bind(api_list_inc)
    .bind(api_detail_inc)
    .bind(page_list_inc)
    .bind(page_detail_inc)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;

    let row = sqlx::query_as::<_, (Option<i64>, String, String, String)>(
        "SELECT repo_id, full_name, last_requested_at, last_sync_status FROM public.public_repo_release_usage WHERE full_name_lower = $1 LIMIT 1",
    )
    .bind(full_name_lower.as_str())
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    let row = UsageRow {
        repo_id: row.0,
        full_name: row.1,
        last_requested_at: row.2,
        last_sync_status: row.3,
    };

    if row.repo_id.is_none()
        && row.last_sync_status != "inaccessible"
        && let Some(resolved) =
            resolve_public_release_usage_from_local_metadata(pool, full_name_lower.as_str()).await?
    {
        return Ok(resolved);
    }
    Ok(row)
}

#[derive(Debug, FromRow)]
struct PublicReleaseLocalMetadataRow {
    repo_id: i64,
    full_name: String,
    is_private: Option<i64>,
    metadata_updated_at: Option<String>,
}

fn split_public_repo_full_name(full_name: &str) -> Option<(&str, &str)> {
    let (owner, repo) = full_name.split_once('/')?;
    let owner = owner.trim();
    let repo = repo.trim();
    (!owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
}

fn public_local_metadata_is_fresh(updated_at: Option<&str>) -> bool {
    const PUBLIC_LOCAL_METADATA_MAX_AGE_HOURS: i64 = 24;
    let Some(updated_at) = updated_at else {
        return false;
    };
    let Ok(updated_at) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return false;
    };
    chrono::Utc::now().signed_duration_since(updated_at.with_timezone(&chrono::Utc))
        <= chrono::Duration::hours(PUBLIC_LOCAL_METADATA_MAX_AGE_HOURS)
}

async fn resolve_public_release_usage_from_local_metadata(
    pool: &PgPool,
    full_name_lower: &str,
) -> Result<Option<UsageRow>, ApiError> {
    let Some(repo) = sqlx::query_as::<_, PublicReleaseLocalMetadataRow>(
        r#"
        SELECT repo_id, full_name, is_private, metadata_updated_at
        FROM (
          SELECT
            repo_id,
            full_name,
            is_private,
            updated_at AS metadata_updated_at,
            0 AS source_rank
          FROM public.starred_repos
          WHERE lower(full_name) = $1
          UNION ALL
          SELECT
            repo_id,
            repo_full_name AS full_name,
            is_private,
            updated_at AS metadata_updated_at,
            1 AS source_rank
          FROM public.owned_repo_star_baselines
          WHERE lower(repo_full_name) = $1
        ) local_metadata
        ORDER BY
          COALESCE(metadata_updated_at, '') DESC,
          CASE
            WHEN is_private = 1 THEN 0
            WHEN is_private IS NULL THEN 1
            ELSE 2
          END ASC,
          source_rank ASC
        LIMIT 1
        "#,
    )
    .bind(full_name_lower)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    else {
        return Ok(None);
    };
    if !public_local_metadata_is_fresh(repo.metadata_updated_at.as_deref()) {
        return Ok(None);
    }
    if repo.is_private != Some(0) {
        return Ok(None);
    }

    let release_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.repo_releases WHERE repo_id = $1 AND is_draft = 0",
    )
    .bind(repo.repo_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;

    let Some((owner, repo_name)) = split_public_repo_full_name(repo.full_name.as_str()) else {
        return Ok(None);
    };
    let now = chrono::Utc::now().to_rfc3339();
    let next_status = if release_count > 0 {
        "ready"
    } else {
        "pending"
    };
    sqlx::query(
        r#"
        UPDATE public.public_repo_release_usage
        SET repo_id = $1,
            owner_login = $2,
            repo_name = $3,
            full_name = $4,
            full_name_lower = $5,
            last_sync_status = $6,
            last_sync_error = NULL,
            updated_at = $7
        WHERE full_name_lower = $8
        "#,
    )
    .bind(repo.repo_id)
    .bind(owner)
    .bind(repo_name)
    .bind(repo.full_name.as_str())
    .bind(full_name_lower)
    .bind(next_status)
    .bind(now.as_str())
    .bind(full_name_lower)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;

    // The legacy resolver enqueued a public repo release sync when the cache was
    // empty. The PostgreSQL worker has no public repo release executor yet, so
    // the usage row keeps its `pending` status until a sync fills the cache.
    let row = sqlx::query_as::<_, (Option<i64>, String, String, String)>(
        "SELECT repo_id, full_name, last_requested_at, last_sync_status FROM public.public_repo_release_usage WHERE full_name_lower = $1 LIMIT 1",
    )
    .bind(full_name_lower)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Some(UsageRow {
        repo_id: row.0,
        full_name: row.1,
        last_requested_at: row.2,
        last_sync_status: row.3,
    }))
}

fn public_pending_response(row: &UsageRow) -> Response {
    let retry_after_seconds = 60_i64;
    let body = json!({
        "status": "pending_sync",
        "message": "Release data is being prepared. Retry after the suggested delay.",
        "reason": if row.repo_id.is_none() {
            "repository_registered_metadata_pending"
        } else {
            "repository_registered_release_sync_pending"
        },
        "retry_after_seconds": retry_after_seconds,
        "repo_full_name": row.full_name,
        "last_requested_at": row.last_requested_at,
    });
    let mut response = (StatusCode::ACCEPTED, Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, header::HeaderValue::from_static("60"));
    response
}

fn ensure_public_release_usage_accessible(row: &UsageRow) -> Result<(), ApiError> {
    if row.last_sync_status == "inaccessible" {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "repository_inaccessible",
            "repository is not publicly accessible",
        ));
    }
    Ok(())
}

fn public_release_usage_should_retry_without_rows(row: &UsageRow) -> bool {
    row.last_sync_status == "pending" || row.last_sync_status == "failed"
}

// ---------------------------------------------------------------------------
// Public release rows
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn load_public_release_base_rows(
    pool: &PgPool,
    repo_id: i64,
    tag: Option<&str>,
    release_ids: Option<&[i64]>,
    bounds: Option<&PublicReleaseRangeBounds>,
    cursor: Option<&PublicReleaseCursor>,
    direction: PublicReleaseCursorDirection,
    limit: i64,
) -> Result<Vec<PublicReleaseBaseRow>, ApiError> {
    const SORT_EXPRESSION: &str = "COALESCE(r.published_at, r.created_at, r.updated_at)";
    let mut query = QueryBuilder::<Postgres>::new(
        r#"
        SELECT
          r.release_id,
          COALESCE(r.published_at, r.created_at, r.updated_at, '') AS sort_ts,
          r.tag_name,
          r.name,
          r.body,
          r.html_url,
          r.published_at,
          r.is_prerelease,
          r.is_draft,
          (
            SELECT prev.tag_name
            FROM public.repo_releases prev
            WHERE prev.repo_id = r.repo_id
              AND prev.is_draft = 0
              AND (
                COALESCE(prev.published_at, prev.created_at, prev.updated_at)
                  < COALESCE(r.published_at, r.created_at, r.updated_at)
                OR (
                  COALESCE(prev.published_at, prev.created_at, prev.updated_at)
                    = COALESCE(r.published_at, r.created_at, r.updated_at)
                  AND prev.release_id < r.release_id
                )
              )
            ORDER BY
              COALESCE(prev.published_at, prev.created_at, prev.updated_at) DESC,
              prev.release_id DESC
            LIMIT 1
          ) AS previous_tag_name
        FROM public.repo_releases r
        WHERE r.repo_id =
        "#,
    );
    query.push_bind(repo_id);
    query.push(" AND r.is_draft = 0");
    if let Some(tag) = tag {
        query.push(" AND r.tag_name = ");
        query.push_bind(tag);
    }
    if let Some(release_ids) = release_ids {
        query.push(" AND r.release_id IN (");
        {
            let mut separated = query.separated(", ");
            for release_id in release_ids {
                separated.push_bind(*release_id);
            }
        }
        query.push(")");
    }
    if let Some(bounds) = bounds {
        query.push(" AND (");
        query.push(SORT_EXPRESSION);
        query.push(" < ");
        query.push_bind(bounds.newer.sort_ts.as_str());
        query.push(" OR (");
        query.push(SORT_EXPRESSION);
        query.push(" = ");
        query.push_bind(bounds.newer.sort_ts.as_str());
        query.push(" AND r.release_id <= ");
        query.push_bind(bounds.newer.release_id);
        query.push("))");
        query.push(" AND (");
        query.push(SORT_EXPRESSION);
        query.push(" > ");
        query.push_bind(bounds.older.sort_ts.as_str());
        query.push(" OR (");
        query.push(SORT_EXPRESSION);
        query.push(" = ");
        query.push_bind(bounds.older.sort_ts.as_str());
        query.push(" AND r.release_id >= ");
        query.push_bind(bounds.older.release_id);
        query.push("))");
    }
    if let Some(cursor) = cursor {
        let operator = match direction {
            PublicReleaseCursorDirection::Older => "<",
            PublicReleaseCursorDirection::Newer => ">",
        };
        let tie_operator = match direction {
            PublicReleaseCursorDirection::Older => "<",
            PublicReleaseCursorDirection::Newer => ">",
        };
        query.push(" AND (");
        query.push(SORT_EXPRESSION);
        query.push(" ");
        query.push(operator);
        query.push_bind(cursor.sort_ts.as_str());
        query.push(" OR (");
        query.push(SORT_EXPRESSION);
        query.push(" = ");
        query.push_bind(cursor.sort_ts.as_str());
        query.push(" AND r.release_id ");
        query.push(tie_operator);
        query.push_bind(cursor.release_id);
        query.push("))");
    }
    query.push(" ORDER BY ");
    query.push(SORT_EXPRESSION);
    match direction {
        PublicReleaseCursorDirection::Older => query.push(" DESC, r.release_id DESC LIMIT "),
        PublicReleaseCursorDirection::Newer => query.push(" ASC, r.release_id ASC LIMIT "),
    };
    query.push_bind(limit);
    let mut rows = query
        .build_query_as::<PublicReleaseBaseRow>()
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    if direction == PublicReleaseCursorDirection::Newer {
        if limit > 0 && rows.len() == limit as usize {
            let extra = rows.pop();
            rows.reverse();
            rows.extend(extra);
        } else {
            rows.reverse();
        }
    }
    Ok(rows)
}

async fn load_public_release_repo_visual(
    pool: &PgPool,
    repo_id: i64,
) -> Result<Option<PublicReleaseRepoVisualRow>, ApiError> {
    if let Some(row) = sqlx::query_as::<_, PublicReleaseRepoVisualRow>(
        "SELECT owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image FROM public.starred_repos WHERE repo_id = $1 AND is_private = 0 ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(repo_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    {
        return Ok(Some(row));
    }
    sqlx::query_as::<_, PublicReleaseRepoVisualRow>(
        "SELECT owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image FROM public.owned_repo_star_baselines WHERE repo_id = $1 ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(repo_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)
}

#[derive(Debug, FromRow)]
struct LegacyTranslationRow {
    release_id: i64,
    source_hash: String,
    status: String,
    title: Option<String>,
    summary: Option<String>,
    error_text: Option<String>,
}

/// Releases whose legacy cache row still has in-flight work.
///
/// The public release reads do not filter on the requesting user, so neither
/// does this probe: it answers whether any `ai_translations` row for the
/// release is queued/running (or points at a queued/batched/running work item).
async fn load_legacy_in_flight_release_ids(
    pool: &PgPool,
    release_ids: &[i64],
    entity_type: &str,
) -> Result<HashSet<i64>, ApiError> {
    if release_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let entity_ids = release_ids.iter().map(i64::to_string).collect::<Vec<_>>();
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT t.entity_id FROM public.ai_translations t \
         LEFT JOIN public.translation_work_items w ON w.id = t.active_work_item_id \
         WHERE t.entity_type = $1 AND t.lang = 'zh-CN' AND t.entity_id = ANY($2) \
           AND (t.status IN ('queued', 'running') \
                OR w.status IN ('queued', 'batched', 'running'))",
    )
    .bind(entity_type)
    .bind(&entity_ids)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .filter_map(|entity_id| entity_id.parse::<i64>().ok())
        .collect())
}

async fn load_public_release_translation_rows(
    pool: &PgPool,
    release_ids: &[i64],
    entity_type: &str,
) -> Result<HashMap<i64, PublicReleaseTranslationRow>, ApiError> {
    if release_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut ready_query = QueryBuilder::<Postgres>::new(
        r#"
        SELECT
          entity_id::bigint AS release_id,
          source_hash,
          status,
          title,
          summary,
          error_text
        FROM public.ai_translations
        WHERE entity_type =
        "#,
    );
    ready_query.push_bind(entity_type.to_owned());
    ready_query.push(" AND lang = 'zh-CN' AND status = 'ready' AND entity_id IN (");
    {
        let mut separated = ready_query.separated(", ");
        for release_id in release_ids {
            separated.push_bind(release_id.to_string());
        }
    }
    ready_query.push(") ORDER BY entity_id, updated_at DESC");
    let ready_rows = ready_query
        .build_query_as::<LegacyTranslationRow>()
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let mut results = HashMap::new();
    for row in ready_rows {
        results
            .entry(row.release_id)
            .or_insert(PublicReleaseTranslationRow {
                source_hash: row.source_hash,
                status: row.status,
                title: row.title,
                summary: row.summary,
                error_text: row.error_text,
                reading: None,
                reading_incompatible: false,
                in_flight: false,
            });
    }
    let missing_ids = release_ids
        .iter()
        .copied()
        .filter(|release_id| !results.contains_key(release_id))
        .collect::<Vec<_>>();
    if missing_ids.is_empty() {
        return Ok(results);
    }
    let mut fallback_query = QueryBuilder::<Postgres>::new(
        r#"
        SELECT
          entity_id::bigint AS release_id,
          source_hash,
          status,
          title,
          summary,
          error_text
        FROM public.ai_translations
        WHERE entity_type =
        "#,
    );
    fallback_query.push_bind(entity_type.to_owned());
    fallback_query.push(
        " AND lang = 'zh-CN' AND status IN ('disabled', 'missing', 'error') AND entity_id IN (",
    );
    {
        let mut separated = fallback_query.separated(", ");
        for release_id in &missing_ids {
            separated.push_bind(release_id.to_string());
        }
    }
    fallback_query.push(") ORDER BY entity_id, updated_at DESC");
    let fallback_rows = fallback_query
        .build_query_as::<LegacyTranslationRow>()
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    for row in fallback_rows {
        results
            .entry(row.release_id)
            .or_insert(PublicReleaseTranslationRow {
                source_hash: row.source_hash,
                status: row.status,
                title: row.title,
                summary: row.summary,
                error_text: row.error_text,
                reading: None,
                reading_incompatible: false,
                in_flight: false,
            });
    }
    Ok(results)
}

async fn load_public_release_global_rows(
    pool: &PgPool,
    overlay: postgres_content_overlay::OverlayState,
    repo_full_name: &str,
    repo_id: i64,
    base_rows: &[PublicReleaseBaseRow],
    load_translated: bool,
    load_smart: bool,
) -> Result<
    (
        HashMap<i64, PublicReleaseTranslationRow>,
        HashMap<i64, PublicReleaseTranslationRow>,
    ),
    ApiError,
> {
    let mut translated_rows = HashMap::new();
    let mut smart_rows = HashMap::new();
    for base in base_rows {
        let repo_identity =
            postgres_content_overlay::release_repo_identity(base.html_url.as_str(), repo_id);
        let source = postgres_content_overlay::ReleaseSource {
            release_id: base.release_id,
            repo_full_name,
            repo_identity: repo_identity.as_str(),
            tag_name: &base.tag_name,
            name: base.name.as_deref(),
            body: base.body.as_deref(),
            previous_tag_name: base.previous_tag_name.as_deref(),
        };
        if load_translated
            && let Some(projection) =
                postgres_content_overlay::public_release_translation(pool, overlay, &source)
                    .await
                    .map_err(ApiError::internal)?
        {
            translated_rows.insert(
                base.release_id,
                public_global_translation_row(projection, false),
            );
        }
        // Public Release pages may only consume the public reading variant. The
        // legacy reading identity is derived from a public evidence snapshot
        // that the PostgreSQL runtime does not collect, so the newest public
        // polishing projection is the equivalent source here.
        if load_smart
            && let Some(projection) =
                postgres_content_overlay::public_release_smart(pool, overlay, &source)
                    .await
                    .map_err(ApiError::internal)?
        {
            smart_rows.insert(
                base.release_id,
                public_global_translation_row(projection, true),
            );
        }
    }
    Ok((translated_rows, smart_rows))
}

fn public_global_translation_row(
    projection: postgres_content_overlay::ContentProjection,
    smart: bool,
) -> PublicReleaseTranslationRow {
    let has_text = projection.has_text();
    let (reading, reading_incompatible) = if smart {
        release_reading_state(&projection.payload)
    } else {
        (None, false)
    };
    let source_hash = projection
        .payload
        .get("source_hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let title = projection.title();
    let summary = projection.translation_summary();
    let error_text = projection.error_text();
    let in_flight = postgres_content_overlay::is_active_work_status(projection.status.as_str());
    let display_status = if reading
        .as_ref()
        .and_then(|reading| reading.get("outcome"))
        .and_then(Value::as_str)
        == Some("insufficient")
        && projection.status == "ready"
    {
        "insufficient".to_owned()
    } else if matches!(
        projection.status.as_str(),
        "queued" | "running" | "deferred_provider" | "blocked_config"
    ) && has_text
    {
        "ready".to_owned()
    } else if reading_incompatible && projection.status == "ready" {
        "incompatible".to_owned()
    } else {
        projection.status
    };
    PublicReleaseTranslationRow {
        source_hash,
        status: display_status,
        title,
        summary,
        error_text,
        reading,
        reading_incompatible,
        in_flight,
    }
}

#[allow(clippy::too_many_arguments)]
async fn load_public_release_rows(
    pool: &PgPool,
    repo_full_name: &str,
    repo_id: i64,
    tag: Option<&str>,
    release_ids: Option<&[i64]>,
    bounds: Option<&PublicReleaseRangeBounds>,
    cursor: Option<&PublicReleaseCursor>,
    direction: PublicReleaseCursorDirection,
    limit: i64,
    content: &str,
) -> Result<Vec<PublicReleaseRow>, ApiError> {
    let base_rows = load_public_release_base_rows(
        pool,
        repo_id,
        tag,
        release_ids,
        bounds,
        cursor,
        direction,
        limit,
    )
    .await?;
    if base_rows.is_empty() {
        return Ok(Vec::new());
    }
    let repo_visual = load_public_release_repo_visual(pool, repo_id).await?;
    let overlay = postgres_content_overlay::OverlayState::load(pool).await;
    let global_mode = overlay.is_global();
    let load_translated = content == "all" || content == "translated";
    let load_smart = content == "all" || content == "polished";
    let ids = base_rows
        .iter()
        .map(|row| row.release_id)
        .collect::<Vec<_>>();
    let (translated, smart) = if global_mode {
        load_public_release_global_rows(
            pool,
            overlay,
            repo_full_name,
            repo_id,
            &base_rows,
            load_translated,
            load_smart,
        )
        .await?
    } else {
        let translated = if load_translated {
            load_public_release_translation_rows(pool, &ids, "release_detail").await?
        } else {
            HashMap::new()
        };
        // Legacy AI rows do not record whether compare evidence came from a
        // public Release body or a private repository snapshot.
        (translated, HashMap::new())
    };

    // Public reads are unauthenticated, so the request lookup stays
    // repository-scoped like the legacy `content_request_links` probe. The
    // in-flight signal comes from the projection status in global mode and from
    // the legacy `ai_translations` cache otherwise.
    let entity_ids = ids.iter().map(i64::to_string).collect::<Vec<_>>();
    let translated_requests = if load_translated {
        postgres_content_overlay::latest_translation_request_ids(
            pool,
            &entity_ids,
            &postgres_content_overlay::RELEASE_TRANSLATION_REQUEST_KINDS,
            None,
        )
        .await
        .map_err(ApiError::internal)?
    } else {
        HashMap::new()
    };
    let smart_requests = if load_smart {
        postgres_content_overlay::latest_translation_request_ids(
            pool,
            &entity_ids,
            &postgres_content_overlay::RELEASE_SMART_REQUEST_KINDS,
            None,
        )
        .await
        .map_err(ApiError::internal)?
    } else {
        HashMap::new()
    };
    let translated_in_flight = if !load_translated {
        HashSet::new()
    } else if global_mode {
        translated
            .iter()
            .filter(|(_, row)| row.in_flight)
            .map(|(release_id, _)| *release_id)
            .collect()
    } else {
        load_legacy_in_flight_release_ids(pool, &ids, "release_detail").await?
    };
    let smart_in_flight = if !load_smart {
        HashSet::new()
    } else if global_mode {
        smart
            .iter()
            .filter(|(_, row)| row.in_flight)
            .map(|(release_id, _)| *release_id)
            .collect()
    } else {
        load_legacy_in_flight_release_ids(pool, &ids, "release_smart").await?
    };

    Ok(base_rows
        .into_iter()
        .map(|base| {
            let detail_row = translated.get(&base.release_id);
            let smart_row = smart.get(&base.release_id);
            let mut row = public_release_row_from_parts(
                base,
                repo_full_name,
                repo_visual.as_ref(),
                detail_row,
                smart_row,
                global_mode,
            );
            let release_id = row.release_id;
            row.trans_request_id = translated_requests.get(&release_id.to_string()).cloned();
            row.trans_in_flight = translated_in_flight.contains(&release_id);
            row.smart_request_id = smart_requests.get(&release_id.to_string()).cloned();
            row.smart_in_flight = smart_in_flight.contains(&release_id);
            row
        })
        .collect())
}

fn public_release_row_from_parts(
    base: PublicReleaseBaseRow,
    repo_full_name: &str,
    repo_visual: Option<&PublicReleaseRepoVisualRow>,
    translated: Option<&PublicReleaseTranslationRow>,
    smart: Option<&PublicReleaseTranslationRow>,
    global_mode: bool,
) -> PublicReleaseRow {
    PublicReleaseRow {
        release_id: base.release_id,
        sort_ts: base.sort_ts,
        repo_full_name: repo_full_name.to_owned(),
        owner_avatar_url: repo_visual.and_then(|row| row.owner_avatar_url.clone()),
        open_graph_image_url: repo_visual.and_then(|row| row.open_graph_image_url.clone()),
        uses_custom_open_graph_image: repo_visual.and_then(|row| row.uses_custom_open_graph_image),
        tag_name: base.tag_name,
        name: base.name,
        body: base.body,
        html_url: base.html_url,
        published_at: base.published_at,
        is_prerelease: base.is_prerelease,
        is_draft: base.is_draft,
        previous_tag_name: base.previous_tag_name,
        trans_source_hash: translated.map(|row| row.source_hash.clone()),
        trans_status: translated.map(|row| row.status.clone()),
        trans_title: translated.and_then(|row| row.title.clone()),
        trans_summary: translated.and_then(|row| row.summary.clone()),
        trans_error_text: translated.and_then(|row| row.error_text.clone()),
        trans_request_id: None,
        trans_in_flight: false,
        smart_source_hash: smart.map(|row| row.source_hash.clone()),
        smart_status: smart.map(|row| row.status.clone()),
        smart_title: smart.and_then(|row| row.title.clone()),
        smart_summary: smart.and_then(|row| row.summary.clone()),
        smart_error_text: smart.and_then(|row| row.error_text.clone()),
        smart_reading: smart.and_then(|row| row.reading.clone()),
        smart_reading_incompatible: smart.is_some_and(|row| row.reading_incompatible),
        smart_request_id: None,
        smart_in_flight: false,
        global_mode,
    }
}

fn public_release_cursor_from_row(row: &PublicReleaseRow) -> PublicReleaseCursor {
    PublicReleaseCursor {
        sort_ts: row.sort_ts.clone(),
        release_id: row.release_id,
    }
}

fn public_release_position_is_in_range(
    position: &PublicReleaseCursor,
    bounds: &PublicReleaseRangeBounds,
) -> bool {
    let at_or_older_than_newer = position.sort_ts < bounds.newer.sort_ts
        || (position.sort_ts == bounds.newer.sort_ts
            && position.release_id <= bounds.newer.release_id);
    let at_or_newer_than_older = position.sort_ts > bounds.older.sort_ts
        || (position.sort_ts == bounds.older.sort_ts
            && position.release_id >= bounds.older.release_id);
    at_or_older_than_newer && at_or_newer_than_older
}

async fn load_public_release_selector_rows(
    pool: &PgPool,
    repo_id: i64,
    selectors: &[PublicReleaseTypedSelector],
) -> Result<Vec<PublicReleaseSelectorRow>, ApiError> {
    if selectors.is_empty() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    let mut tags = Vec::new();
    for selector in selectors {
        match selector {
            PublicReleaseTypedSelector::Id(id) => ids.push(*id),
            PublicReleaseTypedSelector::Tag(tag) => tags.push(tag.clone()),
        }
    }
    let mut query = QueryBuilder::<Postgres>::new(
        r#"
        WITH ordered AS (
          SELECT
            r.release_id,
            r.tag_name,
            COALESCE(r.published_at, r.created_at, r.updated_at, '') AS sort_ts,
            ROW_NUMBER() OVER (
              ORDER BY
                COALESCE(r.published_at, r.created_at, r.updated_at) DESC,
                r.release_id DESC
            ) AS ordinal
          FROM public.repo_releases r
          WHERE r.repo_id =
        "#,
    );
    query.push_bind(repo_id);
    query.push(
        " AND r.is_draft = 0) SELECT release_id, tag_name, sort_ts, ordinal FROM ordered WHERE ",
    );
    if !ids.is_empty() {
        query.push("release_id IN (");
        {
            let mut separated = query.separated(", ");
            for id in &ids {
                separated.push_bind(*id);
            }
        }
        query.push(")");
    }
    if !ids.is_empty() && !tags.is_empty() {
        query.push(" OR ");
    }
    if !tags.is_empty() {
        query.push("tag_name IN (");
        {
            let mut separated = query.separated(", ");
            for tag in &tags {
                separated.push_bind(tag.clone());
            }
        }
        query.push(")");
    }
    query.push(" ORDER BY ordinal");
    query
        .build_query_as::<PublicReleaseSelectorRow>()
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)
}

fn match_public_release_selector<'a>(
    selector: &PublicReleaseTypedSelector,
    rows: &'a [PublicReleaseSelectorRow],
) -> Option<&'a PublicReleaseSelectorRow> {
    rows.iter().find(|row| match selector {
        PublicReleaseTypedSelector::Id(id) => row.release_id == *id,
        PublicReleaseTypedSelector::Tag(tag) => row.tag_name == *tag,
    })
}

fn public_release_target_response(resolved: &ResolvedPublicReleaseSelector) -> Value {
    json!({
        "selector": resolved.raw,
        "release_id": resolved.row.release_id.to_string(),
        "tag_name": resolved.row.tag_name,
        "ordinal": resolved.row.ordinal,
    })
}

fn invalid_public_release_highlight(
    mode: &str,
    requested: Vec<String>,
    message: impl Into<String>,
) -> PublicReleaseHighlightResolution {
    PublicReleaseHighlightResolution {
        spec: None,
        active_release_id: None,
        active_position: None,
        response: Some(json!({
            "mode": mode,
            "status": "invalid",
            "requested": requested,
            "resolved": Vec::<Value>::new(),
            "unresolved": Vec::<String>::new(),
            "total": 0,
            "active_release_id": Value::Null,
            "active_index": Value::Null,
            "message": message.into(),
        })),
    }
}

async fn resolve_public_release_highlight(
    pool: &PgPool,
    repo_id: i64,
    request: &PublicReleaseHighlightRequest,
) -> Result<PublicReleaseHighlightResolution, ApiError> {
    let has_discrete = !request.selectors.is_empty();
    let has_start = request
        .start
        .as_deref()
        .is_some_and(|value| !value.is_empty());
    let has_end = request
        .end
        .as_deref()
        .is_some_and(|value| !value.is_empty());
    if has_discrete && (has_start || has_end) {
        let mut requested = request.selectors.clone();
        requested.extend(request.start.clone());
        requested.extend(request.end.clone());
        return Ok(invalid_public_release_highlight(
            "invalid",
            requested,
            "离散高亮不能与连续范围同时使用",
        ));
    }
    if has_start != has_end {
        return Ok(invalid_public_release_highlight(
            "invalid",
            request
                .start
                .iter()
                .chain(request.end.iter())
                .cloned()
                .collect(),
            "连续范围必须同时提供起点和终点",
        ));
    }
    if has_discrete && request.selectors.len() > PUBLIC_RELEASE_HIGHLIGHT_ID_LIMIT {
        return Ok(invalid_public_release_highlight(
            "invalid",
            request.selectors.clone(),
            format!("离散高亮最多支持 {PUBLIC_RELEASE_HIGHLIGHT_ID_LIMIT} 个目标"),
        ));
    }
    if !has_discrete && !has_start {
        return Ok(PublicReleaseHighlightResolution {
            spec: None,
            response: None,
            active_release_id: None,
            active_position: None,
        });
    }

    let requested = if has_discrete {
        request.selectors.clone()
    } else {
        vec![request.start.clone().unwrap(), request.end.clone().unwrap()]
    };
    let parsed = requested
        .iter()
        .map(|raw| parse_public_release_typed_selector(raw))
        .collect::<Result<Vec<_>, _>>();
    let Ok(parsed) = parsed else {
        return Ok(invalid_public_release_highlight(
            if has_discrete { "discrete" } else { "range" },
            requested,
            "高亮目标必须使用 tag:<tag_name> 或 id:<release_id>",
        ));
    };
    let mut lookup_selectors = parsed.clone();
    let parsed_active = request
        .active
        .as_deref()
        .and_then(|raw| parse_public_release_typed_selector(raw).ok());
    if let Some(active) = &parsed_active {
        lookup_selectors.push(active.clone());
    }
    let rows = load_public_release_selector_rows(pool, repo_id, &lookup_selectors).await?;

    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen = HashSet::new();
    for (raw, selector) in requested.iter().cloned().zip(parsed.iter()) {
        if let Some(row) = match_public_release_selector(selector, &rows) {
            if seen.insert(row.release_id) {
                resolved.push(ResolvedPublicReleaseSelector {
                    raw,
                    row: row.clone(),
                });
            }
        } else {
            unresolved.push(raw);
        }
    }
    resolved.sort_by_key(|target| target.row.ordinal);

    if !has_discrete && !unresolved.is_empty() {
        return Ok(PublicReleaseHighlightResolution {
            spec: None,
            active_release_id: None,
            active_position: None,
            response: Some(json!({
                "mode": "range",
                "status": "partial",
                "requested": requested,
                "resolved": resolved.iter().map(public_release_target_response).collect::<Vec<_>>(),
                "unresolved": unresolved,
                "total": 0,
                "active_release_id": Value::Null,
                "active_index": Value::Null,
                "message": "连续范围的端点未全部命中，已显示普通最新列表",
            })),
        });
    }

    if has_discrete && resolved.is_empty() {
        return Ok(PublicReleaseHighlightResolution {
            spec: None,
            active_release_id: None,
            active_position: None,
            response: Some(json!({
                "mode": "discrete",
                "status": "partial",
                "requested": requested,
                "resolved": Vec::<Value>::new(),
                "unresolved": unresolved,
                "total": 0,
                "active_release_id": Value::Null,
                "active_index": Value::Null,
                "message": "没有命中可高亮的 Release，已显示普通最新列表",
            })),
        });
    }

    let spec = if has_discrete {
        PublicReleaseHighlightSpec::Ids(
            resolved
                .iter()
                .map(|target| target.row.release_id)
                .collect(),
        )
    } else {
        let start = match_public_release_selector(&parsed[0], &rows)
            .expect("resolved range start selector");
        let end =
            match_public_release_selector(&parsed[1], &rows).expect("resolved range end selector");
        PublicReleaseHighlightSpec::Range {
            start_id: start.release_id,
            end_id: end.release_id,
        }
    };
    let active_release_id = parsed_active
        .as_ref()
        .and_then(|selector| match_public_release_selector(selector, &rows))
        .map(|row| row.release_id)
        .filter(|active_id| {
            if has_discrete {
                resolved
                    .iter()
                    .any(|target| target.row.release_id == *active_id)
            } else {
                true
            }
        })
        .or_else(|| resolved.first().map(|target| target.row.release_id));
    let active_position = active_release_id.and_then(|active_id| {
        rows.iter()
            .find(|row| row.release_id == active_id)
            .map(|row| PublicReleaseCursor {
                sort_ts: row.sort_ts.clone(),
                release_id: row.release_id,
            })
    });
    let active_index = if has_discrete {
        active_release_id.and_then(|active_id| {
            resolved
                .iter()
                .position(|target| target.row.release_id == active_id)
                .map(|index| index as i64 + 1)
        })
    } else {
        None
    };
    let status = if unresolved.is_empty() {
        "complete"
    } else {
        "partial"
    };
    Ok(PublicReleaseHighlightResolution {
        spec: Some(spec),
        active_release_id,
        active_position,
        response: Some(json!({
            "mode": if has_discrete { "discrete" } else { "range" },
            "status": status,
            "requested": requested,
            "resolved": resolved.iter().map(public_release_target_response).collect::<Vec<_>>(),
            "unresolved": unresolved,
            "total": if has_discrete { resolved.len() as i64 } else { 0 },
            "active_release_id": active_release_id.map(|id| id.to_string()),
            "active_index": active_index,
            "message": Value::Null,
        })),
    })
}

fn public_release_highlighted_ids(
    spec: &PublicReleaseHighlightSpec,
    bounds: Option<&PublicReleaseRangeBounds>,
    rows: &[PublicReleaseRow],
) -> HashSet<i64> {
    match spec {
        PublicReleaseHighlightSpec::Ids(ids) => ids.iter().copied().collect(),
        PublicReleaseHighlightSpec::Range { .. } => rows
            .iter()
            .filter(|row| {
                bounds.is_none_or(|bounds| {
                    public_release_position_is_in_range(
                        &public_release_cursor_from_row(row),
                        bounds,
                    )
                })
            })
            .map(|row| row.release_id)
            .collect(),
    }
}

fn sort_public_release_rows(rows: &mut [PublicReleaseRow]) {
    rows.sort_by(|left, right| {
        right
            .sort_ts
            .cmp(&left.sort_ts)
            .then_with(|| right.release_id.cmp(&left.release_id))
    });
}

fn dedupe_public_release_rows(rows: Vec<PublicReleaseRow>) -> Vec<PublicReleaseRow> {
    let mut by_id = HashMap::new();
    for row in rows {
        by_id.entry(row.release_id).or_insert(row);
    }
    let mut rows = by_id.into_values().collect::<Vec<_>>();
    sort_public_release_rows(&mut rows);
    rows
}

fn select_public_release_focus_rows(
    rows: Vec<PublicReleaseRow>,
    highlighted_ids: &HashSet<i64>,
    ordinal_by_id: &HashMap<i64, i64>,
    active_ordinal: i64,
    limit: usize,
) -> Vec<PublicReleaseRow> {
    if rows.len() <= limit {
        return rows;
    }
    let mut highlighted = rows
        .iter()
        .filter(|row| highlighted_ids.contains(&row.release_id))
        .cloned()
        .collect::<Vec<_>>();
    sort_public_release_rows(&mut highlighted);
    if highlighted.len() >= limit {
        highlighted.truncate(limit);
        return highlighted;
    }
    let mut context = rows
        .into_iter()
        .filter(|row| !highlighted_ids.contains(&row.release_id))
        .collect::<Vec<_>>();
    context.sort_by(|left, right| {
        let left_ordinal = ordinal_by_id
            .get(&left.release_id)
            .copied()
            .unwrap_or(i64::MAX);
        let right_ordinal = ordinal_by_id
            .get(&right.release_id)
            .copied()
            .unwrap_or(i64::MAX);
        left_ordinal
            .abs_diff(active_ordinal)
            .cmp(&right_ordinal.abs_diff(active_ordinal))
            .then_with(|| right_ordinal.cmp(&left_ordinal))
            .then_with(|| right.sort_ts.cmp(&left.sort_ts))
            .then_with(|| right.release_id.cmp(&left.release_id))
    });
    highlighted.extend(context.into_iter().take(limit - highlighted.len()));
    sort_public_release_rows(&mut highlighted);
    highlighted
}

async fn count_public_release_positions(
    pool: &PgPool,
    repo_id: i64,
    newer: &PublicReleaseCursor,
    older: &PublicReleaseCursor,
) -> Result<i64, ApiError> {
    sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM public.repo_releases r
        WHERE r.repo_id = $1
          AND r.is_draft = 0
          AND (
            COALESCE(r.published_at, r.created_at, r.updated_at) < $2
            OR (
              COALESCE(r.published_at, r.created_at, r.updated_at) = $2
              AND r.release_id <= $3
            )
          )
          AND (
            COALESCE(r.published_at, r.created_at, r.updated_at) > $4
            OR (
              COALESCE(r.published_at, r.created_at, r.updated_at) = $4
              AND r.release_id >= $5
            )
          )
        "#,
    )
    .bind(repo_id)
    .bind(newer.sort_ts.as_str())
    .bind(newer.release_id)
    .bind(older.sort_ts.as_str())
    .bind(older.release_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn build_public_release_segments(
    pool: &PgPool,
    repo_id: i64,
    rows: &[PublicReleaseRow],
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    if rows.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let selectors = rows
        .iter()
        .map(|row| PublicReleaseTypedSelector::Id(row.release_id))
        .collect::<Vec<_>>();
    let positions = load_public_release_selector_rows(pool, repo_id, &selectors).await?;
    let ordinal_by_id = positions
        .into_iter()
        .map(|row| (row.release_id, row.ordinal))
        .collect::<HashMap<_, _>>();
    let mut segments = Vec::new();
    let mut gaps = Vec::new();
    let mut segment_start = 0usize;
    for index in 1..rows.len() {
        let previous = &rows[index - 1];
        let current = &rows[index];
        let previous_ordinal = ordinal_by_id
            .get(&previous.release_id)
            .copied()
            .unwrap_or(0);
        let current_ordinal = ordinal_by_id.get(&current.release_id).copied().unwrap_or(0);
        if current_ordinal == previous_ordinal + 1 {
            continue;
        }
        segments.push(json!({
            "first_release_id": rows[segment_start].release_id.to_string(),
            "last_release_id": previous.release_id.to_string(),
        }));
        gaps.push(json!({
            "newer_cursor": public_release_cursor(previous),
            "older_cursor": public_release_cursor(current),
            "remaining_count": (current_ordinal - previous_ordinal - 1).max(0),
        }));
        segment_start = index;
    }
    segments.push(json!({
        "first_release_id": rows[segment_start].release_id.to_string(),
        "last_release_id": rows.last().unwrap().release_id.to_string(),
    }));
    Ok((segments, gaps))
}

// ---------------------------------------------------------------------------
// Public release item payloads
// ---------------------------------------------------------------------------

fn public_translation_item(row: &PublicReleaseRow, content: &str) -> Option<Value> {
    if content == "original" || content == "polished" {
        return None;
    }
    let original_title = row
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&row.tag_name)
        .to_owned();
    let original_body = row.body.clone().unwrap_or_default();
    let mut item = if row.global_mode {
        match row.trans_status.as_deref() {
            Some("ready") => translated_ready_item(
                row.trans_title.clone(),
                row.trans_summary.clone(),
                None,
                None,
            )
            .or_else(|| Some(translated_missing_item(true, None))),
            Some("failed") => Some(translated_item_json(
                "error",
                None,
                None,
                Some(false),
                row.trans_error_text.as_deref(),
                None,
            )),
            Some(status) => translated_terminal_item(status, row.trans_error_text.as_deref(), None),
            None => Some(translated_missing_item(true, None)),
        }
    } else {
        let source_hash = crate::content_source_identity::release_detail_source_hash(
            &row.repo_full_name,
            &original_title,
            &original_body,
        );
        if row.trans_source_hash.as_deref() != Some(source_hash.as_str()) {
            Some(translated_missing_item(true, None))
        } else {
            match row.trans_status.as_deref() {
                Some("ready") => translated_ready_item(
                    row.trans_title.clone(),
                    row.trans_summary.clone(),
                    None,
                    None,
                )
                .or_else(|| Some(translated_missing_item(true, None))),
                Some(status) => {
                    translated_terminal_item(status, row.trans_error_text.as_deref(), None)
                }
                None => Some(translated_missing_item(true, None)),
            }
        }
    };
    if let Some(item) = item.as_mut() {
        apply_translation_request_metadata(item, row.trans_request_id.clone(), row.trans_in_flight);
    }
    item
}

fn public_smart_item(row: &PublicReleaseRow, content: &str) -> Option<Value> {
    if content == "original" || content == "translated" {
        return None;
    }
    let original_title = row
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&row.tag_name)
        .to_owned();
    let mut item = if row.global_mode {
        let status = row.smart_status.as_deref().map(|status| {
            smart_status_for_reading(
                status,
                row.smart_reading.as_ref(),
                row.smart_reading_incompatible,
            )
        });
        match status.as_deref() {
            Some("ready") => smart_ready_item_with_reading(
                row.smart_title.clone(),
                row.smart_summary.clone(),
                row.smart_reading.clone(),
                None,
                None,
            )
            .or_else(|| Some(smart_missing_item(None, None))),
            Some("failed") => Some(smart_item_json(
                "error",
                None,
                None,
                Some(false),
                row.smart_error_text.as_deref(),
                None,
                None,
            )),
            Some(status) => smart_terminal_item(status, row.smart_error_text.as_deref(), None),
            None => Some(smart_missing_item(None, None)),
        }
    } else {
        let smart_body = crate::release_content::release_feed_body(row.body.as_deref());
        let source_hash = crate::content_source_identity::release_smart_feed_source_hash(
            row.release_id.to_string().as_str(),
            &row.repo_full_name,
            &original_title,
            smart_body.as_deref(),
            &row.tag_name,
            row.previous_tag_name.as_deref(),
        );
        if row.smart_source_hash.as_deref() != Some(source_hash.as_str()) {
            Some(smart_missing_item(None, None))
        } else {
            match row.smart_status.as_deref() {
                Some("ready") => smart_ready_item(
                    row.smart_title.clone(),
                    row.smart_summary.clone(),
                    None,
                    None,
                )
                .or_else(|| Some(smart_missing_item(None, None))),
                Some(status) => smart_terminal_item(status, row.smart_error_text.as_deref(), None),
                None => Some(smart_missing_item(None, None)),
            }
        }
    };
    if let Some(item) = item.as_mut() {
        apply_translation_request_metadata(item, row.smart_request_id.clone(), row.smart_in_flight);
    }
    item
}

fn public_list_item_from_row(
    row: PublicReleaseRow,
    content: &str,
    is_highlighted: bool,
    is_active_highlight: bool,
) -> Value {
    let translated = public_translation_item(&row, content);
    let smart = public_smart_item(&row, content);
    let repo_visual = repo_visual_json(
        row.owner_avatar_url.clone(),
        row.open_graph_image_url.clone(),
        row.uses_custom_open_graph_image.unwrap_or(0) != 0,
    );
    json!({
        "release_id": row.release_id.to_string(),
        "repo_full_name": row.repo_full_name,
        "repo_visual": repo_visual,
        "tag_name": row.tag_name,
        "previous_tag_name": row.previous_tag_name,
        "name": row.name,
        "body": if content == "translated" { None } else { row.body },
        "html_url": row.html_url,
        "published_at": row.published_at,
        "is_prerelease": row.is_prerelease,
        "is_draft": row.is_draft,
        "is_highlighted": is_highlighted,
        "is_active_highlight": is_active_highlight,
        "translated": translated,
        "smart": smart,
    })
}

// ---------------------------------------------------------------------------
// Public release handlers
// ---------------------------------------------------------------------------

pub(crate) async fn public_list_repo_releases_http(
    State(state): State<PostgresServerState>,
    Path((owner, repo)): Path<(String, String)>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let PublicReleaseHttpQuery {
        query,
        highlight_request,
        focus,
        until_cursor,
        refresh,
    } = parse_public_release_http_query(raw_query.as_deref())?;
    if refresh.is_some()
        && (query
            .cursor
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || until_cursor
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()))
    {
        return Err(ApiError::bad_request(
            "refresh=if_stale is only supported on the first release window",
        ));
    }
    if refresh.is_some() {
        require_public_release_refresh_api_key(&state, &headers).await?;
    }
    public_list_repo_releases_impl(
        &state,
        owner,
        repo,
        PublicReleaseListRequest {
            query,
            highlight_request,
            focus,
            until_cursor,
            refresh_request: refresh,
        },
    )
    .await
}

async fn public_list_repo_releases_impl(
    state: &PostgresServerState,
    owner: String,
    repo: String,
    request: PublicReleaseListRequest,
) -> Result<Response, ApiError> {
    let PublicReleaseListRequest {
        query,
        highlight_request,
        focus,
        until_cursor,
        refresh_request,
    } = request;
    validate_public_release_query(&query)?;
    let usage = upsert_public_release_usage(
        &state.runtime.pool,
        owner,
        repo,
        false,
        public_source_is_page(&query),
    )
    .await?;
    ensure_public_release_usage_accessible(&usage)?;
    let Some(repo_id) = usage.repo_id else {
        return Ok(public_pending_response(&usage));
    };
    if refresh_request.is_some() {
        // The legacy handler refreshed the repository release cache through the
        // SQLite sync engine and served cached rows when that attach failed.
        // The PostgreSQL worker has no public repo release sync executor, so the
        // cached window is served directly.
        tracing::warn!(
            repo_id,
            repo_full_name = usage.full_name.as_str(),
            "public release refresh requested; PostgreSQL public repo release sync is unavailable"
        );
    }
    let explicit_active_requested = highlight_request.active.as_deref().is_some();
    let mut highlight_resolution = if highlight_request.is_empty() {
        PublicReleaseHighlightResolution {
            spec: None,
            response: None,
            active_release_id: None,
            active_position: None,
        }
    } else {
        resolve_public_release_highlight(&state.runtime.pool, repo_id, &highlight_request).await?
    };
    let highlight_spec = highlight_resolution.spec.clone();
    let direction = parse_public_release_cursor_direction(query.direction.as_deref())?;
    let cursor = query
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse_public_release_cursor)
        .transpose()?;
    let until_cursor = until_cursor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse_public_release_cursor)
        .transpose()?;
    if until_cursor.is_some() && cursor.is_none() {
        return Err(ApiError::bad_request("until_cursor requires cursor"));
    }
    let focus_selector = focus
        .as_deref()
        .map(parse_public_release_typed_selector)
        .transpose()
        .map_err(|_| ApiError::bad_request("focus must be id:<id> or tag:<tag>"))?;
    if focus_selector.is_some() && cursor.is_some() {
        return Err(ApiError::bad_request(
            "focus is only supported on the first release window",
        ));
    }
    let focus_position = if let Some(selector) = focus_selector.as_ref() {
        let row = load_public_release_selector_rows(
            &state.runtime.pool,
            repo_id,
            std::slice::from_ref(selector),
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "release_not_found_or_not_cached",
                "release not found or not cached",
            )
        })?;
        Some(PublicReleaseCursor {
            sort_ts: row.sort_ts,
            release_id: row.release_id,
        })
    } else {
        None
    };
    let gap_bounds = cursor
        .as_ref()
        .zip(until_cursor.as_ref())
        .map(|(cursor, until)| match direction {
            PublicReleaseCursorDirection::Older => PublicReleaseRangeBounds {
                newer: cursor.clone(),
                older: until.clone(),
            },
            PublicReleaseCursorDirection::Newer => PublicReleaseRangeBounds {
                newer: until.clone(),
                older: cursor.clone(),
            },
        });
    let content = query.content.as_deref().unwrap_or("all");
    let pool = &state.runtime.pool;
    let mut range_bounds: Option<PublicReleaseRangeBounds> = None;
    let mut recommended_next_cursor = None;
    let mut recommended_previous_cursor = None;
    let mut limit = query.limit.unwrap_or(6).clamp(1, 30);
    let mut rows = match highlight_spec.as_ref() {
        None => {
            load_public_release_rows(
                pool,
                usage.full_name.as_str(),
                repo_id,
                None,
                None,
                gap_bounds.as_ref(),
                cursor.as_ref(),
                direction,
                limit.saturating_add(1),
                content,
            )
            .await?
        }
        Some(PublicReleaseHighlightSpec::Ids(ids)) => {
            limit = query
                .limit
                .unwrap_or(PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT)
                .clamp(1, PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT)
                .max(ids.len() as i64);
            if cursor.is_some() {
                load_public_release_rows(
                    pool,
                    usage.full_name.as_str(),
                    repo_id,
                    None,
                    None,
                    gap_bounds.as_ref(),
                    cursor.as_ref(),
                    direction,
                    limit.saturating_add(1),
                    content,
                )
                .await?
            } else {
                let highlighted_ids = ids.iter().copied().collect::<HashSet<_>>();
                let mut focus_rows = load_public_release_rows(
                    pool,
                    usage.full_name.as_str(),
                    repo_id,
                    None,
                    Some(ids.as_slice()),
                    None,
                    None,
                    PublicReleaseCursorDirection::Older,
                    ids.len() as i64,
                    content,
                )
                .await?;
                let mut has_more_newer = false;
                let mut has_more_older = false;
                if let Some(active_position) = highlight_resolution.active_position.as_ref() {
                    let newer_rows = load_public_release_rows(
                        pool,
                        usage.full_name.as_str(),
                        repo_id,
                        None,
                        None,
                        None,
                        Some(active_position),
                        PublicReleaseCursorDirection::Newer,
                        PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT.saturating_add(1),
                        content,
                    )
                    .await?;
                    has_more_newer =
                        newer_rows.len() > PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT as usize;
                    focus_rows.extend(newer_rows);

                    let older_rows = load_public_release_rows(
                        pool,
                        usage.full_name.as_str(),
                        repo_id,
                        None,
                        None,
                        None,
                        Some(active_position),
                        PublicReleaseCursorDirection::Older,
                        PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT.saturating_add(1),
                        content,
                    )
                    .await?;
                    has_more_older =
                        older_rows.len() > PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT as usize;
                    focus_rows.extend(older_rows);
                }

                let candidate_rows = dedupe_public_release_rows(focus_rows);
                let candidate_first_id = candidate_rows.first().map(|row| row.release_id);
                let candidate_last_id = candidate_rows.last().map(|row| row.release_id);
                let selected_rows = if candidate_rows.len() > limit as usize {
                    let selectors = candidate_rows
                        .iter()
                        .map(|row| PublicReleaseTypedSelector::Id(row.release_id))
                        .collect::<Vec<_>>();
                    let ordinal_by_id =
                        load_public_release_selector_rows(pool, repo_id, &selectors)
                            .await?
                            .into_iter()
                            .map(|row| (row.release_id, row.ordinal))
                            .collect::<HashMap<_, _>>();
                    let active_ordinal = highlight_resolution
                        .active_position
                        .as_ref()
                        .and_then(|position| ordinal_by_id.get(&position.release_id))
                        .copied()
                        .unwrap_or(0);
                    select_public_release_focus_rows(
                        candidate_rows,
                        &highlighted_ids,
                        &ordinal_by_id,
                        active_ordinal,
                        limit as usize,
                    )
                } else {
                    candidate_rows
                };
                if let Some(first) = selected_rows.first()
                    && (has_more_newer || candidate_first_id != Some(first.release_id))
                {
                    recommended_previous_cursor = Some(public_release_cursor(first));
                }
                if let Some(last) = selected_rows.last()
                    && (has_more_older || candidate_last_id != Some(last.release_id))
                {
                    recommended_next_cursor = Some(public_release_cursor(last));
                }
                selected_rows
            }
        }
        Some(PublicReleaseHighlightSpec::Range { start_id, end_id }) => {
            let boundary_ids = [*start_id, *end_id];
            let boundary_rows = load_public_release_rows(
                pool,
                usage.full_name.as_str(),
                repo_id,
                None,
                Some(&boundary_ids),
                None,
                None,
                PublicReleaseCursorDirection::Older,
                2,
                content,
            )
            .await?;
            let boundary_by_id = boundary_rows
                .iter()
                .map(|row| (row.release_id, row))
                .collect::<HashMap<_, _>>();
            let unresolved_ids = boundary_ids
                .iter()
                .copied()
                .filter(|id| !boundary_by_id.contains_key(id))
                .collect::<Vec<_>>();

            if unresolved_ids.is_empty() {
                let start = public_release_cursor_from_row(boundary_by_id[start_id]);
                let end = public_release_cursor_from_row(boundary_by_id[end_id]);
                let (newer, older) =
                    if compare_public_release_positions(&start, &end) == Ordering::Less {
                        (start, end)
                    } else {
                        (end, start)
                    };
                range_bounds = Some(PublicReleaseRangeBounds { newer, older });
                if let Some(response) = highlight_resolution.response.as_mut() {
                    let bounds = range_bounds.as_ref().expect("range bounds");
                    response["total"] = json!(
                        count_public_release_positions(pool, repo_id, &bounds.newer, &bounds.older)
                            .await?
                    );
                    let active_position = highlight_resolution.active_position.clone();
                    if let Some(active_position) = active_position
                        .filter(|position| public_release_position_is_in_range(position, bounds))
                    {
                        response["active_index"] = json!(
                            count_public_release_positions(
                                pool,
                                repo_id,
                                &bounds.newer,
                                &active_position
                            )
                            .await?
                        );
                    } else {
                        let bounds = range_bounds.as_ref().expect("range bounds");
                        highlight_resolution.active_release_id = Some(bounds.newer.release_id);
                        highlight_resolution.active_position = Some(bounds.newer.clone());
                        response["active_release_id"] = json!(bounds.newer.release_id.to_string());
                        response["active_index"] = json!(1);
                    }
                }
                limit = query
                    .limit
                    .unwrap_or(PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT)
                    .clamp(1, PUBLIC_RELEASE_HIGHLIGHT_WINDOW_LIMIT);
                if cursor.is_none() && explicit_active_requested {
                    let active_position = highlight_resolution
                        .active_position
                        .as_ref()
                        .expect("resolved range active position")
                        .clone();
                    let newer_limit = (limit.saturating_sub(1)) / 2;
                    let older_limit = limit.saturating_sub(1 + newer_limit);
                    let mut centered_rows = load_public_release_rows(
                        pool,
                        usage.full_name.as_str(),
                        repo_id,
                        None,
                        None,
                        gap_bounds.as_ref().or(range_bounds.as_ref()),
                        Some(&active_position),
                        PublicReleaseCursorDirection::Newer,
                        newer_limit.saturating_add(1),
                        content,
                    )
                    .await?;
                    centered_rows.truncate(newer_limit as usize);
                    centered_rows.extend(
                        load_public_release_rows(
                            pool,
                            usage.full_name.as_str(),
                            repo_id,
                            None,
                            Some(&[active_position.release_id]),
                            range_bounds.as_ref(),
                            None,
                            PublicReleaseCursorDirection::Older,
                            1,
                            content,
                        )
                        .await?,
                    );
                    centered_rows.extend(
                        load_public_release_rows(
                            pool,
                            usage.full_name.as_str(),
                            repo_id,
                            None,
                            None,
                            range_bounds.as_ref(),
                            Some(&active_position),
                            PublicReleaseCursorDirection::Older,
                            older_limit,
                            content,
                        )
                        .await?,
                    );
                    if let Some(first) = centered_rows.first()
                        && public_release_cursor_from_row(first)
                            != range_bounds.as_ref().expect("range bounds").newer
                    {
                        recommended_previous_cursor = Some(public_release_cursor(first));
                    }
                    if let Some(last) = centered_rows.last()
                        && public_release_cursor_from_row(last)
                            != range_bounds.as_ref().expect("range bounds").older
                    {
                        recommended_next_cursor = Some(public_release_cursor(last));
                    }
                    centered_rows
                } else {
                    load_public_release_rows(
                        pool,
                        usage.full_name.as_str(),
                        repo_id,
                        None,
                        None,
                        gap_bounds.as_ref().or(range_bounds.as_ref()),
                        cursor.as_ref(),
                        direction,
                        limit.saturating_add(1),
                        content,
                    )
                    .await?
                }
            } else {
                limit = boundary_rows.len() as i64;
                boundary_rows
            }
        }
    };
    if let Some(focus_position) = focus_position.as_ref() {
        let required_highlight_ids = match highlight_spec.as_ref() {
            Some(PublicReleaseHighlightSpec::Ids(ids)) => ids.iter().copied().collect(),
            _ => HashSet::new(),
        };
        let mut required_focus_ids = required_highlight_ids.clone();
        required_focus_ids.insert(focus_position.release_id);
        let focus_limit = query
            .limit
            .unwrap_or(limit)
            .clamp(1, 30)
            .max(required_focus_ids.len() as i64);
        let newer_limit = (focus_limit.saturating_sub(1)) / 2;
        let older_limit = focus_limit.saturating_sub(1 + newer_limit);
        let mut focused_rows = load_public_release_rows(
            pool,
            usage.full_name.as_str(),
            repo_id,
            None,
            Some(&[focus_position.release_id]),
            None,
            None,
            PublicReleaseCursorDirection::Older,
            1,
            content,
        )
        .await?;
        let mut newer_rows = load_public_release_rows(
            pool,
            usage.full_name.as_str(),
            repo_id,
            None,
            None,
            None,
            Some(focus_position),
            PublicReleaseCursorDirection::Newer,
            newer_limit.saturating_add(1),
            content,
        )
        .await?;
        let has_more_newer = newer_rows.len() > newer_limit as usize;
        newer_rows.truncate(newer_limit as usize);
        focused_rows.extend(newer_rows);
        let mut older_rows = load_public_release_rows(
            pool,
            usage.full_name.as_str(),
            repo_id,
            None,
            None,
            None,
            Some(focus_position),
            PublicReleaseCursorDirection::Older,
            older_limit.saturating_add(1),
            content,
        )
        .await?;
        let has_more_older = older_rows.len() > older_limit as usize;
        older_rows.truncate(older_limit as usize);
        focused_rows.extend(older_rows);
        if let Some(PublicReleaseHighlightSpec::Ids(ids)) = highlight_spec.as_ref() {
            focused_rows.extend(
                load_public_release_rows(
                    pool,
                    usage.full_name.as_str(),
                    repo_id,
                    None,
                    Some(ids.as_slice()),
                    None,
                    None,
                    PublicReleaseCursorDirection::Older,
                    ids.len() as i64,
                    content,
                )
                .await?,
            );
        }
        focused_rows.extend(rows);
        rows = dedupe_public_release_rows(focused_rows);
        sort_public_release_rows(&mut rows);
        if rows.len() > focus_limit as usize {
            let selectors = rows
                .iter()
                .map(|row| PublicReleaseTypedSelector::Id(row.release_id))
                .collect::<Vec<_>>();
            let ordinal_by_id = load_public_release_selector_rows(pool, repo_id, &selectors)
                .await?
                .into_iter()
                .map(|row| (row.release_id, row.ordinal))
                .collect::<HashMap<_, _>>();
            let active_ordinal = ordinal_by_id
                .get(&focus_position.release_id)
                .copied()
                .unwrap_or(0);
            rows = select_public_release_focus_rows(
                rows,
                &required_focus_ids,
                &ordinal_by_id,
                active_ordinal,
                focus_limit as usize,
            );
        }
        sort_public_release_rows(&mut rows);
        limit = limit.max(focus_limit);
        if has_more_newer {
            recommended_previous_cursor = rows.first().map(public_release_cursor);
        }
        if has_more_older {
            recommended_next_cursor = rows.last().map(public_release_cursor);
        }
    }
    if rows.is_empty() && public_release_usage_should_retry_without_rows(&usage) {
        return Ok(public_pending_response(&usage));
    }

    let has_more = rows.len() > limit as usize;
    let mut next_cursor = recommended_next_cursor;
    let mut previous_cursor = recommended_previous_cursor;
    if has_more {
        match direction {
            PublicReleaseCursorDirection::Older => {
                next_cursor = rows
                    .get(limit.saturating_sub(1) as usize)
                    .map(public_release_cursor);
            }
            PublicReleaseCursorDirection::Newer => {
                previous_cursor = rows.first().map(public_release_cursor);
            }
        }
        rows.truncate(limit as usize);
    }
    if highlight_spec.is_some() && cursor.is_some() && !rows.is_empty() {
        match direction {
            PublicReleaseCursorDirection::Older => {
                previous_cursor = rows.first().map(public_release_cursor);
            }
            PublicReleaseCursorDirection::Newer => {
                next_cursor = rows.last().map(public_release_cursor);
            }
        }
    }
    let highlighted_ids = highlight_spec
        .as_ref()
        .map(|spec| public_release_highlighted_ids(spec, range_bounds.as_ref(), &rows))
        .unwrap_or_default();
    let active_release_id = highlight_resolution.active_release_id;
    let (segments, gaps) = if highlight_resolution.response.is_some() {
        let (segments, gaps) = build_public_release_segments(pool, repo_id, &rows).await?;
        (Some(segments), Some(gaps))
    } else {
        (None, None)
    };
    let items = rows
        .into_iter()
        .map(|row| {
            let is_highlighted = highlighted_ids.contains(&row.release_id);
            let is_active_highlight = active_release_id == Some(row.release_id);
            public_list_item_from_row(row, content, is_highlighted, is_active_highlight)
        })
        .collect::<Vec<_>>();
    let mut body = json!({
        "status": "ready",
        "repo_full_name": usage.full_name,
        "next_cursor": next_cursor,
        "items": items,
    });
    if let Some(previous_cursor) = previous_cursor {
        body["previous_cursor"] = json!(previous_cursor);
    }
    if let Some(highlight) = highlight_resolution.response {
        body["highlight"] = highlight;
    }
    if let Some(segments) = segments {
        body["segments"] = json!(segments);
    }
    if let Some(gaps) = gaps {
        body["gaps"] = json!(gaps);
    }
    Ok(Json(body).into_response())
}

#[derive(Debug, Deserialize)]
pub(crate) struct PublicReleaseContentQuery {
    release_ids: String,
    content: String,
    lang: Option<String>,
}

pub(crate) async fn public_get_repo_release_content(
    State(state): State<PostgresServerState>,
    Path((owner, repo)): Path<(String, String)>,
    Query(query): Query<PublicReleaseContentQuery>,
) -> Result<Response, ApiError> {
    if query.lang.as_deref().unwrap_or("zh-CN") != "zh-CN" {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_language",
            "only zh-CN is supported",
        ));
    }
    if !matches!(query.content.as_str(), "translated" | "polished") {
        return Err(ApiError::bad_request(
            "content must be translated or polished",
        ));
    }
    let mut release_ids = Vec::new();
    let mut seen = HashSet::new();
    for raw in query.release_ids.split(',') {
        let release_id = parse_public_release_id(raw)?;
        if seen.insert(release_id) {
            release_ids.push(release_id);
        }
    }
    if release_ids.is_empty() || release_ids.len() > 30 {
        return Err(ApiError::bad_request(
            "release_ids must contain 1 to 30 ids",
        ));
    }
    let usage = upsert_public_release_usage(&state.runtime.pool, owner, repo, false, true).await?;
    ensure_public_release_usage_accessible(&usage)?;
    let Some(repo_id) = usage.repo_id else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "release_cache_not_ready",
            "release data is not ready",
        ));
    };
    let rows = load_public_release_rows(
        &state.runtime.pool,
        usage.full_name.as_str(),
        repo_id,
        None,
        Some(release_ids.as_slice()),
        None,
        None,
        PublicReleaseCursorDirection::Older,
        release_ids.len() as i64,
        query.content.as_str(),
    )
    .await?;
    Ok(Json(json!({
        "items": rows
            .into_iter()
            .map(|row| public_list_item_from_row(row, query.content.as_str(), false, false))
            .collect::<Vec<_>>(),
    }))
    .into_response())
}

pub(crate) async fn public_get_repo_release_detail(
    State(state): State<PostgresServerState>,
    Path((owner, repo, tag)): Path<(String, String, String)>,
    Query(query): Query<PublicReleaseQuery>,
) -> Result<Response, ApiError> {
    validate_public_release_query(&query)?;
    let usage = upsert_public_release_usage(
        &state.runtime.pool,
        owner,
        repo,
        true,
        public_source_is_page(&query),
    )
    .await?;
    ensure_public_release_usage_accessible(&usage)?;
    let Some(repo_id) = usage.repo_id else {
        return Ok(public_pending_response(&usage));
    };
    let mut rows = load_public_release_rows(
        &state.runtime.pool,
        usage.full_name.as_str(),
        repo_id,
        Some(tag.as_str()),
        None,
        None,
        None,
        PublicReleaseCursorDirection::Older,
        1,
        query.content.as_deref().unwrap_or("all"),
    )
    .await?;
    let Some(row) = rows.pop() else {
        if public_release_usage_should_retry_without_rows(&usage) {
            return Ok(public_pending_response(&usage));
        }
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "release_not_found_or_not_cached",
            "release not found or not cached",
        ));
    };
    let content = query.content.as_deref().unwrap_or("all");
    let translated = public_translation_item(&row, content);
    let smart = public_smart_item(&row, content);
    let repo_visual = repo_visual_json(
        row.owner_avatar_url.clone(),
        row.open_graph_image_url.clone(),
        row.uses_custom_open_graph_image.unwrap_or(0) != 0,
    );
    Ok(Json(json!({
        "release_id": row.release_id.to_string(),
        "repo_full_name": Some(row.repo_full_name),
        "repo_visual": repo_visual,
        "tag_name": row.tag_name,
        "previous_tag_name": row.previous_tag_name,
        "name": row.name,
        "body": if content == "translated" { None } else { row.body },
        "html_url": row.html_url,
        "published_at": row.published_at,
        "is_prerelease": row.is_prerelease,
        "is_draft": row.is_draft,
        "translated": translated,
        "smart": smart,
    }))
    .into_response())
}
