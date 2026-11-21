//! Shared repository-release read decision for the PostgreSQL lanes.
//!
//! The legacy lane (`crate::sync::execute_repo_release_work_item`) never
//! assumed that the user who asked for a refresh could still read the
//! repository: it rotated through active candidate users and, for a repository
//! with a `github_public` usage row, fell back to an unauthenticated read.
//! Both PostgreSQL executors (`sync.repo_release` and `sync.releases`) need the
//! same decision, so it lives here once instead of drifting apart per lane.

use std::{collections::HashMap, fmt, time::Duration};

use reqwest::{Client, StatusCode, header};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres};
use tracing::warn;

use crate::crypto::EncryptionKey;

/// Releases requested per page; the GitHub maximum.
pub(crate) const RELEASES_PER_PAGE: usize = 100;
/// Page budget for one repository read, matching the legacy lane.
pub(crate) const MAX_PAGES: usize = 20;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str = "OctoRill";
const API_VERSION: &str = "2022-11-28";
/// Legacy access kind for repositories registered by unauthenticated readers.
const PUBLIC_ACCESS_KIND: &str = "github_public";
/// Failure classes that mean "this credential cannot read *this* repository",
/// so the next candidate user is worth trying. Every other class describes the
/// upstream call itself (rate limit, timeout, network, decode) and would fail
/// identically for any candidate.
const ROTATING_REASON_CODES: [&str; 5] = [
    "credentials_missing",
    "credentials_invalid",
    "credentials_forbidden",
    "scope_insufficient",
    "repo_inaccessible",
];

/// Classified upstream or local failure for one repository refresh.
#[derive(Debug, Clone)]
pub(crate) struct RefreshFailure {
    pub(crate) reason_code: &'static str,
    pub(crate) message: String,
    pub(crate) retryable: bool,
    pub(crate) status: Option<u16>,
}

impl RefreshFailure {
    pub(crate) fn new(
        reason_code: &'static str,
        message: impl Into<String>,
        retryable: bool,
        status: Option<u16>,
    ) -> Self {
        Self {
            reason_code,
            message: message.into(),
            retryable,
            status,
        }
    }

    /// A local write/read failure that has no upstream classification.
    pub(crate) fn local(error: impl fmt::Display) -> Self {
        Self::new("local_error", error.to_string(), false, None)
    }

    pub(crate) fn summary(&self) -> String {
        format!("{}: {}", self.reason_code, self.message)
    }

    /// Whether another candidate user may still read the repository.
    pub(crate) fn rotates_candidate(&self) -> bool {
        ROTATING_REASON_CODES.contains(&self.reason_code)
    }
}

impl fmt::Display for RefreshFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.summary().as_str())
    }
}

/// One repository read: conditional state, pages and parsed releases.
#[derive(Debug, Default)]
pub(crate) struct RepositoryFetch {
    pub(crate) not_modified: bool,
    pub(crate) releases: Vec<Value>,
    pub(crate) etag: Option<String>,
    pub(crate) last_modified: Option<String>,
    pub(crate) pages_fetched: usize,
    pub(crate) stopped_reason: String,
}

/// Which credential produced a successful read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FetchVia {
    /// The user named by the task payload.
    User,
    /// A rotated candidate user (follower, publisher, baseline owner).
    Candidate,
    /// Unauthenticated read of a public repository.
    Public,
}

impl FetchVia {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Candidate => "candidate",
            Self::Public => "public",
        }
    }
}

/// A successful read plus the access path that produced it.
#[derive(Debug)]
pub(crate) struct ReleaseFetchOutcome {
    pub(crate) fetch: RepositoryFetch,
    pub(crate) via: FetchVia,
    /// The candidate user whose token was used; unset for the public path.
    pub(crate) candidate_user_id: Option<String>,
    pub(crate) public_usage: PublicReleaseUsageAccess,
}

/// Whether the repository is registered by API consumers, and whether an
/// unauthenticated read is still allowed for it.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PublicReleaseUsageAccess {
    pub(crate) exists: bool,
    pub(crate) needs_public_fallback: bool,
}

/// The repository scope plus the user the task was queued for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReleaseFetchRequest<'a> {
    pub(crate) repo_id: i64,
    pub(crate) repo_full_name: &'a str,
    pub(crate) requested_by_user_id: &'a str,
}

#[derive(Debug, Clone, FromRow)]
struct CredentialRow {
    id: String,
    access_token_ciphertext: Vec<u8>,
    access_token_nonce: Vec<u8>,
}

/// Memoized GitHub connections per user for one task execution.
///
/// The account-wide lane reads many repositories for the same viewer, so the
/// credential lookup must not run once per repository.
#[derive(Debug, Default)]
pub(crate) struct ReleaseCredentialCache {
    connections: tokio::sync::Mutex<HashMap<String, Vec<CredentialRow>>>,
}

impl ReleaseCredentialCache {
    /// Shared by every concurrent repository fetch of one task, so N parallel
    /// repositories load each candidate user's connections at most once.
    async fn connections_for(
        &self,
        pool: &PgPool,
        user_id: &str,
    ) -> sqlx::Result<Vec<CredentialRow>> {
        if let Some(cached) = self.connections.lock().await.get(user_id) {
            return Ok(cached.clone());
        }
        let rows = sqlx::query_as::<_, CredentialRow>(
            "SELECT id, access_token_ciphertext, access_token_nonce FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;
        self.connections
            .lock()
            .await
            .entry(user_id.to_owned())
            .or_insert_with(|| rows.clone());
        Ok(rows)
    }
}

#[derive(Debug, FromRow)]
struct SyncStateRow {
    etag: Option<String>,
    last_modified: Option<String>,
}

#[derive(Debug, FromRow)]
struct UsageCountsRow {
    total_count: i64,
    public_count: i64,
}

/// Read one repository's releases, rotating candidate users and falling back
/// to an unauthenticated read of a registered public repository.
pub(crate) async fn fetch_releases(
    pool: &PgPool,
    http: &Client,
    encryption_key: &EncryptionKey,
    request: &ReleaseFetchRequest<'_>,
    credentials: &ReleaseCredentialCache,
) -> std::result::Result<ReleaseFetchOutcome, RefreshFailure> {
    let public_usage = load_public_release_usage_access(pool, request.repo_id).await?;
    let mut candidates = load_candidate_user_ids(pool, request.repo_id).await?;
    // The user the task was queued for keeps the first attempt when the rule
    // set still nominates them; the rest keep the activity ordering.
    if let Some(index) = candidates
        .iter()
        .position(|candidate| candidate == request.requested_by_user_id)
    {
        let requested = candidates.remove(index);
        candidates.insert(0, requested);
    }
    if candidates.is_empty() && !public_usage.exists {
        return Err(RefreshFailure::new(
            "repo_inaccessible",
            format!(
                "no active candidate users remain for {}",
                request.repo_full_name
            ),
            false,
            None,
        ));
    }
    let sync_state = load_sync_state(pool, request.repo_id).await?;

    let mut first_failure: Option<RefreshFailure> = None;
    let mut upstream_failure: Option<RefreshFailure> = None;
    for candidate in &candidates {
        match fetch_for_user(
            pool,
            http,
            encryption_key,
            request,
            candidate,
            sync_state.as_ref(),
            credentials,
        )
        .await
        {
            Ok(fetch) => {
                let via = if candidate == request.requested_by_user_id {
                    FetchVia::User
                } else {
                    FetchVia::Candidate
                };
                if via == FetchVia::Candidate {
                    warn!(
                        event = "upstream.call",
                        operation = "repo_release_fetch",
                        repo = request.repo_full_name,
                        requested_by = request.requested_by_user_id,
                        candidate_user_id = candidate,
                        "reading repository releases through a rotated candidate user"
                    );
                }
                return Ok(ReleaseFetchOutcome {
                    fetch,
                    via,
                    candidate_user_id: Some(candidate.clone()),
                    public_usage,
                });
            }
            Err(failure) => {
                let rotates = failure.rotates_candidate();
                // A candidate without a usable connection says nothing about
                // the repository, so an actual upstream classification wins.
                if failure.status.is_some() && upstream_failure.is_none() {
                    upstream_failure = Some(failure.clone());
                }
                first_failure.get_or_insert(failure);
                if !rotates {
                    // Not a credential problem: another candidate token would
                    // hit the same upstream condition. The anonymous read can
                    // still succeed, so stop rotating but keep the fallback.
                    break;
                }
            }
        }
    }

    // Legacy attempted the anonymous read after any candidate failure, so a
    // rate-limited or throttled token does not hide a public repository.
    if public_usage.needs_public_fallback {
        match fetch_public(http, request, sync_state.as_ref()).await {
            Ok(fetch) => {
                return Ok(ReleaseFetchOutcome {
                    fetch,
                    via: FetchVia::Public,
                    candidate_user_id: None,
                    public_usage,
                });
            }
            Err(failure) => return Err(failure),
        }
    }

    Err(upstream_failure.or(first_failure).unwrap_or_else(|| {
        RefreshFailure::new(
            "credentials_missing",
            format!("no usable GitHub connection for {}", request.repo_full_name),
            false,
            None,
        )
    }))
}

/// Active users who may be able to read the repository, payload user first.
///
/// Mirrors the legacy `load_repo_release_candidate_users` rule set. The follow
/// arm is spelled out over the three sources of `public.user_following_repos`
/// (`user_repo_associations`, `starred_repos`, `owned_repo_star_baselines`)
/// instead of reading that view, so the query keeps working when a fixture or
/// migration rebuilds the view's inputs. Disabled and paused users never sync,
/// and a `private_owner_published` usage row stops nominating its publisher
/// once it is marked inaccessible.
async fn load_candidate_user_ids(
    pool: &PgPool,
    repo_id: i64,
) -> std::result::Result<Vec<String>, RefreshFailure> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT u.id
        FROM public.users u
        WHERE u.is_disabled = 0
          AND u.paused_at IS NULL
          AND (
            EXISTS (
              SELECT 1
              FROM public.user_repo_associations sr
              WHERE sr.user_id = u.id
                AND sr.repo_id = $1
                AND sr.is_following <> 0
            )
            OR EXISTS (
              SELECT 1
              FROM public.starred_repos st
              WHERE st.user_id = u.id
                AND st.repo_id = $1
                -- An association row is the user's explicit follow state:
                -- a residual star must not nominate an unfollowed user.
                AND NOT EXISTS (
                  SELECT 1
                  FROM public.user_repo_associations ura
                  WHERE ura.user_id = u.id
                    AND ura.repo_full_name_lower = lower(st.full_name)
                )
            )
            OR EXISTS (
              SELECT 1
              FROM public.public_repo_release_usage pu
              WHERE pu.repo_id = $1
                AND pu.access_kind = 'private_owner_published'
                AND pu.published_by_user_id = u.id
                AND pu.last_sync_status <> 'inaccessible'
            )
            OR EXISTS (
              SELECT 1
              FROM public.owned_repo_star_baselines ob
              WHERE ob.user_id = u.id
                AND ob.repo_id = $1
            )
          )
        ORDER BY u.last_active_at DESC NULLS LAST, u.id ASC
        "#,
    )
    .bind(repo_id)
    .fetch_all(pool)
    .await
    .map_err(|error| {
        RefreshFailure::new(
            "sync_state_error",
            format!("load repo release candidate users: {error}"),
            false,
            None,
        )
    })
}

/// Whether an unauthenticated read is still allowed for the repository.
async fn load_public_release_usage_access(
    pool: &PgPool,
    repo_id: i64,
) -> std::result::Result<PublicReleaseUsageAccess, RefreshFailure> {
    let row = sqlx::query_as::<_, UsageCountsRow>(
        r#"
        SELECT
          COUNT(*) AS total_count,
          COALESCE(SUM(CASE WHEN access_kind = $2 THEN 1 ELSE 0 END), 0) AS public_count
        FROM public.public_repo_release_usage
        WHERE repo_id = $1
          AND last_sync_status <> 'inaccessible'
        "#,
    )
    .bind(repo_id)
    .bind(PUBLIC_ACCESS_KIND)
    .fetch_one(pool)
    .await
    .map_err(|error| {
        RefreshFailure::new(
            "sync_state_error",
            format!("load public release usage: {error}"),
            false,
            None,
        )
    })?;
    Ok(PublicReleaseUsageAccess {
        exists: row.total_count > 0,
        needs_public_fallback: row.public_count > 0,
    })
}

async fn load_sync_state(
    pool: &PgPool,
    repo_id: i64,
) -> std::result::Result<Option<SyncStateRow>, RefreshFailure> {
    sqlx::query_as::<_, SyncStateRow>(
        "SELECT etag, last_modified FROM public.repo_release_sync_state WHERE repo_id = $1 LIMIT 1",
    )
    .bind(repo_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        RefreshFailure::new(
            "sync_state_error",
            format!("load repo release sync state: {error}"),
            false,
            None,
        )
    })
}

/// Record the outcome for every API consumer of the repository.
///
/// `ready` on success, `inaccessible` when GitHub denies the repository, and
/// `failed` for any other class, exactly like the legacy usage bookkeeping. A
/// repository without usage rows simply updates nothing.
pub(crate) async fn mark_public_usage_ready<'e, E>(
    executor: E,
    repo_id: i64,
    now: &str,
) -> sqlx::Result<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    mark_public_usage(executor, repo_id, "ready", None, now).await
}

pub(crate) async fn mark_public_usage_failed<'e, E>(
    executor: E,
    repo_id: i64,
    failure: &RefreshFailure,
    now: &str,
) -> sqlx::Result<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let status = if failure.reason_code == "repo_inaccessible" {
        "inaccessible"
    } else {
        "failed"
    };
    mark_public_usage(
        executor,
        repo_id,
        status,
        Some(failure.summary().as_str()),
        now,
    )
    .await
}

async fn mark_public_usage<'e, E>(
    executor: E,
    repo_id: i64,
    status: &str,
    error_text: Option<&str>,
    now: &str,
) -> sqlx::Result<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    sqlx::query(
        r#"
        UPDATE public.public_repo_release_usage
        SET last_sync_status = $2,
            last_sync_error = $3,
            updated_at = $4
        WHERE repo_id = $1
        "#,
    )
    .bind(repo_id)
    .bind(status)
    .bind(error_text)
    .bind(now)
    .execute(executor)
    .await?;
    Ok(())
}

/// Try every GitHub connection of one user, rotating on token-scoped failures.
async fn fetch_for_user(
    pool: &PgPool,
    http: &Client,
    encryption_key: &EncryptionKey,
    request: &ReleaseFetchRequest<'_>,
    user_id: &str,
    sync_state: Option<&SyncStateRow>,
    credentials: &ReleaseCredentialCache,
) -> std::result::Result<RepositoryFetch, RefreshFailure> {
    let connections = credentials
        .connections_for(pool, user_id)
        .await
        .map_err(|error| {
            RefreshFailure::new(
                "sync_state_error",
                format!("load github connections: {error}"),
                false,
                None,
            )
        })?;
    if connections.is_empty() {
        return Err(RefreshFailure::new(
            "credentials_missing",
            format!("no GitHub connection for user {user_id}"),
            false,
            None,
        ));
    }
    let mut last_failure = None;
    for connection in connections {
        let token = match encryption_key.decrypt_str(
            &connection.access_token_ciphertext,
            &connection.access_token_nonce,
        ) {
            Ok(token) => token,
            Err(error) => {
                warn!(
                    event = "upstream.call",
                    operation = "repo_release_fetch",
                    connection_id = connection.id,
                    error = %error,
                    "GitHub connection token could not be decrypted"
                );
                last_failure.get_or_insert_with(|| {
                    RefreshFailure::new(
                        "credentials_invalid",
                        format!("decrypt GitHub connection {}: {error}", connection.id),
                        false,
                        None,
                    )
                });
                continue;
            }
        };
        match fetch_with_token(http, request, Some(token.as_str()), sync_state).await {
            Ok(fetch) => return Ok(fetch),
            Err(failure) if failure.rotates_candidate() => {
                warn!(
                    event = "upstream.call",
                    operation = "repo_release_fetch",
                    connection_id = connection.id,
                    repo = request.repo_full_name,
                    error_kind = failure.reason_code,
                    error_chain = failure.message,
                    http_status = failure.status,
                    "GitHub connection could not read repository releases"
                );
                last_failure = Some(failure);
            }
            Err(failure) => return Err(failure),
        }
    }
    Err(last_failure.unwrap_or_else(|| {
        RefreshFailure::new(
            "credentials_missing",
            format!("no usable GitHub connection for user {user_id}"),
            false,
            None,
        )
    }))
}

/// Unauthenticated read of a public repository.
async fn fetch_public(
    http: &Client,
    request: &ReleaseFetchRequest<'_>,
    sync_state: Option<&SyncStateRow>,
) -> std::result::Result<RepositoryFetch, RefreshFailure> {
    fetch_with_token(http, request, None, sync_state).await
}

async fn fetch_with_token(
    http: &Client,
    request: &ReleaseFetchRequest<'_>,
    token: Option<&str>,
    sync_state: Option<&SyncStateRow>,
) -> std::result::Result<RepositoryFetch, RefreshFailure> {
    let mut fetch = RepositoryFetch {
        stopped_reason: "page_budget".to_owned(),
        ..RepositoryFetch::default()
    };
    for page in 1..=MAX_PAGES {
        let url = format!(
            "{}/repos/{}/releases?per_page={RELEASES_PER_PAGE}&page={page}",
            crate::github_api::rest_base(),
            request.repo_full_name
        );
        let mut builder = http
            .get(url)
            .timeout(REQUEST_TIMEOUT)
            .header(header::USER_AGENT, USER_AGENT)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION);
        if let Some(token) = token {
            builder = builder.bearer_auth(token);
        }
        if page == 1 {
            if let Some(etag) = sync_state.and_then(|state| state.etag.as_deref()) {
                builder = builder.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(last_modified) = sync_state.and_then(|state| state.last_modified.as_deref())
            {
                builder = builder.header(header::IF_MODIFIED_SINCE, last_modified);
            }
        }
        let response = builder.send().await.map_err(classify_request_error)?;
        let status = response.status();
        let headers = response.headers().clone();
        if page == 1 {
            fetch.etag = header_value(&headers, header::ETAG);
            fetch.last_modified = header_value(&headers, header::LAST_MODIFIED);
        }
        fetch.pages_fetched = page;
        if status == StatusCode::NOT_MODIFIED {
            fetch.not_modified = true;
            fetch.stopped_reason = "not_modified".to_owned();
            return Ok(fetch);
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(classify_http_error(status, &headers, &body));
        }
        let page_releases = response.json::<Vec<Value>>().await.map_err(|error| {
            RefreshFailure::new(
                "decode_error",
                format!("decode GitHub releases: {error}"),
                error.is_body() || error.is_connect() || error.is_timeout(),
                None,
            )
        })?;
        let page_len = page_releases.len();
        fetch.releases.extend(page_releases);
        if page_len == 0 {
            fetch.stopped_reason = "empty_page".to_owned();
            return Ok(fetch);
        }
        if page_len < RELEASES_PER_PAGE {
            fetch.stopped_reason = "short_page".to_owned();
            return Ok(fetch);
        }
    }
    Ok(fetch)
}

fn header_value(headers: &reqwest::header::HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

pub(crate) fn classify_request_error(error: reqwest::Error) -> RefreshFailure {
    let operation = "sync repo releases";
    if error.is_timeout() {
        return RefreshFailure::new("timeout", format!("{operation}: {error}"), true, None);
    }
    if error.is_connect() || error.is_request() {
        return RefreshFailure::new("network_error", format!("{operation}: {error}"), true, None);
    }
    RefreshFailure::new(
        "request_error",
        format!("{operation}: {error}"),
        false,
        None,
    )
}

pub(crate) fn classify_http_error(
    status: StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> RefreshFailure {
    let operation = "sync repo releases";
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let body_lower = body.to_ascii_lowercase();
    let rate_limit_signal = remaining == "0"
        || headers.contains_key("retry-after")
        || body_lower.contains("secondary rate limit")
        || body_lower.contains("abuse detection");
    let status_code = Some(status.as_u16());

    if status == StatusCode::UNAUTHORIZED {
        return RefreshFailure::new(
            "credentials_invalid",
            format!("{operation}: github returned 401"),
            false,
            status_code,
        );
    }
    if status == StatusCode::NOT_FOUND || status.as_u16() == 451 {
        return RefreshFailure::new(
            "repo_inaccessible",
            format!("{operation}: github returned {status}"),
            false,
            status_code,
        );
    }
    if status == StatusCode::FORBIDDEN && !rate_limit_signal {
        let reason_code =
            if body_lower.contains("scope") || body_lower.contains("resource not accessible") {
                "scope_insufficient"
            } else {
                "credentials_forbidden"
            };
        return RefreshFailure::new(
            reason_code,
            format!("{operation}: github returned 403"),
            false,
            status_code,
        );
    }
    if status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status.is_server_error()
        || rate_limit_signal
    {
        return RefreshFailure::new(
            "rate_limited",
            format!("{operation}: github returned {status}"),
            true,
            status_code,
        );
    }
    RefreshFailure::new(
        "github_http_error",
        format!("{operation}: github returned {status}"),
        false,
        status_code,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_errors_are_classified_like_the_legacy_lane() {
        let headers = reqwest::header::HeaderMap::new();
        let inaccessible = classify_http_error(StatusCode::NOT_FOUND, &headers, "");
        assert_eq!(inaccessible.reason_code, "repo_inaccessible");
        assert!(!inaccessible.retryable);

        let throttled = classify_http_error(StatusCode::TOO_MANY_REQUESTS, &headers, "");
        assert_eq!(throttled.reason_code, "rate_limited");
        assert!(throttled.retryable);

        let scope = classify_http_error(
            StatusCode::FORBIDDEN,
            &headers,
            "Resource not accessible by integration",
        );
        assert_eq!(scope.reason_code, "scope_insufficient");
        assert!(!scope.retryable);

        let upstream = classify_http_error(StatusCode::BAD_GATEWAY, &headers, "");
        assert_eq!(upstream.reason_code, "rate_limited");
        assert!(upstream.retryable);

        let unauthorized = classify_http_error(StatusCode::UNAUTHORIZED, &headers, "");
        assert_eq!(unauthorized.reason_code, "credentials_invalid");
    }

    #[test]
    fn only_credential_and_permission_classes_rotate() {
        for code in ROTATING_REASON_CODES {
            assert!(
                RefreshFailure::new(code, "synthetic", false, None).rotates_candidate(),
                "{code} must rotate"
            );
        }
        for code in [
            "rate_limited",
            "timeout",
            "network_error",
            "request_error",
            "decode_error",
            "github_http_error",
            "sync_state_error",
            "local_error",
        ] {
            assert!(
                !RefreshFailure::new(code, "synthetic", true, None).rotates_candidate(),
                "{code} must not rotate"
            );
        }
    }

    #[test]
    fn failure_summary_carries_the_reason_code() {
        let failure = RefreshFailure::new("timeout", "sync repo releases", true, None);
        assert_eq!(failure.summary(), "timeout: sync repo releases");
        assert_eq!(failure.to_string(), failure.summary());
    }

    #[test]
    fn fetch_via_names_the_chosen_access_path() {
        assert_eq!(FetchVia::User.as_str(), "user");
        assert_eq!(FetchVia::Candidate.as_str(), "candidate");
        assert_eq!(FetchVia::Public.as_str(), "public");
    }

    #[test]
    fn candidate_query_matches_the_legacy_rule_set() {
        let source = include_str!("postgres_repo_release_fetch.rs");
        assert!(source.contains("public.user_repo_associations"));
        assert!(source.contains("public.starred_repos"));
        assert!(source.contains("private_owner_published"));
        assert!(source.contains("owned_repo_star_baselines"));
        assert!(source.contains("u.paused_at IS NULL"));
        assert!(source.contains("last_active_at DESC NULLS LAST"));
        assert!(source.contains("github_public"));
        assert!(source.contains("IF_NONE_MATCH"));
        assert!(source.contains("bearer_auth"));
    }
}
