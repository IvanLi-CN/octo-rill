//! Shared repository-release read decision for the PostgreSQL lanes.
//!
//! The legacy lane (`crate::sync::execute_repo_release_work_item`) never
//! assumed that the user who asked for a refresh could still read the
//! repository: it rotated through active candidate users and, for a repository
//! with a `github_public` usage row, fell back to an unauthenticated read.
//! Both PostgreSQL executors (`sync.repo_release` and `sync.releases`) need the
//! same decision, so it lives here once instead of drifting apart per lane.
//!
//! The unauthenticated fallback is bounded by one process-wide
//! [`AnonymousBudget`]: GitHub counts anonymous requests per egress IP (60 per
//! hour), so a per-token accounting cannot see the exhaustion coming. A read
//! that finds the budget spent is deferred, not failed.

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, LazyLock, Mutex as StdMutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
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
/// Failure classes whose outcome can differ for another candidate user: the
/// credential cannot read *this* repository, or GitHub is throttling that
/// token. Rate limits are counted per token (and per token plus IP for the
/// secondary limit), so one throttled candidate says nothing about the next.
/// Every other class describes the upstream call itself (timeout, network,
/// decode) and would fail identically for any candidate.
const ROTATING_REASON_CODES: [&str; 6] = [
    "credentials_missing",
    "credentials_invalid",
    "credentials_forbidden",
    "scope_insufficient",
    "repo_inaccessible",
    "rate_limited",
];

/// Unauthenticated reads share GitHub's primary allowance for the egress IP:
/// 60 requests per hour. The local budget starts below that ceiling because the
/// same address also serves redirect hops and any other client behind the same
/// NAT, and because one repository refresh may spend several pages of it.
pub(crate) const ANONYMOUS_BUDGET_LIMIT: u32 = 50;
/// GitHub's primary rate-limit window; the fallback when no header names it.
const ANONYMOUS_BUDGET_WINDOW_SECS: i64 = 3_600;

/// A read that was not sent because the anonymous budget is spent.
///
/// This is not a failure: nothing upstream rejected the request and no error
/// state must be recorded. The next scheduled cycle retries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BudgetDeferral {
    /// Anonymous requests left when the read was deferred; always zero.
    pub(crate) remaining: u32,
    /// Unix seconds when GitHub refills the allowance, when a response said so.
    pub(crate) reset_at_unix: Option<i64>,
}

impl BudgetDeferral {
    pub(crate) fn summary(self) -> String {
        match self
            .reset_at_unix
            .and_then(|reset| chrono::DateTime::from_timestamp(reset, 0))
        {
            Some(reset) => format!(
                "deferred_budget: anonymous GitHub read budget exhausted; refills at {}",
                reset.to_rfc3339()
            ),
            None => "deferred_budget: anonymous GitHub read budget exhausted".to_owned(),
        }
    }
}

/// Why a repository read produced no snapshot.
#[derive(Debug, Clone)]
pub(crate) enum ReleaseFetchError {
    /// The upstream read, or the local bookkeeping around it, failed.
    Failed(RefreshFailure),
    /// The anonymous budget is spent, so no request was sent at all.
    Deferred(BudgetDeferral),
}

impl ReleaseFetchError {
    /// Whether another candidate user may still read the repository.
    fn rotates_candidate(&self) -> bool {
        match self {
            Self::Failed(failure) => failure.rotates_candidate(),
            Self::Deferred(_) => false,
        }
    }

    fn http_status(&self) -> Option<u16> {
        match self {
            Self::Failed(failure) => failure.status,
            Self::Deferred(_) => None,
        }
    }
}

impl From<RefreshFailure> for ReleaseFetchError {
    fn from(failure: RefreshFailure) -> Self {
        Self::Failed(failure)
    }
}

/// Process-wide allowance for unauthenticated GitHub requests.
///
/// GitHub counts anonymous requests per egress IP, so every anonymous read of
/// this process — the single-repository worker lane and the account lane alike —
/// draws from the instance behind [`shared_anonymous_budget`]. The guarded
/// struct is constructed directly by tests so the window logic needs neither a
/// server nor the wall clock.
#[derive(Debug)]
pub(crate) struct AnonymousBudget {
    limit: u32,
    state: StdMutex<BudgetState>,
}

#[derive(Debug)]
struct BudgetState {
    remaining: u32,
    /// Unix seconds when GitHub refills the allowance, when known.
    reset_at_unix: Option<i64>,
    /// Unix seconds of the last charge, the fallback window anchor.
    last_spend_unix: Option<i64>,
}

impl BudgetState {
    /// Whether the local window has ended.
    ///
    /// The instant comes from GitHub's reset header. Without one, the guard
    /// falls back to an hour after the last charge: a local counter that
    /// outlived its real allowance would otherwise wedge anonymous reads.
    fn refill_due(&self, now_unix: i64) -> bool {
        self.reset_at_unix
            .or_else(|| {
                self.last_spend_unix
                    .map(|spent| spent + ANONYMOUS_BUDGET_WINDOW_SECS)
            })
            .is_some_and(|refill_at| now_unix >= refill_at)
    }
}

/// The one budget of this process. Both release executors share this `Arc`.
static SHARED_ANONYMOUS_BUDGET: LazyLock<Arc<AnonymousBudget>> = LazyLock::new(|| {
    publish_anonymous_budget(ANONYMOUS_BUDGET_LIMIT);
    Arc::new(AnonymousBudget::with_limit(ANONYMOUS_BUDGET_LIMIT))
});

pub(crate) fn shared_anonymous_budget() -> Arc<AnonymousBudget> {
    Arc::clone(&SHARED_ANONYMOUS_BUDGET)
}

impl AnonymousBudget {
    pub(crate) fn with_limit(limit: u32) -> Self {
        Self {
            limit,
            state: StdMutex::new(BudgetState {
                remaining: limit,
                reset_at_unix: None,
                last_spend_unix: None,
            }),
        }
    }

    /// Requests left before the next refill. Read by the budget tests.
    #[cfg(test)]
    pub(crate) fn remaining(&self) -> u32 {
        self.guard().remaining
    }

    /// Charge one anonymous request, or report that the read must be deferred.
    ///
    /// The caller charges *before* sending, so a request that ends in an error
    /// or a 304 still counts: GitHub charged it too.
    pub(crate) fn acquire(&self, now_unix: i64) -> std::result::Result<(), BudgetDeferral> {
        let mut state = self.guard();
        if state.refill_due(now_unix) {
            // The window rolled over. Refill to the local ceiling; the next
            // response header calibrates the true count.
            state.remaining = self.limit;
            state.reset_at_unix = None;
            state.last_spend_unix = None;
        }
        if state.remaining == 0 {
            return Err(BudgetDeferral {
                remaining: 0,
                reset_at_unix: state.reset_at_unix,
            });
        }
        state.remaining -= 1;
        state.last_spend_unix = Some(now_unix);
        let remaining = state.remaining;
        drop(state);
        publish_anonymous_budget(remaining);
        Ok(())
    }

    /// Account for one redirect hop the shared HTTP client followed.
    ///
    /// The hop was already sent, so this only records it: it never defers, and
    /// it cannot take the count below zero.
    pub(crate) fn charge_redirect(&self) {
        let mut state = self.guard();
        state.remaining = state.remaining.saturating_sub(1);
        let remaining = state.remaining;
        drop(state);
        publish_anonymous_budget(remaining);
    }

    /// Reconcile with the `X-RateLimit-*` headers of a request just spent.
    ///
    /// The remaining count is authoritative only downwards: those headers also
    /// count anonymous requests this process never issued from the same egress
    /// IP. A later reset window is the exception — GitHub refilled the counter,
    /// so that header value replaces the stale local count.
    pub(crate) fn observe(&self, remaining: Option<u32>, reset_at_unix: Option<i64>) {
        let mut state = self.guard();
        let new_window = matches!(
            (reset_at_unix, state.reset_at_unix),
            (Some(reset), Some(stored)) if reset > stored
        );
        if new_window {
            state.remaining = remaining.unwrap_or(self.limit).min(self.limit);
            state.reset_at_unix = reset_at_unix;
        } else {
            if reset_at_unix.is_some() {
                state.reset_at_unix = reset_at_unix;
            }
            if let Some(remaining) = remaining {
                state.remaining = state.remaining.min(remaining);
            }
        }
        let remaining = state.remaining;
        drop(state);
        publish_anonymous_budget(remaining);
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, BudgetState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Publish the remaining allowance for scrapers; a no-op without a registry.
fn publish_anonymous_budget(remaining: u32) {
    if let Some(metrics) = crate::metrics::global() {
        metrics.set_anonymous_github_budget(remaining);
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// GitHub's primary rate-limit view of a response, for anonymous calibration.
fn parse_rate_limit(headers: &reqwest::header::HeaderMap) -> (Option<u32>, Option<i64>) {
    let parse = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<i64>().ok())
    };
    (
        parse("x-ratelimit-remaining").and_then(|value| u32::try_from(value).ok()),
        parse("x-ratelimit-reset"),
    )
}

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
///
/// `budget` is the process-wide anonymous allowance; only the fallback draws
/// from it, so candidate rotation is decided exactly as before.
pub(crate) async fn fetch_releases(
    pool: &PgPool,
    http: &Client,
    encryption_key: &EncryptionKey,
    request: &ReleaseFetchRequest<'_>,
    credentials: &ReleaseCredentialCache,
    budget: &AnonymousBudget,
) -> std::result::Result<ReleaseFetchOutcome, ReleaseFetchError> {
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
        )
        .into());
    }
    let sync_state = load_sync_state(pool, request.repo_id).await?;

    let (fetch, via, candidate_user_id) = match rotate_candidates(
        &candidates,
        request.requested_by_user_id,
        request.repo_full_name,
        &CredentialAttempt {
            pool,
            http,
            encryption_key,
            request,
            sync_state: sync_state.as_ref(),
            credentials,
        },
    )
    .await
    {
        Ok(read) => read,
        // A deferral describes the budget, not the repository: retrying another
        // credential or the anonymous path would spend money we do not have.
        Err(deferral @ ReleaseFetchError::Deferred(_)) => return Err(deferral),
        Err(ReleaseFetchError::Failed(failure)) => {
            // Legacy attempted the anonymous read after any candidate failure,
            // so a throttled token does not hide a public repository.
            if !public_usage.needs_public_fallback {
                return Err(failure.into());
            }
            let fetch = fetch_public(http, request, sync_state.as_ref(), budget).await?;
            return Ok(ReleaseFetchOutcome {
                fetch,
                via: FetchVia::Public,
                candidate_user_id: None,
                public_usage,
            });
        }
    };
    Ok(ReleaseFetchOutcome {
        fetch,
        via,
        candidate_user_id: Some(candidate_user_id),
        public_usage,
    })
}

/// One candidate user's chance at reading the repository.
///
/// Both callers of [`rotate_candidates`] live in this module: the production
/// implementation over GitHub connections, and the rotation tests over
/// scripted answers.
#[async_trait]
trait CandidateAttempt {
    async fn attempt(
        &self,
        candidate: &str,
    ) -> std::result::Result<RepositoryFetch, ReleaseFetchError>;
}

/// The production attempt: every GitHub connection of one candidate user.
struct CredentialAttempt<'a> {
    pool: &'a PgPool,
    http: &'a Client,
    encryption_key: &'a EncryptionKey,
    request: &'a ReleaseFetchRequest<'a>,
    sync_state: Option<&'a SyncStateRow>,
    credentials: &'a ReleaseCredentialCache,
}

#[async_trait]
impl CandidateAttempt for CredentialAttempt<'_> {
    async fn attempt(
        &self,
        candidate: &str,
    ) -> std::result::Result<RepositoryFetch, ReleaseFetchError> {
        fetch_for_user(
            self.pool,
            self.http,
            self.encryption_key,
            self.request,
            candidate,
            self.sync_state,
            self.credentials,
        )
        .await
    }
}

/// Try the candidate users in order, returning the first read that succeeds.
///
/// A failure classified as [`RefreshFailure::rotates_candidate`] means another
/// candidate may still read the repository, so the rotation continues; any
/// other failure describes the upstream call itself and stops it. When every
/// candidate fails, the classification carrying an HTTP status wins over a
/// purely local one, and the first attempt decides between two of equal rank.
async fn rotate_candidates(
    candidates: &[String],
    requested_by_user_id: &str,
    repo_full_name: &str,
    attempt: &impl CandidateAttempt,
) -> std::result::Result<(RepositoryFetch, FetchVia, String), ReleaseFetchError> {
    let mut first_failure: Option<ReleaseFetchError> = None;
    let mut upstream_failure: Option<ReleaseFetchError> = None;
    for candidate in candidates {
        match attempt.attempt(candidate.as_str()).await {
            Ok(fetch) => {
                let via = if candidate == requested_by_user_id {
                    FetchVia::User
                } else {
                    FetchVia::Candidate
                };
                if via == FetchVia::Candidate {
                    warn!(
                        event = "upstream.call",
                        operation = "repo_release_fetch",
                        repo = repo_full_name,
                        requested_by = requested_by_user_id,
                        candidate_user_id = candidate,
                        "reading repository releases through a rotated candidate user"
                    );
                }
                return Ok((fetch, via, candidate.clone()));
            }
            Err(error) => {
                let rotates = error.rotates_candidate();
                // A candidate without a usable connection says nothing about
                // the repository, so an actual upstream classification wins.
                if error.http_status().is_some() && upstream_failure.is_none() {
                    upstream_failure = Some(error.clone());
                }
                first_failure.get_or_insert(error);
                if !rotates {
                    // Not a credential or throttle problem: another candidate
                    // token would hit the same upstream condition. The
                    // anonymous read can still succeed, so stop rotating but
                    // keep the fallback.
                    break;
                }
            }
        }
    }
    Err(upstream_failure.or(first_failure).unwrap_or_else(|| {
        RefreshFailure::new(
            "credentials_missing",
            format!("no usable GitHub connection for {repo_full_name}"),
            false,
            None,
        )
        .into()
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
) -> std::result::Result<RepositoryFetch, ReleaseFetchError> {
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
        )
        .into());
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
                    .into()
                });
                continue;
            }
        };
        match fetch_with_token(http, request, Some(token.as_str()), sync_state, None).await {
            Ok(fetch) => return Ok(fetch),
            Err(ReleaseFetchError::Failed(failure)) if failure.rotates_candidate() => {
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
                last_failure = Some(failure.into());
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_failure.unwrap_or_else(|| {
        RefreshFailure::new(
            "credentials_missing",
            format!("no usable GitHub connection for user {user_id}"),
            false,
            None,
        )
        .into()
    }))
}

/// Unauthenticated read of a public repository.
///
/// Every page of it is charged to the process-wide anonymous budget before the
/// request leaves, and GitHub's rate-limit headers recalibrate that budget on
/// the way back.
async fn fetch_public(
    http: &Client,
    request: &ReleaseFetchRequest<'_>,
    sync_state: Option<&SyncStateRow>,
    budget: &AnonymousBudget,
) -> std::result::Result<RepositoryFetch, ReleaseFetchError> {
    fetch_with_token(http, request, None, sync_state, Some(budget)).await
}

async fn fetch_with_token(
    http: &Client,
    request: &ReleaseFetchRequest<'_>,
    token: Option<&str>,
    sync_state: Option<&SyncStateRow>,
    budget: Option<&AnonymousBudget>,
) -> std::result::Result<RepositoryFetch, ReleaseFetchError> {
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
            .get(url.clone())
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
        // Charge before sending: GitHub counts the request even when it answers
        // 304 or an error, and a redirect hop is another request on the wire.
        if let Some(budget) = budget {
            budget
                .acquire(unix_now())
                .map_err(|deferral| defer_anonymous_read(request, page, deferral))?;
        }
        let response = builder.send().await.map_err(classify_request_error)?;
        let status = response.status();
        let headers = response.headers().clone();
        if let Some(budget) = budget {
            // A renamed repository answers 301 and the shared client follows
            // it; that hop is another request from the same address. The client
            // hides how many hops it took, so charge the one that is certain and
            // let the rate-limit header below correct any residue.
            if response.url().as_str() != url {
                budget.charge_redirect();
            }
            let (remaining, reset_at_unix) = parse_rate_limit(&headers);
            budget.observe(remaining, reset_at_unix);
        }
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
            return Err(classify_http_error(status, &headers, &body).into());
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

/// Report a spent anonymous budget: the request was never sent.
fn defer_anonymous_read(
    request: &ReleaseFetchRequest<'_>,
    page: usize,
    deferral: BudgetDeferral,
) -> ReleaseFetchError {
    warn!(
        event = "upstream.call",
        operation = "repo_release_fetch",
        repo = request.repo_full_name,
        page,
        deferred_reason = "anonymous_budget",
        anonymous_budget_remaining = deferral.remaining,
        anonymous_budget_reset_at = deferral.reset_at_unix,
        "anonymous GitHub read deferred: budget exhausted"
    );
    if let Some(metrics) = crate::metrics::global() {
        metrics.record_anonymous_github_deferral();
    }
    ReleaseFetchError::Deferred(deferral)
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

    type AttemptResult = std::result::Result<RepositoryFetch, ReleaseFetchError>;

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
    fn credential_permission_and_throttle_classes_rotate() {
        for code in ROTATING_REASON_CODES {
            assert!(
                RefreshFailure::new(code, "synthetic", false, None).rotates_candidate(),
                "{code} must rotate"
            );
        }
        for code in [
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

    /// The failure GitHub produces for a token whose primary limit is spent.
    fn throttled_403() -> RefreshFailure {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        let failure = classify_http_error(StatusCode::FORBIDDEN, &headers, "");
        assert_eq!(failure.reason_code, "rate_limited");
        assert_eq!(failure.status, Some(403));
        failure
    }

    /// Rotation probe whose answers are scripted, so candidate order and the
    /// final classification are asserted without a database or network.
    struct ScriptedAttempt {
        calls: std::sync::Mutex<Vec<String>>,
        answer: fn(&str) -> AttemptResult,
    }

    impl ScriptedAttempt {
        fn new(answer: fn(&str) -> AttemptResult) -> Self {
            Self {
                calls: std::sync::Mutex::new(Vec::new()),
                answer,
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl CandidateAttempt for ScriptedAttempt {
        async fn attempt(&self, candidate: &str) -> AttemptResult {
            self.calls.lock().unwrap().push(candidate.to_owned());
            (self.answer)(candidate)
        }
    }

    fn readable() -> RepositoryFetch {
        RepositoryFetch {
            pages_fetched: 1,
            stopped_reason: "empty_page".to_owned(),
            ..RepositoryFetch::default()
        }
    }

    /// The classification of a rotation that ended in failure. A deferral is a
    /// different outcome, so the rotation tests must never see one.
    fn failure_of(error: ReleaseFetchError) -> RefreshFailure {
        match error {
            ReleaseFetchError::Failed(failure) => failure,
            ReleaseFetchError::Deferred(deferral) => {
                panic!("credential rotation reported a budget deferral: {deferral:?}")
            }
        }
    }

    fn throttled_first(candidate: &str) -> AttemptResult {
        if candidate == "user-a" {
            return Err(throttled_403().into());
        }
        Ok(readable())
    }

    fn always_throttled(_candidate: &str) -> AttemptResult {
        Err(throttled_403().into())
    }

    fn missing_then_throttled(candidate: &str) -> AttemptResult {
        if candidate == "user-a" {
            return Err(RefreshFailure::new(
                "credentials_missing",
                "no usable connection",
                false,
                None,
            )
            .into());
        }
        Err(throttled_403().into())
    }

    fn upstream_http_error(_candidate: &str) -> AttemptResult {
        Err(RefreshFailure::new(
            "github_http_error",
            "sync repo releases: github returned 500",
            false,
            Some(500),
        )
        .into())
    }

    #[tokio::test]
    async fn rate_limited_candidate_rotates_to_the_next_user() {
        let candidates = vec!["user-a".to_owned(), "user-b".to_owned()];
        let attempt = ScriptedAttempt::new(throttled_first);
        let (fetch, via, candidate_user_id) =
            rotate_candidates(&candidates, "user-a", "private/repo", &attempt)
                .await
                .expect("the second candidate reads the repository");
        assert_eq!(attempt.calls().as_slice(), ["user-a", "user-b"]);
        assert_eq!(via, FetchVia::Candidate);
        assert_eq!(candidate_user_id, "user-b");
        assert_eq!(fetch.pages_fetched, 1);
    }

    #[tokio::test]
    async fn exhausted_throttled_rotation_reports_rate_limited() {
        let candidates = vec!["user-a".to_owned(), "user-b".to_owned()];
        let attempt = ScriptedAttempt::new(always_throttled);
        let failure = failure_of(
            rotate_candidates(&candidates, "user-a", "private/repo", &attempt)
                .await
                .expect_err("every candidate is throttled"),
        );
        assert_eq!(attempt.calls().as_slice(), ["user-a", "user-b"]);
        assert_eq!(failure.reason_code, "rate_limited");
        assert_eq!(failure.status, Some(403));
    }

    #[tokio::test]
    async fn a_local_failure_does_not_hide_the_upstream_status() {
        let candidates = vec!["user-a".to_owned(), "user-b".to_owned()];
        let attempt = ScriptedAttempt::new(missing_then_throttled);
        let failure = failure_of(
            rotate_candidates(&candidates, "user-a", "private/repo", &attempt)
                .await
                .expect_err("no candidate reads the repository"),
        );
        assert_eq!(attempt.calls().as_slice(), ["user-a", "user-b"]);
        // `first_failure` is the status-less credentials_missing, but the
        // surfaced classification must stay the throttled upstream answer so
        // the worker keeps its retry backoff.
        assert_eq!(failure.reason_code, "rate_limited");
        assert_eq!(failure.status, Some(403));
        assert!(failure.retryable);
    }

    #[tokio::test]
    async fn non_rotating_failures_stop_the_rotation() {
        let candidates = vec!["user-a".to_owned(), "user-b".to_owned()];
        let attempt = ScriptedAttempt::new(upstream_http_error);
        let failure = failure_of(
            rotate_candidates(&candidates, "user-a", "private/repo", &attempt)
                .await
                .expect_err("the upstream call itself failed"),
        );
        assert_eq!(attempt.calls().as_slice(), ["user-a"]);
        assert_eq!(failure.reason_code, "github_http_error");
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
    fn anonymous_budget_defers_once_spent() {
        let budget = AnonymousBudget::with_limit(2);
        assert!(budget.acquire(1_000).is_ok());
        assert!(budget.acquire(1_000).is_ok());
        assert_eq!(budget.remaining(), 0);
        let deferral = budget
            .acquire(1_000)
            .expect_err("a spent budget must defer, not borrow");
        assert_eq!(deferral.remaining, 0);
        assert_eq!(
            deferral.summary(),
            "deferred_budget: anonymous GitHub read budget exhausted"
        );
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn anonymous_budget_refills_when_the_window_rolls_over() {
        let budget = AnonymousBudget::with_limit(1);
        budget.observe(None, Some(2_000));
        assert!(budget.acquire(1_999).is_ok());
        let deferral = budget
            .acquire(1_999)
            .expect_err("the hour has not rolled over");
        assert_eq!(deferral.reset_at_unix, Some(2_000));
        // The reset timestamp is inclusive: GitHub refills at that second.
        assert!(budget.acquire(2_000).is_ok());
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn anonymous_budget_refills_an_hour_after_a_headerless_spend() {
        let budget = AnonymousBudget::with_limit(1);
        assert!(budget.acquire(1_000).is_ok());
        assert!(budget.acquire(1_000).is_err());
        // No response ever named the reset instant: fall back to one window
        // after the last charge instead of wedging the budget forever.
        assert!(budget.acquire(1_000 + 3_599).is_err());
        assert!(budget.acquire(1_000 + 3_600).is_ok());
    }

    #[test]
    fn rate_limit_headers_lower_the_local_budget() {
        let budget = AnonymousBudget::with_limit(50);
        assert!(budget.acquire(1_000).is_ok());
        // Another client on the same egress IP spent 48 of the allowance.
        budget.observe(Some(2), Some(5_000));
        assert_eq!(budget.remaining(), 2);
        assert!(budget.acquire(1_000).is_ok());
        assert!(budget.acquire(1_000).is_ok());
        assert!(budget.acquire(1_000).is_err());
    }

    #[test]
    fn a_later_reset_window_replaces_the_stale_count() {
        let budget = AnonymousBudget::with_limit(50);
        budget.observe(Some(5), Some(5_000));
        assert_eq!(budget.remaining(), 5);
        // GitHub reports the next hour: the counter refilled, but never beyond
        // the local ceiling.
        budget.observe(Some(59), Some(8_600));
        assert_eq!(budget.remaining(), 50);
    }

    #[test]
    fn a_followed_redirect_is_charged_without_deferring() {
        let budget = AnonymousBudget::with_limit(2);
        assert!(budget.acquire(1_000).is_ok());
        budget.charge_redirect();
        assert_eq!(budget.remaining(), 0);
        // The hop was already on the wire: recording it must not overflow.
        budget.charge_redirect();
        assert_eq!(budget.remaining(), 0);
        assert!(budget.acquire(1_000).is_err());
    }

    #[test]
    fn rate_limit_headers_are_parsed_from_the_response() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_rate_limit(&headers), (None, None));
        headers.insert("x-ratelimit-remaining", "7".parse().unwrap());
        headers.insert("x-ratelimit-reset", "1764000000".parse().unwrap());
        assert_eq!(parse_rate_limit(&headers), (Some(7), Some(1_764_000_000)));
        headers.insert("x-ratelimit-remaining", "not-a-number".parse().unwrap());
        assert_eq!(parse_rate_limit(&headers), (None, Some(1_764_000_000)));
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
