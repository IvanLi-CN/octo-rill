//! PostgreSQL dashboard read surface.
//!
//! Owns the feed item projection, the dashboard readable date sections, the
//! section item cursor pages, and signature-based `/api/dashboard/updates`
//! change detection. Response shapes mirror the legacy SQLite handlers in
//! `api.rs`; only the persistence layer differs (PostgreSQL, `public.` tables).
//!
//! Content-processing payloads come from the legacy local translation tables
//! (`ai_translations` / `translation_work_items`) unless
//! `public.content_processing_control.mode = 'global'`. In global mode
//! [`postgres_content_overlay`] replaces the per-row translation/smart fields
//! with the content-processing projections before the feed assembly runs.

use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool};
use tower_sessions::Session;

use crate::{
    briefs::{self, DailyBriefPreferences},
    postgres_content_overlay,
    postgres_server::PostgresServerState,
    release_links::{
        InternalReleaseRef, ReleaseLocator, parse_internal_release_ref,
        parse_release_locator_from_github_release_url,
    },
};

const SESSION_KEY_USER_ID: &str = "user_id";
const DASHBOARD_UPDATES_SIGNATURE_VERSION: u8 = 1;
const DASHBOARD_READABLE_SECTION_LIMIT: usize = 3;
const DASHBOARD_READABLE_SECTION_ITEM_LIMIT: usize = 30;
const DASHBOARD_READABLE_SUPPLEMENTAL_FETCH_LIMIT: i64 = 10_000;
const RELEASE_FEED_BODY_MAX_CHARS: usize = 3_000;
const BRIEF_RELEASE_REF_LOCATOR_BATCH_LIMIT: usize = 300;
const RELEASE_FEED_MARKDOWN_MISMATCH_ERROR: &str =
    "release translation failed to preserve markdown structure";
const RELEASE_DETAIL_MARKDOWN_MISMATCH_ERROR: &str =
    "release detail translation failed to preserve markdown structure";
const ANNOUNCEMENT_DETAIL_MARKDOWN_MISMATCH_ERROR: &str =
    "announcement detail translation failed to preserve markdown structure";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"code": "unauthorized", "message": "login required"})),
    )
        .into_response()
}

fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"code": "bad_request", "message": message.into()})),
    )
        .into_response()
}

fn internal_error(message: impl Into<String>) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code": "internal_error", "message": message.into()})),
    )
        .into_response()
}

fn database_error(operation: &str, error: sqlx::Error) -> Response {
    tracing::warn!(%error, operation, "PostgreSQL dashboard query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code": "database_error"})),
    )
        .into_response()
}

fn not_found_inaccessible() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "code": "repository_inaccessible",
            "message": "repository is not publicly accessible"
        })),
    )
        .into_response()
}

async fn require_user(session: &Session) -> Result<String, Response> {
    session
        .get::<String>(SESSION_KEY_USER_ID)
        .await
        .ok()
        .flatten()
        .ok_or_else(unauthorized)
}

// ---------------------------------------------------------------------------
// Wire types (legacy response parity)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct RepoVisual {
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: bool,
}

#[derive(Debug, Clone, Serialize)]
struct FeedActor {
    login: String,
    avatar_url: Option<String>,
    html_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TranslatedItem {
    lang: String,
    status: String,
    title: Option<String>,
    summary: Option<String>,
    error_code: Option<String>,
    error_summary: Option<String>,
    error_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_translate: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct SmartItem {
    lang: String,
    status: String,
    title: Option<String>,
    summary: Option<String>,
    error_code: Option<String>,
    error_summary: Option<String>,
    error_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_translate: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct ReleaseReactionCounts {
    plus1: i64,
    laugh: i64,
    heart: i64,
    hooray: i64,
    rocket: i64,
    eyes: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
struct ReleaseReactionViewer {
    plus1: bool,
    laugh: bool,
    heart: bool,
    hooray: bool,
    rocket: bool,
    eyes: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ReleaseReactions {
    counts: ReleaseReactionCounts,
    viewer: ReleaseReactionViewer,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
struct FeedItem {
    kind: String,
    ts: String,
    id: String,
    repo_full_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discussion_number: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discussion_key: Option<String>,
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
struct FeedResponse {
    items: Vec<FeedItem>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FeedQuery {
    cursor: Option<String>,
    limit: Option<i64>,
    types: Option<String>,
    scope: Option<String>,
    items: Option<String>,
    org: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DashboardUpdatesQuery {
    token: Option<String>,
    feed_type: Option<String>,
    include: Option<String>,
    scope: Option<String>,
    items: Option<String>,
    org: Option<String>,
}

#[derive(Debug, Serialize)]
struct DashboardUpdatesResponse {
    token: String,
    generated_at: String,
    lists: DashboardUpdatesLists,
}

#[derive(Debug, Serialize)]
struct DashboardUpdatesLists {
    #[serde(skip_serializing_if = "Option::is_none")]
    feed: Option<DashboardListUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    briefs: Option<DashboardListUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notifications: Option<DashboardListUpdate>,
}

#[derive(Debug, Serialize)]
struct DashboardListUpdate {
    changed: bool,
    new_count: usize,
    latest_keys: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct DashboardUpdatesToken {
    feed: Vec<String>,
    briefs: Vec<String>,
    notifications: Vec<String>,
    #[serde(default)]
    feed_version: u8,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DashboardReadableFeedQuery {
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DashboardReadableSectionItemsQuery {
    cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct DashboardReadableFeedResponse {
    sections: Vec<DashboardReadableSection>,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct DashboardReadableSection {
    id: String,
    #[serde(rename = "display_date")]
    date: String,
    kind: String,
    #[serde(rename = "window_start")]
    window_start: Option<String>,
    #[serde(rename = "window_end")]
    window_end: Option<String>,
    brief: Option<DashboardReadableBrief>,
    items: Vec<FeedItem>,
    supplemental_items: Vec<FeedItem>,
    supplemental_next_cursor: Option<String>,
    items_next_cursor: Option<String>,
    #[serde(rename = "activity_count")]
    item_count: usize,
    can_generate_brief: bool,
}

#[derive(Debug, Serialize, Clone)]
struct DashboardReadableBrief {
    id: String,
    date: String,
    window_start: Option<String>,
    window_end: Option<String>,
    effective_time_zone: Option<String>,
    effective_local_boundary: Option<String>,
    release_count: usize,
    covers_repo_stars: bool,
    covers_followers: bool,
    content_markdown: String,
    created_at: String,
    updated_at: String,
}

// ---------------------------------------------------------------------------
// Feed type / scope selection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct FeedTypeSelection {
    releases: bool,
    stars: bool,
    followers: bool,
    ambient: bool,
}

impl FeedTypeSelection {
    fn all() -> Self {
        Self {
            releases: true,
            stars: true,
            followers: true,
            ambient: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FeedScope {
    Repo { owner: String, repo: String },
    Repos { items: Vec<String> },
    Org { org: String },
    Mine,
    Following,
}

fn parse_feed_types(types: Option<&str>) -> Result<FeedTypeSelection, Response> {
    let Some(types) = types else {
        return Ok(FeedTypeSelection::all());
    };
    let mut selection = FeedTypeSelection {
        releases: false,
        stars: false,
        followers: false,
        ambient: false,
    };
    let mut saw_any = false;
    for part in types.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        saw_any = true;
        match part {
            "releases" | "release" => selection.releases = true,
            "stars" | "star" => selection.stars = true,
            "followers" | "follower" => selection.followers = true,
            "notifications" | "notification" | "inbox" => {
                return Err(bad_request(
                    "feed does not include inbox items; use /api/notifications for inbox",
                ));
            }
            _ => return Err(bad_request(format!("invalid types: {part}"))),
        }
    }
    if !saw_any {
        return Ok(FeedTypeSelection::all());
    }
    Ok(selection)
}

fn normalize_repo_scope_item(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('/');
    let (owner, repo) = trimmed.split_once('/')?;
    let owner = owner.trim();
    let repo = repo.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn parse_feed_scope(
    scope: Option<&str>,
    items: Option<&str>,
    org: Option<&str>,
) -> Result<Option<FeedScope>, Response> {
    let Some(scope) = scope.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    match scope {
        "repo" => {
            let repo_item = items
                .and_then(normalize_repo_scope_item)
                .ok_or_else(|| bad_request("scope=repo requires items=owner/repo"))?;
            let (owner, repo) = repo_item
                .split_once('/')
                .ok_or_else(|| bad_request("invalid repo scope item"))?;
            Ok(Some(FeedScope::Repo {
                owner: owner.to_owned(),
                repo: repo.to_owned(),
            }))
        }
        "repos" => {
            let mut deduped = Vec::new();
            let mut seen = HashSet::new();
            for raw in items
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let normalized = normalize_repo_scope_item(raw)
                    .ok_or_else(|| bad_request("invalid repo scope item"))?;
                let key = normalized.to_ascii_lowercase();
                if seen.insert(key) {
                    deduped.push(normalized);
                }
            }
            if deduped.is_empty() {
                return Ok(Some(FeedScope::Repos { items: Vec::new() }));
            }
            if deduped.len() > 12 {
                return Err(bad_request("scope repos items exceeds limit"));
            }
            Ok(Some(FeedScope::Repos { items: deduped }))
        }
        "org" => {
            let org = org
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| bad_request("scope=org requires org"))?;
            Ok(Some(FeedScope::Org {
                org: org.to_owned(),
            }))
        }
        "mine" => Ok(Some(FeedScope::Mine)),
        "following" => Ok(Some(FeedScope::Following)),
        value => Err(bad_request(format!("invalid scope: {value}"))),
    }
}

fn parse_dashboard_update_include(raw: Option<&str>) -> Result<[bool; 3], Response> {
    let Some(raw) = raw else {
        return Ok([true, true, true]);
    };
    let mut include = [false, false, false];
    let mut saw_any = false;
    for part in raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        saw_any = true;
        match part {
            "feed" => include[0] = true,
            "briefs" => include[1] = true,
            "notifications" | "inbox" => include[2] = true,
            _ => return Err(bad_request(format!("invalid include: {part}"))),
        }
    }
    if saw_any {
        Ok(include)
    } else {
        Ok([true, true, true])
    }
}

fn parse_dashboard_feed_type(raw: Option<&str>) -> Result<FeedTypeSelection, Response> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("all") => Ok(FeedTypeSelection::all()),
        Some("releases") => parse_feed_types(Some("releases")),
        Some("stars") => parse_feed_types(Some("stars")),
        Some("followers") => parse_feed_types(Some("followers")),
        Some(value) => Err(bad_request(format!("invalid feed_type: {value}"))),
    }
}

fn feed_kind_rank(kind: &str) -> Option<i64> {
    match kind {
        "release" => Some(5),
        "release_update" => Some(4),
        "announcement" => Some(4),
        "repo_forked" => Some(3),
        "repo_star_received" => Some(2),
        "follower_received" => Some(1),
        "notification" => Some(0),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct StreamCursor {
    sort_ts: String,
    kind_rank: i64,
    id_key: String,
}

fn parse_cursor(cursor: &str) -> Result<(String, i64, String), Response> {
    let mut it = cursor.split('|');
    let sort_ts = it
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad_request("invalid cursor"))?;
    let kind = it
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad_request("invalid cursor"))?;
    let id_key = it
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| bad_request("invalid cursor"))?;
    if it.next().is_some() {
        return Err(bad_request("invalid cursor"));
    }
    let kind_rank =
        feed_kind_rank(kind.as_str()).ok_or_else(|| bad_request("invalid cursor kind"))?;
    Ok((sort_ts, kind_rank, id_key))
}

fn parse_feed_cursor(cursor: &str) -> Result<StreamCursor, Response> {
    let (sort_ts, kind_rank, id_key) = parse_cursor(cursor)?;
    Ok(StreamCursor {
        sort_ts,
        kind_rank,
        id_key,
    })
}

// ---------------------------------------------------------------------------
// Feed row projection
// ---------------------------------------------------------------------------

#[derive(Debug, FromRow)]
struct FeedRow {
    kind: String,
    #[allow(dead_code)]
    sort_ts: String,
    ts: String,
    id_key: String,
    entity_id: String,
    translation_entity_id: Option<String>,
    release_id: Option<i64>,
    release_repo_id: Option<i64>,
    release_node_id: Option<String>,
    repo_full_name: Option<String>,
    discussion_number: Option<i64>,
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

async fn fetch_feed_items(
    pool: &PgPool,
    user_id: &str,
    cursor: Option<&StreamCursor>,
    types: FeedTypeSelection,
    scope: Option<&FeedScope>,
    viewer_login: Option<&str>,
    limit: i64,
    time_window: Option<(&str, &str)>,
) -> Result<Vec<FeedRow>, sqlx::Error> {
    // The projection contract is the legacy SQLite feed query, retargeted to
    // PostgreSQL: `json_each` becomes `unnest(text[])`, and the release ids
    // are zero-padded through `lpad` to keep the text cursor ordering
    // identical.
    let sql = r#"
        WITH scoped_visible_repos AS (
          SELECT
            vr.repo_id,
            vr.full_name,
            vr.owner_login,
            vr.name,
            vr.owner_avatar_url,
            vr.open_graph_image_url,
            vr.uses_custom_open_graph_image
          FROM public.user_release_visible_repos vr
          WHERE vr.user_id = $1
            AND $2 <> 'mine'
            AND (
              $3 = ''
              OR ($4 = 'org' AND lower(vr.owner_login) = lower($5))
            )
          UNION
          SELECT
            fr.repo_id,
            fr.full_name,
            fr.owner_login,
            fr.name,
            fr.owner_avatar_url,
            fr.open_graph_image_url,
            fr.uses_custom_open_graph_image
          FROM public.user_following_repos fr
          WHERE fr.user_id = $6
            AND $7 = 'following'
          UNION
          SELECT
            ura.repo_id,
            ura.repo_full_name AS full_name,
            ura.owner_login,
            ura.repo_name AS name,
            ura.owner_avatar_url,
            ura.open_graph_image_url,
            ura.uses_custom_open_graph_image
          FROM public.user_repo_associations ura
          WHERE ura.user_id = $8
            AND ura.repo_id IS NOT NULL
            AND (
              ($9 = 'repo' AND lower(ura.repo_full_name) = lower($10))
              OR (
                $11 = 'repos'
                AND EXISTS (
                  SELECT 1
                  FROM unnest($12::text[]) AS item(value)
                  WHERE lower(item.value) = lower(ura.repo_full_name)
                )
              )
              OR ($13 = 'org' AND lower(ura.owner_login) = lower($14))
            )
          UNION
          SELECT
            sr.repo_id,
            sr.full_name,
            sr.owner_login,
            sr.name,
            sr.owner_avatar_url,
            sr.open_graph_image_url,
            sr.uses_custom_open_graph_image
          FROM public.starred_repos sr
          WHERE sr.user_id = $15
            AND NOT EXISTS (
              SELECT 1
              FROM public.user_repo_associations ura
              WHERE ura.user_id = sr.user_id
                AND ura.repo_full_name_lower = lower(sr.full_name)
            )
            AND (
              ($16 = 'repo' AND lower(sr.full_name) = lower($17))
              OR (
                $18 = 'repos'
                AND EXISTS (
                  SELECT 1
                  FROM unnest($19::text[]) AS item(value)
                  WHERE lower(item.value) = lower(sr.full_name)
                )
              )
              OR ($20 = 'org' AND lower(sr.owner_login) = lower($21))
            )
          UNION
          SELECT
            ob.repo_id AS repo_id,
            ob.repo_full_name AS full_name,
            split_part(ob.repo_full_name, '/', 1) AS owner_login,
            split_part(ob.repo_full_name, '/', 2) AS name,
            ob.owner_avatar_url AS owner_avatar_url,
            ob.open_graph_image_url AS open_graph_image_url,
            ob.uses_custom_open_graph_image AS uses_custom_open_graph_image
          FROM public.owned_repo_star_baselines ob
          WHERE ob.user_id = $22
            AND $23 = 'mine'
            AND position('/' in ob.repo_full_name) > 1
            AND lower(split_part(ob.repo_full_name, '/', 1)) = lower($24)
        ),
        items AS (
          SELECT
            'release' AS kind,
            5 AS kind_rank,
            sort_ts,
            ts,
            id_key,
            entity_id,
            entity_id AS translation_entity_id,
            CAST(NULL AS BIGINT) AS discussion_number,
            release_id,
            release_repo_id,
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
              lpad(CAST(r.release_id AS TEXT), 20, '0') AS id_key,
              CAST(r.release_id AS TEXT) AS entity_id,
              r.release_id AS release_id,
              r.repo_id AS release_repo_id,
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
              CAST(NULL AS TEXT) AS subtitle,
              CAST(NULL AS TEXT) AS reason,
              CAST(NULL AS TEXT) AS subject_type,
              r.html_url AS html_url,
              CAST(NULL AS BIGINT) AS unread,
              CAST(NULL AS TEXT) AS actor_login,
              CAST(NULL AS TEXT) AS actor_avatar_url,
              CAST(NULL AS TEXT) AS actor_html_url,
              r.body AS release_body,
              r.react_plus1 AS react_plus1,
              r.react_laugh AS react_laugh,
              r.react_heart AS react_heart,
              r.react_hooray AS react_hooray,
              r.react_rocket AS react_rocket,
              r.react_eyes AS react_eyes
            FROM public.repo_releases r
            JOIN scoped_visible_repos sr
              ON sr.repo_id = r.repo_id
          ) release_projection
          UNION ALL
          SELECT
            e.kind AS kind,
            CASE e.kind
              WHEN 'announcement' THEN 4
              WHEN 'release_update' THEN 4
              WHEN 'repo_forked' THEN 3
              WHEN 'repo_star_received' THEN 2
              WHEN 'follower_received' THEN 1
              ELSE 0
            END AS kind_rank,
            e.occurred_at AS sort_ts,
            e.occurred_at AS ts,
            e.id AS id_key,
            e.id AS entity_id,
            CASE
              WHEN e.kind = 'announcement'
                AND e.repo_full_name IS NOT NULL
                AND e.discussion_number IS NOT NULL
              THEN lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT)
              ELSE CAST(NULL AS TEXT)
            END AS translation_entity_id,
            e.discussion_number AS discussion_number,
            CAST(NULL AS BIGINT) AS release_id,
            CAST(NULL AS BIGINT) AS release_repo_id,
            CAST(NULL AS TEXT) AS release_node_id,
            e.repo_full_name AS repo_full_name,
            COALESCE(e.repo_owner_avatar_url, ob.owner_avatar_url) AS owner_avatar_url,
            COALESCE(e.repo_open_graph_image_url, ob.open_graph_image_url) AS open_graph_image_url,
            COALESCE(
              e.repo_uses_custom_open_graph_image,
              ob.uses_custom_open_graph_image
            ) AS uses_custom_open_graph_image,
            CAST(NULL AS TEXT) AS release_tag_name,
            CAST(NULL AS TEXT) AS release_previous_tag_name,
            e.title AS title,
            CASE e.kind
              WHEN 'announcement' THEN '仓库公告'
              WHEN 'release_update' THEN 'Release'
              WHEN 'repo_forked' THEN 'Fork'
              ELSE CAST(NULL AS TEXT)
            END AS subtitle,
            CAST(NULL AS TEXT) AS reason,
            CASE e.kind
              WHEN 'announcement' THEN 'discussion'
              WHEN 'release_update' THEN 'release'
              WHEN 'repo_forked' THEN 'repository'
              ELSE CAST(NULL AS TEXT)
            END AS subject_type,
            COALESCE(e.html_url, e.actor_html_url, 'https://github.com/' || e.actor_login) AS html_url,
            CAST(NULL AS BIGINT) AS unread,
            e.actor_login AS actor_login,
            e.actor_avatar_url AS actor_avatar_url,
            e.actor_html_url AS actor_html_url,
            e.body AS release_body,
            CAST(NULL AS BIGINT) AS react_plus1,
            CAST(NULL AS BIGINT) AS react_laugh,
            CAST(NULL AS BIGINT) AS react_heart,
            CAST(NULL AS BIGINT) AS react_hooray,
            CAST(NULL AS BIGINT) AS react_rocket,
            CAST(NULL AS BIGINT) AS react_eyes
          FROM public.social_activity_events e
          LEFT JOIN public.owned_repo_star_baselines ob
            ON ob.user_id = e.user_id AND ob.repo_id = e.repo_id
          LEFT JOIN scoped_visible_repos vr
            ON vr.repo_id = e.repo_id
          WHERE e.user_id = $25
            AND ($26 = 0 OR (e.repo_full_name IS NOT NULL AND vr.repo_id IS NOT NULL))
        )
        SELECT
          i.kind, i.sort_ts, i.ts, i.id_key, i.entity_id, i.translation_entity_id, i.release_id, i.release_repo_id, i.release_node_id,
          i.repo_full_name, i.owner_avatar_url, i.open_graph_image_url, i.uses_custom_open_graph_image,
          i.discussion_number,
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
        LEFT JOIN public.ai_translations t
          ON t.user_id = $27
          AND t.entity_type = CASE
            WHEN i.kind = 'release' THEN 'release'
            WHEN i.kind = 'announcement' THEN 'announcement_detail'
            ELSE CAST(NULL AS TEXT)
          END
          AND t.entity_id = i.translation_entity_id
          AND t.lang = 'zh-CN'
          AND t.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN public.translation_work_items tw
          ON tw.id = t.active_work_item_id
        LEFT JOIN public.ai_translations dt
          ON dt.user_id = $28
          AND dt.entity_type = CASE
            WHEN i.kind = 'release' THEN 'release_detail'
            WHEN i.kind = 'announcement' THEN 'announcement_detail'
            ELSE CAST(NULL AS TEXT)
          END
          AND dt.entity_id = i.translation_entity_id
          AND dt.lang = 'zh-CN'
          AND dt.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN public.translation_work_items dtw
          ON dtw.id = dt.active_work_item_id
        LEFT JOIN public.ai_translations s
          ON s.user_id = $29
          AND s.entity_type = CASE
            WHEN i.kind = 'release' THEN 'release_smart'
            WHEN i.kind = 'announcement' THEN 'announcement_smart'
            ELSE CAST(NULL AS TEXT)
          END
          AND s.entity_id = i.translation_entity_id
          AND s.lang = 'zh-CN'
          AND s.status IN ('ready', 'disabled', 'missing', 'error')
        LEFT JOIN public.translation_work_items sw
          ON sw.id = s.active_work_item_id
        WHERE (
          ($30 = 1 AND i.kind = 'release')
          OR ($31 = 1 AND i.kind = 'repo_star_received')
          OR ($32 = 1 AND i.kind = 'follower_received')
          OR ($33 = 1 AND i.kind IN ('announcement', 'release_update', 'repo_forked'))
        )
          AND (
            $34 = 0
            OR (i.sort_ts >= $35 AND i.sort_ts < $36)
          )
          AND (
            $37 = 0
            OR i.sort_ts < $38
            OR (i.sort_ts = $39 AND i.kind_rank < $40)
            OR (i.sort_ts = $41 AND i.kind_rank = $42 AND i.id_key < $43)
          )
        ORDER BY i.sort_ts DESC, i.kind_rank DESC, i.id_key DESC
        LIMIT $44
    "#;

    let has_cursor = cursor.is_some();
    let scope_kind = match scope {
        Some(FeedScope::Repo { .. }) => "repo",
        Some(FeedScope::Repos { .. }) => "repos",
        Some(FeedScope::Org { .. }) => "org",
        Some(FeedScope::Mine) => "mine",
        Some(FeedScope::Following) => "following",
        None => "",
    };
    let scope_repo_item = match scope {
        Some(FeedScope::Repo { owner, repo }) => Some(format!("{owner}/{repo}")),
        _ => None,
    };
    let scope_repo_items = match scope {
        Some(FeedScope::Repos { items }) => Some(items.clone()),
        _ => None,
    };
    let scope_org = match scope {
        Some(FeedScope::Org { org }) => Some(org.clone()),
        _ => None,
    };
    let scope_mine_owner = if matches!(scope, Some(FeedScope::Mine)) {
        Some(viewer_login.unwrap_or("").to_owned())
    } else {
        None
    };
    let scoped_all = scope.is_some();

    let mut query = sqlx::query_as::<_, FeedRow>(sql)
        .bind(user_id)
        .bind(scope_kind)
        .bind(scope_kind)
        .bind(scope_kind)
        .bind(scope_org.as_deref())
        .bind(user_id)
        .bind(scope_kind)
        .bind(user_id)
        .bind(scope_kind)
        .bind(scope_repo_item.as_deref())
        .bind(scope_kind)
        .bind(scope_repo_items.as_ref())
        .bind(scope_kind)
        .bind(scope_org.as_deref())
        .bind(user_id)
        .bind(scope_kind)
        .bind(scope_repo_item.as_deref())
        .bind(scope_kind)
        .bind(scope_repo_items.as_ref())
        .bind(scope_kind)
        .bind(scope_org.as_deref())
        .bind(user_id)
        .bind(scope_kind)
        .bind(scope_mine_owner.as_deref())
        .bind(user_id)
        .bind(if scoped_all { 1_i64 } else { 0_i64 })
        .bind(user_id)
        .bind(user_id)
        .bind(user_id);
    query = query
        .bind(if scoped_all || types.releases {
            1_i64
        } else {
            0_i64
        })
        .bind(if types.stars { 1_i64 } else { 0_i64 })
        .bind(if scoped_all {
            0_i64
        } else if types.followers {
            1_i64
        } else {
            0_i64
        })
        .bind(if scoped_all || types.ambient {
            1_i64
        } else {
            0_i64
        })
        .bind(if time_window.is_some() { 1_i64 } else { 0_i64 })
        .bind(time_window.map(|(start, _)| start))
        .bind(time_window.map(|(_, end)| end))
        .bind(if has_cursor { 1_i64 } else { 0_i64 })
        .bind(cursor.map(|c| c.sort_ts.as_str()))
        .bind(cursor.map(|c| c.sort_ts.as_str()))
        .bind(cursor.map(|c| c.kind_rank))
        .bind(cursor.map(|c| c.sort_ts.as_str()))
        .bind(cursor.map(|c| c.kind_rank))
        .bind(cursor.map(|c| c.id_key.as_str()))
        .bind(limit);
    let mut rows = query.fetch_all(pool).await?;
    overlay_global_feed_processing(pool, user_id, &mut rows).await?;
    Ok(rows)
}

/// Replace the legacy translation/smart fields with the global
/// content-processing projections.
///
/// Mirrors `api::overlay_global_feed_processing`: every release/announcement
/// row restarts from a cleared projection, the global payload (when present)
/// supplies the fields, and the existing assembly below decides the final
/// status. Rows without a projection keep the cleared fields, so they report
/// `missing` instead of a stale `ai_translations` payload.
async fn overlay_global_feed_processing(
    pool: &PgPool,
    user_id: &str,
    rows: &mut [FeedRow],
) -> Result<(), sqlx::Error> {
    let state = postgres_content_overlay::OverlayState::load(pool).await;
    if !state.is_global() {
        return Ok(());
    }
    let mut audiences = HashMap::<i64, postgres_content_overlay::ReleaseAudience>::new();
    for row in rows.iter_mut() {
        if !matches!(row.kind.as_str(), "release" | "announcement") {
            continue;
        }
        clear_feed_row_processing_projection(row);
        let processing = if row.kind == "release" {
            let Some(release_id) = row.release_id.or_else(|| row.entity_id.parse::<i64>().ok())
            else {
                continue;
            };
            let repo_identity = postgres_content_overlay::release_repo_identity(
                row.html_url.as_deref().unwrap_or(""),
                row.release_repo_id.unwrap_or(0),
            );
            let source = postgres_content_overlay::ReleaseSource {
                release_id,
                repo_full_name: row.repo_full_name.as_deref().unwrap_or(""),
                repo_identity: repo_identity.as_str(),
                tag_name: row.release_tag_name.as_deref().unwrap_or(""),
                name: row.title.as_deref(),
                body: row.release_body.as_deref(),
                previous_tag_name: row.release_previous_tag_name.as_deref(),
            };
            let audience = match row.release_repo_id {
                Some(repo_id) => match audiences.get(&repo_id) {
                    Some(audience) => *audience,
                    None => {
                        let audience =
                            postgres_content_overlay::release_audience(pool, user_id, repo_id)
                                .await?;
                        audiences.insert(repo_id, audience);
                        audience
                    }
                },
                None => postgres_content_overlay::ReleaseAudience::Public,
            };
            postgres_content_overlay::release_feed_processing(pool, state, &source, audience)
                .await?
        } else {
            let repo_full_name = row.repo_full_name.as_deref().unwrap_or("");
            let Some(discussion_number) = row.discussion_number else {
                continue;
            };
            let discussion_key = row.translation_entity_id.clone().unwrap_or_else(|| {
                crate::api::announcement_discussion_key(repo_full_name, discussion_number)
            });
            let source = postgres_content_overlay::AnnouncementSource {
                discussion_key: discussion_key.as_str(),
                repo_full_name,
                discussion_number,
                title: row.title.as_deref().unwrap_or(""),
                body: row.release_body.as_deref(),
            };
            postgres_content_overlay::announcement_feed_processing(pool, state, &source).await?
        };
        apply_feed_processing(row, processing);
    }
    Ok(())
}

fn apply_feed_processing(row: &mut FeedRow, processing: postgres_content_overlay::FeedProcessing) {
    let postgres_content_overlay::FeedProcessing {
        translation_source_hash,
        detail_source_hash,
        smart_source_hash,
        translation,
        detail,
        smart,
    } = processing;
    if let Some(projection) = translation {
        row.trans_source_hash = Some(translation_source_hash);
        row.trans_status = Some(projection.translation_status());
        row.trans_title = projection.title();
        row.trans_summary = projection.translation_summary();
        row.trans_error_text = projection.error_text();
        row.trans_work_status = Some(projection.status);
    }
    if let Some(projection) = detail {
        row.detail_trans_source_hash = Some(detail_source_hash);
        row.detail_trans_status = Some(projection.translation_status());
        row.detail_trans_title = projection.title();
        row.detail_trans_summary = projection.translation_summary();
        row.detail_trans_error_text = projection.error_text();
        row.detail_trans_work_status = Some(projection.status);
    }
    if let Some(projection) = smart {
        row.smart_source_hash = Some(smart_source_hash);
        let keep_active = row.kind == "release";
        row.smart_status = Some(projection.smart_status(keep_active));
        row.smart_title = projection.title();
        row.smart_summary = projection.smart_summary();
        row.smart_error_text = projection.error_text();
        row.smart_work_status = Some(projection.status);
    }
}

fn clear_feed_row_processing_projection(row: &mut FeedRow) {
    row.trans_source_hash = None;
    row.trans_status = None;
    row.trans_title = None;
    row.trans_summary = None;
    row.trans_error_text = None;
    row.trans_work_status = None;
    row.detail_trans_source_hash = None;
    row.detail_trans_status = None;
    row.detail_trans_title = None;
    row.detail_trans_summary = None;
    row.detail_trans_error_text = None;
    row.detail_trans_work_status = None;
    row.smart_source_hash = None;
    row.smart_status = None;
    row.smart_title = None;
    row.smart_summary = None;
    row.smart_error_text = None;
    row.smart_work_status = None;
}

// ---------------------------------------------------------------------------
// Translation / smart projection helpers
// ---------------------------------------------------------------------------

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        s.chars().take(max_chars).collect()
    } else {
        s.to_owned()
    }
}

fn release_feed_body(body: Option<&str>) -> Option<String> {
    let normalized = body?.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(trimmed, RELEASE_FEED_BODY_MAX_CHARS))
}

fn release_feed_body_is_over_limit(body: Option<&str>) -> bool {
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

#[derive(Debug, Deserialize)]
struct TranslationJson {
    title_zh: Option<String>,
    summary_md: Option<String>,
    body_md: Option<String>,
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
        request_id: None,
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
        request_id: None,
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
        "failed" => Some(translated_item(
            "error",
            None,
            None,
            Some(false),
            error_text,
        )),
        "queued" | "running" | "not_applicable" | "deferred_provider" | "blocked_config"
        | "cancelled" | "superseded" => {
            Some(translated_item(status, None, None, Some(true), error_text))
        }
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
        && !crate::api::markdown_structure_preserved(src, s)
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
        "insufficient" => Some(smart_item("insufficient", None, None, Some(false), None)),
        "missing" if error_text == Some(crate::api::SMART_NO_VALUABLE_VERSION_INFO) => {
            Some(smart_item("insufficient", None, None, Some(false), None))
        }
        "missing" | "error" | "incompatible" => {
            Some(smart_item(status, None, None, Some(false), error_text))
        }
        "failed" => Some(smart_item("error", None, None, Some(false), error_text)),
        "queued" | "running" | "not_applicable" | "deferred_provider" | "blocked_config"
        | "cancelled" | "superseded" => {
            Some(smart_item(status, None, None, Some(true), error_text))
        }
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

fn smart_ready_item_any(
    raw_title: Option<String>,
    raw_summary: Option<String>,
    auto_translate: Option<bool>,
) -> Option<SmartItem> {
    let (title, summary) = normalize_translation_fields(raw_title, raw_summary);
    if title.is_none() && summary.is_none() {
        return None;
    }
    Some(smart_item("ready", title, summary, auto_translate, None))
}

fn smart_missing_item(auto_translate: Option<bool>) -> SmartItem {
    smart_item("missing", None, None, auto_translate, None)
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

fn feed_item_from_row(r: FeedRow, ai_enabled: bool) -> FeedItem {
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
    let kind = r.kind.clone();
    let discussion_key = r.repo_full_name.as_deref().zip(r.discussion_number).map(
        |(repo_full_name, discussion_number)| {
            crate::api::announcement_discussion_key(repo_full_name, discussion_number)
        },
    );
    let is_release_like = matches!(kind.as_str(), "release" | "announcement");

    let (body, body_truncated) = if is_release_like {
        (
            release_feed_body(r.release_body.as_deref()),
            release_feed_body_is_over_limit(r.release_body.as_deref()),
        )
    } else {
        (None, false)
    };

    if !is_release_like {
        return FeedItem {
            kind,
            ts: r.ts,
            id: r.entity_id,
            repo_full_name: r.repo_full_name,
            discussion_number: None,
            discussion_key: None,
            repo_visual,
            title: r.title,
            body,
            body_truncated,
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

    let translation_entity_id = r
        .translation_entity_id
        .clone()
        .or_else(|| discussion_key.clone());

    let translated = if !ai_enabled {
        Some(translated_item("disabled", None, None, None, None))
    } else {
        let current_hash = match kind.as_str() {
            "announcement" => crate::api::announcement_feed_translation_source_hash(
                r.repo_full_name.as_deref().unwrap_or(""),
                r.discussion_number,
                r.title.as_deref().unwrap_or(""),
                body.as_deref(),
            ),
            _ => crate::api::release_feed_translation_source_hash(
                r.repo_full_name.as_deref().unwrap_or(""),
                r.title.as_deref().unwrap_or(""),
                body.as_deref(),
            ),
        };
        let detail_current_hash = match kind.as_str() {
            "announcement" => crate::api::announcement_detail_source_hash(
                r.repo_full_name.as_deref().unwrap_or(""),
                r.discussion_number,
                r.title.as_deref().unwrap_or(""),
                r.release_body.as_deref().unwrap_or(""),
            ),
            _ => crate::api::release_detail_source_hash(
                r.repo_full_name.as_deref().unwrap_or(""),
                r.title.as_deref().unwrap_or(""),
                r.release_body.as_deref().unwrap_or(""),
            ),
        };
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
        let detail_mismatch_error = if kind == "announcement" {
            ANNOUNCEMENT_DETAIL_MARKDOWN_MISMATCH_ERROR
        } else {
            RELEASE_DETAIL_MARKDOWN_MISMATCH_ERROR
        };

        if r.detail_trans_source_hash.as_deref() == Some(detail_current_hash.as_str()) {
            if let Some(status) = r.detail_trans_status.as_deref()
                && status != "ready"
            {
                if kind == "release"
                    && !body_truncated
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
                    if kind == "release"
                        && !body_truncated
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
                            Some(detail_mismatch_error),
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
            } else if kind == "release" {
                translated_release_feed_ready_item(
                    r.trans_title.clone(),
                    r.trans_summary.clone(),
                    body.as_deref(),
                    None,
                )
            } else {
                translated_ready_item(r.trans_title.clone(), r.trans_summary.clone(), None)
                    .or_else(|| Some(translated_missing_item(true)))
            }
        } else if refresh_in_flight {
            translated_ready_item(r.trans_title.clone(), r.trans_summary.clone(), Some(true))
                .or_else(|| Some(translated_missing_item(false)))
        } else {
            Some(translated_missing_item(true))
        }
    };

    let smart_current_hash = match kind.as_str() {
        "announcement" => crate::translations::announcement_smart_feed_source_hash(
            translation_entity_id
                .as_deref()
                .unwrap_or_else(|| discussion_key.as_deref().unwrap_or(r.entity_id.as_str())),
            r.repo_full_name.as_deref().unwrap_or(""),
            r.discussion_number,
            r.title.as_deref().unwrap_or(""),
            r.release_body.as_deref(),
        ),
        _ => crate::translations::release_smart_feed_source_hash(
            r.entity_id.as_str(),
            r.repo_full_name.as_deref().unwrap_or(""),
            r.title.as_deref().unwrap_or(""),
            body.as_deref(),
            r.release_tag_name.as_deref().unwrap_or(""),
            r.release_previous_tag_name.as_deref(),
        ),
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
                smart_ready_item_any(r.smart_title.clone(), r.smart_summary.clone(), None)
                    .or_else(|| Some(smart_missing_item(None)))
            }
        } else if refresh_in_flight {
            smart_ready_item(r.smart_title.clone(), r.smart_summary.clone(), Some(true))
                .or_else(|| Some(smart_missing_item(Some(false))))
        } else {
            Some(smart_missing_item(None))
        }
    };

    let reactions = if kind == "release" {
        Some(ReleaseReactions {
            counts: release_counts_from_row(&r),
            viewer: ReleaseReactionViewer::default(),
            status: release_reactions_status(&r).to_owned(),
        })
    } else {
        None
    };

    FeedItem {
        kind: kind.clone(),
        ts: r.ts,
        id: r.entity_id,
        repo_full_name: r.repo_full_name,
        discussion_number: r.discussion_number,
        discussion_key,
        repo_visual,
        title: r.title,
        body,
        body_truncated,
        subtitle: r.subtitle,
        reason: r.reason,
        subject_type: r.subject_type,
        html_url: r.html_url,
        unread: r.unread,
        actor: if r.kind == "announcement" {
            actor
        } else {
            None
        },
        translated,
        smart,
        reactions,
    }
}

// ---------------------------------------------------------------------------
// Feed handlers
// ---------------------------------------------------------------------------

async fn viewer_login(state: &PostgresServerState, user_id: &str) -> Result<String, Response> {
    sqlx::query_scalar::<_, String>(
        "SELECT login FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load viewer login", error))?
    .ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code": "unauthorized", "message": "session user has no github connection"})),
        )
            .into_response()
    })
}

pub(crate) async fn feed(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<FeedQuery>,
) -> Response {
    let user_id = match require_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let login = match viewer_login(&state, &user_id).await {
        Ok(login) => login,
        Err(response) => return response,
    };
    let types = match parse_feed_types(query.types.as_deref()) {
        Ok(types) => types,
        Err(response) => return response,
    };
    let scope = match parse_feed_scope(
        query.scope.as_deref(),
        query.items.as_deref(),
        query.org.as_deref(),
    ) {
        Ok(scope) => scope,
        Err(response) => return response,
    };
    if let Some(scope_ref) = scope.as_ref()
        && matches!(scope_ref, FeedScope::Repo { .. } | FeedScope::Repos { .. })
        && let Err(response) =
            prepare_repo_scope_public_repo_access(&state, &user_id, scope_ref).await
    {
        return response;
    }

    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let cursor = query
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let feed_cursor = match cursor {
        Some(c) => match parse_feed_cursor(c) {
            Ok(cursor) => Some(cursor),
            Err(response) => return response,
        },
        None => None,
    };
    let rows = match fetch_feed_items(
        &state.runtime.pool,
        &user_id,
        feed_cursor.as_ref(),
        types,
        scope.as_ref(),
        Some(login.as_str()),
        limit,
        None,
    )
    .await
    {
        Ok(rows) => rows,
        Err(error) => return database_error("feed query", error),
    };

    let ai_enabled = state.ai_model.is_some();
    let mut items = Vec::with_capacity(rows.len());
    let mut next_cursor: Option<String> = None;
    for (idx, r) in rows.into_iter().enumerate() {
        if idx == limit.saturating_sub(1) as usize {
            next_cursor = Some(format!("{}|{}|{}", r.sort_ts, r.kind, r.id_key));
        }
        items.push(feed_item_from_row(r, ai_enabled));
    }
    if items.len() < limit as usize {
        next_cursor = None;
    }
    (StatusCode::OK, Json(FeedResponse { items, next_cursor })).into_response()
}

pub(crate) async fn head_feed(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<FeedQuery>,
) -> Response {
    let user_id = match require_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let scope = match parse_feed_scope(
        query.scope.as_deref(),
        query.items.as_deref(),
        query.org.as_deref(),
    ) {
        Ok(Some(scope)) => scope,
        Ok(None) => {
            return bad_request("HEAD /api/feed requires scope=repo|repos");
        }
        Err(response) => return response,
    };
    if !matches!(scope, FeedScope::Repo { .. } | FeedScope::Repos { .. }) {
        return bad_request("HEAD /api/feed only supports scope=repo|repos");
    }
    match prepare_repo_scope_public_repo_access(&state, &user_id, &scope).await {
        Ok(true) => StatusCode::ACCEPTED.into_response(),
        Ok(false) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// Public repo warmup (HEAD /api/feed)
// ---------------------------------------------------------------------------

fn normalize_public_repo_path(owner: &str, repo: &str) -> Result<(String, String), Response> {
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
        return Err(bad_request("invalid repository path"));
    }
    Ok((owner.to_owned(), repo.to_owned()))
}

#[derive(Debug, FromRow)]
struct PublicReleaseUsageRow {
    repo_id: Option<i64>,
    full_name: String,
    last_sync_status: String,
}

async fn upsert_public_release_usage(
    state: &PostgresServerState,
    owner: &str,
    repo: &str,
) -> Result<PublicReleaseUsageRow, Response> {
    let full_name = format!("{owner}/{repo}");
    let full_name_lower = full_name.to_ascii_lowercase();
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
    sqlx::query(
        r#"
        INSERT INTO public.public_repo_release_usage (
          id, owner_login, repo_name, full_name, full_name_lower,
          first_registered_at, last_requested_at, last_list_requested_at,
          last_detail_requested_at, api_list_requests, api_detail_requests,
          page_list_requests, page_detail_requests, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $6, $6, NULL, 1, 0, 0, 0, $6, $6)
        ON CONFLICT (full_name_lower) DO UPDATE SET
          last_requested_at = EXCLUDED.last_requested_at,
          last_list_requested_at = EXCLUDED.last_list_requested_at,
          api_list_requests = public.public_repo_release_usage.api_list_requests + 1,
          updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(owner)
    .bind(repo)
    .bind(&full_name)
    .bind(&full_name_lower)
    .bind(&now)
    .execute(&state.runtime.pool)
    .await
    .map_err(|error| database_error("register public release usage", error))?;

    let row = sqlx::query_as::<_, PublicReleaseUsageRow>(
        "SELECT repo_id, full_name, last_sync_status FROM public.public_repo_release_usage WHERE full_name_lower = $1 LIMIT 1",
    )
    .bind(&full_name_lower)
    .fetch_one(&state.runtime.pool)
    .await
    .map_err(|error| database_error("read public release usage", error))?;

    if row.repo_id.is_none() && row.last_sync_status != "inaccessible" {
        if let Some(resolved) =
            resolve_public_release_usage_from_local_metadata(state, &full_name_lower).await?
        {
            return Ok(resolved);
        }
    }
    Ok(row)
}

fn public_local_metadata_is_fresh(updated_at: Option<&str>) -> bool {
    const PUBLIC_LOCAL_METADATA_MAX_AGE_HOURS: i64 = 24;
    let Some(updated_at) = updated_at else {
        return false;
    };
    let Ok(updated_at) = DateTime::parse_from_rfc3339(updated_at) else {
        return false;
    };
    Utc::now().signed_duration_since(updated_at.with_timezone(&Utc))
        <= chrono::Duration::hours(PUBLIC_LOCAL_METADATA_MAX_AGE_HOURS)
}

#[derive(Debug, FromRow)]
struct PublicReleaseLocalMetadataRow {
    repo_id: i64,
    full_name: String,
    is_private: Option<i64>,
    metadata_updated_at: Option<String>,
}

async fn resolve_public_release_usage_from_local_metadata(
    state: &PostgresServerState,
    full_name_lower: &str,
) -> Result<Option<PublicReleaseUsageRow>, Response> {
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
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load public release local metadata", error))?
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
    .fetch_one(&state.runtime.pool)
    .await
    .map_err(|error| database_error("count public release metadata releases", error))?;

    let Some((owner, repo_name)) = repo.full_name.split_once('/') else {
        return Ok(None);
    };
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
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
            last_sync_status = $5,
            last_sync_error = NULL,
            updated_at = $6
        WHERE full_name_lower = $7
        "#,
    )
    .bind(repo.repo_id)
    .bind(owner)
    .bind(repo_name)
    .bind(&repo.full_name)
    .bind(next_status)
    .bind(&now)
    .bind(full_name_lower)
    .execute(&state.runtime.pool)
    .await
    .map_err(|error| database_error("refresh public release usage metadata", error))?;

    Ok(Some(PublicReleaseUsageRow {
        repo_id: Some(repo.repo_id),
        full_name: repo.full_name,
        last_sync_status: next_status.to_owned(),
    }))
}

async fn register_manual_feed_repo_association(
    state: &PostgresServerState,
    user_id: &str,
    full_name: &str,
    repo_id: Option<i64>,
) -> Result<(), Response> {
    let Some((owner_login, repo_name)) = full_name.split_once('/') else {
        return Err(bad_request("invalid repo full name"));
    };
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
    sqlx::query(
        r#"
        INSERT INTO public.user_repo_associations (
          id, user_id, repo_id, repo_full_name, repo_full_name_lower,
          owner_login, repo_name, html_url, description, is_private,
          owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image,
          first_source, first_associated_at, last_seen_at, is_following,
          follow_state_source, has_personal_owned_source, has_github_star_source,
          has_manual_feed_source, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, lower($4), $5, $6, $7, NULL, NULL,
                NULL, NULL, 0, 'manual_feed', $8, $8, 0,
                'system_default', 0, 0, 1, $8, $8)
        ON CONFLICT (user_id, repo_full_name_lower) DO UPDATE SET
          repo_id = COALESCE(EXCLUDED.repo_id, public.user_repo_associations.repo_id),
          repo_full_name = EXCLUDED.repo_full_name,
          owner_login = EXCLUDED.owner_login,
          repo_name = EXCLUDED.repo_name,
          html_url = COALESCE(EXCLUDED.html_url, public.user_repo_associations.html_url),
          first_source = CASE
            WHEN EXCLUDED.first_associated_at < public.user_repo_associations.first_associated_at
              OR (
                EXCLUDED.first_associated_at = public.user_repo_associations.first_associated_at
                AND 2 < CASE public.user_repo_associations.first_source
                  WHEN 'personal_owned' THEN 0
                  WHEN 'github_star' THEN 1
                  ELSE 2
                END
              )
              THEN EXCLUDED.first_source
            ELSE public.user_repo_associations.first_source
          END,
          first_associated_at = CASE
            WHEN EXCLUDED.first_associated_at < public.user_repo_associations.first_associated_at
              THEN EXCLUDED.first_associated_at
            ELSE public.user_repo_associations.first_associated_at
          END,
          last_seen_at = CASE
            WHEN EXCLUDED.last_seen_at > public.user_repo_associations.last_seen_at
              THEN EXCLUDED.last_seen_at
            ELSE public.user_repo_associations.last_seen_at
          END,
          is_following = CASE
            WHEN public.user_repo_associations.follow_state_source = 'user_explicit'
              THEN public.user_repo_associations.is_following
            WHEN EXCLUDED.is_following != 0
              THEN 1
            ELSE public.user_repo_associations.is_following
          END,
          has_manual_feed_source = GREATEST(
            public.user_repo_associations.has_manual_feed_source,
            EXCLUDED.has_manual_feed_source
          ),
          updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(user_id)
    .bind(repo_id)
    .bind(full_name)
    .bind(owner_login)
    .bind(repo_name)
    .bind(format!("https://github.com/{full_name}"))
    .bind(&now)
    .execute(&state.runtime.pool)
    .await
    .map_err(|error| database_error("register manual feed association", error))?;
    Ok(())
}

async fn prepare_repo_scope_public_repo_access(
    state: &PostgresServerState,
    user_id: &str,
    scope: &FeedScope,
) -> Result<bool, Response> {
    let repo_items = match scope {
        FeedScope::Repo { owner, repo } => vec![format!("{owner}/{repo}")],
        FeedScope::Repos { items } => items.clone(),
        _ => return Ok(false),
    };

    let mut needs_warmup = false;
    for item in repo_items {
        let Some((owner_raw, repo_raw)) = item.split_once('/') else {
            continue;
        };
        let (owner, repo) = normalize_public_repo_path(owner_raw, repo_raw)?;
        let full_name_lower = format!("{owner}/{repo}").to_ascii_lowercase();
        let existing = sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
            "SELECT repo_id, is_private FROM public.user_repo_associations WHERE user_id = $1 AND repo_full_name_lower = $2",
        )
        .bind(user_id)
        .bind(&full_name_lower)
        .fetch_optional(&state.runtime.pool)
        .await
        .map_err(|error| database_error("load repo association for warmup", error))?;
        if let Some((repo_id, is_private)) = existing
            && repo_id.is_some()
            && is_private == Some(1)
        {
            continue;
        }
        let usage = upsert_public_release_usage(state, &owner, &repo).await?;
        register_manual_feed_repo_association(
            state,
            user_id,
            usage.full_name.as_str(),
            usage.repo_id,
        )
        .await?;
        if usage.repo_id.is_none() || usage.last_sync_status != "ready" {
            needs_warmup = true;
        }
        if usage.last_sync_status == "inaccessible" {
            return Err(not_found_inaccessible());
        }
    }
    Ok(needs_warmup)
}

// ---------------------------------------------------------------------------
// Dashboard readable sections
// ---------------------------------------------------------------------------

async fn load_brief_preferences(
    state: &PostgresServerState,
    user_id: &str,
) -> Result<DailyBriefPreferences, Response> {
    let stored = sqlx::query_scalar::<_, Option<String>>(
        "SELECT daily_brief_time_zone FROM public.users WHERE id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load daily brief preferences", error))?;
    let Some(stored) = stored else {
        return Err(internal_error("user not found for daily brief preferences"));
    };
    // The legacy loader reads the global schedule from admin runtime settings
    // with the process default as fallback; the local time is not consumed by
    // the dashboard read surface, so a missing settings table degrades to the
    // configured default instead of failing the request.
    let local_time = sqlx::query_scalar::<_, String>(
        "SELECT daily_brief_schedule_local_time FROM public.admin_runtime_settings WHERE id = 1 LIMIT 1",
    )
    .fetch_optional(&state.runtime.pool)
    .await
    .ok()
    .flatten()
    .and_then(|raw| briefs::parse_daily_brief_local_time(raw.as_str()).ok())
    .unwrap_or(state.auth_ext.default_daily_brief_local_time);
    Ok(DailyBriefPreferences {
        local_time,
        time_zone: briefs::resolve_daily_brief_time_zone(
            state.auth_ext.default_daily_brief_time_zone.as_str(),
            stored.as_deref(),
        ),
    })
}

// Brief window timestamps stay TEXT: the imported production schema stores
// them as text while a fresh PostgreSQL baseline stores timestamptz. A
// ``::text`` cast therefore yields either the legacy RFC3339 form or
// PostgreSQL's ``YYYY-MM-DD HH:MM:SS+00`` form, and both must normalize to the
// RFC3339 shape the legacy snapshot helper parses.
fn normalize_timestamp(value: &str) -> String {
    let raw = value.trim();
    if raw.is_empty() {
        return String::new();
    }
    let rfc = |parsed: DateTime<chrono::FixedOffset>| {
        parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
    };
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return rfc(parsed);
    }
    // PostgreSQL renders timestamptz as `YYYY-MM-DD HH:MM:SS[.ffffff]±HH[:MM]`,
    // optionally with a short `+00` offset that RFC3339 parsers reject.
    let mut candidate = raw.replace(' ', "T");
    if let Some(position) = candidate.rfind(['+', '-']) {
        let offset = &candidate[position + 1..];
        let digits = offset.chars().filter(char::is_ascii_digit).count();
        if digits == 2 {
            candidate.push_str(":00");
        }
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(candidate.as_str()) {
        return rfc(parsed);
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f%:z", "%Y-%m-%dT%H:%M:%S%.f%z"] {
        if let Ok(parsed) = DateTime::parse_from_str(candidate.as_str(), fmt) {
            return rfc(parsed);
        }
    }
    if let Ok(parsed) =
        chrono::NaiveDateTime::parse_from_str(candidate.as_str(), "%Y-%m-%dT%H:%M:%S%.f")
    {
        return rfc(parsed.and_utc().fixed_offset());
    }
    raw.to_owned()
}

fn normalize_timestamp_opt(value: Option<&str>) -> Option<String> {
    value
        .map(normalize_timestamp)
        .filter(|value| !value.is_empty())
}

fn brief_row_is_visible(
    date: &str,
    window_start_utc: Option<&str>,
    window_end_utc: Option<&str>,
    effective_time_zone: Option<&str>,
    effective_local_boundary: Option<&str>,
    generation_source: &str,
    updated_at: &str,
    preferences: &DailyBriefPreferences,
    now: DateTime<Utc>,
) -> bool {
    briefs::brief_snapshot_is_visible(
        date,
        window_start_utc,
        window_end_utc,
        effective_time_zone,
        effective_local_boundary,
        generation_source,
        updated_at,
        preferences,
        now,
    )
}

#[derive(Debug, FromRow)]
struct DashboardReadableBriefDateRow {
    date: String,
    window_start_utc: Option<String>,
    window_end_utc: Option<String>,
    effective_time_zone: Option<String>,
    effective_local_boundary: Option<String>,
    generation_source: String,
    updated_at: String,
}

async fn load_dashboard_readable_brief_dates(
    state: &PostgresServerState,
    user_id: &str,
    preferences: &DailyBriefPreferences,
    now: DateTime<Utc>,
) -> Result<Vec<String>, Response> {
    let rows = sqlx::query_as::<_, DashboardReadableBriefDateRow>(
        r#"
        SELECT date, window_start_utc::text AS window_start_utc,
               window_end_utc::text AS window_end_utc,
               effective_time_zone, effective_local_boundary,
               generation_source, updated_at::text AS updated_at
        FROM public.briefs
        WHERE user_id = $1
        ORDER BY date DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load dashboard brief dates", error))?;
    let mut dates = rows
        .into_iter()
        .filter(|row| {
            let window_start = normalize_timestamp_opt(row.window_start_utc.as_deref());
            let window_end = normalize_timestamp_opt(row.window_end_utc.as_deref());
            let updated_at = normalize_timestamp(&row.updated_at);
            brief_row_is_visible(
                &row.date,
                window_start.as_deref(),
                window_end.as_deref(),
                row.effective_time_zone.as_deref(),
                row.effective_local_boundary.as_deref(),
                &row.generation_source,
                &updated_at,
                preferences,
                now,
            )
        })
        .map(|row| row.date)
        .collect::<Vec<_>>();
    dates.dedup();
    Ok(dates)
}

#[derive(Debug, FromRow)]
struct DashboardReadableBriefRow {
    id: String,
    date: String,
    window_start_utc: Option<String>,
    window_end_utc: Option<String>,
    effective_time_zone: Option<String>,
    effective_local_boundary: Option<String>,
    generation_source: String,
    content_markdown: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct DashboardReadableBriefDetail {
    id: String,
    date: String,
    window_start: Option<String>,
    window_end: Option<String>,
    effective_time_zone: Option<String>,
    effective_local_boundary: Option<String>,
    release_ids: Vec<String>,
    covers_repo_stars: bool,
    covers_followers: bool,
    content_markdown: String,
    created_at: String,
    updated_at: String,
}

fn brief_uses_markdown_release_fallback(generation_source: &str) -> bool {
    matches!(
        generation_source,
        "legacy" | "history_recompute_failed" | "content_refresh_failed"
    )
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

fn brief_markdown_has_section(markdown: &str, heading: &str) -> bool {
    markdown.lines().any(|line| line.trim() == heading)
}

async fn resolve_brief_release_refs(
    pool: &PgPool,
    refs: &[InternalReleaseRef],
) -> Result<HashMap<InternalReleaseRef, i64>, sqlx::Error> {
    let mut unique_refs = Vec::<InternalReleaseRef>::new();
    let mut seen_refs = HashSet::<InternalReleaseRef>::new();
    for reference in refs {
        if seen_refs.insert(reference.clone()) {
            unique_refs.push(reference.clone());
        }
    }

    let mut resolved = HashMap::<InternalReleaseRef, i64>::new();
    let mut locators = Vec::<ReleaseLocator>::new();
    for reference in &unique_refs {
        match reference {
            InternalReleaseRef::ReleaseId(release_id) => {
                resolved.insert(reference.clone(), *release_id);
            }
            InternalReleaseRef::Locator(locator) => locators.push(locator.clone()),
        }
    }

    for chunk in locators.chunks(BRIEF_RELEASE_REF_LOCATOR_BATCH_LIMIT) {
        let tags = chunk
            .iter()
            .map(|locator| locator.tag.clone())
            .collect::<Vec<_>>();
        let rows = sqlx::query_as::<_, (i64, String)>(
            r#"
            SELECT release_id, html_url
            FROM public.repo_releases
            WHERE tag_name = ANY($1)
            ORDER BY published_at DESC NULLS LAST, created_at DESC NULLS LAST, release_id DESC
            "#,
        )
        .bind(&tags)
        .fetch_all(pool)
        .await?;
        let mut matched = HashMap::<(String, String, String), i64>::new();
        for (release_id, html_url) in rows {
            let Some(locator) = parse_release_locator_from_github_release_url(&html_url) else {
                continue;
            };
            matched
                .entry((
                    locator.owner.to_ascii_lowercase(),
                    locator.repo.to_ascii_lowercase(),
                    locator.tag,
                ))
                .or_insert(release_id);
        }
        for locator in chunk {
            let key = (
                locator.owner.to_ascii_lowercase(),
                locator.repo.to_ascii_lowercase(),
                locator.tag.clone(),
            );
            if let Some(release_id) = matched.get(&key).copied() {
                resolved.insert(InternalReleaseRef::Locator(locator.clone()), release_id);
            }
        }
    }
    Ok(resolved)
}

async fn load_dashboard_readable_briefs_for_dates(
    state: &PostgresServerState,
    user_id: &str,
    dates: &[String],
    preferences: &DailyBriefPreferences,
    now: DateTime<Utc>,
) -> Result<Vec<DashboardReadableBriefDetail>, Response> {
    if dates.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, DashboardReadableBriefRow>(
        r#"
        SELECT id, date, window_start_utc::text AS window_start_utc,
               window_end_utc::text AS window_end_utc,
               effective_time_zone, effective_local_boundary,
               generation_source, content_markdown,
               created_at::text AS created_at, updated_at::text AS updated_at
        FROM public.briefs
        WHERE user_id = $1
          AND date = ANY($2)
        ORDER BY COALESCE(window_end_utc, created_at) DESC, created_at DESC, id DESC
        "#,
    )
    .bind(user_id)
    .bind(dates)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load dashboard briefs", error))?;
    let rows = rows
        .into_iter()
        .filter(|row| {
            let window_start = normalize_timestamp_opt(row.window_start_utc.as_deref());
            let window_end = normalize_timestamp_opt(row.window_end_utc.as_deref());
            let updated_at = normalize_timestamp(&row.updated_at);
            brief_row_is_visible(
                &row.date,
                window_start.as_deref(),
                window_end.as_deref(),
                row.effective_time_zone.as_deref(),
                row.effective_local_boundary.as_deref(),
                &row.generation_source,
                &updated_at,
                preferences,
                now,
            )
        })
        .collect::<Vec<_>>();

    let memberships = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT m.brief_id, m.release_id
        FROM public.brief_release_memberships m
        JOIN public.briefs b ON b.id = m.brief_id
        WHERE b.user_id = $1
          AND b.date = ANY($2)
        ORDER BY m.brief_id ASC, m.ordinal ASC
        "#,
    )
    .bind(user_id)
    .bind(dates)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load dashboard brief memberships", error))?;
    let mut release_ids_by_brief = HashMap::<String, Vec<String>>::new();
    for (brief_id, release_id) in memberships {
        release_ids_by_brief
            .entry(brief_id)
            .or_default()
            .push(release_id.to_string());
    }

    let mut fallback_refs_by_brief = HashMap::<String, Vec<InternalReleaseRef>>::new();
    let mut fallback_refs = Vec::new();
    for row in &rows {
        if release_ids_by_brief.contains_key(&row.id)
            || !brief_uses_markdown_release_fallback(&row.generation_source)
        {
            continue;
        }
        let refs = extract_brief_release_refs(&row.content_markdown);
        if refs.is_empty() {
            continue;
        }
        fallback_refs.extend(refs.iter().cloned());
        fallback_refs_by_brief.insert(row.id.clone(), refs);
    }
    let resolved_fallback_ids = if fallback_refs.is_empty() {
        HashMap::new()
    } else {
        resolve_brief_release_refs(&state.runtime.pool, &fallback_refs)
            .await
            .map_err(|error| database_error("resolve brief release refs", error))?
    };

    Ok(rows
        .into_iter()
        .map(|row| {
            let release_ids = release_ids_by_brief
                .remove(&row.id)
                .or_else(|| {
                    fallback_refs_by_brief.remove(&row.id).map(|refs| {
                        let mut seen = HashSet::new();
                        refs.into_iter()
                            .filter_map(|reference| resolved_fallback_ids.get(&reference).copied())
                            .filter(|release_id| seen.insert(*release_id))
                            .map(|release_id| release_id.to_string())
                            .collect()
                    })
                })
                .unwrap_or_default();
            DashboardReadableBriefDetail {
                id: row.id,
                date: row.date,
                window_start: normalize_timestamp_opt(row.window_start_utc.as_deref()),
                window_end: normalize_timestamp_opt(row.window_end_utc.as_deref()),
                effective_time_zone: row.effective_time_zone,
                effective_local_boundary: row.effective_local_boundary,
                release_ids,
                covers_repo_stars: brief_markdown_has_section(&row.content_markdown, "### 获星"),
                covers_followers: brief_markdown_has_section(&row.content_markdown, "### 关注"),
                content_markdown: row.content_markdown,
                created_at: normalize_timestamp(&row.created_at),
                updated_at: normalize_timestamp(&row.updated_at),
            }
        })
        .collect())
}

#[derive(Debug, FromRow)]
struct DashboardReadableTimestampRow {
    ts: String,
}

async fn load_dashboard_readable_timestamps(
    state: &PostgresServerState,
    user_id: &str,
) -> Result<Vec<String>, Response> {
    let rows = sqlx::query_as::<_, DashboardReadableTimestampRow>(
        r#"
        WITH visible_repos AS (
          SELECT DISTINCT repo_id
          FROM public.user_release_visible_repos
          WHERE user_id = $1
        )
        SELECT COALESCE(r.published_at, r.created_at, r.updated_at) AS ts
        FROM public.repo_releases r
        JOIN visible_repos vr ON vr.repo_id = r.repo_id
        WHERE COALESCE(r.published_at, r.created_at, r.updated_at) IS NOT NULL
        UNION ALL
        SELECT e.occurred_at AS ts
        FROM public.social_activity_events e
        WHERE e.user_id = $2
        ORDER BY ts DESC
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load dashboard readable timestamps", error))?;
    Ok(rows.into_iter().map(|row| row.ts).collect())
}

fn dashboard_readable_date_window_utc(date: &str, time_zone: &str) -> Option<(String, String)> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let time_zone = time_zone.parse::<Tz>().ok()?;
    let start = time_zone
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()?
        .with_timezone(&Utc);
    let end = time_zone
        .from_local_datetime(&date.succ_opt()?.and_hms_opt(0, 0, 0)?)
        .earliest()?
        .with_timezone(&Utc);
    Some((
        start.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        end.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    ))
}

async fn fetch_dashboard_readable_rows_for_date(
    state: &PostgresServerState,
    user_id: &str,
    date: &str,
    time_zone: &str,
    limit: i64,
) -> Result<Vec<FeedRow>, Response> {
    let Some((start, end)) = dashboard_readable_date_window_utc(date, time_zone) else {
        return Ok(Vec::new());
    };
    fetch_feed_items(
        &state.runtime.pool,
        user_id,
        None,
        FeedTypeSelection::all(),
        None,
        None,
        limit,
        Some((start.as_str(), end.as_str())),
    )
    .await
    .map_err(|error| database_error("load dashboard readable rows", error))
}

fn dashboard_readable_local_date(ts: &str, time_zone: &str) -> String {
    let fallback = ts.chars().take(10).collect::<String>();
    let Ok(parsed) = DateTime::parse_from_rfc3339(ts) else {
        return fallback;
    };
    let Ok(tz) = time_zone.parse::<Tz>() else {
        return parsed.date_naive().to_string();
    };
    parsed.with_timezone(&tz).date_naive().to_string()
}

fn dashboard_readable_item_is_covered(
    item: &FeedItem,
    brief: &DashboardReadableBriefDetail,
) -> bool {
    if item.kind == "release" {
        return brief.release_ids.iter().any(|id| id == &item.id);
    }
    if item.kind == "repo_star_received" {
        return brief.covers_repo_stars;
    }
    if item.kind == "follower_received" {
        return brief.covers_followers;
    }
    false
}

fn dashboard_readable_item_in_brief_window(
    item: &FeedItem,
    brief: &DashboardReadableBriefDetail,
    fallback_date: &str,
    time_zone: &str,
) -> bool {
    let parsed_item = DateTime::parse_from_rfc3339(&item.ts).ok();
    if let (Some(start), Some(end), Some(item_ts)) = (
        brief
            .window_start
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok()),
        brief
            .window_end
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok()),
        parsed_item,
    ) {
        return item_ts >= start && item_ts < end;
    }
    dashboard_readable_local_date(&item.ts, time_zone) == fallback_date
}

fn dashboard_readable_brief_public(brief: &DashboardReadableBriefDetail) -> DashboardReadableBrief {
    DashboardReadableBrief {
        id: brief.id.clone(),
        date: brief.date.clone(),
        window_start: brief.window_start.clone(),
        window_end: brief.window_end.clone(),
        effective_time_zone: brief.effective_time_zone.clone(),
        effective_local_boundary: brief.effective_local_boundary.clone(),
        release_count: brief.release_ids.len(),
        covers_repo_stars: brief.covers_repo_stars,
        covers_followers: brief.covers_followers,
        content_markdown: brief.content_markdown.clone(),
        created_at: brief.created_at.clone(),
        updated_at: brief.updated_at.clone(),
    }
}

// ---------------------------------------------------------------------------
// Readable cursor codecs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DashboardReadableSectionCursor {
    date: String,
    user: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DashboardReadableItemsCursor {
    offset: usize,
    section: Option<String>,
    user: Option<String>,
}

fn dashboard_readable_cursor_subject(user_id: &str) -> String {
    crate::ai::sha256_hex(&format!("dashboard-readable:{user_id}"))
        .chars()
        .take(24)
        .collect()
}

fn encode_dashboard_readable_section_cursor_for_user(user_id: &str, date: &str) -> String {
    let payload = json!({
        "date": date,
        "user": if user_id.is_empty() {
            Value::Null
        } else {
            Value::String(dashboard_readable_cursor_subject(user_id))
        },
    });
    URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes())
}

fn decode_dashboard_readable_section_cursor_for_user(
    raw: &str,
    user_id: Option<&str>,
) -> Result<String, Response> {
    let bytes = URL_SAFE_NO_PAD
        .decode(raw.trim())
        .map_err(|_| bad_request("invalid readable section cursor"))?;
    let cursor = serde_json::from_slice::<DashboardReadableSectionCursor>(&bytes)
        .map_err(|_| bad_request("invalid readable section cursor"))?;
    NaiveDate::parse_from_str(&cursor.date, "%Y-%m-%d")
        .map_err(|_| bad_request("invalid readable section cursor"))?;
    if let Some(user_id) = user_id {
        let expected = dashboard_readable_cursor_subject(user_id);
        if cursor.user.as_deref() != Some(expected.as_str()) {
            return Err(bad_request("invalid readable section cursor"));
        }
    }
    Ok(cursor.date)
}

fn encode_dashboard_readable_items_cursor_for_context(
    offset: usize,
    user_id: &str,
    section_id: &str,
) -> String {
    let payload = json!({
        "offset": offset,
        "section": if section_id.is_empty() {
            Value::Null
        } else {
            Value::String(section_id.to_owned())
        },
        "user": if user_id.is_empty() {
            Value::Null
        } else {
            Value::String(dashboard_readable_cursor_subject(user_id))
        },
    });
    URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes())
}

fn decode_dashboard_readable_items_cursor_for_context(
    raw: &str,
    user_id: Option<&str>,
    section_id: Option<&str>,
) -> Result<usize, Response> {
    let bytes = URL_SAFE_NO_PAD
        .decode(raw.trim())
        .map_err(|_| bad_request("invalid section items cursor"))?;
    let cursor = serde_json::from_slice::<DashboardReadableItemsCursor>(&bytes)
        .map_err(|_| bad_request("invalid section items cursor"))?;
    if let Some(user_id) = user_id {
        let expected = dashboard_readable_cursor_subject(user_id);
        if cursor.user.as_deref() != Some(expected.as_str()) {
            return Err(bad_request("invalid section items cursor"));
        }
    }
    if let Some(section_id) = section_id
        && cursor.section.as_deref() != Some(section_id)
    {
        return Err(bad_request("invalid section items cursor"));
    }
    Ok(cursor.offset)
}

// ---------------------------------------------------------------------------
// Dashboard readable handlers
// ---------------------------------------------------------------------------

pub(crate) async fn dashboard_feed(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<DashboardReadableFeedQuery>,
) -> Response {
    let user_id = match require_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let preferences = match load_brief_preferences(&state, &user_id).await {
        Ok(preferences) => preferences,
        Err(response) => return response,
    };
    let now = Utc::now();
    let time_zone = preferences.time_zone.as_str();
    let brief_dates =
        match load_dashboard_readable_brief_dates(&state, &user_id, &preferences, now).await {
            Ok(dates) => dates,
            Err(response) => return response,
        };
    let mut dates = match load_dashboard_readable_timestamps(&state, &user_id).await {
        Ok(timestamps) => timestamps
            .into_iter()
            .map(|ts| dashboard_readable_local_date(&ts, time_zone))
            .collect::<Vec<_>>(),
        Err(response) => return response,
    };
    dates.extend(brief_dates);
    dates.sort();
    dates.dedup();
    dates.sort_by(|a, b| b.cmp(a));
    if let Some(raw_cursor) = query
        .cursor
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let cursor_date =
            match decode_dashboard_readable_section_cursor_for_user(raw_cursor, Some(&user_id)) {
                Ok(date) => date,
                Err(response) => return response,
            };
        dates.retain(|date| date < &cursor_date);
    }

    let selected_dates = dates
        .iter()
        .take(DASHBOARD_READABLE_SECTION_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    let briefs = match load_dashboard_readable_briefs_for_dates(
        &state,
        &user_id,
        &selected_dates,
        &preferences,
        now,
    )
    .await
    {
        Ok(briefs) => briefs,
        Err(response) => return response,
    };

    let ai_enabled = state.ai_model.is_some();
    let mut sections = Vec::with_capacity(selected_dates.len());
    for date in &selected_dates {
        let brief = briefs
            .iter()
            .find(|candidate| candidate.date == *date)
            .cloned();
        let fetch_limit = if brief.is_some() {
            DASHBOARD_READABLE_SUPPLEMENTAL_FETCH_LIMIT
        } else {
            (DASHBOARD_READABLE_SECTION_ITEM_LIMIT + 1) as i64
        };
        let raw_items = match fetch_dashboard_readable_rows_for_date(
            &state,
            &user_id,
            date,
            time_zone,
            fetch_limit,
        )
        .await
        {
            Ok(rows) => rows
                .into_iter()
                .map(|row| feed_item_from_row(row, ai_enabled))
                .collect::<Vec<_>>(),
            Err(response) => return response,
        };
        let (kind, items, supplemental_items, item_count) = if let Some(brief) = brief.as_ref() {
            let supplemental = raw_items
                .iter()
                .filter(|item| {
                    dashboard_readable_item_in_brief_window(item, brief, date, time_zone)
                        && !dashboard_readable_item_is_covered(item, brief)
                })
                .cloned()
                .collect::<Vec<_>>();
            (
                "brief".to_owned(),
                Vec::new(),
                supplemental,
                raw_items.len(),
            )
        } else {
            let item_count = raw_items.len();
            let items = raw_items
                .into_iter()
                .take(DASHBOARD_READABLE_SECTION_ITEM_LIMIT)
                .collect::<Vec<_>>();
            ("raw".to_owned(), items, Vec::new(), item_count)
        };
        let section_id = encode_dashboard_readable_section_cursor_for_user(&user_id, date);
        let items_next_cursor =
            if kind == "raw" && item_count > DASHBOARD_READABLE_SECTION_ITEM_LIMIT {
                Some(encode_dashboard_readable_items_cursor_for_context(
                    DASHBOARD_READABLE_SECTION_ITEM_LIMIT,
                    &user_id,
                    &section_id,
                ))
            } else {
                None
            };
        let public_brief = brief.as_ref().map(dashboard_readable_brief_public);
        let window_start = brief.as_ref().and_then(|value| value.window_start.clone());
        let window_end = brief.as_ref().and_then(|value| value.window_end.clone());
        let can_generate_brief = if brief.is_none() {
            let key_date = match NaiveDate::parse_from_str(date, "%Y-%m-%d") {
                Ok(date) => date,
                Err(error) => return internal_error(error.to_string()),
            };
            match briefs::daily_brief_key_date_is_closed(&preferences, key_date, now) {
                Ok(closed) => closed,
                Err(error) => return internal_error(error.to_string()),
            }
        } else {
            false
        };
        sections.push(DashboardReadableSection {
            id: section_id,
            date: date.clone(),
            kind,
            window_start,
            window_end,
            brief: public_brief,
            items,
            supplemental_items,
            supplemental_next_cursor: None,
            items_next_cursor,
            item_count,
            can_generate_brief,
        });
    }
    let next_cursor = if dates.len() > selected_dates.len() {
        selected_dates
            .last()
            .map(|date| encode_dashboard_readable_section_cursor_for_user(&user_id, date))
    } else {
        None
    };
    (
        StatusCode::OK,
        Json(DashboardReadableFeedResponse {
            sections,
            next_cursor,
        }),
    )
        .into_response()
}

pub(crate) async fn dashboard_readable_section_items(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(section_id): Path<String>,
    Query(query): Query<DashboardReadableSectionItemsQuery>,
) -> Response {
    let user_id = match require_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let section_date =
        match decode_dashboard_readable_section_cursor_for_user(&section_id, Some(&user_id)) {
            Ok(date) => date,
            Err(response) => return response,
        };
    let offset = match query
        .cursor
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|cursor| {
            decode_dashboard_readable_items_cursor_for_context(
                cursor,
                Some(user_id.as_str()),
                Some(section_id.as_str()),
            )
        })
        .transpose()
    {
        Ok(offset) => offset.unwrap_or(0),
        Err(response) => return response,
    };
    let preferences = match load_brief_preferences(&state, &user_id).await {
        Ok(preferences) => preferences,
        Err(response) => return response,
    };
    let fetch_limit = offset
        .saturating_add(DASHBOARD_READABLE_SECTION_ITEM_LIMIT + 1)
        .min(i64::MAX as usize) as i64;
    let rows = match fetch_dashboard_readable_rows_for_date(
        &state,
        &user_id,
        &section_date,
        preferences.time_zone.as_str(),
        fetch_limit,
    )
    .await
    {
        Ok(rows) => rows,
        Err(response) => return response,
    };
    let ai_enabled = state.ai_model.is_some();
    let rows = rows
        .into_iter()
        .map(|row| feed_item_from_row(row, ai_enabled))
        .collect::<Vec<_>>();
    let page_end = offset.saturating_add(DASHBOARD_READABLE_SECTION_ITEM_LIMIT);
    let has_more = rows.len() > page_end;
    let items: Vec<FeedItem> = rows
        .into_iter()
        .skip(offset)
        .take(DASHBOARD_READABLE_SECTION_ITEM_LIMIT)
        .collect();
    let next_cursor = if has_more {
        Some(encode_dashboard_readable_items_cursor_for_context(
            offset + items.len(),
            user_id.as_str(),
            section_id.as_str(),
        ))
    } else {
        None
    };
    (StatusCode::OK, Json(FeedResponse { items, next_cursor })).into_response()
}

// ---------------------------------------------------------------------------
// Dashboard update signatures
// ---------------------------------------------------------------------------

fn decode_dashboard_updates_token(
    raw: Option<&str>,
) -> Result<Option<DashboardUpdatesToken>, Response> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| bad_request("invalid dashboard updates token"))?;
    serde_json::from_slice::<DashboardUpdatesToken>(&bytes)
        .map(Some)
        .map_err(|_| bad_request("invalid dashboard updates token"))
}

fn encode_dashboard_updates_token(token: &DashboardUpdatesToken) -> Result<String, Response> {
    let bytes = serde_json::to_vec(token).map_err(|error| internal_error(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn dashboard_update_signature_digest(signature: &str) -> String {
    crate::ai::sha256_hex(signature).chars().take(16).collect()
}

fn compact_dashboard_signatures(signatures: &[String]) -> Vec<String> {
    signatures
        .iter()
        .map(|signature| dashboard_update_signature_digest(signature))
        .collect()
}

fn dashboard_update_signature_key(signature: &str) -> &str {
    signature
        .split_once('|')
        .map(|(key, _)| key)
        .unwrap_or(signature)
}

fn build_dashboard_list_update(
    previous_signature_digests: Option<&[String]>,
    latest_signatures: &[String],
) -> DashboardListUpdate {
    let latest_keys = latest_signatures
        .iter()
        .map(|signature| dashboard_update_signature_key(signature).to_owned())
        .collect::<Vec<_>>();
    let latest_digests = compact_dashboard_signatures(latest_signatures);
    let Some(previous_signature_digests) = previous_signature_digests else {
        return DashboardListUpdate {
            changed: false,
            new_count: 0,
            latest_keys,
        };
    };
    if previous_signature_digests == latest_digests {
        return DashboardListUpdate {
            changed: false,
            new_count: 0,
            latest_keys: Vec::new(),
        };
    }
    let previous = previous_signature_digests
        .iter()
        .map(|digest| digest.as_str())
        .collect::<HashSet<_>>();
    let mut changed_keys = Vec::new();
    let mut seen_changed_keys = HashSet::new();
    for (signature, digest) in latest_signatures.iter().zip(latest_digests.iter()) {
        if previous.contains(digest.as_str()) {
            continue;
        }
        let key = dashboard_update_signature_key(signature);
        if seen_changed_keys.insert(key) {
            changed_keys.push(key.to_owned());
        }
    }
    DashboardListUpdate {
        changed: !changed_keys.is_empty(),
        new_count: changed_keys.len(),
        latest_keys: changed_keys,
    }
}

async fn load_dashboard_feed_signatures(
    state: &PostgresServerState,
    user_id: &str,
    types: FeedTypeSelection,
    scope: Option<&FeedScope>,
    viewer_login: Option<&str>,
) -> Result<Vec<String>, Response> {
    let scope_kind = match scope {
        Some(FeedScope::Repo { .. }) => "repo",
        Some(FeedScope::Repos { .. }) => "repos",
        Some(FeedScope::Org { .. }) => "org",
        Some(FeedScope::Mine) => "mine",
        Some(FeedScope::Following) => "following",
        None => "",
    };
    let scope_repo_item = match scope {
        Some(FeedScope::Repo { owner, repo }) => Some(format!("{owner}/{repo}")),
        _ => None,
    };
    let scope_repo_items = match scope {
        Some(FeedScope::Repos { items }) => Some(items.clone()),
        _ => None,
    };
    let scope_org = match scope {
        Some(FeedScope::Org { org }) => Some(org.as_str()),
        _ => None,
    };
    let scope_mine_owner = if matches!(scope, Some(FeedScope::Mine)) {
        viewer_login.unwrap_or("")
    } else {
        ""
    };
    let scoped_all = scope.is_some();

    #[derive(Debug, FromRow)]
    struct DashboardFeedSignatureRow {
        kind: String,
        entity_id: String,
        sort_ts: String,
    }

    let rows = sqlx::query_as::<_, DashboardFeedSignatureRow>(
        r#"
        WITH scoped_visible_repos AS (
          SELECT vr.repo_id, vr.full_name, vr.owner_login
          FROM public.user_release_visible_repos vr
          WHERE vr.user_id = $1
            AND $2 <> 'mine'
            AND ($3 = '' OR ($4 = 'org' AND lower(vr.owner_login) = lower($5)))
          UNION
          SELECT fr.repo_id, fr.full_name, fr.owner_login
          FROM public.user_following_repos fr
          WHERE fr.user_id = $6 AND $7 = 'following'
          UNION
          SELECT ura.repo_id, ura.repo_full_name, ura.owner_login
          FROM public.user_repo_associations ura
          WHERE ura.user_id = $8 AND ura.repo_id IS NOT NULL
            AND (($9 = 'repo' AND lower(ura.repo_full_name) = lower($10))
              OR ($11 = 'repos' AND EXISTS (
                SELECT 1 FROM unnest($12::text[]) AS item(value)
                WHERE lower(item.value) = lower(ura.repo_full_name)
              ))
              OR ($13 = 'org' AND lower(ura.owner_login) = lower($14)))
          UNION
          SELECT sr.repo_id, sr.full_name, sr.owner_login
          FROM public.starred_repos sr
          WHERE sr.user_id = $15
            AND NOT EXISTS (
              SELECT 1 FROM public.user_repo_associations ura
              WHERE ura.user_id = sr.user_id
                AND ura.repo_full_name_lower = lower(sr.full_name)
            )
            AND (($16 = 'repo' AND lower(sr.full_name) = lower($17))
              OR ($18 = 'repos' AND EXISTS (
                SELECT 1 FROM unnest($19::text[]) AS item(value)
                WHERE lower(item.value) = lower(sr.full_name)
              ))
              OR ($20 = 'org' AND lower(sr.owner_login) = lower($21)))
          UNION
          SELECT ob.repo_id, ob.repo_full_name,
                 split_part(ob.repo_full_name, '/', 1)
          FROM public.owned_repo_star_baselines ob
          WHERE ob.user_id = $22 AND $23 = 'mine'
            AND position('/' in ob.repo_full_name) > 1
            AND lower(split_part(ob.repo_full_name, '/', 1)) = lower($24)
        ),
        release_rows AS (
          SELECT 'release' AS kind, CAST(r.release_id AS TEXT) AS entity_id,
                 COALESCE(r.published_at, r.created_at, r.updated_at) AS sort_ts,
                 r.release_id AS numeric_id
          FROM public.repo_releases r
          JOIN scoped_visible_repos vr ON vr.repo_id = r.repo_id
          WHERE $25 = 1
        ),
        social_rows AS (
          SELECT e.kind, e.id AS entity_id, e.occurred_at AS sort_ts, CAST(NULL AS BIGINT) AS numeric_id
          FROM public.social_activity_events e
          LEFT JOIN scoped_visible_repos vr ON vr.repo_id = e.repo_id
          WHERE e.user_id = $26
            AND ($27 = 0 OR (e.repo_full_name IS NOT NULL AND vr.repo_id IS NOT NULL))
            AND (($28 = 1 AND e.kind = 'repo_star_received')
              OR ($29 = 1 AND e.kind = 'follower_received')
              OR ($30 = 1 AND e.kind IN ('announcement', 'release_update', 'repo_forked')))
        ),
        feed_rows AS (
          SELECT kind, entity_id, sort_ts, numeric_id FROM release_rows
          UNION ALL
          SELECT kind, entity_id, sort_ts, numeric_id FROM social_rows
        )
        SELECT f.kind, f.entity_id, f.sort_ts
        FROM feed_rows f
        ORDER BY f.sort_ts DESC,
          CASE f.kind WHEN 'release' THEN 5 WHEN 'announcement' THEN 4 WHEN 'release_update' THEN 4
            WHEN 'repo_forked' THEN 3 WHEN 'repo_star_received' THEN 2
            WHEN 'follower_received' THEN 1 ELSE 0 END DESC,
          COALESCE(f.numeric_id, 0) DESC, f.entity_id DESC
        LIMIT 30
        "#,
    )
    .bind(user_id)
    .bind(scope_kind)
    .bind(scope_kind)
    .bind(scope_kind)
    .bind(scope_org)
    .bind(user_id)
    .bind(scope_kind)
    .bind(user_id)
    .bind(scope_kind)
    .bind(scope_repo_item.as_deref())
    .bind(scope_kind)
    .bind(scope_repo_items.as_ref())
    .bind(scope_kind)
    .bind(scope_org)
    .bind(user_id)
    .bind(scope_kind)
    .bind(scope_repo_item.as_deref())
    .bind(scope_kind)
    .bind(scope_repo_items.as_ref())
    .bind(scope_kind)
    .bind(scope_org)
    .bind(user_id)
    .bind(scope_kind)
    .bind(scope_mine_owner)
    .bind(if scoped_all || types.releases {
        1_i64
    } else {
        0_i64
    })
    .bind(user_id)
    .bind(if scoped_all { 1_i64 } else { 0_i64 })
    .bind(if types.stars { 1_i64 } else { 0_i64 })
    .bind(if types.followers { 1_i64 } else { 0_i64 })
    .bind(if scoped_all || types.ambient {
        1_i64
    } else {
        0_i64
    })
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load dashboard feed signatures", error))?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let key = format!("{}:{}", row.kind, row.entity_id);
            let state_hash = crate::ai::sha256_hex(&format!("sort={}\n", row.sort_ts));
            format!("{key}|{state_hash}")
        })
        .collect())
}

async fn load_dashboard_brief_signatures(
    state: &PostgresServerState,
    user_id: &str,
) -> Result<Vec<String>, Response> {
    #[derive(Debug, FromRow)]
    struct BriefSignatureRow {
        id: String,
        date: String,
        window_start_utc: Option<String>,
        window_end_utc: Option<String>,
        effective_time_zone: Option<String>,
        effective_local_boundary: Option<String>,
        generation_source: String,
        updated_at: String,
    }

    let preferences = load_brief_preferences(state, user_id).await?;
    let now = Utc::now();
    let rows = sqlx::query_as::<_, BriefSignatureRow>(
        r#"
        SELECT id, date, window_start_utc::text AS window_start_utc,
               window_end_utc::text AS window_end_utc,
               effective_time_zone, effective_local_boundary,
               generation_source, updated_at::text AS updated_at
        FROM public.briefs
        WHERE user_id = $1
        ORDER BY COALESCE(window_end_utc, created_at) DESC, created_at DESC, id DESC
        LIMIT 50
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load dashboard brief signatures", error))?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            let window_start = normalize_timestamp_opt(row.window_start_utc.as_deref());
            let window_end = normalize_timestamp_opt(row.window_end_utc.as_deref());
            let updated_at = normalize_timestamp(&row.updated_at);
            brief_row_is_visible(
                &row.date,
                window_start.as_deref(),
                window_end.as_deref(),
                row.effective_time_zone.as_deref(),
                row.effective_local_boundary.as_deref(),
                &row.generation_source,
                &updated_at,
                &preferences,
                now,
            )
        })
        .map(|row| format!("brief:{}|{}", row.id, normalize_timestamp(&row.updated_at)))
        .collect())
}

async fn load_dashboard_notification_signatures(
    state: &PostgresServerState,
    user_id: &str,
) -> Result<Vec<String>, Response> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT 'notification:' || thread_id || '|' || COALESCE(updated_at, '') || '|' || CAST(unread AS TEXT)
        FROM public.notifications
        WHERE user_id = $1
        ORDER BY updated_at DESC, thread_id DESC
        LIMIT 50
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.runtime.pool)
    .await
    .map_err(|error| database_error("load dashboard notification signatures", error))
}

pub(crate) async fn dashboard_updates(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<DashboardUpdatesQuery>,
) -> Response {
    let user_id = match require_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let login = match viewer_login(&state, &user_id).await {
        Ok(login) => login,
        Err(response) => return response,
    };
    let previous = match decode_dashboard_updates_token(query.token.as_deref()) {
        Ok(previous) => previous,
        Err(response) => return response,
    };
    let include = match parse_dashboard_update_include(query.include.as_deref()) {
        Ok(include) => include,
        Err(response) => return response,
    };
    let feed_types = match parse_dashboard_feed_type(query.feed_type.as_deref()) {
        Ok(types) => types,
        Err(response) => return response,
    };
    let scope = match parse_feed_scope(
        query.scope.as_deref(),
        query.items.as_deref(),
        query.org.as_deref(),
    ) {
        Ok(scope) => scope,
        Err(response) => return response,
    };

    let mut next_token = DashboardUpdatesToken::default();
    let mut lists = DashboardUpdatesLists {
        feed: None,
        briefs: None,
        notifications: None,
    };

    if include[0] {
        let signatures = match load_dashboard_feed_signatures(
            &state,
            &user_id,
            feed_types,
            scope.as_ref(),
            Some(login.as_str()),
        )
        .await
        {
            Ok(signatures) => signatures,
            Err(response) => return response,
        };
        let previous_feed = previous.as_ref().map(|token| token.feed.as_slice());
        let feed_version = previous.as_ref().map_or(0, |token| token.feed_version);
        lists.feed = Some(
            if previous.is_some() && feed_version < DASHBOARD_UPDATES_SIGNATURE_VERSION {
                DashboardListUpdate {
                    changed: false,
                    new_count: 0,
                    latest_keys: Vec::new(),
                }
            } else {
                build_dashboard_list_update(previous_feed, &signatures)
            },
        );
        next_token.feed = compact_dashboard_signatures(&signatures);
        next_token.feed_version = DASHBOARD_UPDATES_SIGNATURE_VERSION;
    } else if let Some(previous) = previous.as_ref() {
        next_token.feed = previous.feed.clone();
        next_token.feed_version = previous.feed_version;
    }

    if include[1] {
        let signatures = match load_dashboard_brief_signatures(&state, &user_id).await {
            Ok(signatures) => signatures,
            Err(response) => return response,
        };
        let previous_briefs = previous.as_ref().map(|token| token.briefs.as_slice());
        lists.briefs = Some(build_dashboard_list_update(previous_briefs, &signatures));
        next_token.briefs = compact_dashboard_signatures(&signatures);
    } else if let Some(previous) = previous.as_ref() {
        next_token.briefs = previous.briefs.clone();
    }

    if include[2] {
        let signatures = match load_dashboard_notification_signatures(&state, &user_id).await {
            Ok(signatures) => signatures,
            Err(response) => return response,
        };
        let previous_notifications = previous
            .as_ref()
            .map(|token| token.notifications.as_slice());
        lists.notifications = Some(build_dashboard_list_update(
            previous_notifications,
            &signatures,
        ));
        next_token.notifications = compact_dashboard_signatures(&signatures);
    } else if let Some(previous) = previous.as_ref() {
        next_token.notifications = previous.notifications.clone();
    }

    let token = match encode_dashboard_updates_token(&next_token) {
        Ok(token) => token,
        Err(response) => return response,
    };
    (
        StatusCode::OK,
        Json(DashboardUpdatesResponse {
            token,
            generated_at: Utc::now().to_rfc3339(),
            lists,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod timestamp_normalization_tests {
    use super::normalize_timestamp;

    #[test]
    fn legacy_text_and_postgres_text_render_identically() {
        // SQLite stored RFC3339 text; PostgreSQL adds its own text form.
        assert_eq!(
            normalize_timestamp("2026-02-08T00:00:00Z"),
            "2026-02-08T00:00:00Z"
        );
        assert_eq!(
            normalize_timestamp("2026-02-08 00:00:00+00"),
            "2026-02-08T00:00:00Z"
        );
        assert_eq!(
            normalize_timestamp("2026-02-08 00:00:00.250000+00"),
            "2026-02-08T00:00:00.250Z"
        );
        assert_eq!(
            normalize_timestamp("2026-02-08 08:00:00+08:00"),
            "2026-02-08T00:00:00Z"
        );
        assert_eq!(
            normalize_timestamp("2026-02-08 00:00:00"),
            "2026-02-08T00:00:00Z"
        );
        assert_eq!(normalize_timestamp("  "), "");
    }
}
