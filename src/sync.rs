use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use reqwest::{
    Response,
    header::{ACCEPT, HeaderMap, USER_AGENT},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::Row;
use tokio::{fs::OpenOptions, io::AsyncWriteExt, sync::Mutex, task::JoinSet};

use crate::{jobs, local_id, runtime, state::AppState};

const REST_API_BASE: &str = "https://api.github.com";
const GRAPHQL_URL: &str = "https://api.github.com/graphql";
const API_VERSION: &str = "2022-11-28";
const SUBSCRIPTION_STAR_WORKERS: usize = 5;
const SUBSCRIPTION_SOCIAL_WORKERS: usize = 4;
const SUBSCRIPTION_NOTIFICATION_WORKERS: usize = 5;
const SUBSCRIPTION_RETRY_LIMIT: usize = 3;
const SUBSCRIPTION_RETRY_BACKOFF_MS: [u64; 3] = [500, 1_000, 2_000];
const SUBSCRIPTION_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const REPO_RELEASE_WORKERS: usize = 5;
const REPO_RELEASE_QUEUE_POLL_INTERVAL: Duration = Duration::from_millis(450);
const REPO_RELEASE_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(150);
const REPO_RELEASE_FRESHNESS_WINDOW: Duration = Duration::from_secs(30 * 60);
const SMART_PREHEAT_RECENT_RELEASE_LIMIT: usize = 30;
const SOCIAL_STARGAZER_FETCH_CONCURRENCY: usize = 4;
const REPO_RELEASE_PRIORITY_SYSTEM: i64 = 1;
const REPO_RELEASE_PRIORITY_INTERACTIVE: i64 = 2;
const GITHUB_WEB_BASE: &str = "https://github.com";
const GITHUB_NOTIFICATIONS_PAGE_SIZE: usize = 50;
const NOTIFICATIONS_SINCE_KEY: &str = "notifications_since";
const NOTIFICATION_OPEN_URL_REPAIR_KEY: &str = "notifications_open_url_repair_v2";
const NOTIFICATION_OPEN_URL_REPAIR_PENDING: &str = "pending";
const NOTIFICATION_OPEN_URL_REPAIR_BATCH_SIZE: usize = 100;

static REPO_RELEASE_CLAIM_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Debug, Serialize)]
pub struct SyncStarredResult {
    pub repos: usize,
}

#[derive(Debug, Serialize)]
pub struct SyncReleasesResult {
    pub repos: usize,
    pub releases: usize,
}

#[derive(Debug, Serialize)]
pub struct SyncAccessRefreshResult {
    pub starred: SyncStarredResult,
    pub release: SharedReleaseDemandResult,
    pub social: SyncSocialActivityResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub social_error: Option<String>,
    pub notifications: SyncNotificationsResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifications_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SyncNotificationsResult {
    pub notifications: usize,
    pub since: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct SyncSocialActivityResult {
    pub repo_stars: usize,
    pub followers: usize,
    pub events: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_repos: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_errors: Vec<String>,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct SyncSubscriptionStarSummary {
    pub total_users: usize,
    pub succeeded_users: usize,
    pub failed_users: usize,
    pub total_repos: usize,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct SyncSubscriptionReleaseSummary {
    pub total_repos: usize,
    pub succeeded_repos: usize,
    pub failed_repos: usize,
    pub candidate_failures: usize,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct SyncSubscriptionSocialSummary {
    pub total_users: usize,
    pub succeeded_users: usize,
    pub failed_users: usize,
    pub repo_stars: usize,
    pub followers: usize,
    pub events: usize,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct SyncSubscriptionNotificationsSummary {
    pub total_users: usize,
    pub succeeded_users: usize,
    pub failed_users: usize,
    pub notifications: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct SyncSubscriptionsResult {
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub star: SyncSubscriptionStarSummary,
    pub release: SyncSubscriptionReleaseSummary,
    pub social: SyncSubscriptionSocialSummary,
    pub notifications: SyncSubscriptionNotificationsSummary,
    pub releases_written: usize,
    pub critical_events: usize,
}

pub fn skipped_subscription_result(_schedule_key: &str, skip_reason: &str) -> Value {
    json!({
        "skipped": true,
        "skip_reason": skip_reason,
        "star": SyncSubscriptionStarSummary::default(),
        "release": SyncSubscriptionReleaseSummary::default(),
        "social": SyncSubscriptionSocialSummary::default(),
        "notifications": SyncSubscriptionNotificationsSummary::default(),
        "releases_written": 0,
        "critical_events": 0,
    })
}

fn repo_release_claim_lock() -> &'static tokio::sync::Mutex<()> {
    REPO_RELEASE_CLAIM_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub fn spawn_repo_release_workers(state: Arc<AppState>) {
    for _ in 0..REPO_RELEASE_WORKERS.max(1) {
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                match claim_next_repo_release_work_item(state.as_ref()).await {
                    Ok(Some(work_item)) => {
                        if let Err(err) =
                            process_repo_release_work_item(state.clone(), work_item).await
                        {
                            tracing::warn!(?err, "repo release worker: process work item failed");
                        }
                    }
                    Ok(None) => tokio::time::sleep(REPO_RELEASE_QUEUE_POLL_INTERVAL).await,
                    Err(err) => {
                        tracing::warn!(?err, "repo release worker: claim failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        });
    }
}

pub fn spawn_repo_release_recovery_worker(state: Arc<AppState>) -> tokio::task::AbortHandle {
    tokio::spawn(async move {
        loop {
            if let Err(err) = recover_repo_release_runtime_state(state.as_ref()).await {
                tracing::warn!(
                    ?err,
                    "repo release worker: recover stale runtime state failed"
                );
            }
            tokio::time::sleep(runtime::RUNTIME_LEASE_HEARTBEAT_INTERVAL).await;
        }
    })
    .abort_handle()
}

pub async fn recover_repo_release_runtime_state_on_startup(state: &AppState) -> Result<()> {
    recover_repo_release_runtime_state_with_mode(state, runtime::RuntimeRecoveryMode::Startup).await
}

#[derive(Debug, sqlx::FromRow)]
struct StarredRepoRow {
    repo_id: i64,
    full_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct EligibleUserRow {
    id: String,
    last_active_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepoReleaseOrigin {
    System,
    Interactive,
}

impl RepoReleaseOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Interactive => "interactive",
        }
    }

    fn priority(self) -> i64 {
        match self {
            Self::System => REPO_RELEASE_PRIORITY_SYSTEM,
            Self::Interactive => REPO_RELEASE_PRIORITY_INTERACTIVE,
        }
    }
}

#[derive(Debug, Clone)]
struct ReleaseDemandRepo {
    repo_id: i64,
    full_name: String,
    is_new_repo: bool,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct SharedReleaseDemandResult {
    pub repos: usize,
    pub releases: usize,
    pub reused_running: usize,
    pub reused_fresh: usize,
    pub queued: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct RepoReleaseWorkItemRow {
    id: String,
    repo_id: i64,
    repo_full_name: String,
    status: String,
    request_origin: String,
    priority: i64,
    has_new_repo_watchers: i64,
    deadline_at: String,
    last_success_at: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ReleaseCandidateUserRow {
    user_id: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct StaleRepoReleaseWorkRow {
    id: String,
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
struct StarredData {
    viewer: Viewer,
}

#[derive(Debug, Deserialize)]
struct OwnedRepoData {
    viewer: OwnedRepoViewer,
}

#[derive(Debug, Deserialize)]
struct Viewer {
    #[serde(rename = "starredRepositories")]
    starred_repositories: StarredRepositories,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnedRepoViewer {
    login: String,
    repositories: OwnedRepositories,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StarredRepositories {
    page_info: PageInfo,
    edges: Vec<StarredEdge>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnedRepositories {
    page_info: PageInfo,
    nodes: Vec<OwnedRepoNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StarredEdge {
    starred_at: String,
    node: RepoNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnedRepoNode {
    database_id: Option<i64>,
    name_with_owner: String,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: Option<bool>,
    owner: RepoOwner,
}

impl OwnedRepoNode {
    fn uses_custom_open_graph_image(&self) -> bool {
        self.uses_custom_open_graph_image.unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoNode {
    database_id: Option<i64>,
    name_with_owner: String,
    name: String,
    description: Option<String>,
    url: String,
    is_private: bool,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: Option<bool>,
    owner: RepoOwner,
}

impl RepoNode {
    fn uses_custom_open_graph_image(&self) -> bool {
        self.uses_custom_open_graph_image.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RepoOwner {
    login: String,
    #[serde(alias = "avatarUrl")]
    avatar_url: Option<String>,
}

#[derive(Debug, Clone)]
struct StarredRepoSnapshot {
    repo_id: i64,
    full_name: String,
    owner_login: String,
    name: String,
    description: Option<String>,
    html_url: String,
    stargazed_at: String,
    is_private: bool,
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubActor {
    id: i64,
    login: String,
    avatar_url: Option<String>,
    html_url: Option<String>,
}

#[derive(Debug, Clone)]
struct OwnedRepoSnapshot {
    repo_id: i64,
    full_name: String,
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubStargazer {
    starred_at: Option<String>,
    user: GitHubActor,
}

#[derive(Debug, Clone)]
struct RepoStargazerSnapshot {
    repo_id: i64,
    repo_full_name: String,
    actor: GitHubActor,
    starred_at: Option<String>,
}

#[derive(Debug, Clone)]
struct FollowerSnapshot {
    actor: GitHubActor,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct RepoStarCurrentMemberRow {
    actor_github_user_id: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct RepoStarCurrentMemberEventRow {
    actor_github_user_id: i64,
    actor_login: String,
    actor_avatar_url: Option<String>,
    actor_html_url: Option<String>,
    starred_at: Option<String>,
    created_at: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct OwnedRepoStarBaselineRow {
    repo_id: i64,
    members_snapshot_initialized: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct FollowerCurrentMemberRow {
    actor_github_user_id: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct FollowerCurrentMemberEventRow {
    actor_github_user_id: i64,
    actor_login: String,
    actor_avatar_url: Option<String>,
    actor_html_url: Option<String>,
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    id: i64,
    node_id: Option<String>,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    created_at: Option<String>,
    prerelease: bool,
    draft: bool,
    reactions: Option<GitHubReleaseReactions>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseReactions {
    #[serde(rename = "+1")]
    plus1: i64,
    laugh: i64,
    heart: i64,
    hooray: i64,
    rocket: i64,
    eyes: i64,
}

#[derive(Debug, Deserialize)]
struct GitHubNotification {
    id: String,
    unread: Option<bool>,
    reason: Option<String>,
    updated_at: Option<String>,
    url: Option<String>,
    subject: NotificationSubject,
    repository: NotificationRepo,
}

#[derive(Debug, Deserialize)]
struct NotificationSubject {
    title: Option<String>,
    #[serde(rename = "type")]
    subject_type: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotificationRepo {
    full_name: Option<String>,
}

#[derive(Debug, Clone)]
struct RelatedUserRef {
    user_id: String,
    last_active_at: Option<String>,
}

#[derive(Debug, Clone)]
struct AggregatedRepo {
    repo_id: i64,
    full_name: String,
    is_private: bool,
    related_users: Vec<RelatedUserRef>,
}

#[derive(Debug)]
struct StarPhaseSuccess {
    user_id: String,
    last_active_at: Option<String>,
    repos: Vec<StarredRepoSnapshot>,
}

#[derive(Clone)]
struct SubscriptionRunContext {
    state: Arc<AppState>,
    task_id: String,
    logger: Arc<SubscriptionRunLogger>,
    critical_events: Arc<AtomicUsize>,
}

impl SubscriptionRunContext {
    async fn new(state: &AppState, task_id: &str) -> Result<Self> {
        let state = Arc::new(state.clone());
        let logger = Arc::new(SubscriptionRunLogger::open(state.as_ref(), task_id).await?);
        Ok(Self {
            state,
            task_id: task_id.to_owned(),
            logger,
            critical_events: Arc::new(AtomicUsize::new(0)),
        })
    }

    async fn log(
        &self,
        level: &str,
        stage: &str,
        event_type: &str,
        message: impl Into<String>,
        payload: Value,
    ) -> Result<()> {
        let message = message.into();
        self.logger
            .write(level, stage, event_type, &self.task_id, &message, payload)
            .await
    }

    async fn key_event(
        &self,
        message: impl Into<String>,
        event: SubscriptionEventRecord<'_>,
    ) -> Result<()> {
        let counts_as_critical = subscription_event_counts_as_critical(event.severity);
        let message = message.into();
        let event_payload = merge_message_into_payload(event.payload, &message);
        self.logger
            .write(
                event.severity,
                event.stage,
                event.event_type,
                &self.task_id,
                &message,
                event_payload.clone(),
            )
            .await?;
        append_subscription_event(
            self.state.as_ref(),
            &self.task_id,
            SubscriptionEventRecord {
                stage: event.stage,
                event_type: event.event_type,
                severity: event.severity,
                recoverable: event.recoverable,
                attempt: event.attempt,
                user_id: event.user_id,
                repo_id: event.repo_id,
                repo_full_name: event.repo_full_name,
                payload: event_payload,
            },
        )
        .await?;
        if counts_as_critical {
            self.critical_events.fetch_add(1, AtomicOrdering::Relaxed);
        }
        Ok(())
    }

    async fn is_cancel_requested(&self) -> Result<bool> {
        let flag = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT cancel_requested
            FROM job_tasks
            WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind(self.task_id.as_str())
        .fetch_optional(&self.state.pool)
        .await
        .context("failed to query subscription cancel_requested")?;

        Ok(flag.unwrap_or(0) != 0)
    }
}

fn subscription_event_counts_as_critical(severity: &str) -> bool {
    severity == "error"
}

fn subscription_retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(
        SUBSCRIPTION_RETRY_BACKOFF_MS
            .get(attempt.saturating_sub(1))
            .copied()
            .unwrap_or(2_000),
    )
}

fn subscription_timeout_error(operation: &str) -> SyncRequestError {
    SyncRequestError::retryable(
        "timeout",
        format!(
            "{operation}: timed out after {}s",
            SUBSCRIPTION_HTTP_TIMEOUT.as_secs()
        ),
        None,
    )
}

async fn with_subscription_timeout<T, Fut>(
    operation: &str,
    future: Fut,
) -> Result<T, SyncRequestError>
where
    Fut: Future<Output = Result<T, SyncRequestError>>,
{
    tokio::time::timeout(SUBSCRIPTION_HTTP_TIMEOUT, future)
        .await
        .map_err(|_| subscription_timeout_error(operation))?
}

struct SubscriptionEventRecord<'a> {
    stage: &'a str,
    event_type: &'a str,
    severity: &'a str,
    recoverable: bool,
    attempt: usize,
    user_id: Option<&'a str>,
    repo_id: Option<i64>,
    repo_full_name: Option<&'a str>,
    payload: Value,
}

struct SubscriptionRunLogger {
    file: Mutex<tokio::fs::File>,
}

impl SubscriptionRunLogger {
    async fn open(state: &AppState, task_id: &str) -> Result<Self> {
        let path = jobs::load_task_log_path(state, task_id)
            .await?
            .ok_or_else(|| anyhow!("subscription sync log path missing"))?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("failed to open subscription log file {path}"))?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    async fn write(
        &self,
        level: &str,
        stage: &str,
        event_type: &str,
        task_id: &str,
        message: &str,
        payload: Value,
    ) -> Result<()> {
        let line = serde_json::to_vec(&json!({
            "at": chrono::Utc::now().to_rfc3339(),
            "level": level,
            "stage": stage,
            "event_type": event_type,
            "task_id": task_id,
            "message": message,
            "payload": payload,
        }))
        .context("serialize subscription log line")?;
        let mut file = self.file.lock().await;
        file.write_all(&line)
            .await
            .context("failed to write subscription log line")?;
        file.write_all(b"\n")
            .await
            .context("failed to write subscription log newline")?;
        file.flush()
            .await
            .context("failed to flush subscription log line")?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct SyncRequestError {
    reason_code: &'static str,
    message: String,
    retryable: bool,
    status: Option<u16>,
}

impl SyncRequestError {
    fn retryable(
        reason_code: &'static str,
        message: impl Into<String>,
        status: Option<StatusCode>,
    ) -> Self {
        Self {
            reason_code,
            message: message.into(),
            retryable: true,
            status: status.map(|value| value.as_u16()),
        }
    }

    fn non_retryable(
        reason_code: &'static str,
        message: impl Into<String>,
        status: Option<StatusCode>,
    ) -> Self {
        Self {
            reason_code,
            message: message.into(),
            retryable: false,
            status: status.map(|value| value.as_u16()),
        }
    }

    fn into_anyhow(self) -> anyhow::Error {
        anyhow!(self.message)
    }
}

pub async fn sync_starred(state: &AppState, user_id: &str) -> Result<SyncStarredResult> {
    let repos = fetch_starred_snapshot(state, user_id)
        .await
        .map_err(SyncRequestError::into_anyhow)?;
    replace_starred_repos(state, user_id, &repos).await?;
    Ok(SyncStarredResult { repos: repos.len() })
}

pub async fn sync_releases(state: &AppState, user_id: &str) -> Result<SyncReleasesResult> {
    let before_release_ids = load_release_ids_for_user(state, user_id).await?;
    let demand = attach_and_wait_for_user_release_demand(
        state,
        None,
        user_id,
        RepoReleaseOrigin::Interactive,
        "manual_release_sync",
    )
    .await?;

    let after_release_ids = load_release_ids_for_user(state, user_id).await?;
    let mut new_release_ids = after_release_ids
        .difference(&before_release_ids)
        .copied()
        .collect::<Vec<_>>();
    new_release_ids.sort_unstable_by(|left, right| right.cmp(left));
    let smart_preheat_release_ids = merge_smart_preheat_release_ids(
        &new_release_ids,
        &load_recent_release_ids_for_user(state, user_id, SMART_PREHEAT_RECENT_RELEASE_LIMIT)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!(
                    ?err,
                    user_id,
                    "sync.releases: load recent release ids for smart preheat failed"
                );
                Vec::new()
            }),
    );
    if let Err(err) = enqueue_background_release_translation_task(
        state,
        user_id,
        &new_release_ids,
        "sync.releases.auto_translate",
        None,
        Some(user_id),
    )
    .await
    {
        tracing::warn!(
            ?err,
            user_id,
            "sync.releases: enqueue background translation failed"
        );
    }
    if let Err(err) = enqueue_background_release_smart_task(
        state,
        user_id,
        &smart_preheat_release_ids,
        "sync.releases.auto_smart",
        None,
        Some(user_id),
    )
    .await
    {
        tracing::warn!(
            ?err,
            user_id,
            "sync.releases: enqueue background smart summary failed"
        );
    }

    Ok(SyncReleasesResult {
        repos: demand.repos,
        releases: demand.releases,
    })
}

pub async fn sync_social_activity(
    state: &AppState,
    user_id: &str,
) -> Result<SyncSocialActivityResult> {
    let token = load_access_token_or_classified(state, user_id)
        .await
        .map_err(SyncRequestError::into_anyhow)?;

    let mut source_errors = Vec::new();
    let owned_repos = match fetch_owned_repo_snapshot(state, &token).await {
        Ok(repos) => Some(repos),
        Err(err) => {
            tracing::warn!(
                ?err,
                user_id,
                "sync social activity: skip owned repo snapshot"
            );
            source_errors.push(format!("owned_repos({}): {}", err.reason_code, err.message));
            None
        }
    };
    let followers = match fetch_followers_snapshot(state, &token).await {
        Ok(followers) => Some(followers),
        Err(err) => {
            tracing::warn!(
                ?err,
                user_id,
                "sync social activity: skip followers snapshot"
            );
            source_errors.push(format!("followers({}): {}", err.reason_code, err.message));
            None
        }
    };
    let repo_collection = if let Some(owned_repos) = owned_repos.as_deref() {
        collect_repo_stargazer_snapshots(state, &token, owned_repos).await
    } else {
        RepoStargazerCollectionResult::default()
    };

    let events = match (owned_repos.as_deref(), followers.as_deref()) {
        (Some(owned_repos), Some(followers)) => {
            apply_social_activity_snapshot(
                state,
                user_id,
                owned_repos,
                repo_collection.repo_members.as_slice(),
                followers,
            )
            .await?
        }
        _ => {
            apply_social_activity_snapshot_partial(
                state,
                user_id,
                owned_repos.as_deref(),
                owned_repos
                    .as_ref()
                    .map(|_| repo_collection.repo_members.as_slice()),
                followers.as_deref(),
            )
            .await?
        }
    };

    Ok(SyncSocialActivityResult {
        repo_stars: repo_collection.repo_stars,
        followers: followers.as_ref().map_or(0, Vec::len),
        events,
        failed_repos: repo_collection.failed_repos,
        source_errors,
    })
}

pub async fn sync_social_activity_best_effort(
    state: &AppState,
    user_id: &str,
    context: &'static str,
) -> (SyncSocialActivityResult, Option<String>) {
    match sync_social_activity(state, user_id).await {
        Ok(result) => (result, None),
        Err(err) => {
            tracing::warn!(
                ?err,
                user_id,
                context,
                "social activity sync failed, continuing"
            );
            (SyncSocialActivityResult::default(), Some(err.to_string()))
        }
    }
}

async fn collect_repo_stargazer_snapshots(
    state: &AppState,
    access_token: &str,
    repos: &[OwnedRepoSnapshot],
) -> RepoStargazerCollectionResult {
    collect_repo_stargazer_snapshots_with(
        state,
        access_token,
        repos,
        |state, access_token, repo| {
            Box::pin(async move {
                fetch_repo_stargazers_snapshot(&state, access_token.as_str(), &repo).await
            })
        },
    )
    .await
}

type RepoStargazerFetchFuture =
    Pin<Box<dyn Future<Output = Result<Vec<RepoStargazerSnapshot>, anyhow::Error>> + Send>>;

#[derive(Debug, Default)]
struct RepoStargazerCollectionResult {
    repo_stars: usize,
    repo_members: Vec<(OwnedRepoSnapshot, Vec<RepoStargazerSnapshot>)>,
    failed_repos: Vec<String>,
}

async fn collect_repo_stargazer_snapshots_with<F>(
    state: &AppState,
    access_token: &str,
    repos: &[OwnedRepoSnapshot],
    mut fetcher: F,
) -> RepoStargazerCollectionResult
where
    F: FnMut(AppState, String, OwnedRepoSnapshot) -> RepoStargazerFetchFuture,
{
    let mut repo_stars = 0usize;
    let mut repo_members = Vec::with_capacity(repos.len());
    let mut failed_repos = Vec::new();
    let mut join_set = JoinSet::new();
    let mut pending = repos.iter().cloned().enumerate();
    let concurrency = SOCIAL_STARGAZER_FETCH_CONCURRENCY.max(1);

    loop {
        while join_set.len() < concurrency {
            let Some((index, repo)) = pending.next() else {
                break;
            };
            let repo_for_task = repo.clone();
            let future = fetcher(state.clone(), access_token.to_owned(), repo);
            join_set.spawn(async move { (index, repo_for_task, future.await) });
        }

        let Some(joined) = join_set.join_next().await else {
            break;
        };

        match joined {
            Ok((index, repo, Ok(members))) => {
                repo_stars += members.len();
                repo_members.push((index, repo, members));
            }
            Ok((_, repo, Err(err))) => {
                tracing::warn!(
                    ?err,
                    repo = repo.full_name.as_str(),
                    "sync social activity: skip repo stargazers snapshot"
                );
                failed_repos.push(repo.full_name);
            }
            Err(err) => {
                tracing::warn!(
                    ?err,
                    "sync social activity: repo stargazers task join failed"
                );
            }
        }
    }

    repo_members.sort_by_key(|(index, _, _)| *index);

    RepoStargazerCollectionResult {
        repo_stars,
        repo_members: repo_members
            .into_iter()
            .map(|(_, repo, members)| (repo, members))
            .collect(),
        failed_repos,
    }
}

pub async fn sync_access_refresh(
    state: &AppState,
    task_id: &str,
    user_id: &str,
) -> Result<SyncAccessRefreshResult> {
    let before_repos = load_user_starred_repo_rows(state, user_id).await?;
    let before_repo_ids = before_repos
        .iter()
        .map(|repo| repo.repo_id)
        .collect::<HashSet<_>>();
    let starred = sync_starred(state, user_id).await?;
    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "star_refreshed",
            "repos": starred.repos,
        }),
    )
    .await?;

    let release = attach_and_wait_for_user_release_demand(
        state,
        Some((task_id, before_repo_ids)),
        user_id,
        RepoReleaseOrigin::Interactive,
        "access_refresh",
    )
    .await?;

    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "release_summary",
            "repos": release.repos,
            "releases": release.releases,
            "queued": release.queued,
            "reused_running": release.reused_running,
            "reused_fresh": release.reused_fresh,
            "failed": release.failed,
        }),
    )
    .await?;

    let (social, social_error) =
        sync_social_activity_best_effort(state, user_id, "sync.access_refresh").await;
    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "social_summary",
            "repo_stars": social.repo_stars,
            "followers": social.followers,
            "events": social.events,
            "failed_repos": social.failed_repos,
            "error": social_error,
        }),
    )
    .await?;

    let (notifications, notifications_error) = match sync_notifications(state, user_id).await {
        Ok(result) => (result, None),
        Err(err) => {
            tracing::warn!(
                ?err,
                user_id,
                "sync.access_refresh: notifications sync failed, completing with partial data"
            );
            (
                SyncNotificationsResult {
                    notifications: 0,
                    since: None,
                },
                Some(err.to_string()),
            )
        }
    };
    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "notifications_summary",
            "notifications": notifications.notifications,
            "since": notifications.since,
            "error": notifications_error,
        }),
    )
    .await?;

    Ok(SyncAccessRefreshResult {
        starred,
        release,
        social,
        social_error,
        notifications,
        notifications_error,
    })
}

#[derive(Debug, Default)]
struct AttachReleaseDemandResult {
    work_item_ids: Vec<String>,
    repos: usize,
    reused_running: usize,
    reused_fresh: usize,
    queued: usize,
}

#[derive(Debug, Default)]
struct WaitReleaseDemandResult {
    releases: usize,
    failed: usize,
    candidate_failures: usize,
}

struct RepoReleaseWatcherUpsert<'a> {
    work_item_id: &'a str,
    task_id: &'a str,
    user_id: Option<&'a str>,
    origin: RepoReleaseOrigin,
    reason: &'a str,
    is_new_repo: bool,
    status: &'a str,
    error_text: Option<&'a str>,
    now_rfc3339: &'a str,
}

async fn load_user_starred_repo_rows(
    state: &AppState,
    user_id: &str,
) -> Result<Vec<StarredRepoRow>> {
    sqlx::query_as::<_, StarredRepoRow>(
        r#"
        SELECT repo_id, full_name
        FROM starred_repos
        WHERE user_id = ?
        ORDER BY
          CASE WHEN stargazed_at IS NULL THEN 1 ELSE 0 END ASC,
          stargazed_at DESC,
          full_name ASC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .context("failed to query starred repos")
}

async fn apply_social_activity_snapshot(
    state: &AppState,
    user_id: &str,
    owned_repos: &[OwnedRepoSnapshot],
    repo_members: &[(OwnedRepoSnapshot, Vec<RepoStargazerSnapshot>)],
    followers: &[FollowerSnapshot],
) -> Result<usize> {
    apply_social_activity_snapshot_partial(
        state,
        user_id,
        Some(owned_repos),
        Some(repo_members),
        Some(followers),
    )
    .await
}

async fn apply_social_activity_snapshot_partial(
    state: &AppState,
    user_id: &str,
    owned_repos: Option<&[OwnedRepoSnapshot]>,
    repo_members: Option<&[(OwnedRepoSnapshot, Vec<RepoStargazerSnapshot>)]>,
    followers: Option<&[FollowerSnapshot]>,
) -> Result<usize> {
    debug_assert_eq!(owned_repos.is_some(), repo_members.is_some());

    let now = Utc::now().to_rfc3339();
    let mut tx = state
        .pool
        .begin()
        .await
        .context("begin social activity snapshot tx")?;
    let mut events_written = 0usize;

    let follower_baseline_exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM follower_sync_baselines
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .context("query follower sync baseline")?
        > 0;
    let repo_tracking_baseline_exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM repo_star_sync_baselines
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .context("query repo star sync baseline")?
        > 0;
    let repo_baselines = sqlx::query_as::<_, OwnedRepoStarBaselineRow>(
        r#"
        SELECT repo_id, members_snapshot_initialized
        FROM owned_repo_star_baselines
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await
    .context("query repo star baselines")?;
    let repo_tracking_initialized = repo_tracking_baseline_exists;
    let repo_snapshot_initialized_ids = repo_baselines
        .iter()
        .filter(|row| row.members_snapshot_initialized != 0)
        .map(|row| row.repo_id)
        .collect::<HashSet<_>>();
    let known_repo_ids = repo_baselines
        .iter()
        .map(|row| row.repo_id)
        .collect::<HashSet<_>>();
    let newly_discovered_repo_ids = owned_repos
        .unwrap_or(&[])
        .iter()
        .filter(|repo| !known_repo_ids.contains(&repo.repo_id))
        .map(|repo| repo.repo_id)
        .collect::<HashSet<_>>();
    let successful_repo_snapshot_ids = repo_members
        .unwrap_or(&[])
        .iter()
        .map(|(repo, _)| repo.repo_id)
        .collect::<HashSet<_>>();

    if let Some(followers) = followers {
        let current_follower_rows = sqlx::query_as::<_, FollowerCurrentMemberRow>(
            r#"
            SELECT actor_github_user_id
            FROM follower_current_members
            WHERE user_id = ?
            "#,
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await
        .context("query follower current members")?;
        let current_follower_ids = current_follower_rows
            .into_iter()
            .map(|row| row.actor_github_user_id)
            .collect::<HashSet<_>>();
        let next_follower_ids = followers
            .iter()
            .map(|item| item.actor.id)
            .collect::<HashSet<_>>();

        for follower in followers {
            if !current_follower_ids.contains(&follower.actor.id) {
                let inserted = insert_social_activity_event_tx(
                    &mut tx,
                    SocialActivityEventInsert {
                        user_id,
                        kind: "follower_received",
                        repo_id: None,
                        repo_full_name: None,
                        repo_visual: None,
                        actor: &follower.actor,
                        occurred_at: now.as_str(),
                        detected_at: now.as_str(),
                    },
                )
                .await?;
                if inserted {
                    events_written += 1;
                }
            }

            upsert_follower_current_member_tx(&mut tx, user_id, follower, now.as_str()).await?;
        }

        for current_id in current_follower_ids.difference(&next_follower_ids) {
            sqlx::query(
                r#"
                DELETE FROM follower_current_members
                WHERE user_id = ? AND actor_github_user_id = ?
                "#,
            )
            .bind(user_id)
            .bind(*current_id)
            .execute(&mut *tx)
            .await
            .context("delete stale follower current member")?;
        }

        if !follower_baseline_exists {
            sqlx::query(
                r#"
                INSERT INTO follower_sync_baselines (id, user_id, initialized_at, updated_at)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE
                SET updated_at = excluded.updated_at
                "#,
            )
            .bind(local_id::generate_local_id())
            .bind(user_id)
            .bind(now.as_str())
            .bind(now.as_str())
            .execute(&mut *tx)
            .await
            .context("upsert follower sync baseline")?;
        }

        events_written +=
            materialize_follower_current_members_tx(&mut tx, user_id, now.as_str()).await?;
    }

    if let Some(owned_repos) = owned_repos {
        let current_owned_repo_ids = owned_repos
            .iter()
            .map(|repo| repo.repo_id)
            .collect::<HashSet<_>>();

        if current_owned_repo_ids.is_empty() {
            sqlx::query(
                r#"
                DELETE FROM repo_star_current_members
                WHERE user_id = ?
                "#,
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .context("delete repo star current members for no-longer-owned repos")?;

            sqlx::query(
                r#"
                DELETE FROM owned_repo_star_baselines
                WHERE user_id = ?
                "#,
            )
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .context("delete repo star baselines for no-longer-owned repos")?;
        } else {
            let mut delete_members = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                r#"
                    DELETE FROM repo_star_current_members
                    WHERE user_id = 
                    "#,
            );
            delete_members.push_bind(user_id);
            delete_members.push(" AND repo_id NOT IN (");
            {
                let mut separated = delete_members.separated(", ");
                for repo_id in &current_owned_repo_ids {
                    separated.push_bind(repo_id);
                }
            }
            delete_members.push(")");
            delete_members
                .build()
                .execute(&mut *tx)
                .await
                .context("delete stale repo star current members")?;

            let mut delete_baselines = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                r#"
                    DELETE FROM owned_repo_star_baselines
                    WHERE user_id = 
                    "#,
            );
            delete_baselines.push_bind(user_id);
            delete_baselines.push(" AND repo_id NOT IN (");
            {
                let mut separated = delete_baselines.separated(", ");
                for repo_id in &current_owned_repo_ids {
                    separated.push_bind(repo_id);
                }
            }
            delete_baselines.push(")");
            delete_baselines
                .build()
                .execute(&mut *tx)
                .await
                .context("delete stale repo star baselines")?;
        }

        for repo in owned_repos {
            let was_known_repo = known_repo_ids.contains(&repo.repo_id);
            let fetched_snapshot_this_run = successful_repo_snapshot_ids.contains(&repo.repo_id);
            let should_persist_baseline =
                !repo_tracking_initialized || was_known_repo || fetched_snapshot_this_run;

            if !should_persist_baseline {
                continue;
            }

            let snapshot_initialized =
                repo_snapshot_initialized_ids.contains(&repo.repo_id) || fetched_snapshot_this_run;
            upsert_owned_repo_star_baseline_tx(
                &mut tx,
                user_id,
                repo,
                snapshot_initialized,
                now.as_str(),
            )
            .await
            .with_context(|| format!("upsert repo star baseline for {}", repo.full_name))?;
        }

        for (repo, members) in repo_members.unwrap_or(&[]) {
            let repo_snapshot_initialized = repo_snapshot_initialized_ids.contains(&repo.repo_id);
            let repo_waiting_for_first_success =
                known_repo_ids.contains(&repo.repo_id) && !repo_snapshot_initialized;
            let emit_current_members = repo_snapshot_initialized
                || !repo_tracking_initialized
                || newly_discovered_repo_ids.contains(&repo.repo_id)
                || repo_waiting_for_first_success;
            let current_rows = sqlx::query_as::<_, RepoStarCurrentMemberRow>(
                r#"
                SELECT actor_github_user_id
                FROM repo_star_current_members
                WHERE user_id = ? AND repo_id = ?
                "#,
            )
            .bind(user_id)
            .bind(repo.repo_id)
            .fetch_all(&mut *tx)
            .await
            .with_context(|| format!("query repo star members for {}", repo.full_name))?;
            let current_ids = current_rows
                .into_iter()
                .map(|row| row.actor_github_user_id)
                .collect::<HashSet<_>>();
            let next_ids = members
                .iter()
                .map(|item| item.actor.id)
                .collect::<HashSet<_>>();

            for member in members {
                if emit_current_members && !current_ids.contains(&member.actor.id) {
                    let occurred_at = member.starred_at.as_deref().unwrap_or(now.as_str());
                    let inserted = insert_social_activity_event_tx(
                        &mut tx,
                        SocialActivityEventInsert {
                            user_id,
                            kind: "repo_star_received",
                            repo_id: Some(repo.repo_id),
                            repo_full_name: Some(repo.full_name.as_str()),
                            repo_visual: Some(repo),
                            actor: &member.actor,
                            occurred_at,
                            detected_at: now.as_str(),
                        },
                    )
                    .await?;
                    if inserted {
                        events_written += 1;
                    }
                }

                upsert_repo_star_current_member_tx(&mut tx, user_id, member, now.as_str()).await?;
            }

            for current_id in current_ids.difference(&next_ids) {
                sqlx::query(
                    r#"
                    DELETE FROM repo_star_current_members
                    WHERE user_id = ? AND repo_id = ? AND actor_github_user_id = ?
                    "#,
                )
                .bind(user_id)
                .bind(repo.repo_id)
                .bind(*current_id)
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!(
                        "delete stale repo star current member for {}",
                        repo.full_name
                    )
                })?;
            }

            events_written +=
                materialize_repo_star_current_members_tx(&mut tx, user_id, repo, now.as_str())
                    .await?;

            upsert_owned_repo_star_baseline_tx(&mut tx, user_id, repo, true, now.as_str())
                .await
                .with_context(|| {
                    format!("mark repo star snapshot initialized for {}", repo.full_name)
                })?;
        }

        sqlx::query(
            r#"
            INSERT INTO repo_star_sync_baselines (id, user_id, initialized_at, updated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE
            SET updated_at = excluded.updated_at
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&mut *tx)
        .await
        .context("upsert repo star sync baseline")?;
    }

    tx.commit()
        .await
        .context("commit social activity snapshot tx")?;
    Ok(events_written)
}

async fn upsert_owned_repo_star_baseline_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    repo: &OwnedRepoSnapshot,
    members_snapshot_initialized: bool,
    now: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO owned_repo_star_baselines (
          id,
          user_id,
          repo_id,
          repo_full_name,
          owner_avatar_url,
          open_graph_image_url,
          uses_custom_open_graph_image,
          members_snapshot_initialized,
          initialized_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, repo_id) DO UPDATE
        SET repo_full_name = excluded.repo_full_name,
            owner_avatar_url = excluded.owner_avatar_url,
            open_graph_image_url = excluded.open_graph_image_url,
            uses_custom_open_graph_image = excluded.uses_custom_open_graph_image,
            members_snapshot_initialized = excluded.members_snapshot_initialized,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(repo.repo_id)
    .bind(repo.full_name.as_str())
    .bind(repo.owner_avatar_url.as_deref())
    .bind(repo.open_graph_image_url.as_deref())
    .bind(repo.uses_custom_open_graph_image as i64)
    .bind(if members_snapshot_initialized {
        1_i64
    } else {
        0_i64
    })
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .context("upsert owned repo star baseline")?;

    Ok(())
}

struct SocialActivityEventInsert<'a> {
    user_id: &'a str,
    kind: &'a str,
    repo_id: Option<i64>,
    repo_full_name: Option<&'a str>,
    repo_visual: Option<&'a OwnedRepoSnapshot>,
    actor: &'a GitHubActor,
    occurred_at: &'a str,
    detected_at: &'a str,
}

async fn insert_social_activity_event_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event: SocialActivityEventInsert<'_>,
) -> Result<bool> {
    let repo_owner_avatar_url = event
        .repo_visual
        .and_then(|repo| repo.owner_avatar_url.as_deref());
    let repo_open_graph_image_url = event
        .repo_visual
        .and_then(|repo| repo.open_graph_image_url.as_deref());
    let repo_uses_custom_open_graph_image = event.repo_visual.map(|repo| {
        if repo.uses_custom_open_graph_image {
            1_i64
        } else {
            0_i64
        }
    });
    let result = sqlx::query(
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
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(event.user_id)
    .bind(event.kind)
    .bind(event.repo_id)
    .bind(event.repo_full_name)
    .bind(repo_owner_avatar_url)
    .bind(repo_open_graph_image_url)
    .bind(repo_uses_custom_open_graph_image)
    .bind(event.actor.id)
    .bind(event.actor.login.as_str())
    .bind(event.actor.avatar_url.as_deref())
    .bind(event.actor.html_url.as_deref())
    .bind(event.occurred_at)
    .bind(event.detected_at)
    .bind(event.detected_at)
    .bind(event.detected_at)
    .execute(&mut **tx)
    .await
    .context("insert social activity event")?;
    Ok(result.rows_affected() > 0)
}

async fn upsert_follower_current_member_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    follower: &FollowerSnapshot,
    now: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO follower_current_members (
          id,
          user_id,
          actor_github_user_id,
          actor_login,
          actor_avatar_url,
          actor_html_url,
          created_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, actor_github_user_id) DO UPDATE
        SET actor_login = excluded.actor_login,
            actor_avatar_url = excluded.actor_avatar_url,
            actor_html_url = excluded.actor_html_url,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(follower.actor.id)
    .bind(follower.actor.login.as_str())
    .bind(follower.actor.avatar_url.as_deref())
    .bind(follower.actor.html_url.as_deref())
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .context("upsert follower current member")?;
    Ok(())
}

async fn upsert_repo_star_current_member_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    member: &RepoStargazerSnapshot,
    now: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO repo_star_current_members (
          id,
          user_id,
          repo_id,
          repo_full_name,
          actor_github_user_id,
          actor_login,
          actor_avatar_url,
          actor_html_url,
          starred_at,
          created_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, repo_id, actor_github_user_id) DO UPDATE
        SET repo_full_name = excluded.repo_full_name,
            actor_login = excluded.actor_login,
            actor_avatar_url = excluded.actor_avatar_url,
            actor_html_url = excluded.actor_html_url,
            starred_at = excluded.starred_at,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(member.repo_id)
    .bind(member.repo_full_name.as_str())
    .bind(member.actor.id)
    .bind(member.actor.login.as_str())
    .bind(member.actor.avatar_url.as_deref())
    .bind(member.actor.html_url.as_deref())
    .bind(member.starred_at.as_deref())
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .context("upsert repo star current member")?;
    Ok(())
}

async fn materialize_follower_current_members_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    now: &str,
) -> Result<usize> {
    let rows = sqlx::query_as::<_, FollowerCurrentMemberEventRow>(
        r#"
        SELECT
          cm.actor_github_user_id,
          cm.actor_login,
          cm.actor_avatar_url,
          cm.actor_html_url,
          cm.created_at
        FROM follower_current_members cm
        LEFT JOIN social_activity_events e
          ON e.user_id = cm.user_id
         AND e.kind = 'follower_received'
         AND e.repo_id IS NULL
         AND e.actor_github_user_id = cm.actor_github_user_id
         AND e.occurred_at = COALESCE(cm.created_at, ?)
        WHERE cm.user_id = ?
          AND e.id IS NULL
        ORDER BY cm.actor_github_user_id ASC
        "#,
    )
    .bind(now)
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await
    .context("query follower members for social history materialization")?;
    let mut inserted = 0usize;

    for row in rows {
        let actor = GitHubActor {
            id: row.actor_github_user_id,
            login: row.actor_login,
            avatar_url: row.actor_avatar_url,
            html_url: row.actor_html_url,
        };
        let occurred_at = row.created_at.as_deref().unwrap_or(now);
        if insert_social_activity_event_tx(
            tx,
            SocialActivityEventInsert {
                user_id,
                kind: "follower_received",
                repo_id: None,
                repo_full_name: None,
                repo_visual: None,
                actor: &actor,
                occurred_at,
                detected_at: now,
            },
        )
        .await?
        {
            inserted += 1;
        }
    }

    Ok(inserted)
}

async fn materialize_repo_star_current_members_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    repo: &OwnedRepoSnapshot,
    now: &str,
) -> Result<usize> {
    let rows = sqlx::query_as::<_, RepoStarCurrentMemberEventRow>(
        r#"
        SELECT
          cm.actor_github_user_id,
          cm.actor_login,
          cm.actor_avatar_url,
          cm.actor_html_url,
          cm.starred_at,
          cm.created_at
        FROM repo_star_current_members cm
        LEFT JOIN social_activity_events e
          ON e.user_id = cm.user_id
         AND e.kind = 'repo_star_received'
         AND e.repo_id = cm.repo_id
         AND e.actor_github_user_id = cm.actor_github_user_id
         AND e.occurred_at = COALESCE(cm.starred_at, cm.created_at, ?)
        WHERE cm.user_id = ?
          AND cm.repo_id = ?
          AND e.id IS NULL
        ORDER BY cm.actor_github_user_id ASC
        "#,
    )
    .bind(now)
    .bind(user_id)
    .bind(repo.repo_id)
    .fetch_all(&mut **tx)
    .await
    .with_context(|| {
        format!(
            "query repo star members for social history materialization for {}",
            repo.full_name
        )
    })?;
    let mut inserted = 0usize;

    for row in rows {
        let actor = GitHubActor {
            id: row.actor_github_user_id,
            login: row.actor_login,
            avatar_url: row.actor_avatar_url,
            html_url: row.actor_html_url,
        };
        let occurred_at = row
            .starred_at
            .as_deref()
            .or(row.created_at.as_deref())
            .unwrap_or(now);
        if insert_social_activity_event_tx(
            tx,
            SocialActivityEventInsert {
                user_id,
                kind: "repo_star_received",
                repo_id: Some(repo.repo_id),
                repo_full_name: Some(repo.full_name.as_str()),
                repo_visual: Some(repo),
                actor: &actor,
                occurred_at,
                detected_at: now,
            },
        )
        .await?
        {
            inserted += 1;
        }
    }

    Ok(inserted)
}

async fn load_release_ids_for_user(state: &AppState, user_id: &str) -> Result<HashSet<i64>> {
    let rows = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT DISTINCT r.release_id
        FROM repo_releases r
        JOIN starred_repos sr
          ON sr.user_id = ? AND sr.repo_id = r.repo_id
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .context("failed to query release ids for user")?;
    Ok(rows.into_iter().collect())
}

async fn load_release_ids_for_repo_ids(state: &AppState, repo_ids: &[i64]) -> Result<HashSet<i64>> {
    if repo_ids.is_empty() {
        return Ok(HashSet::new());
    }

    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"
        SELECT DISTINCT release_id
        FROM repo_releases
        WHERE repo_id IN (
        "#,
    );
    {
        let mut separated = query.separated(", ");
        for repo_id in repo_ids {
            separated.push_bind(repo_id);
        }
    }
    query.push(")");

    let rows = query
        .build_query_scalar::<i64>()
        .fetch_all(&state.pool)
        .await
        .context("failed to query release ids for repo ids")?;
    Ok(rows.into_iter().collect())
}

async fn load_user_relevant_release_ids(
    state: &AppState,
    user_id: &str,
    release_ids: &[i64],
) -> Result<Vec<i64>> {
    if release_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"
        SELECT DISTINCT r.release_id
        FROM repo_releases r
        JOIN starred_repos sr
          ON sr.user_id = 
        "#,
    );
    query.push_bind(user_id);
    query.push(" AND sr.repo_id = r.repo_id WHERE r.release_id IN (");
    {
        let mut separated = query.separated(", ");
        for release_id in release_ids {
            separated.push_bind(release_id);
        }
    }
    query.push(") ORDER BY r.release_id DESC");

    query
        .build_query_scalar::<i64>()
        .fetch_all(&state.pool)
        .await
        .context("failed to query relevant release ids for user")
}

async fn load_recent_release_ids_for_user(
    state: &AppState,
    user_id: &str,
    limit: usize,
) -> Result<Vec<i64>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT DISTINCT r.release_id
        FROM repo_releases r
        JOIN starred_repos sr
          ON sr.user_id = ? AND sr.repo_id = r.repo_id
        ORDER BY COALESCE(r.published_at, r.created_at, r.updated_at) DESC, r.release_id DESC
        LIMIT ?
        "#,
    )
    .bind(user_id)
    .bind(limit as i64)
    .fetch_all(&state.pool)
    .await
    .context("failed to query recent release ids for user")
}

fn merge_smart_preheat_release_ids(
    new_release_ids: &[i64],
    recent_release_ids: &[i64],
) -> Vec<i64> {
    let mut seen = HashSet::new();
    let mut merged = Vec::with_capacity(
        new_release_ids.len()
            + recent_release_ids
                .len()
                .min(SMART_PREHEAT_RECENT_RELEASE_LIMIT),
    );

    for release_id in new_release_ids {
        if seen.insert(*release_id) {
            merged.push(*release_id);
        }
    }

    let mut recent_added = 0usize;
    for release_id in recent_release_ids {
        if recent_added >= SMART_PREHEAT_RECENT_RELEASE_LIMIT {
            break;
        }
        if seen.insert(*release_id) {
            merged.push(*release_id);
            recent_added += 1;
        }
    }

    merged
}

async fn enqueue_background_release_translation_task(
    state: &AppState,
    user_id: &str,
    release_ids: &[i64],
    source: &str,
    parent_task_id: Option<&str>,
    requested_by: Option<&str>,
) -> Result<Option<String>> {
    if release_ids.is_empty() || state.config.ai.is_none() {
        return Ok(None);
    }

    let task = jobs::enqueue_task(
        state,
        jobs::NewTask {
            task_type: jobs::TASK_TRANSLATE_RELEASE_BATCH.to_owned(),
            payload: json!({
                "user_id": user_id,
                "release_ids": release_ids,
            }),
            source: source.to_owned(),
            requested_by: requested_by.map(str::to_owned),
            parent_task_id: parent_task_id.map(str::to_owned),
        },
    )
    .await
    .context("failed to enqueue background release translation task")?;
    Ok(Some(task.task_id))
}

async fn enqueue_background_release_smart_task(
    state: &AppState,
    user_id: &str,
    release_ids: &[i64],
    source: &str,
    parent_task_id: Option<&str>,
    requested_by: Option<&str>,
) -> Result<Option<String>> {
    if release_ids.is_empty() || state.config.ai.is_none() {
        return Ok(None);
    }

    let task = jobs::enqueue_task(
        state,
        jobs::NewTask {
            task_type: jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH.to_owned(),
            payload: json!({
                "user_id": user_id,
                "release_ids": release_ids,
            }),
            source: source.to_owned(),
            requested_by: requested_by.map(str::to_owned),
            parent_task_id: parent_task_id.map(str::to_owned),
        },
    )
    .await
    .context("failed to enqueue background release smart task")?;
    Ok(Some(task.task_id))
}

async fn attach_and_wait_for_user_release_demand(
    state: &AppState,
    task_context: Option<(&str, HashSet<i64>)>,
    user_id: &str,
    origin: RepoReleaseOrigin,
    reason: &str,
) -> Result<SharedReleaseDemandResult> {
    let repos = load_user_starred_repo_rows(state, user_id).await?;
    let previous_repo_ids = task_context
        .as_ref()
        .map(|(_, ids)| ids.clone())
        .unwrap_or_default();
    let demand_repos = repos
        .iter()
        .map(|repo| ReleaseDemandRepo {
            repo_id: repo.repo_id,
            full_name: repo.full_name.clone(),
            is_new_repo: !previous_repo_ids.contains(&repo.repo_id),
        })
        .collect::<Vec<_>>();
    let task_id = task_context.as_ref().map(|(task_id, _)| *task_id);
    let attached =
        attach_release_demand(state, task_id, Some(user_id), &demand_repos, origin, reason).await?;

    if let Some(task_id) = task_id {
        jobs::append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "task_id": task_id,
                "stage": "release_attached",
                "repos": attached.repos,
                "queued": attached.queued,
                "reused_running": attached.reused_running,
                "reused_fresh": attached.reused_fresh,
            }),
        )
        .await?;
    }

    let waited = wait_for_release_demand(state, task_id, &attached.work_item_ids).await?;
    Ok(SharedReleaseDemandResult {
        repos: attached.repos,
        releases: waited.releases,
        reused_running: attached.reused_running,
        reused_fresh: attached.reused_fresh,
        queued: attached.queued,
        failed: waited.failed,
    })
}

async fn attach_release_demand(
    state: &AppState,
    task_id: Option<&str>,
    user_id: Option<&str>,
    repos: &[ReleaseDemandRepo],
    origin: RepoReleaseOrigin,
    reason: &str,
) -> Result<AttachReleaseDemandResult> {
    let mut result = AttachReleaseDemandResult {
        repos: repos.len(),
        ..AttachReleaseDemandResult::default()
    };
    if repos.is_empty() {
        return Ok(result);
    }

    let now = Utc::now();
    let now_rfc3339 = now.to_rfc3339();
    let deadline_at = repo_release_deadline_at(now, origin);
    let freshness_cutoff = repo_release_fresh_cutoff(now);

    for repo in repos {
        let mut tx = state
            .pool
            .begin()
            .await
            .context("begin repo release attach tx")?;
        let existing = sqlx::query_as::<_, RepoReleaseWorkItemRow>(
            r#"
            SELECT
              id,
              repo_id,
              repo_full_name,
              status,
              request_origin,
              priority,
              has_new_repo_watchers,
              deadline_at,
              last_success_at
            FROM repo_release_work_items
            WHERE repo_id = ?
            LIMIT 1
            "#,
        )
        .bind(repo.repo_id)
        .fetch_optional(&mut *tx)
        .await
        .with_context(|| {
            format!(
                "failed to load repo release work item for {}",
                repo.full_name
            )
        })?;

        let work_item_id = if let Some(existing) = existing {
            let is_fresh = existing
                .last_success_at
                .as_deref()
                .is_some_and(|value| value >= freshness_cutoff.as_str());
            let next_priority = existing.priority.max(origin.priority());
            let next_origin = if next_priority == REPO_RELEASE_PRIORITY_INTERACTIVE {
                RepoReleaseOrigin::Interactive.as_str()
            } else {
                existing.request_origin.as_str()
            };
            let next_has_new_repo_watchers =
                if existing.has_new_repo_watchers != 0 || repo.is_new_repo {
                    1
                } else {
                    0
                };
            let next_deadline =
                earlier_timestamp(existing.deadline_at.as_str(), deadline_at.as_str());

            if is_fresh && existing.status == jobs::STATUS_SUCCEEDED {
                if let Some(task_id) = task_id {
                    upsert_repo_release_watcher(
                        &mut tx,
                        RepoReleaseWatcherUpsert {
                            work_item_id: &existing.id,
                            task_id,
                            user_id,
                            origin,
                            reason,
                            is_new_repo: repo.is_new_repo,
                            status: "succeeded",
                            error_text: None,
                            now_rfc3339: &now_rfc3339,
                        },
                    )
                    .await?;
                }
                sqlx::query(
                    r#"
                    UPDATE repo_release_work_items
                    SET request_origin = ?, priority = ?, has_new_repo_watchers = ?, deadline_at = ?, updated_at = ?
                    WHERE id = ?
                    "#,
                )
                .bind(next_origin)
                .bind(next_priority)
                .bind(next_has_new_repo_watchers)
                .bind(next_deadline.as_str())
                .bind(now_rfc3339.as_str())
                .bind(&existing.id)
                .execute(&mut *tx)
                .await
                .with_context(|| format!("failed to refresh fresh repo release work item {}", repo.full_name))?;
                result.reused_fresh += 1;
                existing.id
            } else {
                let next_status = if existing.status == jobs::STATUS_RUNNING {
                    jobs::STATUS_RUNNING
                } else {
                    jobs::STATUS_QUEUED
                };
                sqlx::query(
                    r#"
                    UPDATE repo_release_work_items
                    SET
                      repo_full_name = ?,
                      status = ?,
                      request_origin = ?,
                      priority = ?,
                      has_new_repo_watchers = ?,
                      deadline_at = ?,
                      error_text = CASE WHEN ? = 'running' THEN error_text ELSE NULL END,
                      started_at = CASE WHEN ? = 'running' THEN started_at ELSE NULL END,
                      finished_at = CASE WHEN ? = 'running' THEN finished_at ELSE NULL END,
                      runtime_owner_id = CASE WHEN ? = 'running' THEN runtime_owner_id ELSE NULL END,
                      lease_heartbeat_at = CASE WHEN ? = 'running' THEN lease_heartbeat_at ELSE NULL END,
                      updated_at = ?
                    WHERE id = ?
                    "#,
                )
                .bind(&repo.full_name)
                .bind(next_status)
                .bind(next_origin)
                .bind(next_priority)
                .bind(next_has_new_repo_watchers)
                .bind(next_deadline.as_str())
                .bind(existing.status.as_str())
                .bind(existing.status.as_str())
                .bind(existing.status.as_str())
                .bind(existing.status.as_str())
                .bind(existing.status.as_str())
                .bind(now_rfc3339.as_str())
                .bind(&existing.id)
                .execute(&mut *tx)
                .await
                .with_context(|| format!("failed to update repo release work item {}", repo.full_name))?;
                if let Some(task_id) = task_id {
                    upsert_repo_release_watcher(
                        &mut tx,
                        RepoReleaseWatcherUpsert {
                            work_item_id: &existing.id,
                            task_id,
                            user_id,
                            origin,
                            reason,
                            is_new_repo: repo.is_new_repo,
                            status: "pending",
                            error_text: None,
                            now_rfc3339: &now_rfc3339,
                        },
                    )
                    .await?;
                }
                if existing.status == jobs::STATUS_RUNNING {
                    result.reused_running += 1;
                } else {
                    result.queued += 1;
                }
                existing.id
            }
        } else {
            let work_item_id = local_id::generate_local_id();
            sqlx::query(
                r#"
                INSERT INTO repo_release_work_items (
                  id,
                  repo_id,
                  repo_full_name,
                  status,
                  request_origin,
                  priority,
                  has_new_repo_watchers,
                  deadline_at,
                  last_release_count,
                  last_candidate_failures,
                  last_success_at,
                  error_text,
                  created_at,
                  started_at,
                  finished_at,
                  updated_at,
                  runtime_owner_id,
                  lease_heartbeat_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, NULL, NULL, ?, NULL, NULL, ?, NULL, NULL)
                "#,
            )
            .bind(&work_item_id)
            .bind(repo.repo_id)
            .bind(&repo.full_name)
            .bind(jobs::STATUS_QUEUED)
            .bind(origin.as_str())
            .bind(origin.priority())
            .bind(if repo.is_new_repo { 1_i64 } else { 0_i64 })
            .bind(deadline_at.as_str())
            .bind(now_rfc3339.as_str())
            .bind(now_rfc3339.as_str())
            .execute(&mut *tx)
            .await
            .with_context(|| {
                format!("failed to insert repo release work item {}", repo.full_name)
            })?;
            if let Some(task_id) = task_id {
                upsert_repo_release_watcher(
                    &mut tx,
                    RepoReleaseWatcherUpsert {
                        work_item_id: &work_item_id,
                        task_id,
                        user_id,
                        origin,
                        reason,
                        is_new_repo: repo.is_new_repo,
                        status: "pending",
                        error_text: None,
                        now_rfc3339: &now_rfc3339,
                    },
                )
                .await?;
            }
            result.queued += 1;
            work_item_id
        };

        tx.commit().await.context("commit repo release attach tx")?;
        result.work_item_ids.push(work_item_id);
    }

    Ok(result)
}

async fn upsert_repo_release_watcher(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    watcher: RepoReleaseWatcherUpsert<'_>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO repo_release_watchers (
          id,
          work_item_id,
          task_id,
          user_id,
          origin,
          priority,
          reason,
          is_new_repo,
          status,
          error_text,
          created_at,
          updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id, work_item_id) DO UPDATE SET
          user_id = excluded.user_id,
          origin = excluded.origin,
          priority = excluded.priority,
          reason = excluded.reason,
          is_new_repo = excluded.is_new_repo,
          status = CASE
            WHEN repo_release_watchers.status = 'succeeded' THEN repo_release_watchers.status
            ELSE excluded.status
          END,
          error_text = excluded.error_text,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(watcher.work_item_id)
    .bind(watcher.task_id)
    .bind(watcher.user_id)
    .bind(watcher.origin.as_str())
    .bind(watcher.origin.priority())
    .bind(watcher.reason)
    .bind(if watcher.is_new_repo { 1_i64 } else { 0_i64 })
    .bind(watcher.status)
    .bind(watcher.error_text)
    .bind(watcher.now_rfc3339)
    .bind(watcher.now_rfc3339)
    .execute(&mut **tx)
    .await
    .context("failed to upsert repo release watcher")?;
    Ok(())
}

async fn wait_for_release_demand(
    state: &AppState,
    task_id: Option<&str>,
    work_item_ids: &[String],
) -> Result<WaitReleaseDemandResult> {
    if work_item_ids.is_empty() {
        return Ok(WaitReleaseDemandResult::default());
    }

    loop {
        if let Some(task_id) = task_id
            && is_job_cancel_requested(state, task_id).await?
        {
            break;
        }

        let pending = if let Some(task_id) = task_id {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COUNT(*)
                FROM repo_release_watchers
                WHERE task_id = ? AND status = 'pending'
                "#,
            )
            .bind(task_id)
            .fetch_one(&state.pool)
            .await
            .context("failed to count pending repo release watchers")?
        } else {
            let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "SELECT COUNT(*) FROM repo_release_work_items WHERE status IN ('queued', 'running') AND id IN (",
            );
            {
                let mut separated = builder.separated(", ");
                for work_item_id in work_item_ids {
                    separated.push_bind(work_item_id);
                }
            }
            builder.push(")");
            builder
                .build_query_scalar::<i64>()
                .fetch_one(&state.pool)
                .await
                .context("failed to count pending repo release work items")?
        };
        if pending == 0 {
            break;
        }
        tokio::time::sleep(REPO_RELEASE_WAIT_POLL_INTERVAL).await;
    }

    if let Some(task_id) = task_id {
        let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN rw.status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_count,
              COALESCE(SUM(CASE WHEN rw.status = 'succeeded' THEN wi.last_release_count ELSE 0 END), 0) AS release_count,
              COALESCE(SUM(CASE WHEN rw.status = 'succeeded' THEN wi.last_candidate_failures ELSE 0 END), 0) AS candidate_failures
            FROM repo_release_watchers rw
            JOIN repo_release_work_items wi ON wi.id = rw.work_item_id
            WHERE rw.task_id = "#,
        );
        builder.push_bind(task_id);
        let row = builder
            .build()
            .fetch_one(&state.pool)
            .await
            .context("failed to summarize repo release watchers")?;
        return Ok(WaitReleaseDemandResult {
            releases: usize::try_from(row.get::<i64, _>("release_count")).unwrap_or_default(),
            failed: usize::try_from(row.get::<i64, _>("failed_count")).unwrap_or_default(),
            candidate_failures: usize::try_from(row.get::<i64, _>("candidate_failures"))
                .unwrap_or_default(),
        });
    }

    let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"
        SELECT
          COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_count,
          COALESCE(SUM(CASE WHEN status = 'succeeded' THEN last_release_count ELSE 0 END), 0) AS release_count,
          COALESCE(SUM(CASE WHEN status = 'succeeded' THEN last_candidate_failures ELSE 0 END), 0) AS candidate_failures
        FROM repo_release_work_items
        WHERE id IN (
        "#,
    );
    {
        let mut separated = builder.separated(", ");
        for work_item_id in work_item_ids {
            separated.push_bind(work_item_id);
        }
    }
    builder.push(")");
    let row = builder
        .build()
        .fetch_one(&state.pool)
        .await
        .context("failed to summarize repo release work items")?;
    Ok(WaitReleaseDemandResult {
        releases: usize::try_from(row.get::<i64, _>("release_count")).unwrap_or_default(),
        failed: usize::try_from(row.get::<i64, _>("failed_count")).unwrap_or_default(),
        candidate_failures: usize::try_from(row.get::<i64, _>("candidate_failures"))
            .unwrap_or_default(),
    })
}

fn repo_release_deadline_at(now: DateTime<Utc>, origin: RepoReleaseOrigin) -> String {
    let offset = match origin {
        RepoReleaseOrigin::Interactive => chrono::Duration::minutes(2),
        RepoReleaseOrigin::System => chrono::Duration::minutes(10),
    };
    (now + offset).to_rfc3339()
}

fn repo_release_fresh_cutoff(now: DateTime<Utc>) -> String {
    (now - chrono::Duration::from_std(REPO_RELEASE_FRESHNESS_WINDOW).unwrap_or_default())
        .to_rfc3339()
}

fn earlier_timestamp(left: &str, right: &str) -> String {
    if right < left {
        right.to_owned()
    } else {
        left.to_owned()
    }
}

async fn is_job_cancel_requested(state: &AppState, task_id: &str) -> Result<bool> {
    let cancel_requested = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT cancel_requested
        FROM job_tasks
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(&state.pool)
    .await
    .context("failed to query job cancel_requested")?
    .unwrap_or(0);
    Ok(cancel_requested != 0)
}

pub async fn sync_subscriptions(
    state: &AppState,
    task_id: &str,
    payload: &Value,
) -> Result<SyncSubscriptionsResult> {
    let trigger = payload
        .get("trigger")
        .and_then(Value::as_str)
        .unwrap_or("manual")
        .to_owned();
    let schedule_key = payload
        .get("schedule_key")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| jobs::current_subscription_schedule_key(chrono::Utc::now()));

    let context = SubscriptionRunContext::new(state, task_id).await?;
    context
        .log(
            "info",
            "scheduler",
            "run_started",
            "subscription sync run started",
            json!({
                "trigger": trigger,
                "schedule_key": schedule_key,
            }),
        )
        .await?;

    let users = sqlx::query_as::<_, EligibleUserRow>(
        r#"
        SELECT id, last_active_at
        FROM users
        WHERE is_disabled = 0
        ORDER BY
          CASE WHEN last_active_at IS NULL THEN 1 ELSE 0 END ASC,
          last_active_at DESC,
          id ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .context("failed to query users for subscription sync")?;

    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "collect",
            "trigger": trigger,
            "schedule_key": schedule_key,
            "total_users": users.len(),
        }),
    )
    .await?;

    let (successful_users, star_summary) = run_star_phase(&context, users).await?;
    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "star_summary",
            "total_users": star_summary.total_users,
            "succeeded_users": star_summary.succeeded_users,
            "failed_users": star_summary.failed_users,
            "total_repos": star_summary.total_repos,
        }),
    )
    .await?;

    let repos = aggregate_repos(&successful_users);
    let repo_ids = repos.iter().map(|repo| repo.repo_id).collect::<Vec<_>>();
    let before_release_ids = load_release_ids_for_repo_ids(state, &repo_ids).await?;
    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "repo_collect",
            "total_repos": repos.len(),
        }),
    )
    .await?;

    let (release_summary, releases_written) = run_release_phase(&context, repos).await?;
    let after_release_ids = load_release_ids_for_repo_ids(state, &repo_ids).await?;
    let mut new_release_ids = after_release_ids
        .difference(&before_release_ids)
        .copied()
        .collect::<Vec<_>>();
    new_release_ids.sort_unstable_by(|left, right| right.cmp(left));
    if !new_release_ids.is_empty() && state.config.ai.is_some() {
        for user in &successful_users {
            let user_release_ids =
                load_user_relevant_release_ids(state, user.user_id.as_str(), &new_release_ids)
                    .await?;
            let smart_preheat_release_ids = merge_smart_preheat_release_ids(
                &user_release_ids,
                &load_recent_release_ids_for_user(
                    state,
                    user.user_id.as_str(),
                    SMART_PREHEAT_RECENT_RELEASE_LIMIT,
                )
                .await
                .unwrap_or_else(|err| {
                    tracing::warn!(
                        ?err,
                        user_id = user.user_id.as_str(),
                        "sync.subscriptions: load recent release ids for smart preheat failed"
                    );
                    Vec::new()
                }),
            );
            if !user_release_ids.is_empty()
                && let Err(err) = enqueue_background_release_translation_task(
                    state,
                    user.user_id.as_str(),
                    &user_release_ids,
                    "sync.subscriptions.auto_translate",
                    Some(task_id),
                    None,
                )
                .await
            {
                tracing::warn!(
                    ?err,
                    user_id = user.user_id.as_str(),
                    "sync.subscriptions: enqueue background translation failed"
                );
            }
            if !smart_preheat_release_ids.is_empty()
                && let Err(err) = enqueue_background_release_smart_task(
                    state,
                    user.user_id.as_str(),
                    &smart_preheat_release_ids,
                    "sync.subscriptions.auto_smart",
                    Some(task_id),
                    None,
                )
                .await
            {
                tracing::warn!(
                    ?err,
                    user_id = user.user_id.as_str(),
                    "sync.subscriptions: enqueue background smart summary failed"
                );
            }
        }
    }
    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "release_summary",
            "total_repos": release_summary.total_repos,
            "succeeded_repos": release_summary.succeeded_repos,
            "failed_repos": release_summary.failed_repos,
            "candidate_failures": release_summary.candidate_failures,
            "releases_written": releases_written,
        }),
    )
    .await?;

    if context.is_cancel_requested().await? {
        context
            .log(
                "warning",
                "scheduler",
                "run_canceled",
                "subscription sync canceled before social phase",
                json!({
                    "total_users": star_summary.total_users,
                    "successful_users": star_summary.succeeded_users,
                    "release_repos": release_summary.total_repos,
                    "releases_written": releases_written,
                }),
            )
            .await?;
        return Ok(SyncSubscriptionsResult {
            skipped: false,
            skip_reason: None,
            star: star_summary,
            release: release_summary,
            social: SyncSubscriptionSocialSummary::default(),
            notifications: SyncSubscriptionNotificationsSummary::default(),
            releases_written,
            critical_events: context.critical_events.load(AtomicOrdering::Relaxed),
        });
    }

    let social_summary = run_social_phase(&context, &successful_users).await?;
    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "social_summary",
            "total_users": social_summary.total_users,
            "succeeded_users": social_summary.succeeded_users,
            "failed_users": social_summary.failed_users,
            "repo_stars": social_summary.repo_stars,
            "followers": social_summary.followers,
            "events": social_summary.events,
        }),
    )
    .await?;

    if context.is_cancel_requested().await? {
        context
            .log(
                "warning",
                "scheduler",
                "run_canceled",
                "subscription sync canceled before notifications phase",
                json!({
                    "total_users": social_summary.total_users,
                    "succeeded_users": social_summary.succeeded_users,
                    "failed_users": social_summary.failed_users,
                }),
            )
            .await?;
        return Ok(SyncSubscriptionsResult {
            skipped: false,
            skip_reason: None,
            star: star_summary,
            release: release_summary,
            social: social_summary,
            notifications: SyncSubscriptionNotificationsSummary::default(),
            releases_written,
            critical_events: context.critical_events.load(AtomicOrdering::Relaxed),
        });
    }

    let notifications_summary = run_notifications_phase(&context, &successful_users).await?;
    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "notifications_summary",
            "total_users": notifications_summary.total_users,
            "succeeded_users": notifications_summary.succeeded_users,
            "failed_users": notifications_summary.failed_users,
            "notifications": notifications_summary.notifications,
        }),
    )
    .await?;

    let result = SyncSubscriptionsResult {
        skipped: false,
        skip_reason: None,
        star: star_summary,
        release: release_summary,
        social: social_summary,
        notifications: notifications_summary,
        releases_written,
        critical_events: context.critical_events.load(AtomicOrdering::Relaxed),
    };

    jobs::append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "summary",
            "critical_events": result.critical_events,
            "releases_written": result.releases_written,
        }),
    )
    .await?;

    context
        .log(
            "info",
            "scheduler",
            "run_completed",
            "subscription sync run completed",
            serde_json::to_value(&result).unwrap_or_else(|_| json!({"ok": true})),
        )
        .await?;

    Ok(result)
}

async fn run_star_phase(
    context: &SubscriptionRunContext,
    users: Vec<EligibleUserRow>,
) -> Result<(Vec<StarPhaseSuccess>, SyncSubscriptionStarSummary)> {
    let mut join_set = JoinSet::new();
    let mut successful_users = Vec::new();
    let mut summary = SyncSubscriptionStarSummary {
        total_users: users.len(),
        ..SyncSubscriptionStarSummary::default()
    };

    for user in users {
        while join_set.len() >= SUBSCRIPTION_STAR_WORKERS {
            collect_star_result(
                join_set.join_next().await,
                &mut successful_users,
                &mut summary,
            )?;
            if context.is_cancel_requested().await? {
                context
                    .log(
                        "warning",
                        "star",
                        "run_canceled",
                        "subscription sync canceled during star phase",
                        json!({
                            "completed_users": summary.succeeded_users + summary.failed_users,
                            "total_users": summary.total_users,
                        }),
                    )
                    .await?;
                return Ok((successful_users, summary));
            }
        }
        if context.is_cancel_requested().await? {
            context
                .log(
                    "warning",
                    "star",
                    "run_canceled",
                    "subscription sync canceled during star phase",
                    json!({
                        "completed_users": summary.succeeded_users + summary.failed_users,
                        "total_users": summary.total_users,
                    }),
                )
                .await?;
            return Ok((successful_users, summary));
        }
        let worker_context = context.clone();
        join_set.spawn(async move { sync_starred_for_user(worker_context, user).await });
    }

    while !join_set.is_empty() {
        collect_star_result(
            join_set.join_next().await,
            &mut successful_users,
            &mut summary,
        )?;
        if context.is_cancel_requested().await? {
            context
                .log(
                    "warning",
                    "star",
                    "run_canceled",
                    "subscription sync canceled during star phase",
                    json!({
                        "completed_users": summary.succeeded_users + summary.failed_users,
                        "total_users": summary.total_users,
                    }),
                )
                .await?;
            return Ok((successful_users, summary));
        }
    }

    Ok((successful_users, summary))
}

fn collect_star_result(
    joined: Option<Result<Result<Option<StarPhaseSuccess>>, tokio::task::JoinError>>,
    successful_users: &mut Vec<StarPhaseSuccess>,
    summary: &mut SyncSubscriptionStarSummary,
) -> Result<()> {
    let Some(joined) = joined else {
        return Ok(());
    };
    match joined {
        Ok(Ok(Some(success))) => {
            summary.succeeded_users += 1;
            summary.total_repos += success.repos.len();
            successful_users.push(success);
            Ok(())
        }
        Ok(Ok(None)) => {
            summary.failed_users += 1;
            Ok(())
        }
        Ok(Err(err)) => Err(err),
        Err(err) => Err(anyhow!("star worker join failed: {err}")),
    }
}

async fn sync_starred_for_user(
    context: SubscriptionRunContext,
    user: EligibleUserRow,
) -> Result<Option<StarPhaseSuccess>> {
    let state = context.state.clone();
    sync_starred_for_user_with_fetch(
        context,
        user,
        move |user_id| {
            let state = state.clone();
            async move { fetch_starred_snapshot(state.as_ref(), &user_id).await }
        },
        |attempt| async move {
            tokio::time::sleep(subscription_retry_delay(attempt)).await;
        },
    )
    .await
}

async fn sync_starred_for_user_with_fetch<Fetch, FetchFut, Sleep, SleepFut>(
    context: SubscriptionRunContext,
    user: EligibleUserRow,
    mut fetch: Fetch,
    mut sleep: Sleep,
) -> Result<Option<StarPhaseSuccess>>
where
    Fetch: FnMut(String) -> FetchFut,
    FetchFut: Future<Output = Result<Vec<StarredRepoSnapshot>, SyncRequestError>>,
    Sleep: FnMut(usize) -> SleepFut,
    SleepFut: Future<Output = ()>,
{
    context
        .log(
            "info",
            "star",
            "user_started",
            format!("refreshing starred repositories for user #{}", user.id),
            json!({
                "user_id": user.id,
                "last_active_at": user.last_active_at,
            }),
        )
        .await?;

    for attempt in 1..=SUBSCRIPTION_RETRY_LIMIT {
        if context.is_cancel_requested().await? {
            context
                .log(
                    "warning",
                    "star",
                    "user_canceled",
                    format!("star sync canceled for user #{}", user.id),
                    json!({
                        "user_id": user.id,
                        "attempt": attempt,
                    }),
                )
                .await?;
            return Ok(None);
        }
        match fetch(user.id.clone()).await {
            Ok(repos) => {
                replace_starred_repos(context.state.as_ref(), &user.id, &repos).await?;
                context
                    .log(
                        "info",
                        "star",
                        "user_succeeded",
                        format!("user #{} starred snapshot refreshed", user.id),
                        json!({
                            "user_id": user.id,
                            "repo_count": repos.len(),
                            "attempt": attempt,
                        }),
                    )
                    .await?;
                return Ok(Some(StarPhaseSuccess {
                    user_id: user.id.clone(),
                    last_active_at: user.last_active_at.clone(),
                    repos,
                }));
            }
            Err(err) if err.retryable && attempt < SUBSCRIPTION_RETRY_LIMIT => {
                context
                    .key_event(
                        format!("retryable star sync error for user #{}", user.id),
                        SubscriptionEventRecord {
                            stage: "star",
                            event_type: err.reason_code,
                            severity: "warning",
                            recoverable: true,
                            attempt,
                            user_id: Some(user.id.as_str()),
                            repo_id: None,
                            repo_full_name: None,
                            payload: json!({
                                "user_id": user.id,
                                "reason_code": err.reason_code,
                                "status": err.status,
                                "error": err.message,
                            }),
                        },
                    )
                    .await?;
                sleep(attempt).await;
            }
            Err(err) => {
                let failure_message = if err.retryable && attempt > 1 {
                    format!(
                        "failed to refresh starred repositories for user #{} after {} attempts",
                        user.id, attempt
                    )
                } else {
                    format!(
                        "failed to refresh starred repositories for user #{}",
                        user.id
                    )
                };
                context
                    .key_event(
                        failure_message,
                        SubscriptionEventRecord {
                            stage: "star",
                            event_type: err.reason_code,
                            severity: "error",
                            recoverable: false,
                            attempt,
                            user_id: Some(user.id.as_str()),
                            repo_id: None,
                            repo_full_name: None,
                            payload: json!({
                                "user_id": user.id,
                                "reason_code": err.reason_code,
                                "status": err.status,
                                "error": err.message,
                            }),
                        },
                    )
                    .await?;
                return Ok(None);
            }
        }
    }

    unreachable!("star sync retry loop must return before exhausting attempts")
}

fn aggregate_repos(users: &[StarPhaseSuccess]) -> Vec<AggregatedRepo> {
    let mut grouped = HashMap::<i64, AggregatedRepo>::new();
    for user in users {
        for repo in &user.repos {
            let entry = grouped
                .entry(repo.repo_id)
                .or_insert_with(|| AggregatedRepo {
                    repo_id: repo.repo_id,
                    full_name: repo.full_name.clone(),
                    is_private: repo.is_private,
                    related_users: Vec::new(),
                });
            entry.is_private = entry.is_private || repo.is_private;
            entry.related_users.push(RelatedUserRef {
                user_id: user.user_id.clone(),
                last_active_at: user.last_active_at.clone(),
            });
        }
    }

    let mut repos = grouped.into_values().collect::<Vec<_>>();
    for repo in &mut repos {
        repo.related_users.sort_by(|left, right| {
            cmp_last_active_desc(
                left.last_active_at.as_deref(),
                right.last_active_at.as_deref(),
            )
            .then_with(|| left.user_id.cmp(&right.user_id))
        });
    }
    repos.sort_by(|left, right| {
        right
            .related_users
            .len()
            .cmp(&left.related_users.len())
            .then_with(|| left.full_name.cmp(&right.full_name))
    });
    repos
}

async fn run_release_phase(
    context: &SubscriptionRunContext,
    repos: Vec<AggregatedRepo>,
) -> Result<(SyncSubscriptionReleaseSummary, usize)> {
    let demand_repos = repos
        .iter()
        .map(|repo| ReleaseDemandRepo {
            repo_id: repo.repo_id,
            full_name: repo.full_name.clone(),
            is_new_repo: false,
        })
        .collect::<Vec<_>>();
    let attached = attach_release_demand(
        context.state.as_ref(),
        Some(context.task_id.as_str()),
        None,
        &demand_repos,
        RepoReleaseOrigin::System,
        "subscription_sync",
    )
    .await?;

    context
        .log(
            "info",
            "release",
            "release_attached",
            "subscription release demand attached to shared repo queue",
            json!({
                "repos": attached.repos,
                "queued": attached.queued,
                "reused_running": attached.reused_running,
                "reused_fresh": attached.reused_fresh,
            }),
        )
        .await?;

    let waited = wait_for_release_demand(
        context.state.as_ref(),
        Some(context.task_id.as_str()),
        &attached.work_item_ids,
    )
    .await?;

    Ok((
        SyncSubscriptionReleaseSummary {
            total_repos: attached.repos,
            succeeded_repos: attached.repos.saturating_sub(waited.failed),
            failed_repos: waited.failed,
            candidate_failures: waited.candidate_failures,
        },
        waited.releases,
    ))
}

async fn run_social_phase(
    context: &SubscriptionRunContext,
    users: &[StarPhaseSuccess],
) -> Result<SyncSubscriptionSocialSummary> {
    let mut join_set = JoinSet::new();
    let mut summary = SyncSubscriptionSocialSummary {
        total_users: users.len(),
        ..SyncSubscriptionSocialSummary::default()
    };

    context
        .log(
            "info",
            "social",
            "phase_started",
            "subscription social sync phase started",
            json!({
                "total_users": summary.total_users,
            }),
        )
        .await?;

    for user in users {
        while join_set.len() >= SUBSCRIPTION_SOCIAL_WORKERS {
            collect_social_result(join_set.join_next().await, &mut summary)?;
            if context.is_cancel_requested().await? {
                context
                    .log(
                        "warning",
                        "social",
                        "run_canceled",
                        "subscription sync canceled during social phase",
                        json!({
                            "completed_users": summary.succeeded_users + summary.failed_users,
                            "total_users": summary.total_users,
                        }),
                    )
                    .await?;
                return Ok(summary);
            }
        }
        if context.is_cancel_requested().await? {
            context
                .log(
                    "warning",
                    "social",
                    "run_canceled",
                    "subscription sync canceled during social phase",
                    json!({
                        "completed_users": summary.succeeded_users + summary.failed_users,
                        "total_users": summary.total_users,
                    }),
                )
                .await?;
            return Ok(summary);
        }
        let worker_context = context.clone();
        let user_id = user.user_id.clone();
        join_set.spawn(async move { sync_social_for_user(worker_context, user_id).await });
    }

    while !join_set.is_empty() {
        collect_social_result(join_set.join_next().await, &mut summary)?;
        if context.is_cancel_requested().await? {
            context
                .log(
                    "warning",
                    "social",
                    "run_canceled",
                    "subscription sync canceled during social phase",
                    json!({
                        "completed_users": summary.succeeded_users + summary.failed_users,
                        "total_users": summary.total_users,
                    }),
                )
                .await?;
            return Ok(summary);
        }
    }

    context
        .log(
            "info",
            "social",
            "phase_completed",
            "subscription social sync phase completed",
            serde_json::to_value(&summary).unwrap_or_else(|_| json!({"ok": true})),
        )
        .await?;

    Ok(summary)
}

fn collect_social_result(
    joined: Option<Result<Result<Option<SyncSocialActivityResult>>, tokio::task::JoinError>>,
    summary: &mut SyncSubscriptionSocialSummary,
) -> Result<()> {
    let Some(joined) = joined else {
        return Ok(());
    };

    match joined {
        Ok(Ok(Some(success))) => {
            summary.succeeded_users += 1;
            summary.repo_stars += success.repo_stars;
            summary.followers += success.followers;
            summary.events += success.events;
            Ok(())
        }
        Ok(Ok(None)) => {
            summary.failed_users += 1;
            Ok(())
        }
        Ok(Err(err)) => Err(err),
        Err(err) => Err(anyhow!("social worker join failed: {err}")),
    }
}

async fn sync_social_for_user(
    context: SubscriptionRunContext,
    user_id: String,
) -> Result<Option<SyncSocialActivityResult>> {
    context
        .log(
            "info",
            "social",
            "user_started",
            format!("refreshing social activity for user #{}", user_id),
            json!({
                "user_id": user_id.as_str(),
            }),
        )
        .await?;

    if context.is_cancel_requested().await? {
        context
            .log(
                "warning",
                "social",
                "user_canceled",
                format!("social sync canceled for user #{}", user_id),
                json!({
                    "user_id": user_id.as_str(),
                }),
            )
            .await?;
        return Ok(None);
    }

    let (result, error) = sync_social_activity_best_effort(
        context.state.as_ref(),
        user_id.as_str(),
        "sync.subscriptions",
    )
    .await;

    if let Some(error) = error {
        context
            .key_event(
                format!("failed to refresh social activity for user #{}", user_id),
                SubscriptionEventRecord {
                    stage: "social",
                    event_type: "social_sync_failed",
                    severity: "error",
                    recoverable: false,
                    attempt: 1,
                    user_id: Some(user_id.as_str()),
                    repo_id: None,
                    repo_full_name: None,
                    payload: json!({
                        "user_id": user_id.as_str(),
                        "error": error,
                    }),
                },
            )
            .await?;
        return Ok(None);
    }

    for source_error in &result.source_errors {
        context
            .key_event(
                format!("social sync degraded for user #{}", user_id),
                SubscriptionEventRecord {
                    stage: "social",
                    event_type: "social_source_degraded",
                    severity: "warning",
                    recoverable: true,
                    attempt: 1,
                    user_id: Some(user_id.as_str()),
                    repo_id: None,
                    repo_full_name: None,
                    payload: json!({
                        "user_id": user_id.as_str(),
                        "error": source_error,
                    }),
                },
            )
            .await?;
    }

    if !result.failed_repos.is_empty() {
        context
            .key_event(
                format!(
                    "repo stargazer snapshots partially failed for user #{}",
                    user_id
                ),
                SubscriptionEventRecord {
                    stage: "social",
                    event_type: "social_repo_stargazers_partial",
                    severity: "warning",
                    recoverable: true,
                    attempt: 1,
                    user_id: Some(user_id.as_str()),
                    repo_id: None,
                    repo_full_name: None,
                    payload: json!({
                        "user_id": user_id.as_str(),
                        "failed_repos": result.failed_repos.clone(),
                    }),
                },
            )
            .await?;
    }

    context
        .log(
            "info",
            "social",
            "user_succeeded",
            format!("social activity refreshed for user #{}", user_id),
            json!({
                "user_id": user_id,
                "repo_stars": result.repo_stars,
                "followers": result.followers,
                "events": result.events,
            }),
        )
        .await?;

    Ok(Some(result))
}

async fn run_notifications_phase(
    context: &SubscriptionRunContext,
    users: &[StarPhaseSuccess],
) -> Result<SyncSubscriptionNotificationsSummary> {
    let mut join_set = JoinSet::new();
    let mut summary = SyncSubscriptionNotificationsSummary {
        total_users: users.len(),
        ..SyncSubscriptionNotificationsSummary::default()
    };

    context
        .log(
            "info",
            "notifications",
            "phase_started",
            "subscription notifications sync phase started",
            json!({
                "total_users": summary.total_users,
            }),
        )
        .await?;

    for user in users {
        while join_set.len() >= SUBSCRIPTION_NOTIFICATION_WORKERS {
            collect_notification_result(join_set.join_next().await, &mut summary)?;
            if context.is_cancel_requested().await? {
                context
                    .log(
                        "warning",
                        "notifications",
                        "run_canceled",
                        "subscription sync canceled during notifications phase",
                        json!({
                            "completed_users": summary.succeeded_users + summary.failed_users,
                            "total_users": summary.total_users,
                        }),
                    )
                    .await?;
                return Ok(summary);
            }
        }
        if context.is_cancel_requested().await? {
            context
                .log(
                    "warning",
                    "notifications",
                    "run_canceled",
                    "subscription sync canceled during notifications phase",
                    json!({
                        "completed_users": summary.succeeded_users + summary.failed_users,
                        "total_users": summary.total_users,
                    }),
                )
                .await?;
            return Ok(summary);
        }
        let worker_context = context.clone();
        let user_id = user.user_id.clone();
        join_set.spawn(async move { sync_notifications_for_user(worker_context, user_id).await });
    }

    while !join_set.is_empty() {
        collect_notification_result(join_set.join_next().await, &mut summary)?;
        if context.is_cancel_requested().await? {
            context
                .log(
                    "warning",
                    "notifications",
                    "run_canceled",
                    "subscription sync canceled during notifications phase",
                    json!({
                        "completed_users": summary.succeeded_users + summary.failed_users,
                        "total_users": summary.total_users,
                    }),
                )
                .await?;
            return Ok(summary);
        }
    }

    context
        .log(
            "info",
            "notifications",
            "phase_completed",
            "subscription notifications sync phase completed",
            serde_json::to_value(&summary).unwrap_or_else(|_| json!({"ok": true})),
        )
        .await?;

    Ok(summary)
}

fn collect_notification_result(
    joined: Option<Result<Result<Option<SyncNotificationsResult>>, tokio::task::JoinError>>,
    summary: &mut SyncSubscriptionNotificationsSummary,
) -> Result<()> {
    let Some(joined) = joined else {
        return Ok(());
    };

    match joined {
        Ok(Ok(Some(success))) => {
            summary.succeeded_users += 1;
            summary.notifications += success.notifications;
            Ok(())
        }
        Ok(Ok(None)) => {
            summary.failed_users += 1;
            Ok(())
        }
        Ok(Err(err)) => Err(err),
        Err(err) => Err(anyhow!("notifications worker join failed: {err}")),
    }
}

async fn sync_notifications_for_user(
    context: SubscriptionRunContext,
    user_id: String,
) -> Result<Option<SyncNotificationsResult>> {
    context
        .log(
            "info",
            "notifications",
            "user_started",
            format!("refreshing inbox notifications for user #{}", user_id),
            json!({
                "user_id": user_id.as_str(),
            }),
        )
        .await?;

    if context.is_cancel_requested().await? {
        context
            .log(
                "warning",
                "notifications",
                "user_canceled",
                format!("notifications sync canceled for user #{}", user_id),
                json!({
                    "user_id": user_id.as_str(),
                }),
            )
            .await?;
        return Ok(None);
    }

    match sync_notifications(context.state.as_ref(), user_id.as_str()).await {
        Ok(result) => {
            context
                .log(
                    "info",
                    "notifications",
                    "user_succeeded",
                    format!("inbox notifications refreshed for user #{}", user_id),
                    json!({
                        "user_id": user_id.as_str(),
                        "notifications": result.notifications,
                        "since": result.since.clone(),
                    }),
                )
                .await?;
            Ok(Some(result))
        }
        Err(err) => {
            context
                .key_event(
                    format!(
                        "failed to refresh inbox notifications for user #{}",
                        user_id
                    ),
                    SubscriptionEventRecord {
                        stage: "notifications",
                        event_type: "notifications_sync_failed",
                        severity: "error",
                        recoverable: false,
                        attempt: 1,
                        user_id: Some(user_id.as_str()),
                        repo_id: None,
                        repo_full_name: None,
                        payload: json!({
                            "user_id": user_id.as_str(),
                            "error": err.to_string(),
                        }),
                    },
                )
                .await?;
            Ok(None)
        }
    }
}

async fn claim_next_repo_release_work_item(
    state: &AppState,
) -> Result<Option<RepoReleaseWorkItemRow>> {
    let _claim_guard = repo_release_claim_lock().lock().await;
    let mut tx = state
        .pool
        .begin()
        .await
        .context("begin repo release claim tx")?;

    let work_item_id = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM repo_release_work_items
        WHERE status = ?
        ORDER BY
          priority DESC,
          has_new_repo_watchers DESC,
          deadline_at ASC,
          created_at ASC
        LIMIT 1
        "#,
    )
    .bind(jobs::STATUS_QUEUED)
    .fetch_optional(&mut *tx)
    .await
    .context("select queued repo release work item")?;

    let Some(work_item_id) = work_item_id else {
        tx.commit()
            .await
            .context("commit empty repo release claim tx")?;
        return Ok(None);
    };

    let now = Utc::now().to_rfc3339();
    let updated = sqlx::query(
        r#"
        UPDATE repo_release_work_items
        SET
          status = ?,
          started_at = COALESCE(started_at, ?),
          runtime_owner_id = ?,
          lease_heartbeat_at = ?,
          updated_at = ?
        WHERE id = ? AND status = ?
        "#,
    )
    .bind(jobs::STATUS_RUNNING)
    .bind(now.as_str())
    .bind(state.runtime_owner_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&work_item_id)
    .bind(jobs::STATUS_QUEUED)
    .execute(&mut *tx)
    .await
    .context("claim repo release work item")?;

    if updated.rows_affected() == 0 {
        tx.commit()
            .await
            .context("commit failed repo release claim tx")?;
        return Ok(None);
    }

    let work_item = sqlx::query_as::<_, RepoReleaseWorkItemRow>(
        r#"
        SELECT
          id,
          repo_id,
          repo_full_name,
          status,
          request_origin,
          priority,
          has_new_repo_watchers,
          deadline_at,
          last_success_at
        FROM repo_release_work_items
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(&work_item_id)
    .fetch_one(&mut *tx)
    .await
    .context("reload claimed repo release work item")?;

    tx.commit().await.context("commit repo release claim tx")?;
    Ok(Some(work_item))
}

async fn process_repo_release_work_item(
    state: Arc<AppState>,
    work_item: RepoReleaseWorkItemRow,
) -> Result<()> {
    let heartbeat = runtime::spawn_lease_heartbeat(
        "repo_release_work_items",
        runtime::RUNTIME_LEASE_HEARTBEAT_INTERVAL,
        {
            let state = state.clone();
            let work_item_id = work_item.id.clone();
            move || {
                let state = state.clone();
                let work_item_id = work_item_id.clone();
                async move {
                    heartbeat_repo_release_work_item_lease(state.as_ref(), work_item_id.as_str())
                        .await
                }
            }
        },
    );

    let result = execute_repo_release_work_item(state.as_ref(), &work_item).await;
    heartbeat.stop().await;

    let now = Utc::now().to_rfc3339();
    match result {
        Ok((release_count, candidate_failures)) => {
            sqlx::query(
                r#"
                UPDATE repo_release_work_items
                SET
                  status = ?,
                  priority = 0,
                  has_new_repo_watchers = 0,
                  deadline_at = ?,
                  last_release_count = ?,
                  last_candidate_failures = ?,
                  last_success_at = ?,
                  error_text = NULL,
                  finished_at = ?,
                  updated_at = ?,
                  runtime_owner_id = NULL,
                  lease_heartbeat_at = NULL
                WHERE id = ?
                "#,
            )
            .bind(jobs::STATUS_SUCCEEDED)
            .bind(now.as_str())
            .bind(i64::try_from(release_count).unwrap_or(i64::MAX))
            .bind(i64::try_from(candidate_failures).unwrap_or(i64::MAX))
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(&work_item.id)
            .execute(&state.pool)
            .await
            .with_context(|| {
                format!("failed to finalize repo release work item {}", work_item.id)
            })?;
            mark_repo_release_watchers(state.as_ref(), &work_item.id, "succeeded", None, &now)
                .await?;
        }
        Err(err) => {
            let error_message = err.to_string();
            sqlx::query(
                r#"
                UPDATE repo_release_work_items
                SET
                  status = ?,
                  priority = 0,
                  has_new_repo_watchers = 0,
                  deadline_at = ?,
                  error_text = ?,
                  finished_at = ?,
                  updated_at = ?,
                  runtime_owner_id = NULL,
                  lease_heartbeat_at = NULL
                WHERE id = ?
                "#,
            )
            .bind(jobs::STATUS_FAILED)
            .bind(now.as_str())
            .bind(error_message.as_str())
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(&work_item.id)
            .execute(&state.pool)
            .await
            .with_context(|| format!("failed to fail repo release work item {}", work_item.id))?;
            mark_repo_release_watchers(
                state.as_ref(),
                &work_item.id,
                "failed",
                Some(error_message.as_str()),
                &now,
            )
            .await?;
        }
    }

    Ok(())
}

async fn execute_repo_release_work_item(
    state: &AppState,
    work_item: &RepoReleaseWorkItemRow,
) -> Result<(usize, usize)> {
    let candidates = sqlx::query_as::<_, ReleaseCandidateUserRow>(
        r#"
        SELECT DISTINCT u.id AS user_id, u.last_active_at
        FROM starred_repos sr
        JOIN users u ON u.id = sr.user_id
        WHERE sr.repo_id = ?
          AND u.is_disabled = 0
        ORDER BY
          CASE WHEN u.last_active_at IS NULL THEN 1 ELSE 0 END ASC,
          u.last_active_at DESC,
          u.id ASC
        "#,
    )
    .bind(work_item.repo_id)
    .fetch_all(&state.pool)
    .await
    .with_context(|| {
        format!(
            "failed to load repo release candidates for {}",
            work_item.repo_full_name
        )
    })?;

    if candidates.is_empty() {
        return Err(anyhow!(
            "no active candidate users remain for {}",
            work_item.repo_full_name
        ));
    }

    let mut candidate_failures = 0usize;
    for candidate in candidates {
        for attempt in 1..=SUBSCRIPTION_RETRY_LIMIT {
            match fetch_repo_releases_for_user(
                state,
                candidate.user_id.as_str(),
                work_item.repo_full_name.as_str(),
            )
            .await
            {
                Ok(releases) => {
                    let release_count =
                        upsert_repo_releases(state, work_item.repo_id, &releases).await?;
                    return Ok((release_count, candidate_failures));
                }
                Err(err) if err.retryable && attempt < SUBSCRIPTION_RETRY_LIMIT => {
                    candidate_failures += 1;
                    tokio::time::sleep(subscription_retry_delay(attempt)).await;
                }
                Err(_) => {
                    candidate_failures += 1;
                    break;
                }
            }
        }
    }

    Err(anyhow!(
        "all candidate users failed to sync {}",
        work_item.repo_full_name
    ))
}

async fn upsert_repo_releases(
    state: &AppState,
    repo_id: i64,
    releases: &[GitHubRelease],
) -> Result<usize> {
    let now = Utc::now().to_rfc3339();
    for release in releases {
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
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(release_id) DO UPDATE SET
              repo_id = excluded.repo_id,
              node_id = excluded.node_id,
              tag_name = excluded.tag_name,
              name = excluded.name,
              body = excluded.body,
              html_url = excluded.html_url,
              published_at = excluded.published_at,
              created_at = excluded.created_at,
              is_prerelease = excluded.is_prerelease,
              is_draft = excluded.is_draft,
              updated_at = excluded.updated_at,
              react_plus1 = excluded.react_plus1,
              react_laugh = excluded.react_laugh,
              react_heart = excluded.react_heart,
              react_hooray = excluded.react_hooray,
              react_rocket = excluded.react_rocket,
              react_eyes = excluded.react_eyes
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(repo_id)
        .bind(release.id)
        .bind(release.node_id.as_deref())
        .bind(release.tag_name.as_str())
        .bind(release.name.as_deref())
        .bind(release.body.as_deref())
        .bind(release.html_url.as_str())
        .bind(release.published_at.as_deref())
        .bind(release.created_at.as_deref())
        .bind(release.prerelease as i64)
        .bind(release.draft as i64)
        .bind(now.as_str())
        .bind(
            release
                .reactions
                .as_ref()
                .map(|value| value.plus1)
                .unwrap_or(0),
        )
        .bind(
            release
                .reactions
                .as_ref()
                .map(|value| value.laugh)
                .unwrap_or(0),
        )
        .bind(
            release
                .reactions
                .as_ref()
                .map(|value| value.heart)
                .unwrap_or(0),
        )
        .bind(
            release
                .reactions
                .as_ref()
                .map(|value| value.hooray)
                .unwrap_or(0),
        )
        .bind(
            release
                .reactions
                .as_ref()
                .map(|value| value.rocket)
                .unwrap_or(0),
        )
        .bind(
            release
                .reactions
                .as_ref()
                .map(|value| value.eyes)
                .unwrap_or(0),
        )
        .execute(&state.pool)
        .await
        .with_context(|| format!("failed to upsert shared release {}", release.tag_name))?;
    }

    Ok(releases.len())
}

async fn mark_repo_release_watchers(
    state: &AppState,
    work_item_id: &str,
    status: &str,
    error_text: Option<&str>,
    now_rfc3339: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE repo_release_watchers
        SET status = ?, error_text = ?, updated_at = ?
        WHERE work_item_id = ? AND status = 'pending'
        "#,
    )
    .bind(status)
    .bind(error_text)
    .bind(now_rfc3339)
    .bind(work_item_id)
    .execute(&state.pool)
    .await
    .context("failed to update repo release watchers")?;
    Ok(())
}

async fn heartbeat_repo_release_work_item_lease(
    state: &AppState,
    work_item_id: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE repo_release_work_items
        SET lease_heartbeat_at = ?, updated_at = ?
        WHERE id = ? AND status = ? AND runtime_owner_id = ?
        "#,
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(work_item_id)
    .bind(jobs::STATUS_RUNNING)
    .bind(state.runtime_owner_id.as_str())
    .execute(&state.pool)
    .await
    .context("failed to heartbeat repo release work item")?;
    Ok(())
}

pub async fn recover_repo_release_runtime_state(state: &AppState) -> Result<()> {
    recover_repo_release_runtime_state_with_mode(state, runtime::RuntimeRecoveryMode::Sweep).await
}

async fn recover_repo_release_runtime_state_with_mode(
    state: &AppState,
    mode: runtime::RuntimeRecoveryMode,
) -> Result<()> {
    let cutoff = runtime::stale_cutoff_timestamp(Utc::now());
    let stale_rows = match mode {
        runtime::RuntimeRecoveryMode::Startup => {
            sqlx::query_as::<_, StaleRepoReleaseWorkRow>(
                r#"
                SELECT id
                FROM repo_release_work_items
                WHERE status = ?
                  AND (
                    runtime_owner_id IS NULL
                    OR lease_heartbeat_at IS NULL
                    OR julianday(lease_heartbeat_at) <= julianday(?)
                    OR (
                      runtime_owner_id != ?
                      AND NOT EXISTS (
                        SELECT 1
                        FROM runtime_owners
                        WHERE runtime_owner_id = repo_release_work_items.runtime_owner_id
                          AND julianday(lease_heartbeat_at) > julianday(?)
                      )
                    )
                  )
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(jobs::STATUS_RUNNING)
            .bind(cutoff.as_str())
            .bind(state.runtime_owner_id.as_str())
            .bind(cutoff.as_str())
            .fetch_all(&state.pool)
            .await
        }
        runtime::RuntimeRecoveryMode::Sweep => {
            sqlx::query_as::<_, StaleRepoReleaseWorkRow>(
                r#"
                SELECT id
                FROM repo_release_work_items
                WHERE status = ?
                  AND (
                    runtime_owner_id IS NULL
                    OR lease_heartbeat_at IS NULL
                    OR julianday(lease_heartbeat_at) <= julianday(?)
                  )
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(jobs::STATUS_RUNNING)
            .bind(cutoff.as_str())
            .fetch_all(&state.pool)
            .await
        }
    }
    .context("failed to load stale repo release work items")?;

    let now = Utc::now().to_rfc3339();
    for row in stale_rows {
        sqlx::query(
            r#"
            UPDATE repo_release_work_items
            SET
              status = ?,
              priority = 0,
              has_new_repo_watchers = 0,
              deadline_at = ?,
              error_text = ?,
              finished_at = ?,
              updated_at = ?,
              runtime_owner_id = NULL,
              lease_heartbeat_at = NULL
            WHERE id = ?
            "#,
        )
        .bind(jobs::STATUS_FAILED)
        .bind(now.as_str())
        .bind(runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(&row.id)
        .execute(&state.pool)
        .await
        .with_context(|| format!("failed to recover stale repo release work item {}", row.id))?;
        mark_repo_release_watchers(
            state,
            &row.id,
            "failed",
            Some(runtime::RUNTIME_LEASE_EXPIRED_ERROR),
            &now,
        )
        .await?;
    }

    Ok(())
}

fn cmp_last_active_desc(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.cmp(left),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn merge_message_into_payload(payload: Value, message: &str) -> Value {
    match payload {
        Value::Object(mut object) => {
            object.insert("message".to_owned(), Value::String(message.to_owned()));
            Value::Object(object)
        }
        other => json!({
            "message": message,
            "details": other,
        }),
    }
}

async fn append_subscription_event(
    state: &AppState,
    task_id: &str,
    event: SubscriptionEventRecord<'_>,
) -> Result<()> {
    let payload_json =
        serde_json::to_string(&event.payload).context("serialize subscription event")?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO sync_subscription_events (
          id,
          task_id,
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
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(task_id)
    .bind(event.stage)
    .bind(event.event_type)
    .bind(event.severity)
    .bind(if event.recoverable { 1_i64 } else { 0_i64 })
    .bind(i64::try_from(event.attempt).unwrap_or(i64::MAX))
    .bind(event.user_id)
    .bind(event.repo_id)
    .bind(event.repo_full_name)
    .bind(payload_json)
    .bind(now.as_str())
    .execute(&state.pool)
    .await
    .context("failed to insert sync_subscription_event")?;
    Ok(())
}

fn classify_reqwest_error(operation: &str, err: reqwest::Error) -> SyncRequestError {
    if err.is_timeout() || err.is_connect() || err.is_request() {
        return SyncRequestError::retryable("network_error", format!("{operation}: {err}"), None);
    }
    SyncRequestError::non_retryable("request_error", format!("{operation}: {err}"), None)
}

fn classify_github_http_error(
    operation: &str,
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
) -> SyncRequestError {
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let body_lower = body.to_ascii_lowercase();
    if status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status.is_server_error()
        || remaining == "0"
        || body_lower.contains("secondary rate limit")
        || body_lower.contains("abuse detection")
    {
        return SyncRequestError::retryable(
            "rate_limited",
            format!("{operation}: github returned {status}"),
            Some(status),
        );
    }

    if status == StatusCode::UNAUTHORIZED {
        return SyncRequestError::non_retryable(
            "credentials_invalid",
            format!("{operation}: github returned 401"),
            Some(status),
        );
    }

    if status == StatusCode::FORBIDDEN {
        let reason_code =
            if body_lower.contains("scope") || body_lower.contains("resource not accessible") {
                "scope_insufficient"
            } else {
                "credentials_forbidden"
            };
        return SyncRequestError::non_retryable(
            reason_code,
            format!("{operation}: github returned 403"),
            Some(status),
        );
    }

    if status == StatusCode::NOT_FOUND || status.as_u16() == 451 {
        return SyncRequestError::non_retryable(
            "repo_inaccessible",
            format!("{operation}: github returned {status}"),
            Some(status),
        );
    }

    SyncRequestError::non_retryable(
        "github_http_error",
        format!("{operation}: github returned {status}"),
        Some(status),
    )
}

fn classify_graphql_errors(operation: &str, errors: &[GraphQlError]) -> SyncRequestError {
    let message = errors
        .iter()
        .map(|item| item.message.clone())
        .collect::<Vec<_>>()
        .join("; ");
    let message_lower = message.to_ascii_lowercase();
    if message_lower.contains("rate limit") {
        return SyncRequestError::retryable(
            "rate_limited",
            format!("{operation}: {message}"),
            None,
        );
    }
    if message_lower.contains("scope") || message_lower.contains("resource not accessible") {
        return SyncRequestError::non_retryable(
            "scope_insufficient",
            format!("{operation}: {message}"),
            None,
        );
    }
    SyncRequestError::non_retryable("graphql_error", format!("{operation}: {message}"), None)
}

async fn load_access_token_or_classified(
    state: &AppState,
    user_id: &str,
) -> Result<String, SyncRequestError> {
    state.load_access_token(user_id).await.map_err(|err| {
        let message = err.to_string();
        if message.contains("access token not found") {
            SyncRequestError::non_retryable(
                "credentials_missing",
                format!("load access token for user #{user_id}: {message}"),
                None,
            )
        } else {
            SyncRequestError::non_retryable(
                "credentials_invalid",
                format!("load access token for user #{user_id}: {message}"),
                None,
            )
        }
    })
}

async fn fetch_json_response<T: DeserializeOwned>(
    response: Response,
    operation: &str,
) -> Result<T, SyncRequestError> {
    let status = response.status();
    if !status.is_success() {
        let headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();
        return Err(classify_github_http_error(
            operation, status, &headers, &body,
        ));
    }
    response.json::<T>().await.map_err(|err| {
        SyncRequestError::non_retryable("decode_error", format!("{operation}: {err}"), Some(status))
    })
}

async fn fetch_github_rest_page<T: DeserializeOwned>(
    state: &AppState,
    access_token: &str,
    url: &str,
    accept: &str,
    operation: &str,
) -> Result<T, SyncRequestError> {
    with_subscription_timeout(operation, async {
        let response = state
            .http
            .get(url)
            .bearer_auth(access_token)
            .header(USER_AGENT, "OctoRill")
            .header(ACCEPT, accept)
            .header("X-GitHub-Api-Version", API_VERSION)
            .send()
            .await
            .map_err(|err| classify_reqwest_error(operation, err))?;

        fetch_json_response::<T>(response, operation).await
    })
    .await
}

async fn fetch_owned_repo_snapshot(
    state: &AppState,
    access_token: &str,
) -> Result<Vec<OwnedRepoSnapshot>, SyncRequestError> {
    let query = r#"
      query($after: String) {
        viewer {
          login
          repositories(
            first: 100
            after: $after
            ownerAffiliations: [OWNER]
            orderBy: {field: UPDATED_AT, direction: DESC}
          ) {
            pageInfo { hasNextPage endCursor }
            nodes {
              databaseId
              nameWithOwner
              openGraphImageUrl
              usesCustomOpenGraphImage
              owner {
                login
                avatarUrl(size: 80)
              }
            }
          }
        }
      }
    "#;

    let mut after: Option<String> = None;
    let mut repos = Vec::new();

    loop {
        let payload = with_subscription_timeout("sync social owned repos graphql", async {
            let response = state
                .http
                .post(GRAPHQL_URL)
                .bearer_auth(access_token)
                .header(USER_AGENT, "OctoRill")
                .header(ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", API_VERSION)
                .json(&json!({
                    "query": query,
                    "variables": { "after": after },
                }))
                .send()
                .await
                .map_err(|err| classify_reqwest_error("sync social owned repos graphql", err))?;

            fetch_json_response::<GraphQlResponse<OwnedRepoData>>(
                response,
                "sync social owned repos graphql",
            )
            .await
        })
        .await?;

        if let Some(errors) = payload.errors.as_ref().filter(|items| !items.is_empty()) {
            return Err(classify_graphql_errors(
                "sync social owned repos graphql",
                errors,
            ));
        }

        let viewer = payload
            .data
            .ok_or_else(|| {
                SyncRequestError::non_retryable(
                    "graphql_missing_data",
                    "sync social owned repos graphql: missing graphql data",
                    None,
                )
            })?
            .viewer;
        let viewer_login = viewer.login;
        let page = viewer.repositories;
        for node in page.nodes {
            if let Some(repo) = owned_repo_snapshot_from_node(node, viewer_login.as_str()) {
                repos.push(repo);
            }
        }

        if !page.page_info.has_next_page {
            break;
        }
        after = page.page_info.end_cursor;
        if after.is_none() {
            break;
        }
    }

    repos.sort_by(|left, right| left.repo_id.cmp(&right.repo_id));
    repos.dedup_by(|left, right| left.repo_id == right.repo_id);
    Ok(repos)
}

fn owned_repo_snapshot_from_node(
    node: OwnedRepoNode,
    viewer_login: &str,
) -> Option<OwnedRepoSnapshot> {
    let repo_id = node.database_id?;
    if !node.owner.login.eq_ignore_ascii_case(viewer_login) {
        return None;
    }
    let uses_custom_open_graph_image = node.uses_custom_open_graph_image();

    Some(OwnedRepoSnapshot {
        repo_id,
        full_name: node.name_with_owner,
        owner_avatar_url: node.owner.avatar_url,
        open_graph_image_url: node.open_graph_image_url,
        uses_custom_open_graph_image,
    })
}

async fn fetch_followers_snapshot(
    state: &AppState,
    access_token: &str,
) -> Result<Vec<FollowerSnapshot>, SyncRequestError> {
    let mut page = 1usize;
    let mut followers = Vec::new();

    loop {
        let operation = format!("sync social followers page {page}");
        let url = format!("{REST_API_BASE}/user/followers?per_page=100&page={page}");
        let items = fetch_github_rest_page::<Vec<GitHubActor>>(
            state,
            access_token,
            url.as_str(),
            "application/vnd.github+json",
            operation.as_str(),
        )
        .await?;

        let count = items.len();
        followers.extend(items.into_iter().map(|actor| FollowerSnapshot { actor }));
        if count < 100 {
            break;
        }
        page += 1;
    }

    followers.sort_by(|left, right| left.actor.id.cmp(&right.actor.id));
    followers.dedup_by(|left, right| left.actor.id == right.actor.id);
    Ok(followers)
}

async fn fetch_repo_stargazers_snapshot(
    state: &AppState,
    access_token: &str,
    repo: &OwnedRepoSnapshot,
) -> Result<Vec<RepoStargazerSnapshot>, anyhow::Error> {
    let mut page = 1usize;
    let mut members = Vec::new();

    loop {
        let operation = format!("sync social stargazers {} page {page}", repo.full_name);
        let url = format!(
            "{REST_API_BASE}/repos/{}/stargazers?per_page=100&page={page}",
            repo.full_name
        );
        let items = fetch_github_rest_page::<Vec<GitHubStargazer>>(
            state,
            access_token,
            url.as_str(),
            "application/vnd.github.star+json",
            operation.as_str(),
        )
        .await
        .map_err(SyncRequestError::into_anyhow)?;

        let count = items.len();
        members.extend(items.into_iter().map(|item| RepoStargazerSnapshot {
            repo_id: repo.repo_id,
            repo_full_name: repo.full_name.clone(),
            actor: item.user,
            starred_at: item.starred_at,
        }));
        if count < 100 {
            break;
        }
        page += 1;
    }

    members.sort_by(|left, right| left.actor.id.cmp(&right.actor.id));
    members.dedup_by(|left, right| left.actor.id == right.actor.id);
    Ok(members)
}

async fn fetch_starred_snapshot(
    state: &AppState,
    user_id: &str,
) -> Result<Vec<StarredRepoSnapshot>, SyncRequestError> {
    let token = load_access_token_or_classified(state, user_id).await?;
    let query = r#"
      query($after: String) {
        viewer {
          starredRepositories(first: 100, after: $after, orderBy: {field: STARRED_AT, direction: DESC}) {
            pageInfo { hasNextPage endCursor }
            edges {
              starredAt
              node {
                databaseId
                nameWithOwner
                name
                description
                url
                isPrivate
                openGraphImageUrl
                usesCustomOpenGraphImage
                owner {
                  login
                  avatarUrl(size: 80)
                }
              }
            }
          }
        }
      }
    "#;

    let mut after: Option<String> = None;
    let mut all = Vec::new();

    loop {
        let payload = with_subscription_timeout("sync starred graphql", async {
            let response = state
                .http
                .post(GRAPHQL_URL)
                .bearer_auth(&token)
                .header(USER_AGENT, "OctoRill")
                .header(ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", API_VERSION)
                .json(&json!({
                    "query": query,
                    "variables": { "after": after },
                }))
                .send()
                .await
                .map_err(|err| classify_reqwest_error("sync starred graphql", err))?;

            fetch_json_response::<GraphQlResponse<StarredData>>(response, "sync starred graphql")
                .await
        })
        .await?;
        if let Some(errors) = payload.errors.as_ref().filter(|items| !items.is_empty()) {
            return Err(classify_graphql_errors("sync starred graphql", errors));
        }
        let page = payload
            .data
            .ok_or_else(|| {
                SyncRequestError::non_retryable(
                    "graphql_missing_data",
                    "sync starred graphql: missing graphql data",
                    None,
                )
            })?
            .viewer
            .starred_repositories;
        for edge in page.edges {
            let Some(repo_id) = edge.node.database_id else {
                continue;
            };
            let uses_custom_open_graph_image = edge.node.uses_custom_open_graph_image();
            all.push(StarredRepoSnapshot {
                repo_id,
                full_name: edge.node.name_with_owner,
                owner_login: edge.node.owner.login,
                name: edge.node.name,
                description: edge.node.description,
                html_url: edge.node.url,
                stargazed_at: edge.starred_at,
                is_private: edge.node.is_private,
                owner_avatar_url: edge.node.owner.avatar_url,
                open_graph_image_url: edge.node.open_graph_image_url,
                uses_custom_open_graph_image,
            });
        }
        if !page.page_info.has_next_page {
            break;
        }
        after = page.page_info.end_cursor;
        if after.is_none() {
            break;
        }
    }

    Ok(all)
}

async fn replace_starred_repos(
    state: &AppState,
    user_id: &str,
    repos: &[StarredRepoSnapshot],
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut tx = state
        .pool
        .begin()
        .await
        .context("begin replace starred_repos tx")?;
    sqlx::query(r#"DELETE FROM starred_repos WHERE user_id = ?"#)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("failed to clear starred_repos")?;

    for repo in repos {
        sqlx::query(
            r#"
            INSERT INTO starred_repos (
              id, user_id, repo_id, full_name, owner_login, name, description, html_url,
              stargazed_at, is_private, updated_at, owner_avatar_url, open_graph_image_url,
              uses_custom_open_graph_image
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id)
        .bind(repo.repo_id)
        .bind(&repo.full_name)
        .bind(&repo.owner_login)
        .bind(&repo.name)
        .bind(repo.description.as_deref())
        .bind(&repo.html_url)
        .bind(&repo.stargazed_at)
        .bind(repo.is_private as i64)
        .bind(&now)
        .bind(repo.owner_avatar_url.as_deref())
        .bind(repo.open_graph_image_url.as_deref())
        .bind(repo.uses_custom_open_graph_image as i64)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("failed to insert starred repo {}", repo.full_name))?;
    }

    tx.commit()
        .await
        .context("commit replace starred_repos tx")?;
    Ok(())
}

async fn fetch_repo_releases_for_user(
    state: &AppState,
    user_id: &str,
    repo_full_name: &str,
) -> Result<Vec<GitHubRelease>, SyncRequestError> {
    let token = load_access_token_or_classified(state, user_id).await?;
    fetch_repo_releases_with_token(state, &token, repo_full_name).await
}

async fn fetch_repo_releases_with_token(
    state: &AppState,
    token: &str,
    repo_full_name: &str,
) -> Result<Vec<GitHubRelease>, SyncRequestError> {
    let mut page = 1usize;
    let mut releases = Vec::new();
    loop {
        let url =
            format!("{REST_API_BASE}/repos/{repo_full_name}/releases?per_page=100&page={page}");
        let operation = format!("sync releases {repo_full_name}");
        let page_releases = with_subscription_timeout(operation.as_str(), async {
            let response = state
                .http
                .get(url)
                .bearer_auth(token)
                .header(USER_AGENT, "OctoRill")
                .header(ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", API_VERSION)
                .send()
                .await
                .map_err(|err| classify_reqwest_error(operation.as_str(), err))?;
            fetch_json_response::<Vec<GitHubRelease>>(response, operation.as_str()).await
        })
        .await?;
        if page_releases.is_empty() {
            break;
        }
        releases.extend(page_releases);
        if page >= 50 {
            break;
        }
        page += 1;
    }
    Ok(releases)
}

pub async fn sync_notifications(
    state: &AppState,
    user_id: &str,
) -> Result<SyncNotificationsResult> {
    let token = state.load_access_token(user_id).await?;
    sync_notifications_with_fetch(
        state,
        user_id,
        |since, before, page| {
            let client = state.http.clone();
            let token = token.clone();
            Box::pin(async move {
                let mut url = format!(
                    "{REST_API_BASE}/notifications?all=true&per_page={GITHUB_NOTIFICATIONS_PAGE_SIZE}"
                );
                if let Some(ref since) = since {
                    url.push_str("&since=");
                    url.push_str(&urlencoding::encode(since));
                }
                if let Some(ref before) = before {
                    url.push_str("&before=");
                    url.push_str(&urlencoding::encode(before));
                }
                url.push_str("&page=");
                url.push_str(&page.to_string());

                client
                    .get(url)
                    .bearer_auth(&token)
                    .header(USER_AGENT, "OctoRill")
                    .header(ACCEPT, "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", API_VERSION)
                    .send()
                    .await
                    .context("github notifications request failed")?
                    .error_for_status()
                    .context("github notifications returned error")?
                    .json::<Vec<GitHubNotification>>()
                    .await
                    .context("github notifications json decode failed")
            }) as Pin<Box<dyn Future<Output = Result<Vec<GitHubNotification>>> + Send>>
        },
        |thread_id| {
            let client = state.http.clone();
            let token = token.clone();
            Box::pin(async move {
                let url = format!("{REST_API_BASE}/notifications/threads/{thread_id}");
                let response = client
                    .get(url)
                    .bearer_auth(&token)
                    .header(USER_AGENT, "OctoRill")
                    .header(ACCEPT, "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", API_VERSION)
                    .send()
                    .await
                    .context("github notification thread request failed")?;
                let status = response.status();
                if status.is_success() {
                    return response
                        .json::<GitHubNotification>()
                        .await
                        .map(Some)
                        .context("github notification thread json decode failed");
                }
                let headers = response.headers().clone();
                let body = response
                    .text()
                    .await
                    .context("github notification thread error body decode failed")?;
                let error =
                    classify_github_http_error("github notification thread", status, &headers, &body);
                if is_terminal_notification_thread_error(&error) {
                    return Ok(None);
                }
                Err(error.into_anyhow())
            }) as Pin<Box<dyn Future<Output = Result<Option<GitHubNotification>>> + Send>>
        },
    )
    .await
}

async fn sync_notifications_with_fetch<F, G>(
    state: &AppState,
    user_id: &str,
    mut fetch_page: F,
    mut fetch_thread: G,
) -> Result<SyncNotificationsResult>
where
    F: FnMut(
        Option<String>,
        Option<String>,
        usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<GitHubNotification>>> + Send>>,
    G: FnMut(String) -> Pin<Box<dyn Future<Output = Result<Option<GitHubNotification>>> + Send>>,
{
    let repair_state = load_sync_state_value(state, user_id, NOTIFICATION_OPEN_URL_REPAIR_KEY)
        .await
        .context("failed to query notification open-url repair state")?;
    let since = load_sync_state_value(state, user_id, NOTIFICATIONS_SINCE_KEY)
        .await
        .context("failed to query notifications since")?;
    let fetch_since = if repair_state.is_none() {
        None
    } else {
        since.clone()
    };

    let sync_started_at = chrono::Utc::now().to_rfc3339();
    let mut notifications = 0usize;
    let before = Some(sync_started_at.clone());
    let mut page = 1usize;
    loop {
        let res = fetch_page(fetch_since.clone(), before.clone(), page).await?;
        if res.is_empty() {
            break;
        }
        notifications += res.len();
        upsert_notifications(state, user_id, &res, &sync_started_at).await?;
        if res.len() < GITHUB_NOTIFICATIONS_PAGE_SIZE {
            break;
        }
        let Some(next_page) = page.checked_add(1) else {
            break;
        };
        page = next_page;
    }

    let repair_complete =
        repair_cached_notification_open_urls(state, user_id, &sync_started_at, &mut fetch_thread)
            .await?;
    let repair_state_value = if repair_complete {
        sync_started_at.as_str()
    } else {
        NOTIFICATION_OPEN_URL_REPAIR_PENDING
    };
    store_sync_state_value(
        state,
        user_id,
        NOTIFICATION_OPEN_URL_REPAIR_KEY,
        repair_state_value,
    )
    .await
    .context("failed to update notification open-url repair state")?;

    store_sync_state_value(state, user_id, NOTIFICATIONS_SINCE_KEY, &sync_started_at)
        .await
        .context("failed to update notifications since")?;

    Ok(SyncNotificationsResult {
        notifications,
        since,
    })
}

async fn upsert_notifications(
    state: &AppState,
    user_id: &str,
    notifications: &[GitHubNotification],
    now: &str,
) -> Result<()> {
    for notification in notifications {
        let api_url = notification
            .subject
            .url
            .as_deref()
            .or(notification.url.as_deref());
        let html_url = resolve_notification_open_url(
            api_url,
            notification.repository.full_name.as_deref(),
            Some(notification.id.as_str()),
        );
        sqlx::query(
            r#"
            INSERT INTO notifications (
              id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason,
              updated_at, unread, url, html_url, last_seen_at
            )
            SELECT
              ?, ?, ?, ?, ?, ?, ?, ?,
              COALESCE(?, (
                SELECT unread
                FROM notifications
                WHERE user_id = ? AND thread_id = ?
              ), 0),
              ?, ?, ?
            ON CONFLICT(user_id, thread_id) DO UPDATE SET
              repo_full_name = excluded.repo_full_name,
              subject_title = excluded.subject_title,
              subject_type = excluded.subject_type,
              reason = excluded.reason,
              updated_at = excluded.updated_at,
              unread = excluded.unread,
              url = excluded.url,
              html_url = excluded.html_url
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id)
        .bind(&notification.id)
        .bind(notification.repository.full_name.as_deref())
        .bind(notification.subject.title.as_deref())
        .bind(notification.subject.subject_type.as_deref())
        .bind(notification.reason.as_deref())
        .bind(notification.updated_at.as_deref())
        .bind(notification.unread.map(i64::from))
        .bind(user_id)
        .bind(&notification.id)
        .bind(api_url)
        .bind(html_url)
        .bind(now)
        .execute(&state.pool)
        .await
        .context("failed to upsert notification")?;
    }

    Ok(())
}

async fn repair_cached_notification_open_urls<G>(
    state: &AppState,
    user_id: &str,
    now: &str,
    fetch_thread: &mut G,
) -> Result<bool>
where
    G: FnMut(String) -> Pin<Box<dyn Future<Output = Result<Option<GitHubNotification>>> + Send>>,
{
    enum ThreadRefresh {
        NotNeeded,
        Refreshed(Option<GitHubNotification>),
        Failed,
    }

    let rows = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        ),
    >(
        r#"
        SELECT
          thread_id,
          repo_full_name,
          subject_title,
          subject_type,
          reason,
          updated_at,
          url,
          unread,
          html_url
        FROM notifications
        WHERE user_id = ?
          AND (
            url LIKE 'https://api.github.com/notifications/threads/%'
            OR html_url IS NULL
          )
        ORDER BY updated_at DESC, thread_id DESC
        LIMIT ?
        "#,
    )
    .bind(user_id)
    .bind(NOTIFICATION_OPEN_URL_REPAIR_BATCH_SIZE as i64)
    .fetch_all(&state.pool)
    .await
    .context("failed to load cached notifications for open-url repair")?;

    if rows.is_empty() {
        return Ok(true);
    }

    for (
        thread_id,
        repo_full_name,
        subject_title,
        subject_type,
        reason,
        updated_at,
        url,
        unread,
        html_url,
    ) in rows
    {
        let needs_thread_lookup =
            url.as_deref().is_some_and(is_notification_thread_api_url) || html_url.is_none();

        let thread_refresh = if needs_thread_lookup {
            match (*fetch_thread)(thread_id.clone()).await {
                Ok(thread) => ThreadRefresh::Refreshed(thread),
                Err(error) => {
                    tracing::warn!(
                        user_id,
                        thread_id,
                        error = ?error,
                        "failed to refresh notification thread during open-url repair"
                    );
                    ThreadRefresh::Failed
                }
            }
        } else {
            ThreadRefresh::NotNeeded
        };
        let thread = match &thread_refresh {
            ThreadRefresh::Refreshed(thread) => thread.as_ref(),
            ThreadRefresh::NotNeeded | ThreadRefresh::Failed => None,
        };

        let resolved_repo_full_name = thread
            .as_ref()
            .and_then(|item| item.repository.full_name.clone())
            .or(repo_full_name.clone());
        let resolved_subject_title = thread
            .as_ref()
            .and_then(|item| item.subject.title.clone())
            .or(subject_title.clone());
        let resolved_subject_type = thread
            .as_ref()
            .and_then(|item| item.subject.subject_type.clone())
            .or(subject_type.clone());
        let resolved_reason = thread
            .as_ref()
            .and_then(|item| item.reason.clone())
            .or(reason.clone());
        let resolved_updated_at = thread
            .as_ref()
            .and_then(|item| item.updated_at.clone())
            .or(updated_at.clone());
        let resolved_unread = thread
            .as_ref()
            .and_then(|item| item.unread)
            .map(|unread| unread as i64)
            .unwrap_or(unread);
        let resolved_api_url = match (&thread_refresh, thread) {
            (_, Some(item)) => item
                .subject
                .url
                .as_deref()
                .and_then(non_thread_notification_api_url)
                .or_else(|| {
                    item.url
                        .as_deref()
                        .and_then(non_thread_notification_api_url)
                })
                .or_else(|| url.as_deref().and_then(non_thread_notification_api_url)),
            (ThreadRefresh::Failed, _) => url.clone(),
            (ThreadRefresh::Refreshed(None), _) if needs_thread_lookup => {
                url.as_deref().and_then(non_thread_notification_api_url)
            }
            _ => url.clone(),
        };
        let resolved_html_url = match thread_refresh {
            ThreadRefresh::Failed => html_url.unwrap_or_else(|| {
                resolve_notification_open_url(
                    url.as_deref(),
                    resolved_repo_full_name.as_deref(),
                    Some(thread_id.as_str()),
                )
            }),
            ThreadRefresh::NotNeeded | ThreadRefresh::Refreshed(_) => {
                resolve_notification_open_url(
                    resolved_api_url.as_deref(),
                    resolved_repo_full_name.as_deref(),
                    Some(thread_id.as_str()),
                )
            }
        };

        sqlx::query(
            r#"
            UPDATE notifications
            SET
              repo_full_name = ?,
              subject_title = ?,
              subject_type = ?,
              reason = ?,
              updated_at = ?,
              unread = ?,
              url = ?,
              html_url = ?,
              last_seen_at = ?
            WHERE user_id = ? AND thread_id = ?
            "#,
        )
        .bind(resolved_repo_full_name)
        .bind(resolved_subject_title)
        .bind(resolved_subject_type)
        .bind(resolved_reason)
        .bind(resolved_updated_at)
        .bind(resolved_unread)
        .bind(resolved_api_url)
        .bind(resolved_html_url)
        .bind(now)
        .bind(user_id)
        .bind(thread_id)
        .execute(&state.pool)
        .await
        .context("failed to repair cached notification open url")?;
    }

    let remaining = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM notifications
        WHERE user_id = ?
          AND (
            url LIKE 'https://api.github.com/notifications/threads/%'
            OR html_url IS NULL
          )
        "#,
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .context("failed to count remaining cached notifications for open-url repair")?;

    Ok(remaining == 0)
}

async fn load_sync_state_value(
    state: &AppState,
    user_id: &str,
    key: &str,
) -> Result<Option<String>> {
    sqlx::query_scalar::<_, String>(r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#)
        .bind(user_id)
        .bind(key)
        .fetch_optional(&state.pool)
        .await
        .context("failed to query sync_state")
}

async fn store_sync_state_value(
    state: &AppState,
    user_id: &str,
    key: &str,
    value: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO sync_state (id, user_id, key, value, updated_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(user_id, key) DO UPDATE SET
          value = excluded.value,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(key)
    .bind(value)
    .bind(value)
    .execute(&state.pool)
    .await
    .context("failed to upsert sync_state")?;

    Ok(())
}

fn resolve_notification_open_url(
    api_url: Option<&str>,
    repo_full_name: Option<&str>,
    thread_id: Option<&str>,
) -> String {
    api_url
        .and_then(resolve_github_api_resource_html_url)
        .unwrap_or_else(|| fallback_notification_open_url(thread_id, repo_full_name))
}

fn is_notification_thread_api_url(api_url: &str) -> bool {
    api_url.starts_with("https://api.github.com/notifications/threads/")
}

fn non_thread_notification_api_url(api_url: &str) -> Option<String> {
    (!is_notification_thread_api_url(api_url)).then(|| api_url.to_owned())
}

fn is_terminal_notification_thread_error(error: &SyncRequestError) -> bool {
    matches!(
        error.reason_code,
        "scope_insufficient" | "credentials_forbidden" | "repo_inaccessible"
    )
}

fn resolve_github_api_resource_html_url(api_url: &str) -> Option<String> {
    let parsed = url::Url::parse(api_url).ok()?;
    if parsed.host_str() == Some("github.com") {
        return Some(parsed.to_string());
    }
    if parsed.host_str() != Some("api.github.com") {
        return None;
    }

    let segments = parsed.path_segments()?.collect::<Vec<_>>();
    match segments.as_slice() {
        ["repos", owner, repo, "issues", number] => {
            Some(format!("{GITHUB_WEB_BASE}/{owner}/{repo}/issues/{number}"))
        }
        ["repos", owner, repo, "pulls", number] => {
            Some(format!("{GITHUB_WEB_BASE}/{owner}/{repo}/pull/{number}"))
        }
        ["repos", owner, repo, "discussions", number] => Some(format!(
            "{GITHUB_WEB_BASE}/{owner}/{repo}/discussions/{number}"
        )),
        ["repos", owner, repo, "commits", sha] => {
            Some(format!("{GITHUB_WEB_BASE}/{owner}/{repo}/commit/{sha}"))
        }
        _ => None,
    }
}

fn fallback_notification_open_url(thread_id: Option<&str>, repo_full_name: Option<&str>) -> String {
    match thread_id
        .map(str::trim)
        .filter(|thread_id| !thread_id.is_empty())
    {
        Some(thread_id) => format!("{GITHUB_WEB_BASE}/notifications/threads/{thread_id}"),
        None => match repo_full_name {
            Some(repo_full_name) if !repo_full_name.trim().is_empty() => format!(
                "{GITHUB_WEB_BASE}/notifications?query={}",
                urlencoding::encode(&format!("repo:{repo_full_name}"))
            ),
            _ => format!("{GITHUB_WEB_BASE}/notifications"),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        fs,
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering as AtomicTestOrdering},
        },
    };

    use serde_json::{Value, json};
    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use url::Url;

    use super::{
        EligibleUserRow, FollowerSnapshot, GitHubActor, GitHubNotification,
        NOTIFICATION_OPEN_URL_REPAIR_BATCH_SIZE, NOTIFICATION_OPEN_URL_REPAIR_KEY,
        NOTIFICATION_OPEN_URL_REPAIR_PENDING, NOTIFICATIONS_SINCE_KEY, NotificationRepo,
        NotificationSubject, OwnedRepoNode, OwnedRepoSnapshot, ReleaseDemandRepo, RepoOwner,
        RepoReleaseOrigin, RepoStargazerSnapshot, SocialActivityEventInsert, StarPhaseSuccess,
        StarredRepoSnapshot, SubscriptionRunContext, SyncRequestError, aggregate_repos,
        apply_social_activity_snapshot, apply_social_activity_snapshot_partial,
        attach_and_wait_for_user_release_demand, attach_release_demand, classify_github_http_error,
        cmp_last_active_desc, collect_repo_stargazer_snapshots_with,
        insert_social_activity_event_tx, is_terminal_notification_thread_error,
        owned_repo_snapshot_from_node, recover_repo_release_runtime_state_on_startup,
        repo_release_deadline_at, resolve_notification_open_url,
        subscription_event_counts_as_critical, subscription_timeout_error,
        sync_notifications_with_fetch, sync_starred_for_user_with_fetch, wait_for_release_demand,
    };
    use crate::{
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        jobs, local_id,
        state::{AppState, build_oauth_client},
    };
    use axum::http::{HeaderMap, HeaderValue, StatusCode};

    fn test_user_id(seed: &str) -> String {
        crate::local_id::test_local_id(seed)
    }

    #[test]
    fn cmp_last_active_desc_places_recent_users_first() {
        let recent = Some("2026-03-06T12:30:00Z");
        let stale = Some("2026-03-05T12:30:00Z");
        assert!(cmp_last_active_desc(recent, stale).is_lt());
        assert!(cmp_last_active_desc(stale, recent).is_gt());
        assert!(cmp_last_active_desc(recent, None).is_lt());
        assert!(cmp_last_active_desc(None, recent).is_gt());
    }

    #[test]
    fn owned_repo_snapshot_from_node_preserves_visuals_for_viewer_owned_repo() {
        let snapshot = owned_repo_snapshot_from_node(
            OwnedRepoNode {
                database_id: Some(42),
                name_with_owner: "octo/rocket".to_owned(),
                open_graph_image_url: Some(
                    "https://repository-images.githubusercontent.com/42/rocket".to_owned(),
                ),
                uses_custom_open_graph_image: Some(true),
                owner: RepoOwner {
                    login: "octo".to_owned(),
                    avatar_url: Some("https://avatars.githubusercontent.com/u/42".to_owned()),
                },
            },
            "octo",
        )
        .expect("viewer-owned repo snapshot");

        assert_eq!(snapshot.repo_id, 42);
        assert_eq!(snapshot.full_name, "octo/rocket");
        assert_eq!(
            snapshot.owner_avatar_url.as_deref(),
            Some("https://avatars.githubusercontent.com/u/42")
        );
        assert_eq!(
            snapshot.open_graph_image_url.as_deref(),
            Some("https://repository-images.githubusercontent.com/42/rocket")
        );
        assert!(snapshot.uses_custom_open_graph_image);
    }

    #[test]
    fn owned_repo_snapshot_from_node_defaults_visual_flag_when_graphql_returns_null() {
        let snapshot = owned_repo_snapshot_from_node(
            OwnedRepoNode {
                database_id: Some(42),
                name_with_owner: "octo/rocket".to_owned(),
                open_graph_image_url: Some(
                    "https://repository-images.githubusercontent.com/42/rocket".to_owned(),
                ),
                uses_custom_open_graph_image: None,
                owner: RepoOwner {
                    login: "octo".to_owned(),
                    avatar_url: Some("https://avatars.githubusercontent.com/u/42".to_owned()),
                },
            },
            "octo",
        )
        .expect("viewer-owned repo snapshot");

        assert!(!snapshot.uses_custom_open_graph_image);
    }

    #[test]
    fn owned_repo_snapshot_from_node_skips_non_viewer_owned_repo() {
        let snapshot = owned_repo_snapshot_from_node(
            OwnedRepoNode {
                database_id: Some(99),
                name_with_owner: "acme/shared".to_owned(),
                open_graph_image_url: Some(
                    "https://repository-images.githubusercontent.com/99/shared".to_owned(),
                ),
                uses_custom_open_graph_image: Some(true),
                owner: RepoOwner {
                    login: "acme".to_owned(),
                    avatar_url: Some("https://avatars.githubusercontent.com/u/99".to_owned()),
                },
            },
            "octo",
        );

        assert!(snapshot.is_none());
    }

    #[test]
    fn owned_repo_node_deserializes_minimal_graphql_payload_with_null_visual_flag() {
        let node: OwnedRepoNode = serde_json::from_value(json!({
            "databaseId": 42,
            "nameWithOwner": "octo/rocket",
            "openGraphImageUrl": "https://repository-images.githubusercontent.com/42/rocket",
            "usesCustomOpenGraphImage": null,
            "owner": {
                "login": "octo",
                "avatarUrl": "https://avatars.githubusercontent.com/u/42"
            }
        }))
        .expect("deserialize owned repo node");

        assert_eq!(node.database_id, Some(42));
        assert_eq!(node.name_with_owner, "octo/rocket");
        assert!(!node.uses_custom_open_graph_image());
    }

    #[test]
    fn github_notification_deserializes_nullable_unread() {
        let notification: GitHubNotification = serde_json::from_value(json!({
            "id": "123",
            "unread": null,
            "reason": "subscribed",
            "updated_at": "2026-04-13T09:00:00Z",
            "url": "https://api.github.com/notifications/threads/123",
            "subject": {
                "title": "Issue",
                "type": "Issue",
                "url": "https://api.github.com/repos/octo/rocket/issues/1"
            },
            "repository": {
                "full_name": "octo/rocket"
            }
        }))
        .expect("deserialize notification with null unread");

        assert_eq!(notification.unread, None);
    }

    #[tokio::test]
    async fn upsert_notifications_defaults_new_null_unread_to_read() {
        let pool = setup_pool().await;
        let user_id = test_user_id("notifications-null-unread-new-row");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());
        let now = "2026-04-13T10:00:00Z";

        let mut notification = mock_notification(
            "thread-new-null-unread",
            Some("https://api.github.com/repos/octo/rocket/issues/2"),
            Some("octo/rocket"),
            Some("Issue"),
            now,
        );
        notification.unread = None;

        super::upsert_notifications(state.as_ref(), user_id.as_str(), &[notification], now)
            .await
            .expect("upsert notifications");

        let unread = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT unread
            FROM notifications
            WHERE user_id = ? AND thread_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind("thread-new-null-unread")
        .fetch_one(&pool)
        .await
        .expect("load new notification unread");

        assert_eq!(unread, 0);
    }

    #[tokio::test]
    async fn upsert_notifications_preserves_existing_unread_when_payload_is_null() {
        let pool = setup_pool().await;
        let user_id = test_user_id("notifications-null-unread-preserve");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());
        let now = "2026-04-13T10:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO notifications (
              id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason,
              updated_at, unread, url, html_url, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("thread-read")
        .bind("octo/rocket")
        .bind("Read thread")
        .bind("Issue")
        .bind("subscribed")
        .bind("2026-04-13T09:00:00Z")
        .bind(0_i64)
        .bind("https://api.github.com/notifications/threads/thread-read")
        .bind("https://github.com/octo/rocket/issues/1")
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed read notification");

        let mut notification = mock_notification(
            "thread-read",
            Some("https://api.github.com/repos/octo/rocket/issues/1"),
            Some("octo/rocket"),
            Some("Issue"),
            now,
        );
        notification.unread = None;

        super::upsert_notifications(state.as_ref(), user_id.as_str(), &[notification], now)
            .await
            .expect("upsert notifications");

        let unread = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT unread
            FROM notifications
            WHERE user_id = ? AND thread_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind("thread-read")
        .fetch_one(&pool)
        .await
        .expect("load notification unread");

        assert_eq!(unread, 0);
    }

    #[test]
    fn aggregate_repos_orders_by_related_users_then_name() {
        let users = vec![
            StarPhaseSuccess {
                user_id: test_user_id("2"),
                last_active_at: Some("2026-03-06T12:00:00Z".to_owned()),
                repos: vec![
                    StarredRepoSnapshot {
                        repo_id: 2,
                        full_name: "octo/beta".to_owned(),
                        owner_login: "octo".to_owned(),
                        name: "beta".to_owned(),
                        description: None,
                        html_url: "https://github.com/octo/beta".to_owned(),
                        stargazed_at: "2026-03-06T12:00:00Z".to_owned(),
                        is_private: false,
                        owner_avatar_url: None,
                        open_graph_image_url: None,
                        uses_custom_open_graph_image: false,
                    },
                    StarredRepoSnapshot {
                        repo_id: 1,
                        full_name: "octo/alpha".to_owned(),
                        owner_login: "octo".to_owned(),
                        name: "alpha".to_owned(),
                        description: None,
                        html_url: "https://github.com/octo/alpha".to_owned(),
                        stargazed_at: "2026-03-06T12:00:00Z".to_owned(),
                        is_private: false,
                        owner_avatar_url: None,
                        open_graph_image_url: None,
                        uses_custom_open_graph_image: false,
                    },
                ],
            },
            StarPhaseSuccess {
                user_id: test_user_id("1"),
                last_active_at: Some("2026-03-06T13:00:00Z".to_owned()),
                repos: vec![StarredRepoSnapshot {
                    repo_id: 1,
                    full_name: "octo/alpha".to_owned(),
                    owner_login: "octo".to_owned(),
                    name: "alpha".to_owned(),
                    description: None,
                    html_url: "https://github.com/octo/alpha".to_owned(),
                    stargazed_at: "2026-03-06T13:00:00Z".to_owned(),
                    is_private: false,
                    owner_avatar_url: None,
                    open_graph_image_url: None,
                    uses_custom_open_graph_image: false,
                }],
            },
        ];

        let repos = aggregate_repos(&users);
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].full_name, "octo/alpha");
        assert_eq!(repos[0].related_users.len(), 2);
        assert_eq!(repos[0].related_users[0].user_id, test_user_id("1"));
        assert_eq!(repos[1].full_name, "octo/beta");
    }

    #[test]
    fn subscription_event_counts_only_errors_as_critical() {
        assert!(subscription_event_counts_as_critical("error"));
        assert!(!subscription_event_counts_as_critical("warning"));
        assert!(!subscription_event_counts_as_critical("info"));
    }

    #[test]
    fn subscription_timeout_error_is_retryable() {
        let err = subscription_timeout_error("sync starred graphql");
        assert!(err.retryable);
        assert_eq!(err.reason_code, "timeout");
        assert!(err.message.contains("timed out after"));
    }

    #[test]
    fn resolve_notification_open_url_maps_common_targets_and_safe_fallbacks() {
        assert_eq!(
            resolve_notification_open_url(
                Some("https://api.github.com/repos/octo/alpha/issues/12"),
                Some("octo/alpha"),
                Some("thread-12"),
            ),
            "https://github.com/octo/alpha/issues/12"
        );
        assert_eq!(
            resolve_notification_open_url(
                Some("https://api.github.com/repos/octo/alpha/pulls/34"),
                Some("octo/alpha"),
                Some("thread-34"),
            ),
            "https://github.com/octo/alpha/pull/34"
        );
        assert_eq!(
            resolve_notification_open_url(
                Some("https://api.github.com/repos/octo/alpha/discussions/56"),
                Some("octo/alpha"),
                Some("thread-56"),
            ),
            "https://github.com/octo/alpha/discussions/56"
        );
        assert_eq!(
            resolve_notification_open_url(
                Some("https://api.github.com/repos/octo/alpha/check-suites/78"),
                Some("octo/alpha"),
                Some("thread-78"),
            ),
            "https://github.com/notifications/threads/thread-78"
        );
        assert_eq!(
            resolve_notification_open_url(None, Some("octo/alpha"), None),
            "https://github.com/notifications?query=repo%3Aocto%2Falpha"
        );
        assert_eq!(
            resolve_notification_open_url(None, None, None),
            "https://github.com/notifications"
        );
    }

    #[test]
    fn notification_thread_error_distinguishes_terminal_forbidden_from_rate_limits() {
        let forbidden = classify_github_http_error(
            "github notification thread",
            StatusCode::FORBIDDEN,
            &HeaderMap::new(),
            "resource not accessible by integration",
        );
        assert!(is_terminal_notification_thread_error(&forbidden));

        let not_found = classify_github_http_error(
            "github notification thread",
            StatusCode::NOT_FOUND,
            &HeaderMap::new(),
            "",
        );
        assert!(is_terminal_notification_thread_error(&not_found));

        let mut rate_limited_headers = HeaderMap::new();
        rate_limited_headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        let rate_limited = classify_github_http_error(
            "github notification thread",
            StatusCode::FORBIDDEN,
            &rate_limited_headers,
            "secondary rate limit",
        );
        assert!(rate_limited.retryable);
        assert!(!is_terminal_notification_thread_error(&rate_limited));
    }

    #[tokio::test]
    async fn sync_notifications_repairs_cached_urls_uses_subject_targets_and_paginates() {
        let pool = setup_pool().await;
        let user_id = test_user_id("notif-sync");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());
        let existing_since = "2026-03-05T00:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO sync_state (id, user_id, key, value, updated_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind(NOTIFICATIONS_SINCE_KEY)
        .bind(existing_since)
        .bind(existing_since)
        .execute(&pool)
        .await
        .expect("seed notifications since");

        sqlx::query(
            r#"
            INSERT INTO notifications (
              id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason,
              updated_at, unread, url, html_url, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("cached-thread")
        .bind("octo/alpha")
        .bind("Old cached notification")
        .bind("PullRequest")
        .bind("state_change")
        .bind("2026-03-06T00:00:00Z")
        .bind(1_i64)
        .bind("https://api.github.com/notifications/threads/123")
        .bind("https://github.com/octo/alpha")
        .bind("2026-03-06T00:00:00Z")
        .execute(&pool)
        .await
        .expect("seed stale notification");

        let observed = Arc::new(tokio::sync::Mutex::new(Vec::<(
            Option<String>,
            Option<String>,
            usize,
        )>::new()));
        let result = sync_notifications_with_fetch(
            state.as_ref(),
            user_id.as_str(),
            {
                let observed = observed.clone();
                move |since, before, page| {
                    let observed = observed.clone();
                    Box::pin(async move {
                        observed
                            .lock()
                            .await
                            .push((since.clone(), before.clone(), page));
                        let items = match page {
                            1 => {
                                let mut items = vec![
                                    mock_notification(
                                        "cached-thread",
                                        None,
                                        Some("octo/alpha"),
                                        Some("PullRequest"),
                                        "2026-03-06T03:30:00Z",
                                    ),
                                    mock_notification(
                                        "thread-1",
                                        Some("https://api.github.com/repos/octo/alpha/issues/12"),
                                        Some("octo/alpha"),
                                        Some("Issue"),
                                        "2026-03-06T03:00:00Z",
                                    ),
                                    mock_notification(
                                        "thread-2",
                                        Some("https://api.github.com/repos/octo/alpha/check-suites/99"),
                                        Some("octo/alpha"),
                                        Some("CheckSuite"),
                                        "2026-03-06T02:00:00Z",
                                    ),
                                ];
                                for index in 0..47 {
                                    items.push(mock_notification(
                                        &format!("filler-{index}"),
                                        Some(&format!(
                                            "https://api.github.com/repos/octo/alpha/issues/{}",
                                            100 + index
                                        )),
                                        Some("octo/alpha"),
                                        Some("Issue"),
                                        "2026-03-06T02:30:00Z",
                                    ));
                                }
                                items
                            }
                            2 => vec![mock_notification(
                                "thread-3",
                                Some("https://api.github.com/repos/octo/beta/pulls/34"),
                                Some("octo/beta"),
                                Some("PullRequest"),
                                "2026-03-06T01:00:00Z",
                            )],
                            _ => vec![],
                        };
                        Ok(items)
                    })
                }
            },
            move |thread_id| {
                Box::pin(async move {
                    Ok(match thread_id.as_str() {
                        "cached-thread" => Some(mock_notification(
                            "cached-thread",
                            Some("https://api.github.com/repos/octo/alpha/pulls/120"),
                            Some("octo/alpha"),
                            Some("PullRequest"),
                            "2026-03-06T03:30:00Z",
                        )),
                        _ => None,
                    })
                })
            },
        )
        .await
        .expect("sync notifications");

        assert_eq!(result.notifications, 51);
        assert_eq!(result.since.as_deref(), Some(existing_since));

        let observed = observed.lock().await.clone();
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0].0, None);
        assert_eq!(observed[0].2, 1);
        assert_eq!(observed[1].0, None);
        assert_eq!(observed[1].2, 2);
        assert_eq!(observed[0].1, observed[1].1);
        assert!(observed[0].1.is_some());

        let stored = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            r#"
            SELECT thread_id, url, html_url
            FROM notifications
            WHERE user_id = ?
            ORDER BY thread_id
            "#,
        )
        .bind(user_id.as_str())
        .fetch_all(&pool)
        .await
        .expect("load notifications");
        assert_eq!(stored.len(), 51);
        assert!(stored.contains(&(
            "cached-thread".to_owned(),
            Some("https://api.github.com/repos/octo/alpha/pulls/120".to_owned()),
            Some("https://github.com/octo/alpha/pull/120".to_owned()),
        )));
        assert!(stored.contains(&(
            "thread-1".to_owned(),
            Some("https://api.github.com/repos/octo/alpha/issues/12".to_owned()),
            Some("https://github.com/octo/alpha/issues/12".to_owned()),
        )));
        assert!(stored.contains(&(
            "thread-2".to_owned(),
            Some("https://api.github.com/repos/octo/alpha/check-suites/99".to_owned()),
            Some("https://github.com/notifications/threads/thread-2".to_owned()),
        )));
        assert!(stored.contains(&(
            "thread-3".to_owned(),
            Some("https://api.github.com/repos/octo/beta/pulls/34".to_owned()),
            Some("https://github.com/octo/beta/pull/34".to_owned()),
        )));

        let repair_marker = sqlx::query_scalar::<_, String>(
            r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
        )
        .bind(user_id.as_str())
        .bind(NOTIFICATION_OPEN_URL_REPAIR_KEY)
        .fetch_one(&pool)
        .await
        .expect("read repair marker");
        assert!(!repair_marker.is_empty());

        let since_value = sqlx::query_scalar::<_, String>(
            r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
        )
        .bind(user_id.as_str())
        .bind(NOTIFICATIONS_SINCE_KEY)
        .fetch_one(&pool)
        .await
        .expect("read notifications since");
        assert!(!since_value.is_empty());

        let observed_second = Arc::new(tokio::sync::Mutex::new(Vec::<(
            Option<String>,
            Option<String>,
            usize,
        )>::new()));
        let second = sync_notifications_with_fetch(
            state.as_ref(),
            user_id.as_str(),
            {
                let observed_second = observed_second.clone();
                move |since, before, page| {
                    let observed_second = observed_second.clone();
                    Box::pin(async move {
                        observed_second
                            .lock()
                            .await
                            .push((since.clone(), before.clone(), page));
                        if page == 1 {
                            Ok(vec![mock_notification(
                                "thread-4",
                                Some("https://api.github.com/repos/octo/gamma/issues/88"),
                                Some("octo/gamma"),
                                Some("Issue"),
                                "2026-03-06T04:00:00Z",
                            )])
                        } else {
                            Ok(vec![])
                        }
                    })
                }
            },
            move |_thread_id| Box::pin(async { Ok(None) }),
        )
        .await
        .expect("second sync notifications");
        assert_eq!(second.notifications, 1);
        assert!(second.since.is_some());
        let second_calls = observed_second.lock().await.clone();
        assert_eq!(second_calls.len(), 1);
        assert_eq!(second_calls[0].2, 1);
        assert!(second_calls[0].1.is_some());
        assert!(second_calls[0].0.is_some());
    }

    #[tokio::test]
    async fn sync_notifications_repair_ignores_thread_lookup_failures() {
        let pool = setup_pool().await;
        let user_id = test_user_id("notifications-repair-ignore-errors");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());

        let result = sync_notifications_with_fetch(
            state.as_ref(),
            user_id.as_str(),
            move |since, before, page| {
                Box::pin(async move {
                    assert_eq!(since, None);
                    assert!(before.is_some());
                    Ok(if page == 1 {
                        vec![mock_notification(
                            "thread-lookup-fails",
                            None,
                            Some("octo/alpha"),
                            Some("PullRequest"),
                            "2026-03-06T03:00:00Z",
                        )]
                    } else {
                        vec![]
                    })
                })
            },
            move |thread_id| Box::pin(async move { anyhow::bail!("boom for {thread_id}") }),
        )
        .await
        .expect("sync notifications");

        assert_eq!(result.notifications, 1);

        let stored = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            r#"
            SELECT url, html_url
            FROM notifications
            WHERE user_id = ? AND thread_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind("thread-lookup-fails")
        .fetch_one(&pool)
        .await
        .expect("load notification");
        assert_eq!(
            stored,
            (
                Some("https://api.github.com/notifications/threads/thread-lookup-fails".to_owned()),
                Some("https://github.com/notifications/threads/thread-lookup-fails".to_owned()),
            )
        );

        let repair_marker = sqlx::query_scalar::<_, String>(
            r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
        )
        .bind(user_id.as_str())
        .bind(NOTIFICATION_OPEN_URL_REPAIR_KEY)
        .fetch_one(&pool)
        .await
        .expect("read repair marker");
        assert_eq!(repair_marker, NOTIFICATION_OPEN_URL_REPAIR_PENDING);

        let since_value = sqlx::query_scalar::<_, String>(
            r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
        )
        .bind(user_id.as_str())
        .bind(NOTIFICATIONS_SINCE_KEY)
        .fetch_one(&pool)
        .await
        .expect("read notifications since");
        assert!(!since_value.is_empty());
    }

    #[tokio::test]
    async fn sync_notifications_repair_batches_old_rows_before_marking_complete() {
        let pool = setup_pool().await;
        let user_id = test_user_id("notifications-repair-batch-state");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());

        let existing_since = "2026-03-05T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO sync_state (id, user_id, key, value, updated_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind(NOTIFICATIONS_SINCE_KEY)
        .bind(existing_since)
        .bind(existing_since)
        .execute(&pool)
        .await
        .expect("seed notifications since");

        for index in 0..=(NOTIFICATION_OPEN_URL_REPAIR_BATCH_SIZE as i64) {
            let thread_id = format!("stale-{index:03}");
            sqlx::query(
                r#"
                INSERT INTO notifications (
                  id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason,
                  updated_at, unread, url, html_url, last_seen_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(local_id::generate_local_id())
            .bind(user_id.as_str())
            .bind(&thread_id)
            .bind("octo/alpha")
            .bind(format!("Stale {thread_id}"))
            .bind("PullRequest")
            .bind("state_change")
            .bind(format!("2026-03-06T00:{:02}:00Z", index % 60))
            .bind(1_i64)
            .bind(format!(
                "https://api.github.com/notifications/threads/{thread_id}"
            ))
            .bind("https://github.com/notifications?query=repo%3Aocto%2Falpha")
            .bind("2026-03-06T00:00:00Z")
            .execute(&pool)
            .await
            .expect("seed stale notification");
        }

        let observed = Arc::new(tokio::sync::Mutex::new(Vec::<Option<String>>::new()));
        let first = sync_notifications_with_fetch(
            state.as_ref(),
            user_id.as_str(),
            {
                let observed = observed.clone();
                move |since, _before, page| {
                    let observed = observed.clone();
                    Box::pin(async move {
                        observed.lock().await.push(since.clone());
                        assert_eq!(page, 1);
                        Ok(Vec::new())
                    })
                }
            },
            move |thread_id| {
                Box::pin(async move {
                    Ok(Some(mock_notification(
                        &thread_id,
                        Some(&format!(
                            "https://api.github.com/repos/octo/alpha/pulls/{thread_id}"
                        )),
                        Some("octo/alpha"),
                        Some("PullRequest"),
                        "2026-03-06T03:30:00Z",
                    )))
                })
            },
        )
        .await
        .expect("first sync notifications");

        assert_eq!(first.notifications, 0);
        let observed = observed.lock().await.clone();
        assert_eq!(observed.as_slice(), &[None]);

        let repair_state = sqlx::query_scalar::<_, String>(
            r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
        )
        .bind(user_id.as_str())
        .bind(NOTIFICATION_OPEN_URL_REPAIR_KEY)
        .fetch_one(&pool)
        .await
        .expect("read repair state");
        assert_eq!(repair_state, NOTIFICATION_OPEN_URL_REPAIR_PENDING);

        let remaining = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM notifications
            WHERE user_id = ?
              AND url LIKE 'https://api.github.com/notifications/threads/%'
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count remaining stale notifications");
        assert_eq!(remaining, 1);

        let observed_second = Arc::new(tokio::sync::Mutex::new(Vec::<Option<String>>::new()));
        let second = sync_notifications_with_fetch(
            state.as_ref(),
            user_id.as_str(),
            {
                let observed_second = observed_second.clone();
                move |since, _before, page| {
                    let observed_second = observed_second.clone();
                    Box::pin(async move {
                        observed_second.lock().await.push(since.clone());
                        assert_eq!(page, 1);
                        Ok(Vec::new())
                    })
                }
            },
            move |thread_id| {
                Box::pin(async move {
                    Ok(Some(mock_notification(
                        &thread_id,
                        Some(&format!(
                            "https://api.github.com/repos/octo/alpha/pulls/{thread_id}"
                        )),
                        Some("octo/alpha"),
                        Some("PullRequest"),
                        "2026-03-06T03:30:00Z",
                    )))
                })
            },
        )
        .await
        .expect("second sync notifications");

        assert_eq!(second.notifications, 0);
        let observed_second = observed_second.lock().await.clone();
        assert_eq!(observed_second.len(), 1);
        assert!(observed_second[0].is_some());

        let final_remaining = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM notifications
            WHERE user_id = ?
              AND url LIKE 'https://api.github.com/notifications/threads/%'
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count final stale notifications");
        assert_eq!(final_remaining, 0);

        let final_repair_state = sqlx::query_scalar::<_, String>(
            r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
        )
        .bind(user_id.as_str())
        .bind(NOTIFICATION_OPEN_URL_REPAIR_KEY)
        .fetch_one(&pool)
        .await
        .expect("read final repair state");
        assert_ne!(final_repair_state, NOTIFICATION_OPEN_URL_REPAIR_PENDING);
    }

    #[tokio::test]
    async fn sync_notifications_repairs_new_thread_backed_rows_after_backfill_complete() {
        let pool = setup_pool().await;
        let user_id = test_user_id("notifications-repair-fresh-after-backfill");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());

        let existing_since = "2026-03-05T00:00:00Z";
        let completed_repair_at = "2026-03-05T12:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO sync_state (id, user_id, key, value, updated_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind(NOTIFICATIONS_SINCE_KEY)
        .bind(existing_since)
        .bind(existing_since)
        .execute(&pool)
        .await
        .expect("seed notifications since");
        sqlx::query(
            r#"
            INSERT INTO sync_state (id, user_id, key, value, updated_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind(NOTIFICATION_OPEN_URL_REPAIR_KEY)
        .bind(completed_repair_at)
        .bind(completed_repair_at)
        .execute(&pool)
        .await
        .expect("seed repair state");

        let thread_calls = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
        let result = sync_notifications_with_fetch(
            state.as_ref(),
            user_id.as_str(),
            move |since, before, page| {
                Box::pin(async move {
                    assert_eq!(since.as_deref(), Some(existing_since));
                    assert!(before.is_some());
                    assert_eq!(page, 1);
                    Ok(if page == 1 {
                        vec![mock_notification(
                            "fresh-thread",
                            None,
                            Some("octo/alpha"),
                            Some("PullRequest"),
                            "2026-03-06T03:00:00Z",
                        )]
                    } else {
                        vec![]
                    })
                })
            },
            {
                let thread_calls = thread_calls.clone();
                move |thread_id| {
                    let thread_calls = thread_calls.clone();
                    Box::pin(async move {
                        thread_calls.lock().await.push(thread_id.clone());
                        Ok(Some(mock_notification(
                            &thread_id,
                            Some("https://api.github.com/repos/octo/alpha/pulls/99"),
                            Some("octo/alpha"),
                            Some("PullRequest"),
                            "2026-03-06T03:00:00Z",
                        )))
                    })
                }
            },
        )
        .await
        .expect("sync notifications");

        assert_eq!(result.notifications, 1);
        assert_eq!(thread_calls.lock().await.as_slice(), ["fresh-thread"]);

        let stored = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            r#"
            SELECT url, html_url
            FROM notifications
            WHERE user_id = ? AND thread_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind("fresh-thread")
        .fetch_one(&pool)
        .await
        .expect("load repaired notification");
        assert_eq!(
            stored,
            (
                Some("https://api.github.com/repos/octo/alpha/pulls/99".to_owned()),
                Some("https://github.com/octo/alpha/pull/99".to_owned()),
            )
        );

        let repair_marker = sqlx::query_scalar::<_, String>(
            r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
        )
        .bind(user_id.as_str())
        .bind(NOTIFICATION_OPEN_URL_REPAIR_KEY)
        .fetch_one(&pool)
        .await
        .expect("read repair marker");
        assert_ne!(repair_marker, NOTIFICATION_OPEN_URL_REPAIR_PENDING);
        assert_ne!(repair_marker, completed_repair_at);
    }

    #[tokio::test]
    async fn sync_notifications_clears_thread_urls_when_refreshed_target_is_missing() {
        let pool = setup_pool().await;
        let user_id = test_user_id("notifications-repair-clears-thread-url");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());

        let result = sync_notifications_with_fetch(
            state.as_ref(),
            user_id.as_str(),
            move |since, before, page| {
                Box::pin(async move {
                    assert_eq!(since, None);
                    assert!(before.is_some());
                    assert_eq!(page, 1);
                    Ok(if page == 1 {
                        vec![mock_notification(
                            "target-missing",
                            None,
                            Some("octo/alpha"),
                            Some("Release"),
                            "2026-03-06T03:00:00Z",
                        )]
                    } else {
                        vec![]
                    })
                })
            },
            move |thread_id| {
                Box::pin(async move {
                    Ok(Some(mock_notification(
                        &thread_id,
                        None,
                        Some("octo/alpha"),
                        Some("Release"),
                        "2026-03-06T03:00:00Z",
                    )))
                })
            },
        )
        .await
        .expect("sync notifications");

        assert_eq!(result.notifications, 1);

        let stored = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            r#"
            SELECT url, html_url
            FROM notifications
            WHERE user_id = ? AND thread_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind("target-missing")
        .fetch_one(&pool)
        .await
        .expect("load notification");
        assert_eq!(
            stored,
            (
                None,
                Some("https://github.com/notifications/threads/target-missing".to_owned()),
            )
        );

        let repair_marker = sqlx::query_scalar::<_, String>(
            r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
        )
        .bind(user_id.as_str())
        .bind(NOTIFICATION_OPEN_URL_REPAIR_KEY)
        .fetch_one(&pool)
        .await
        .expect("read repair marker");
        assert_ne!(repair_marker, NOTIFICATION_OPEN_URL_REPAIR_PENDING);
    }

    #[tokio::test]
    async fn sync_starred_for_user_retries_recoverable_errors_before_success() {
        let pool = setup_pool().await;
        seed_user(&pool, test_user_id("7").as_str()).await;
        let state = setup_state(pool.clone());
        seed_sync_task(&state, "task-star-retry-success").await;
        let context = SubscriptionRunContext::new(state.as_ref(), "task-star-retry-success")
            .await
            .expect("build subscription context");
        let attempts = Arc::new(AtomicUsize::new(0));
        let repos = vec![StarredRepoSnapshot {
            repo_id: 100,
            full_name: "octo/alpha".to_owned(),
            owner_login: "octo".to_owned(),
            name: "alpha".to_owned(),
            description: Some("alpha repo".to_owned()),
            html_url: "https://github.com/octo/alpha".to_owned(),
            stargazed_at: "2026-03-06T13:00:00Z".to_owned(),
            is_private: false,
            owner_avatar_url: Some("https://avatars.githubusercontent.com/u/100".to_owned()),
            open_graph_image_url: Some(
                "https://repository-images.githubusercontent.com/100/alpha".to_owned(),
            ),
            uses_custom_open_graph_image: true,
        }];

        let result = sync_starred_for_user_with_fetch(
            context.clone(),
            EligibleUserRow {
                id: test_user_id("7"),
                last_active_at: Some("2026-03-06T13:00:00Z".to_owned()),
            },
            {
                let attempts = attempts.clone();
                let repos = repos.clone();
                move |_user_id| {
                    let attempts = attempts.clone();
                    let repos = repos.clone();
                    async move {
                        let attempt = attempts.fetch_add(1, AtomicTestOrdering::SeqCst) + 1;
                        if attempt < 3 {
                            Err(SyncRequestError::retryable(
                                "network_error",
                                format!("temporary failure #{attempt}"),
                                None,
                            ))
                        } else {
                            Ok(repos)
                        }
                    }
                }
            },
            |_| async {},
        )
        .await
        .expect("sync starred for user");

        let success = result.expect("successful star sync");
        assert_eq!(success.repos.len(), 1);
        assert_eq!(attempts.load(AtomicTestOrdering::SeqCst), 3);

        let critical_events = context.critical_events.load(AtomicTestOrdering::Relaxed);
        assert_eq!(critical_events, 0);

        let stored_repos =
            sqlx::query_scalar::<_, i64>(r#"SELECT COUNT(*) FROM starred_repos WHERE user_id = ?"#)
                .bind(test_user_id("7"))
                .fetch_one(&pool)
                .await
                .expect("count starred repos");
        assert_eq!(stored_repos, 1);

        let visual_row = sqlx::query_as::<_, (Option<String>, Option<String>, i64)>(
            r#"
            SELECT owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image
            FROM starred_repos
            WHERE user_id = ? AND repo_id = ?
            LIMIT 1
            "#,
        )
        .bind(test_user_id("7"))
        .bind(100_i64)
        .fetch_one(&pool)
        .await
        .expect("load stored repo visual metadata");
        assert_eq!(
            visual_row.0.as_deref(),
            Some("https://avatars.githubusercontent.com/u/100")
        );
        assert_eq!(
            visual_row.1.as_deref(),
            Some("https://repository-images.githubusercontent.com/100/alpha")
        );
        assert_eq!(visual_row.2, 1);

        let event_rows = sqlx::query_as::<_, (String, String, i64, i64)>(
            r#"
            SELECT severity, event_type, attempt, recoverable
            FROM sync_subscription_events
            WHERE task_id = ?
            ORDER BY rowid ASC
            "#,
        )
        .bind("task-star-retry-success")
        .fetch_all(&pool)
        .await
        .expect("load retry events");
        assert_eq!(event_rows.len(), 2);
        assert_eq!(
            event_rows[0],
            ("warning".to_owned(), "network_error".to_owned(), 1, 1)
        );
        assert_eq!(
            event_rows[1],
            ("warning".to_owned(), "network_error".to_owned(), 2, 1)
        );
    }

    #[tokio::test]
    async fn sync_starred_for_user_marks_exhausted_retry_as_critical_failure() {
        let pool = setup_pool().await;
        seed_user(&pool, test_user_id("8").as_str()).await;
        let state = setup_state(pool.clone());
        seed_sync_task(&state, "task-star-retry-failure").await;
        let context = SubscriptionRunContext::new(state.as_ref(), "task-star-retry-failure")
            .await
            .expect("build subscription context");
        let attempts = Arc::new(AtomicUsize::new(0));

        let result = sync_starred_for_user_with_fetch(
            context.clone(),
            EligibleUserRow {
                id: test_user_id("8"),
                last_active_at: Some("2026-03-06T12:00:00Z".to_owned()),
            },
            {
                let attempts = attempts.clone();
                move |_user_id| {
                    let attempts = attempts.clone();
                    async move {
                        let attempt = attempts.fetch_add(1, AtomicTestOrdering::SeqCst) + 1;
                        Err(SyncRequestError::retryable(
                            "network_error",
                            format!("temporary failure #{attempt}"),
                            None,
                        ))
                    }
                }
            },
            |_| async {},
        )
        .await
        .expect("sync starred for user");

        assert!(result.is_none());
        assert_eq!(attempts.load(AtomicTestOrdering::SeqCst), 3);
        assert_eq!(
            context.critical_events.load(AtomicTestOrdering::Relaxed),
            1,
            "exhausted retry should count as a critical failure"
        );

        let event_rows = sqlx::query_as::<_, (String, i64, i64)>(
            r#"
            SELECT severity, attempt, recoverable
            FROM sync_subscription_events
            WHERE task_id = ?
            ORDER BY rowid ASC
            "#,
        )
        .bind("task-star-retry-failure")
        .fetch_all(&pool)
        .await
        .expect("load failure events");
        assert_eq!(event_rows.len(), 3);
        assert_eq!(event_rows[0], ("warning".to_owned(), 1, 1));
        assert_eq!(event_rows[1], ("warning".to_owned(), 2, 1));
        assert_eq!(event_rows[2], ("error".to_owned(), 3, 0));
    }

    #[tokio::test]
    async fn recover_repo_release_runtime_state_keeps_deadline_non_null() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-06T00:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO repo_release_work_items (
              id,
              repo_id,
              repo_full_name,
              status,
              request_origin,
              priority,
              has_new_repo_watchers,
              deadline_at,
              last_release_count,
              last_candidate_failures,
              last_success_at,
              error_text,
              created_at,
              started_at,
              finished_at,
              updated_at,
              runtime_owner_id,
              lease_heartbeat_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("repo-work-stale-1")
        .bind(42_i64)
        .bind("octo/alpha")
        .bind(jobs::STATUS_RUNNING)
        .bind("interactive")
        .bind(2_i64)
        .bind(1_i64)
        .bind("2026-03-06T00:01:00Z")
        .bind(0_i64)
        .bind(0_i64)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(now)
        .bind(Some(now))
        .bind(Option::<String>::None)
        .bind(now)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("seed stale repo release work item");

        recover_repo_release_runtime_state_on_startup(state.as_ref())
            .await
            .expect("recover repo release runtime state");

        let row = sqlx::query_as::<_, (String, String, Option<String>)>(
            r#"
            SELECT status, deadline_at, finished_at
            FROM repo_release_work_items
            WHERE id = ?
            "#,
        )
        .bind("repo-work-stale-1")
        .fetch_one(&pool)
        .await
        .expect("load recovered repo release work item");

        assert_eq!(row.0, jobs::STATUS_FAILED);
        assert!(!row.1.is_empty(), "deadline_at should stay non-null");
        assert!(row.2.is_some(), "recovered work item should be finalized");
    }

    #[tokio::test]
    async fn social_activity_initial_snapshot_writes_visible_events() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-baseline");
        seed_user(&pool, user_id.as_str()).await;

        let events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            &[OwnedRepoSnapshot {
                repo_id: 42,
                full_name: "octo/alpha".to_owned(),
                owner_avatar_url: None,
                open_graph_image_url: None,
                uses_custom_open_graph_image: false,
            }],
            &[(
                OwnedRepoSnapshot {
                    repo_id: 42,
                    full_name: "octo/alpha".to_owned(),
                    owner_avatar_url: None,
                    open_graph_image_url: None,
                    uses_custom_open_graph_image: false,
                },
                vec![RepoStargazerSnapshot {
                    repo_id: 42,
                    repo_full_name: "octo/alpha".to_owned(),
                    actor: GitHubActor {
                        id: 101,
                        login: "octocat".to_owned(),
                        avatar_url: Some("https://avatars.example/octocat.png".to_owned()),
                        html_url: Some("https://github.com/octocat".to_owned()),
                    },
                    starred_at: Some("2026-03-06T10:00:00Z".to_owned()),
                }],
            )],
            &[FollowerSnapshot {
                actor: GitHubActor {
                    id: 201,
                    login: "monalisa".to_owned(),
                    avatar_url: Some("https://avatars.example/monalisa.png".to_owned()),
                    html_url: Some("https://github.com/monalisa".to_owned()),
                },
            }],
        )
        .await
        .expect("apply initial social snapshot");

        assert_eq!(events, 2);

        let baseline_count: i64 =
            sqlx::query_scalar(r#"SELECT COUNT(*) FROM follower_sync_baselines WHERE user_id = ?"#)
                .bind(user_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("count follower baseline");
        assert_eq!(baseline_count, 1);

        let history_rows: Vec<(String, Option<String>, String, String)> = sqlx::query_as(
            r#"
            SELECT kind, repo_full_name, actor_login, occurred_at
            FROM social_activity_events
            WHERE user_id = ?
            ORDER BY kind ASC
            "#,
        )
        .bind(user_id.as_str())
        .fetch_all(&pool)
        .await
        .expect("load social events");
        assert_eq!(history_rows.len(), 2);
        assert_eq!(history_rows[0].0, "follower_received");
        assert_eq!(history_rows[0].1, None);
        assert_eq!(history_rows[0].2, "monalisa");
        assert_ne!(history_rows[0].3, "2026-03-06T10:00:00Z");
        assert_eq!(history_rows[1].0, "repo_star_received");
        assert_eq!(history_rows[1].1.as_deref(), Some("octo/alpha"));
        assert_eq!(history_rows[1].2, "octocat");
        assert_eq!(history_rows[1].3, "2026-03-06T10:00:00Z");
    }

    #[tokio::test]
    async fn social_activity_materializes_legacy_followers_without_existing_history() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-legacy-followers");
        seed_user(&pool, user_id.as_str()).await;

        sqlx::query(
            r#"
            INSERT INTO follower_sync_baselines (id, user_id, initialized_at, updated_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("2026-03-06T09:00:00Z")
        .bind("2026-03-06T09:00:00Z")
        .execute(&pool)
        .await
        .expect("insert follower baseline");
        sqlx::query(
            r#"
            INSERT INTO follower_current_members (
              id,
              user_id,
              actor_github_user_id,
              actor_login,
              actor_avatar_url,
              actor_html_url,
              created_at,
              updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind(201_i64)
        .bind("monalisa")
        .bind("https://avatars.example/monalisa.png")
        .bind("https://github.com/monalisa")
        .bind("2026-03-06T09:05:00Z")
        .bind("2026-03-06T09:05:00Z")
        .execute(&pool)
        .await
        .expect("insert follower current member");

        let events = apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            None,
            None,
            Some(&[FollowerSnapshot {
                actor: GitHubActor {
                    id: 201,
                    login: "monalisa".to_owned(),
                    avatar_url: Some("https://avatars.example/monalisa.png".to_owned()),
                    html_url: Some("https://github.com/monalisa".to_owned()),
                },
            }]),
        )
        .await
        .expect("materialize legacy followers");
        assert_eq!(events, 1);

        let row: (String, String) = sqlx::query_as(
            r#"
            SELECT actor_login, occurred_at
            FROM social_activity_events
            WHERE user_id = ? AND kind = 'follower_received'
            ORDER BY occurred_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load follower history");
        assert_eq!(row.0, "monalisa");
        assert_eq!(row.1, "2026-03-06T09:05:00Z");

        let second_events = apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            None,
            None,
            Some(&[FollowerSnapshot {
                actor: GitHubActor {
                    id: 201,
                    login: "monalisa".to_owned(),
                    avatar_url: Some("https://avatars.example/monalisa.png".to_owned()),
                    html_url: Some("https://github.com/monalisa".to_owned()),
                },
            }]),
        )
        .await
        .expect("re-run follower migration");
        assert_eq!(second_events, 0);
    }

    #[tokio::test]
    async fn social_activity_materializes_legacy_repo_stars_without_existing_history() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-legacy-stars");
        seed_user(&pool, user_id.as_str()).await;

        let repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/alpha".to_owned(),
            owner_avatar_url: Some("https://avatars.example/octo.png".to_owned()),
            open_graph_image_url: Some(
                "https://repository-images.githubusercontent.com/42/alpha".to_owned(),
            ),
            uses_custom_open_graph_image: true,
        };

        sqlx::query(
            r#"
            INSERT INTO repo_star_sync_baselines (id, user_id, initialized_at, updated_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("2026-03-06T08:00:00Z")
        .bind("2026-03-06T08:00:00Z")
        .execute(&pool)
        .await
        .expect("insert repo star baseline");
        sqlx::query(
            r#"
            INSERT INTO owned_repo_star_baselines (
              id,
              user_id,
              repo_id,
              repo_full_name,
              owner_avatar_url,
              open_graph_image_url,
              uses_custom_open_graph_image,
              members_snapshot_initialized,
              initialized_at,
              updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind(repo.repo_id)
        .bind(repo.full_name.as_str())
        .bind(repo.owner_avatar_url.as_deref())
        .bind(repo.open_graph_image_url.as_deref())
        .bind(1_i64)
        .bind(1_i64)
        .bind("2026-03-06T08:00:00Z")
        .bind("2026-03-06T08:00:00Z")
        .execute(&pool)
        .await
        .expect("insert owned repo baseline");
        sqlx::query(
            r#"
            INSERT INTO repo_star_current_members (
              id,
              user_id,
              repo_id,
              repo_full_name,
              actor_github_user_id,
              actor_login,
              actor_avatar_url,
              actor_html_url,
              starred_at,
              created_at,
              updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind(repo.repo_id)
        .bind(repo.full_name.as_str())
        .bind(301_i64)
        .bind("ghost")
        .bind("https://avatars.example/ghost.png")
        .bind("https://github.com/ghost")
        .bind("2026-03-01T12:00:00Z")
        .bind("2026-03-06T08:05:00Z")
        .bind("2026-03-06T08:05:00Z")
        .execute(&pool)
        .await
        .expect("insert repo star current member");

        let events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&repo),
            &[(
                repo.clone(),
                vec![RepoStargazerSnapshot {
                    repo_id: repo.repo_id,
                    repo_full_name: repo.full_name.clone(),
                    actor: GitHubActor {
                        id: 301,
                        login: "ghost".to_owned(),
                        avatar_url: Some("https://avatars.example/ghost.png".to_owned()),
                        html_url: Some("https://github.com/ghost".to_owned()),
                    },
                    starred_at: Some("2026-03-01T12:00:00Z".to_owned()),
                }],
            )],
            &[],
        )
        .await
        .expect("materialize legacy repo stars");
        assert_eq!(events, 1);

        let row: (String, String, Option<String>, i64) = sqlx::query_as(
            r#"
            SELECT actor_login, occurred_at, repo_open_graph_image_url, repo_uses_custom_open_graph_image
            FROM social_activity_events
            WHERE user_id = ? AND kind = 'repo_star_received'
            ORDER BY occurred_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load repo star history");
        assert_eq!(row.0, "ghost");
        assert_eq!(row.1, "2026-03-01T12:00:00Z");
        assert_eq!(
            row.2.as_deref(),
            Some("https://repository-images.githubusercontent.com/42/alpha")
        );
        assert_eq!(row.3, 1);

        let second_events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&repo),
            &[(
                repo.clone(),
                vec![RepoStargazerSnapshot {
                    repo_id: repo.repo_id,
                    repo_full_name: repo.full_name.clone(),
                    actor: GitHubActor {
                        id: 301,
                        login: "ghost".to_owned(),
                        avatar_url: Some("https://avatars.example/ghost.png".to_owned()),
                        html_url: Some("https://github.com/ghost".to_owned()),
                    },
                    starred_at: Some("2026-03-01T12:00:00Z".to_owned()),
                }],
            )],
            &[],
        )
        .await
        .expect("re-run repo star migration");
        assert_eq!(second_events, 0);
    }

    #[tokio::test]
    async fn social_activity_materializes_missing_legacy_followers_when_history_is_partial() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-partial-legacy-followers");
        seed_user(&pool, user_id.as_str()).await;

        sqlx::query(
            r#"
            INSERT INTO follower_sync_baselines (id, user_id, initialized_at, updated_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("2026-03-06T09:00:00Z")
        .bind("2026-03-06T09:00:00Z")
        .execute(&pool)
        .await
        .expect("insert follower baseline");

        for (actor_id, login, created_at) in [
            (201_i64, "monalisa", "2026-03-06T09:05:00Z"),
            (202_i64, "hubot", "2026-03-06T09:06:00Z"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO follower_current_members (
                  id,
                  user_id,
                  actor_github_user_id,
                  actor_login,
                  actor_avatar_url,
                  actor_html_url,
                  created_at,
                  updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(local_id::generate_local_id())
            .bind(user_id.as_str())
            .bind(actor_id)
            .bind(login)
            .bind(format!("https://avatars.example/{login}.png"))
            .bind(format!("https://github.com/{login}"))
            .bind(created_at)
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("insert follower current member");
        }

        let mut tx = pool.begin().await.expect("begin follower history tx");
        let inserted = insert_social_activity_event_tx(
            &mut tx,
            SocialActivityEventInsert {
                user_id: user_id.as_str(),
                kind: "follower_received",
                repo_id: None,
                repo_full_name: None,
                repo_visual: None,
                actor: &GitHubActor {
                    id: 201,
                    login: "monalisa".to_owned(),
                    avatar_url: Some("https://avatars.example/monalisa.png".to_owned()),
                    html_url: Some("https://github.com/monalisa".to_owned()),
                },
                occurred_at: "2026-03-06T09:05:00Z",
                detected_at: "2026-03-06T09:05:00Z",
            },
        )
        .await
        .expect("insert existing follower history");
        assert!(inserted);
        tx.commit().await.expect("commit follower history tx");

        let events = apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            None,
            None,
            Some(&[
                FollowerSnapshot {
                    actor: GitHubActor {
                        id: 201,
                        login: "monalisa".to_owned(),
                        avatar_url: Some("https://avatars.example/monalisa.png".to_owned()),
                        html_url: Some("https://github.com/monalisa".to_owned()),
                    },
                },
                FollowerSnapshot {
                    actor: GitHubActor {
                        id: 202,
                        login: "hubot".to_owned(),
                        avatar_url: Some("https://avatars.example/hubot.png".to_owned()),
                        html_url: Some("https://github.com/hubot".to_owned()),
                    },
                },
            ]),
        )
        .await
        .expect("materialize partial legacy followers");
        assert_eq!(events, 1);

        let rows = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT actor_login, occurred_at
            FROM social_activity_events
            WHERE user_id = ? AND kind = 'follower_received'
            ORDER BY actor_github_user_id ASC
            "#,
        )
        .bind(user_id.as_str())
        .fetch_all(&pool)
        .await
        .expect("load follower history rows");
        assert_eq!(
            rows,
            vec![
                ("monalisa".to_owned(), "2026-03-06T09:05:00Z".to_owned()),
                ("hubot".to_owned(), "2026-03-06T09:06:00Z".to_owned()),
            ]
        );

        let second_events = apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            None,
            None,
            Some(&[
                FollowerSnapshot {
                    actor: GitHubActor {
                        id: 201,
                        login: "monalisa".to_owned(),
                        avatar_url: Some("https://avatars.example/monalisa.png".to_owned()),
                        html_url: Some("https://github.com/monalisa".to_owned()),
                    },
                },
                FollowerSnapshot {
                    actor: GitHubActor {
                        id: 202,
                        login: "hubot".to_owned(),
                        avatar_url: Some("https://avatars.example/hubot.png".to_owned()),
                        html_url: Some("https://github.com/hubot".to_owned()),
                    },
                },
            ]),
        )
        .await
        .expect("re-run partial follower migration");
        assert_eq!(second_events, 0);
    }

    #[tokio::test]
    async fn social_activity_materializes_missing_legacy_repo_stars_when_history_is_partial() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-partial-legacy-stars");
        seed_user(&pool, user_id.as_str()).await;

        let repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/alpha".to_owned(),
            owner_avatar_url: Some("https://avatars.example/octo.png".to_owned()),
            open_graph_image_url: Some(
                "https://repository-images.githubusercontent.com/42/alpha".to_owned(),
            ),
            uses_custom_open_graph_image: true,
        };

        sqlx::query(
            r#"
            INSERT INTO repo_star_sync_baselines (id, user_id, initialized_at, updated_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind("2026-03-06T08:00:00Z")
        .bind("2026-03-06T08:00:00Z")
        .execute(&pool)
        .await
        .expect("insert repo star baseline");
        sqlx::query(
            r#"
            INSERT INTO owned_repo_star_baselines (
              id,
              user_id,
              repo_id,
              repo_full_name,
              owner_avatar_url,
              open_graph_image_url,
              uses_custom_open_graph_image,
              members_snapshot_initialized,
              initialized_at,
              updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(user_id.as_str())
        .bind(repo.repo_id)
        .bind(repo.full_name.as_str())
        .bind(repo.owner_avatar_url.as_deref())
        .bind(repo.open_graph_image_url.as_deref())
        .bind(1_i64)
        .bind(1_i64)
        .bind("2026-03-06T08:00:00Z")
        .bind("2026-03-06T08:00:00Z")
        .execute(&pool)
        .await
        .expect("insert owned repo baseline");

        for (actor_id, login, starred_at, created_at) in [
            (
                301_i64,
                "ghost",
                "2026-03-01T12:00:00Z",
                "2026-03-06T08:05:00Z",
            ),
            (
                302_i64,
                "mona",
                "2026-03-02T12:00:00Z",
                "2026-03-06T08:06:00Z",
            ),
        ] {
            sqlx::query(
                r#"
                INSERT INTO repo_star_current_members (
                  id,
                  user_id,
                  repo_id,
                  repo_full_name,
                  actor_github_user_id,
                  actor_login,
                  actor_avatar_url,
                  actor_html_url,
                  starred_at,
                  created_at,
                  updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(local_id::generate_local_id())
            .bind(user_id.as_str())
            .bind(repo.repo_id)
            .bind(repo.full_name.as_str())
            .bind(actor_id)
            .bind(login)
            .bind(format!("https://avatars.example/{login}.png"))
            .bind(format!("https://github.com/{login}"))
            .bind(starred_at)
            .bind(created_at)
            .bind(created_at)
            .execute(&pool)
            .await
            .expect("insert repo star current member");
        }

        let mut tx = pool.begin().await.expect("begin repo star history tx");
        let inserted = insert_social_activity_event_tx(
            &mut tx,
            SocialActivityEventInsert {
                user_id: user_id.as_str(),
                kind: "repo_star_received",
                repo_id: Some(repo.repo_id),
                repo_full_name: Some(repo.full_name.as_str()),
                repo_visual: Some(&repo),
                actor: &GitHubActor {
                    id: 301,
                    login: "ghost".to_owned(),
                    avatar_url: Some("https://avatars.example/ghost.png".to_owned()),
                    html_url: Some("https://github.com/ghost".to_owned()),
                },
                occurred_at: "2026-03-01T12:00:00Z",
                detected_at: "2026-03-06T08:05:00Z",
            },
        )
        .await
        .expect("insert existing repo star history");
        assert!(inserted);
        tx.commit().await.expect("commit repo star history tx");

        let events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&repo),
            &[(
                repo.clone(),
                vec![
                    RepoStargazerSnapshot {
                        repo_id: repo.repo_id,
                        repo_full_name: repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 301,
                            login: "ghost".to_owned(),
                            avatar_url: Some("https://avatars.example/ghost.png".to_owned()),
                            html_url: Some("https://github.com/ghost".to_owned()),
                        },
                        starred_at: Some("2026-03-01T12:00:00Z".to_owned()),
                    },
                    RepoStargazerSnapshot {
                        repo_id: repo.repo_id,
                        repo_full_name: repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 302,
                            login: "mona".to_owned(),
                            avatar_url: Some("https://avatars.example/mona.png".to_owned()),
                            html_url: Some("https://github.com/mona".to_owned()),
                        },
                        starred_at: Some("2026-03-02T12:00:00Z".to_owned()),
                    },
                ],
            )],
            &[],
        )
        .await
        .expect("materialize partial legacy repo stars");
        assert_eq!(events, 1);

        let rows = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT actor_login, occurred_at
            FROM social_activity_events
            WHERE user_id = ? AND kind = 'repo_star_received' AND repo_id = ?
            ORDER BY actor_github_user_id ASC
            "#,
        )
        .bind(user_id.as_str())
        .bind(repo.repo_id)
        .fetch_all(&pool)
        .await
        .expect("load repo star history rows");
        assert_eq!(
            rows,
            vec![
                ("ghost".to_owned(), "2026-03-01T12:00:00Z".to_owned()),
                ("mona".to_owned(), "2026-03-02T12:00:00Z".to_owned()),
            ]
        );

        let second_events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&repo),
            &[(
                repo.clone(),
                vec![
                    RepoStargazerSnapshot {
                        repo_id: repo.repo_id,
                        repo_full_name: repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 301,
                            login: "ghost".to_owned(),
                            avatar_url: Some("https://avatars.example/ghost.png".to_owned()),
                            html_url: Some("https://github.com/ghost".to_owned()),
                        },
                        starred_at: Some("2026-03-01T12:00:00Z".to_owned()),
                    },
                    RepoStargazerSnapshot {
                        repo_id: repo.repo_id,
                        repo_full_name: repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 302,
                            login: "mona".to_owned(),
                            avatar_url: Some("https://avatars.example/mona.png".to_owned()),
                            html_url: Some("https://github.com/mona".to_owned()),
                        },
                        starred_at: Some("2026-03-02T12:00:00Z".to_owned()),
                    },
                ],
            )],
            &[],
        )
        .await
        .expect("re-run partial repo star migration");
        assert_eq!(second_events, 0);
    }

    #[tokio::test]
    async fn social_activity_incremental_diff_writes_events_and_keeps_history_after_removal() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-diff");
        seed_user(&pool, user_id.as_str()).await;

        let repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/alpha".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };

        apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&repo),
            &[(repo.clone(), vec![])],
            &[],
        )
        .await
        .expect("seed empty baseline");

        let events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&repo),
            &[(
                repo.clone(),
                vec![RepoStargazerSnapshot {
                    repo_id: repo.repo_id,
                    repo_full_name: repo.full_name.clone(),
                    actor: GitHubActor {
                        id: 301,
                        login: "ghost".to_owned(),
                        avatar_url: Some("https://avatars.example/ghost.png".to_owned()),
                        html_url: Some("https://github.com/ghost".to_owned()),
                    },
                    starred_at: Some("2026-03-06T11:00:00Z".to_owned()),
                }],
            )],
            &[FollowerSnapshot {
                actor: GitHubActor {
                    id: 302,
                    login: "linus".to_owned(),
                    avatar_url: Some("https://avatars.example/linus.png".to_owned()),
                    html_url: Some("https://github.com/linus".to_owned()),
                },
            }],
        )
        .await
        .expect("write incremental events");
        assert_eq!(events, 2);

        let actor_snapshot: (String, Option<String>) = sqlx::query_as(
            r#"
            SELECT actor_login, actor_avatar_url
            FROM social_activity_events
            WHERE user_id = ? AND kind = 'repo_star_received'
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load star event");
        assert_eq!(actor_snapshot.0, "ghost");
        assert_eq!(
            actor_snapshot.1.as_deref(),
            Some("https://avatars.example/ghost.png")
        );

        apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&repo),
            &[(repo.clone(), vec![])],
            &[],
        )
        .await
        .expect("remove current members");

        let current_star_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM repo_star_current_members WHERE user_id = ?"#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count current stars");
        assert_eq!(current_star_count, 0);

        let current_follower_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM follower_current_members WHERE user_id = ?"#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count current followers");
        assert_eq!(current_follower_count, 0);

        let history_count: i64 =
            sqlx::query_scalar(r#"SELECT COUNT(*) FROM social_activity_events WHERE user_id = ?"#)
                .bind(user_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("count social history");
        assert_eq!(history_count, 2);
    }

    #[tokio::test]
    async fn social_activity_partial_snapshot_only_updates_available_sources() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-partial-snapshot");
        seed_user(&pool, user_id.as_str()).await;

        let repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/alpha".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let follower = FollowerSnapshot {
            actor: GitHubActor {
                id: 201,
                login: "monalisa".to_owned(),
                avatar_url: Some("https://avatars.example/monalisa.png".to_owned()),
                html_url: Some("https://github.com/monalisa".to_owned()),
            },
        };

        let initial_events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&repo),
            &[(repo.clone(), vec![])],
            std::slice::from_ref(&follower),
        )
        .await
        .expect("seed initial social baseline");
        assert_eq!(initial_events, 1);

        let repo_members = vec![(
            repo.clone(),
            vec![RepoStargazerSnapshot {
                repo_id: repo.repo_id,
                repo_full_name: repo.full_name.clone(),
                actor: GitHubActor {
                    id: 301,
                    login: "octocat".to_owned(),
                    avatar_url: Some("https://avatars.example/octocat.png".to_owned()),
                    html_url: Some("https://github.com/octocat".to_owned()),
                },
                starred_at: Some("2026-03-06T11:00:00Z".to_owned()),
            }],
        )];
        let repo_only_events = apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            Some(std::slice::from_ref(&repo)),
            Some(repo_members.as_slice()),
            None,
        )
        .await
        .expect("apply repo-only snapshot");
        assert_eq!(repo_only_events, 1);

        let follower_members_after_repo_only: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM follower_current_members
            WHERE user_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count follower members after repo-only snapshot");
        assert_eq!(follower_members_after_repo_only, 1);

        let repo_members_after_repo_only: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM repo_star_current_members
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("count repo members after repo-only snapshot");
        assert_eq!(repo_members_after_repo_only, 1);

        let followers = vec![
            follower.clone(),
            FollowerSnapshot {
                actor: GitHubActor {
                    id: 202,
                    login: "gaearon".to_owned(),
                    avatar_url: Some("https://avatars.example/gaearon.png".to_owned()),
                    html_url: Some("https://github.com/gaearon".to_owned()),
                },
            },
        ];
        let follower_only_events = apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            None,
            None,
            Some(followers.as_slice()),
        )
        .await
        .expect("apply follower-only snapshot");
        assert_eq!(follower_only_events, 1);

        let follower_members_after_follower_only: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM follower_current_members
            WHERE user_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count follower members after follower-only snapshot");
        assert_eq!(follower_members_after_follower_only, 2);

        let repo_members_after_follower_only: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM repo_star_current_members
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("count repo members after follower-only snapshot");
        assert_eq!(repo_members_after_follower_only, 1);

        let event_kinds = sqlx::query_scalar::<_, String>(
            r#"
            SELECT kind
            FROM social_activity_events
            WHERE user_id = ?
            ORDER BY kind ASC
            "#,
        )
        .bind(user_id.as_str())
        .fetch_all(&pool)
        .await
        .expect("load social activity event kinds");
        assert_eq!(
            event_kinds,
            vec![
                "follower_received".to_owned(),
                "follower_received".to_owned(),
                "repo_star_received".to_owned()
            ]
        );
    }

    #[tokio::test]
    async fn social_activity_first_repo_snapshot_after_follower_only_baseline_emits_events() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-repo-bootstrap-after-follower-only");
        seed_user(&pool, user_id.as_str()).await;

        let follower = FollowerSnapshot {
            actor: GitHubActor {
                id: 201,
                login: "monalisa".to_owned(),
                avatar_url: Some("https://avatars.example/monalisa.png".to_owned()),
                html_url: Some("https://github.com/monalisa".to_owned()),
            },
        };
        apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            None,
            None,
            Some(std::slice::from_ref(&follower)),
        )
        .await
        .expect("seed follower-only baseline");

        let repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/alpha".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let repo_members = vec![(
            repo.clone(),
            vec![RepoStargazerSnapshot {
                repo_id: repo.repo_id,
                repo_full_name: repo.full_name.clone(),
                actor: GitHubActor {
                    id: 301,
                    login: "octocat".to_owned(),
                    avatar_url: Some("https://avatars.example/octocat.png".to_owned()),
                    html_url: Some("https://github.com/octocat".to_owned()),
                },
                starred_at: Some("2026-03-06T11:00:00Z".to_owned()),
            }],
        )];
        let events = apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            Some(std::slice::from_ref(&repo)),
            Some(repo_members.as_slice()),
            None,
        )
        .await
        .expect("apply first repo snapshot after follower-only baseline");
        assert_eq!(events, 1);

        let history_rows: Vec<(String, Option<String>, String)> = sqlx::query_as(
            r#"
            SELECT kind, repo_full_name, actor_login
            FROM social_activity_events
            WHERE user_id = ?
            ORDER BY kind ASC, actor_login ASC
            "#,
        )
        .bind(user_id.as_str())
        .fetch_all(&pool)
        .await
        .expect("load social history");
        assert_eq!(
            history_rows,
            vec![
                ("follower_received".to_owned(), None, "monalisa".to_owned()),
                (
                    "repo_star_received".to_owned(),
                    Some("octo/alpha".to_owned()),
                    "octocat".to_owned(),
                ),
            ]
        );

        let baseline_state: i64 = sqlx::query_scalar(
            r#"
            SELECT members_snapshot_initialized
            FROM owned_repo_star_baselines
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("load repo baseline state");
        assert_eq!(baseline_state, 1);
    }

    #[tokio::test]
    async fn social_activity_follower_events_dedupe_when_repo_id_is_null() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-follow-dedupe");
        seed_user(&pool, user_id.as_str()).await;

        let actor = GitHubActor {
            id: 401,
            login: "dedupe-cat".to_owned(),
            avatar_url: Some("https://avatars.example/dedupe-cat.png".to_owned()),
            html_url: Some("https://github.com/dedupe-cat".to_owned()),
        };
        let occurred_at = "2026-03-06T12:00:00Z";
        let detected_at = "2026-03-06T12:00:01Z";

        let inserted_first = {
            let mut tx = state.pool.begin().await.expect("begin first tx");
            let inserted = insert_social_activity_event_tx(
                &mut tx,
                SocialActivityEventInsert {
                    user_id: user_id.as_str(),
                    kind: "follower_received",
                    repo_id: None,
                    repo_full_name: None,
                    repo_visual: None,
                    actor: &actor,
                    occurred_at,
                    detected_at,
                },
            )
            .await
            .expect("insert first follower event");
            tx.commit().await.expect("commit first tx");
            inserted
        };
        assert!(inserted_first);

        let inserted_second = {
            let mut tx = state.pool.begin().await.expect("begin second tx");
            let inserted = insert_social_activity_event_tx(
                &mut tx,
                SocialActivityEventInsert {
                    user_id: user_id.as_str(),
                    kind: "follower_received",
                    repo_id: None,
                    repo_full_name: None,
                    repo_visual: None,
                    actor: &actor,
                    occurred_at,
                    detected_at,
                },
            )
            .await
            .expect("insert duplicate follower event");
            tx.commit().await.expect("commit second tx");
            inserted
        };
        assert!(!inserted_second);

        let history_count: i64 =
            sqlx::query_scalar(r#"SELECT COUNT(*) FROM social_activity_events WHERE user_id = ?"#)
                .bind(user_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("count deduped follower history");
        assert_eq!(history_count, 1);
    }

    #[tokio::test]
    async fn collect_repo_stargazer_snapshots_skips_failed_repo_without_aborting() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        let repos = vec![
            OwnedRepoSnapshot {
                repo_id: 42,
                full_name: "octo/fail".to_owned(),
                owner_avatar_url: None,
                open_graph_image_url: None,
                uses_custom_open_graph_image: false,
            },
            OwnedRepoSnapshot {
                repo_id: 43,
                full_name: "octo/pass".to_owned(),
                owner_avatar_url: None,
                open_graph_image_url: None,
                uses_custom_open_graph_image: false,
            },
        ];

        let result =
            collect_repo_stargazer_snapshots_with(state.as_ref(), "token", &repos, |_, _, repo| {
                Box::pin(async move {
                    if repo.repo_id == 42 {
                        return Err(anyhow::anyhow!("repo unavailable"));
                    }
                    Ok(vec![RepoStargazerSnapshot {
                        repo_id: repo.repo_id,
                        repo_full_name: repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 501,
                            login: "good-cat".to_owned(),
                            avatar_url: Some("https://avatars.example/good-cat.png".to_owned()),
                            html_url: Some("https://github.com/good-cat".to_owned()),
                        },
                        starred_at: Some("2026-03-06T13:00:00Z".to_owned()),
                    }])
                })
            })
            .await;

        assert_eq!(result.repo_stars, 1);
        assert_eq!(result.failed_repos, vec!["octo/fail"]);
        assert_eq!(result.repo_members.len(), 1);
        assert_eq!(result.repo_members[0].0.full_name, "octo/pass");
        assert_eq!(result.repo_members[0].1.len(), 1);
    }

    #[tokio::test]
    async fn collect_repo_stargazer_snapshots_fetches_multiple_repos_concurrently() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        let repos = vec![
            OwnedRepoSnapshot {
                repo_id: 42,
                full_name: "octo/alpha".to_owned(),
                owner_avatar_url: None,
                open_graph_image_url: None,
                uses_custom_open_graph_image: false,
            },
            OwnedRepoSnapshot {
                repo_id: 43,
                full_name: "octo/beta".to_owned(),
                owner_avatar_url: None,
                open_graph_image_url: None,
                uses_custom_open_graph_image: false,
            },
        ];
        let barrier = Arc::new(tokio::sync::Barrier::new(2));

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            collect_repo_stargazer_snapshots_with(
                state.as_ref(),
                "token",
                &repos,
                move |_, _, repo| {
                    let barrier = barrier.clone();
                    Box::pin(async move {
                        barrier.wait().await;
                        Ok(vec![RepoStargazerSnapshot {
                            repo_id: repo.repo_id,
                            repo_full_name: repo.full_name.clone(),
                            actor: GitHubActor {
                                id: 700 + repo.repo_id,
                                login: format!("cat-{}", repo.repo_id),
                                avatar_url: Some(format!(
                                    "https://avatars.example/cat-{}.png",
                                    repo.repo_id
                                )),
                                html_url: Some(format!("https://github.com/cat-{}", repo.repo_id)),
                            },
                            starred_at: Some("2026-03-06T15:00:00Z".to_owned()),
                        }])
                    })
                },
            ),
        )
        .await;

        assert!(
            result.is_ok(),
            "expected concurrent repo fetch to satisfy barrier"
        );
        let result = result.expect("repo fetch result");
        assert_eq!(result.repo_stars, 2);
        assert!(result.failed_repos.is_empty());
        assert_eq!(result.repo_members.len(), 2);
        assert_eq!(result.repo_members[0].0.full_name, "octo/alpha");
        assert_eq!(result.repo_members[1].0.full_name, "octo/beta");
    }

    #[tokio::test]
    async fn social_activity_failed_repo_bootstrap_emits_events_on_first_successful_recovery() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-failed-repo-bootstrap");
        seed_user(&pool, user_id.as_str()).await;

        let stable_repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/alpha".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let skipped_repo = OwnedRepoSnapshot {
            repo_id: 43,
            full_name: "octo/beta".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };

        apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            &[stable_repo.clone(), skipped_repo.clone()],
            &[(stable_repo.clone(), vec![])],
            &[],
        )
        .await
        .expect("seed partial bootstrap state");

        let seeded_baseline_state: i64 = sqlx::query_scalar(
            r#"
            SELECT members_snapshot_initialized
            FROM owned_repo_star_baselines
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(skipped_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("load skipped repo baseline after partial bootstrap");
        assert_eq!(seeded_baseline_state, 0);

        let recovery_events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            &[stable_repo.clone(), skipped_repo.clone()],
            &[
                (stable_repo, vec![]),
                (
                    skipped_repo.clone(),
                    vec![RepoStargazerSnapshot {
                        repo_id: skipped_repo.repo_id,
                        repo_full_name: skipped_repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 801,
                            login: "existing-star".to_owned(),
                            avatar_url: Some(
                                "https://avatars.example/existing-star.png".to_owned(),
                            ),
                            html_url: Some("https://github.com/existing-star".to_owned()),
                        },
                        starred_at: Some("2026-03-06T15:30:00Z".to_owned()),
                    }],
                ),
            ],
            &[],
        )
        .await
        .expect("recover skipped repo snapshot");

        assert_eq!(recovery_events, 1);

        let baseline_state: i64 = sqlx::query_scalar(
            r#"
            SELECT members_snapshot_initialized
            FROM owned_repo_star_baselines
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(skipped_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("load skipped repo baseline state");
        assert_eq!(baseline_state, 1);

        let history_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM social_activity_events
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(skipped_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("count skipped repo history");
        assert_eq!(history_count, 1);

        let current_member_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM repo_star_current_members
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(skipped_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("count skipped repo current members after recovery");
        assert_eq!(current_member_count, 1);

        let new_star_events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&skipped_repo),
            &[(
                skipped_repo.clone(),
                vec![
                    RepoStargazerSnapshot {
                        repo_id: skipped_repo.repo_id,
                        repo_full_name: skipped_repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 801,
                            login: "existing-star".to_owned(),
                            avatar_url: Some(
                                "https://avatars.example/existing-star.png".to_owned(),
                            ),
                            html_url: Some("https://github.com/existing-star".to_owned()),
                        },
                        starred_at: Some("2026-03-06T15:30:00Z".to_owned()),
                    },
                    RepoStargazerSnapshot {
                        repo_id: skipped_repo.repo_id,
                        repo_full_name: skipped_repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 802,
                            login: "fresh-star".to_owned(),
                            avatar_url: Some("https://avatars.example/fresh-star.png".to_owned()),
                            html_url: Some("https://github.com/fresh-star".to_owned()),
                        },
                        starred_at: Some("2026-03-06T16:00:00Z".to_owned()),
                    },
                ],
            )],
            &[],
        )
        .await
        .expect("record fresh star after recovery");
        assert_eq!(new_star_events, 1);

        let row: (String, String) = sqlx::query_as(
            r#"
            SELECT repo_full_name, actor_login
            FROM social_activity_events
            WHERE user_id = ? AND repo_id = ?
            ORDER BY occurred_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .bind(skipped_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("load skipped repo history");
        assert_eq!(row.0, "octo/beta");
        assert_eq!(row.1, "fresh-star");
    }

    #[tokio::test]
    async fn social_activity_new_repo_after_initial_baseline_emits_current_star_events() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-new-repo");
        seed_user(&pool, user_id.as_str()).await;

        let initial_repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/alpha".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let new_repo = OwnedRepoSnapshot {
            repo_id: 43,
            full_name: "octo/beta".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let initial_repos = vec![initial_repo.clone()];
        let initial_repo_members = vec![(initial_repo, vec![])];

        apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            &initial_repos,
            &initial_repo_members,
            &[],
        )
        .await
        .expect("seed initial repo baseline");

        let next_repos = vec![new_repo.clone()];
        let next_repo_members = vec![(
            new_repo.clone(),
            vec![RepoStargazerSnapshot {
                repo_id: new_repo.repo_id,
                repo_full_name: new_repo.full_name.clone(),
                actor: GitHubActor {
                    id: 601,
                    login: "new-star".to_owned(),
                    avatar_url: Some("https://avatars.example/new-star.png".to_owned()),
                    html_url: Some("https://github.com/new-star".to_owned()),
                },
                starred_at: Some("2026-03-06T14:00:00Z".to_owned()),
            }],
        )];
        let events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            &next_repos,
            &next_repo_members,
            &[],
        )
        .await
        .expect("emit new repo star event");

        assert_eq!(events, 1);

        let row: (String, String) = sqlx::query_as(
            r#"
            SELECT repo_full_name, actor_login
            FROM social_activity_events
            WHERE user_id = ? AND kind = 'repo_star_received'
            ORDER BY occurred_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load new repo star event");
        assert_eq!(row.0, "octo/beta");
        assert_eq!(row.1, "new-star");
    }

    #[tokio::test]
    async fn social_activity_zero_repo_bootstrap_allows_first_repo_star_events() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-zero-repo-bootstrap");
        seed_user(&pool, user_id.as_str()).await;

        apply_social_activity_snapshot(state.as_ref(), user_id.as_str(), &[], &[], &[])
            .await
            .expect("seed zero-repo baseline");

        let repo_tracking_baseline_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM repo_star_sync_baselines
            WHERE user_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count repo star sync baseline");
        assert_eq!(repo_tracking_baseline_count, 1);

        let first_repo = OwnedRepoSnapshot {
            repo_id: 44,
            full_name: "octo/first".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&first_repo),
            &[(
                first_repo.clone(),
                vec![RepoStargazerSnapshot {
                    repo_id: first_repo.repo_id,
                    repo_full_name: first_repo.full_name.clone(),
                    actor: GitHubActor {
                        id: 901,
                        login: "first-star".to_owned(),
                        avatar_url: Some("https://avatars.example/first-star.png".to_owned()),
                        html_url: Some("https://github.com/first-star".to_owned()),
                    },
                    starred_at: Some("2026-03-06T14:45:00Z".to_owned()),
                }],
            )],
            &[],
        )
        .await
        .expect("emit first repo star event after zero-repo bootstrap");

        assert_eq!(events, 1);

        let row: (String, String) = sqlx::query_as(
            r#"
            SELECT repo_full_name, actor_login
            FROM social_activity_events
            WHERE user_id = ? AND kind = 'repo_star_received'
            ORDER BY occurred_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load first repo star event");
        assert_eq!(row.0, "octo/first");
        assert_eq!(row.1, "first-star");
    }

    #[tokio::test]
    async fn social_activity_new_repo_retry_after_failed_first_fetch_emits_current_star_events() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-new-repo-retry");
        seed_user(&pool, user_id.as_str()).await;

        let initial_repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/alpha".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let new_repo = OwnedRepoSnapshot {
            repo_id: 43,
            full_name: "octo/beta".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };

        apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&initial_repo),
            &[(initial_repo.clone(), vec![])],
            &[],
        )
        .await
        .expect("seed initial repo baseline");

        let failed_retry_events = apply_social_activity_snapshot_partial(
            state.as_ref(),
            user_id.as_str(),
            Some(&[initial_repo.clone(), new_repo.clone()]),
            Some(&[(initial_repo.clone(), vec![])]),
            Some(&[]),
        )
        .await
        .expect("record post-baseline repo fetch failure");
        assert_eq!(failed_retry_events, 0);

        let pending_repo_baseline_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM owned_repo_star_baselines
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(new_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("count pending repo baseline");
        assert_eq!(pending_repo_baseline_count, 0);

        let recovered_events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            &[initial_repo.clone(), new_repo.clone()],
            &[
                (initial_repo, vec![]),
                (
                    new_repo.clone(),
                    vec![RepoStargazerSnapshot {
                        repo_id: new_repo.repo_id,
                        repo_full_name: new_repo.full_name.clone(),
                        actor: GitHubActor {
                            id: 701,
                            login: "recovered-star".to_owned(),
                            avatar_url: Some(
                                "https://avatars.example/recovered-star.png".to_owned(),
                            ),
                            html_url: Some("https://github.com/recovered-star".to_owned()),
                        },
                        starred_at: Some("2026-03-06T14:30:00Z".to_owned()),
                    }],
                ),
            ],
            &[],
        )
        .await
        .expect("emit recovered new repo star event");

        assert_eq!(recovered_events, 1);

        let row: (String, String) = sqlx::query_as(
            r#"
            SELECT repo_full_name, actor_login
            FROM social_activity_events
            WHERE user_id = ? AND kind = 'repo_star_received'
            ORDER BY occurred_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load recovered repo star event");
        assert_eq!(row.0, "octo/beta");
        assert_eq!(row.1, "recovered-star");
    }

    #[tokio::test]
    async fn social_activity_drops_tracking_for_repos_that_are_no_longer_owned() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let user_id = test_user_id("social-reowned-repo");
        seed_user(&pool, user_id.as_str()).await;

        let stable_repo = OwnedRepoSnapshot {
            repo_id: 42,
            full_name: "octo/stable".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let churn_repo = OwnedRepoSnapshot {
            repo_id: 43,
            full_name: "octo/churn".to_owned(),
            owner_avatar_url: None,
            open_graph_image_url: None,
            uses_custom_open_graph_image: false,
        };
        let churn_member = RepoStargazerSnapshot {
            repo_id: churn_repo.repo_id,
            repo_full_name: churn_repo.full_name.clone(),
            actor: GitHubActor {
                id: 901,
                login: "returning-star".to_owned(),
                avatar_url: Some("https://avatars.example/returning-star.png".to_owned()),
                html_url: Some("https://github.com/returning-star".to_owned()),
            },
            starred_at: Some("2026-03-06T16:00:00Z".to_owned()),
        };

        apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            &[stable_repo.clone(), churn_repo.clone()],
            &[
                (stable_repo.clone(), vec![]),
                (churn_repo.clone(), vec![churn_member.clone()]),
            ],
            &[],
        )
        .await
        .expect("seed initial repo ownership");

        let removed_events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            std::slice::from_ref(&stable_repo),
            &[(stable_repo.clone(), vec![])],
            &[],
        )
        .await
        .expect("remove churn repo from ownership");
        assert_eq!(removed_events, 0);

        let churn_baseline_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM owned_repo_star_baselines
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(churn_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("count removed churn baseline");
        assert_eq!(churn_baseline_count, 0);

        let churn_member_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM repo_star_current_members
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(churn_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("count removed churn current members");
        assert_eq!(churn_member_count, 0);

        let reacquired_events = apply_social_activity_snapshot(
            state.as_ref(),
            user_id.as_str(),
            &[stable_repo.clone(), churn_repo.clone()],
            &[
                (stable_repo, vec![]),
                (churn_repo.clone(), vec![churn_member]),
            ],
            &[],
        )
        .await
        .expect("reacquire churn repo with fresh tracking");
        assert_eq!(reacquired_events, 0);

        let churn_history_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM social_activity_events
            WHERE user_id = ? AND repo_id = ?
            "#,
        )
        .bind(user_id.as_str())
        .bind(churn_repo.repo_id)
        .fetch_one(&pool)
        .await
        .expect("count churn repo history");
        assert_eq!(churn_history_count, 1);
    }

    #[tokio::test]
    async fn wait_for_release_demand_excludes_failed_work_item_release_totals() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-06T00:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO repo_release_work_items (
              id,
              repo_id,
              repo_full_name,
              status,
              request_origin,
              priority,
              has_new_repo_watchers,
              deadline_at,
              last_release_count,
              last_candidate_failures,
              last_success_at,
              error_text,
              created_at,
              started_at,
              finished_at,
              updated_at,
              runtime_owner_id,
              lease_heartbeat_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("repo-work-failed-1")
        .bind(42_i64)
        .bind("octo/alpha")
        .bind(jobs::STATUS_FAILED)
        .bind(RepoReleaseOrigin::Interactive.as_str())
        .bind(RepoReleaseOrigin::Interactive.priority())
        .bind(0_i64)
        .bind(repo_release_deadline_at(
            chrono::DateTime::parse_from_rfc3339(now)
                .expect("parse now")
                .with_timezone(&chrono::Utc),
            RepoReleaseOrigin::Interactive,
        ))
        .bind(7_i64)
        .bind(2_i64)
        .bind(Some(now))
        .bind(Some("boom"))
        .bind(now)
        .bind(Some(now))
        .bind(Some(now))
        .bind(now)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("seed failed repo release work item");

        let result =
            wait_for_release_demand(state.as_ref(), None, &["repo-work-failed-1".to_owned()])
                .await
                .expect("wait for release demand");

        assert_eq!(result.failed, 1);
        assert_eq!(result.releases, 0);
        assert_eq!(result.candidate_failures, 0);
    }

    #[tokio::test]
    async fn attach_and_wait_release_demand_reuses_fresh_cache_and_emits_progress() {
        let pool = setup_pool().await;
        let user_id = test_user_id("9");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());
        seed_sync_task(&state, "task-access-fresh").await;

        let now = chrono::Utc::now();
        let now_rfc3339 = now.to_rfc3339();
        let deadline_at = repo_release_deadline_at(now, RepoReleaseOrigin::System);

        sqlx::query(
            r#"
            INSERT INTO starred_repos (
              id,
              user_id,
              repo_id,
              full_name,
              owner_login,
              name,
              description,
              html_url,
              stargazed_at,
              is_private,
              updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("star-shared-release")
        .bind(user_id.as_str())
        .bind(42_i64)
        .bind("octo/alpha")
        .bind("octo")
        .bind("alpha")
        .bind(Option::<String>::None)
        .bind("https://github.com/octo/alpha")
        .bind(now_rfc3339.as_str())
        .bind(0_i64)
        .bind(now_rfc3339.as_str())
        .execute(&pool)
        .await
        .expect("seed starred repo");

        sqlx::query(
            r#"
            INSERT INTO repo_release_work_items (
              id,
              repo_id,
              repo_full_name,
              status,
              request_origin,
              priority,
              has_new_repo_watchers,
              deadline_at,
              last_release_count,
              last_candidate_failures,
              last_success_at,
              error_text,
              created_at,
              started_at,
              finished_at,
              updated_at,
              runtime_owner_id,
              lease_heartbeat_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("repo-work-fresh-1")
        .bind(42_i64)
        .bind("octo/alpha")
        .bind(jobs::STATUS_SUCCEEDED)
        .bind(RepoReleaseOrigin::System.as_str())
        .bind(RepoReleaseOrigin::System.priority())
        .bind(0_i64)
        .bind(deadline_at.as_str())
        .bind(7_i64)
        .bind(0_i64)
        .bind(Some(now_rfc3339.as_str()))
        .bind(Option::<String>::None)
        .bind(now_rfc3339.as_str())
        .bind(Some(now_rfc3339.as_str()))
        .bind(Some(now_rfc3339.as_str()))
        .bind(now_rfc3339.as_str())
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("seed fresh repo release work item");

        let result = attach_and_wait_for_user_release_demand(
            state.as_ref(),
            Some(("task-access-fresh", HashSet::new())),
            user_id.as_str(),
            RepoReleaseOrigin::Interactive,
            "access_refresh",
        )
        .await
        .expect("attach and wait for release demand");

        assert_eq!(result.repos, 1);
        assert_eq!(result.releases, 7);
        assert_eq!(result.reused_fresh, 1);
        assert_eq!(result.reused_running, 0);
        assert_eq!(result.queued, 0);
        assert_eq!(result.failed, 0);

        let watcher = sqlx::query_as::<_, (String, i64, String, String)>(
            r#"
            SELECT status, is_new_repo, origin, reason
            FROM repo_release_watchers
            WHERE task_id = ? AND work_item_id = ?
            LIMIT 1
            "#,
        )
        .bind("task-access-fresh")
        .bind("repo-work-fresh-1")
        .fetch_one(&pool)
        .await
        .expect("load repo release watcher");
        assert_eq!(watcher.0, "succeeded");
        assert_eq!(watcher.1, 1);
        assert_eq!(watcher.2, RepoReleaseOrigin::Interactive.as_str());
        assert_eq!(watcher.3, "access_refresh");

        let progress_payload = sqlx::query_scalar::<_, String>(
            r#"
            SELECT payload_json
            FROM job_task_events
            WHERE task_id = ? AND event_type = 'task.progress'
            ORDER BY rowid DESC
            LIMIT 1
            "#,
        )
        .bind("task-access-fresh")
        .fetch_one(&pool)
        .await
        .expect("load task progress payload");
        let progress: serde_json::Value =
            serde_json::from_str(&progress_payload).expect("parse progress payload");
        assert_eq!(
            progress.get("stage").and_then(serde_json::Value::as_str),
            Some("release_attached")
        );
        assert_eq!(
            progress
                .get("reused_fresh")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            progress.get("queued").and_then(serde_json::Value::as_u64),
            Some(0)
        );
    }

    #[tokio::test]
    async fn attach_release_demand_promotes_system_queue_for_interactive_new_repo() {
        let pool = setup_pool().await;
        let user_id = test_user_id("10");
        seed_user(&pool, user_id.as_str()).await;
        let state = setup_state(pool.clone());
        seed_sync_task(&state, "task-access-promote").await;

        let now = chrono::Utc::now();
        let now_rfc3339 = now.to_rfc3339();
        let stale_success = (now - chrono::Duration::hours(2)).to_rfc3339();
        let original_deadline_at = repo_release_deadline_at(now, RepoReleaseOrigin::System);

        sqlx::query(
            r#"
            INSERT INTO repo_release_work_items (
              id,
              repo_id,
              repo_full_name,
              status,
              request_origin,
              priority,
              has_new_repo_watchers,
              deadline_at,
              last_release_count,
              last_candidate_failures,
              last_success_at,
              error_text,
              created_at,
              started_at,
              finished_at,
              updated_at,
              runtime_owner_id,
              lease_heartbeat_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("repo-work-promote-1")
        .bind(43_i64)
        .bind("octo/beta")
        .bind(jobs::STATUS_QUEUED)
        .bind(RepoReleaseOrigin::System.as_str())
        .bind(RepoReleaseOrigin::System.priority())
        .bind(0_i64)
        .bind(original_deadline_at.as_str())
        .bind(0_i64)
        .bind(0_i64)
        .bind(Some(stale_success.as_str()))
        .bind(Option::<String>::None)
        .bind(now_rfc3339.as_str())
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(now_rfc3339.as_str())
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .execute(&pool)
        .await
        .expect("seed queued repo release work item");

        let attached = attach_release_demand(
            state.as_ref(),
            Some("task-access-promote"),
            Some(user_id.as_str()),
            &[ReleaseDemandRepo {
                repo_id: 43,
                full_name: "octo/beta".to_owned(),
                is_new_repo: true,
            }],
            RepoReleaseOrigin::Interactive,
            "access_refresh",
        )
        .await
        .expect("attach release demand");

        assert_eq!(attached.repos, 1);
        assert_eq!(attached.queued, 1);
        assert_eq!(attached.reused_running, 0);
        assert_eq!(attached.reused_fresh, 0);

        let row = sqlx::query_as::<_, (String, String, i64, i64, String)>(
            r#"
            SELECT status, request_origin, priority, has_new_repo_watchers, deadline_at
            FROM repo_release_work_items
            WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind("repo-work-promote-1")
        .fetch_one(&pool)
        .await
        .expect("load promoted repo release work item");

        assert_eq!(row.0, jobs::STATUS_QUEUED);
        assert_eq!(row.1, RepoReleaseOrigin::Interactive.as_str());
        assert_eq!(row.2, RepoReleaseOrigin::Interactive.priority());
        assert_eq!(row.3, 1);
        assert!(
            row.4 < original_deadline_at,
            "interactive demand should tighten the repo deadline"
        );

        let watcher = sqlx::query_as::<_, (String, String, i64, i64)>(
            r#"
            SELECT status, origin, priority, is_new_repo
            FROM repo_release_watchers
            WHERE task_id = ? AND work_item_id = ?
            LIMIT 1
            "#,
        )
        .bind("task-access-promote")
        .bind("repo-work-promote-1")
        .fetch_one(&pool)
        .await
        .expect("load watcher after promotion");

        assert_eq!(watcher.0, "pending");
        assert_eq!(watcher.1, RepoReleaseOrigin::Interactive.as_str());
        assert_eq!(watcher.2, RepoReleaseOrigin::Interactive.priority());
        assert_eq!(watcher.3, 1);
    }

    #[tokio::test]
    async fn enqueue_background_release_ai_tasks_include_smart_preheat() {
        let pool = setup_pool().await;
        let mut state = setup_state(pool.clone());
        Arc::get_mut(&mut state).expect("unique state").config.ai = Some(crate::config::AiConfig {
            base_url: url::Url::parse("https://example.invalid/v1").expect("parse ai url"),
            model: "gpt-test".to_owned(),
            api_key: "test-key".to_owned(),
        });

        let user_id = test_user_id("11");
        seed_user(&pool, user_id.as_str()).await;

        super::enqueue_background_release_translation_task(
            state.as_ref(),
            user_id.as_str(),
            &[101, 102],
            "sync.releases.auto_translate",
            None,
            Some(user_id.as_str()),
        )
        .await
        .expect("enqueue translation preheat");
        super::enqueue_background_release_smart_task(
            state.as_ref(),
            user_id.as_str(),
            &[101, 102],
            "sync.releases.auto_smart",
            None,
            Some(user_id.as_str()),
        )
        .await
        .expect("enqueue smart preheat");

        let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
            r#"
            SELECT task_type, source, payload_json, requested_by
            FROM job_tasks
            ORDER BY rowid ASC
            "#,
        )
        .fetch_all(&pool)
        .await
        .expect("load enqueued tasks");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, jobs::TASK_TRANSLATE_RELEASE_BATCH);
        assert_eq!(rows[0].1, "sync.releases.auto_translate");
        assert_eq!(rows[0].3.as_deref(), Some(user_id.as_str()));
        assert_eq!(rows[1].0, jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH);
        assert_eq!(rows[1].1, "sync.releases.auto_smart");
        assert_eq!(rows[1].3.as_deref(), Some(user_id.as_str()));
        let expected_payload = serde_json::json!({
            "user_id": user_id.as_str(),
            "release_ids": [101, 102],
        });
        let first_payload: serde_json::Value =
            serde_json::from_str(&rows[0].2).expect("parse translation payload");
        let second_payload: serde_json::Value =
            serde_json::from_str(&rows[1].2).expect("parse smart payload");
        assert_eq!(first_payload, expected_payload);
        assert_eq!(second_payload, expected_payload);
    }

    #[test]
    fn merge_smart_preheat_release_ids_keeps_newest_and_extends_recent_window() {
        let merged =
            super::merge_smart_preheat_release_ids(&[105, 104, 103], &[104, 103, 102, 101, 100]);
        assert_eq!(merged, vec![105, 104, 103, 102, 101, 100]);
    }

    #[test]
    fn skipped_subscription_result_includes_social_and_notifications_defaults() {
        let result = super::skipped_subscription_result("2026-03-06T14:30", "previous_run_active");
        assert_eq!(result.get("skipped").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result
                .get("social")
                .and_then(|value| value.get("total_users"))
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            result
                .get("notifications")
                .and_then(|value| value.get("notifications"))
                .and_then(Value::as_u64),
            Some(0)
        );
    }

    #[test]
    fn sync_access_refresh_result_serializes_optional_notifications_error() {
        let value = serde_json::to_value(super::SyncAccessRefreshResult {
            starred: super::SyncStarredResult { repos: 2 },
            release: super::SharedReleaseDemandResult {
                repos: 2,
                releases: 5,
                ..super::SharedReleaseDemandResult::default()
            },
            social: super::SyncSocialActivityResult::default(),
            social_error: None,
            notifications: super::SyncNotificationsResult {
                notifications: 0,
                since: None,
            },
            notifications_error: Some("notifications unavailable".to_owned()),
        })
        .expect("serialize access refresh result");

        assert_eq!(
            value.get("notifications_error"),
            Some(&json!("notifications unavailable"))
        );
    }

    #[tokio::test]
    async fn sync_subscriptions_without_eligible_users_emits_social_and_notifications_summaries() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_sync_task(&state, "task-sync-subscriptions-empty").await;

        let result = super::sync_subscriptions(
            state.as_ref(),
            "task-sync-subscriptions-empty",
            &json!({
                "trigger": "schedule",
                "schedule_key": "2026-03-06T14:30",
            }),
        )
        .await
        .expect("run sync subscriptions");

        assert_eq!(result.star.total_users, 0);
        assert_eq!(result.social.total_users, 0);
        assert_eq!(result.notifications.notifications, 0);

        let stages = sqlx::query_scalar::<_, String>(
            r#"
            SELECT json_extract(payload_json, '$.stage')
            FROM job_task_events
            WHERE task_id = ? AND event_type = 'task.progress'
            ORDER BY rowid ASC
            "#,
        )
        .bind("task-sync-subscriptions-empty")
        .fetch_all(&pool)
        .await
        .expect("load task progress stages");

        assert_eq!(
            stages,
            vec![
                "collect".to_owned(),
                "star_summary".to_owned(),
                "repo_collect".to_owned(),
                "release_summary".to_owned(),
                "social_summary".to_owned(),
                "notifications_summary".to_owned(),
                "summary".to_owned(),
            ]
        );
    }

    async fn seed_sync_task(state: &Arc<AppState>, task_id: &str) {
        fs::create_dir_all(&state.config.task_log_dir).expect("create task log dir");
        let log_path = state.config.task_log_dir.join(format!("{task_id}.ndjson"));
        let now = "2026-03-06T00:00:00Z";
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
        .bind(task_id)
        .bind(jobs::TASK_SYNC_SUBSCRIPTIONS)
        .bind(jobs::STATUS_RUNNING)
        .bind("test")
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind("{}")
        .bind("{}")
        .bind(Option::<String>::None)
        .bind(0_i64)
        .bind(now)
        .bind(Some(now))
        .bind(Option::<String>::None)
        .bind(now)
        .bind(log_path.to_string_lossy().to_string())
        .execute(&state.pool)
        .await
        .expect("seed subscription task");
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
        crate::api::ensure_owned_repo_visual_columns(&pool)
            .await
            .expect("ensure owned repo visual columns");
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
            task_log_dir: std::env::temp_dir().join("octo-rill-task-logs-sync-tests"),
            job_worker_concurrency: 4,
            encryption_key: encryption_key.clone(),
            github: GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse github redirect"),
            },
            ai: None,
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: crate::briefs::DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned(),
        };
        let oauth = build_oauth_client(&config).expect("build oauth client");
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
            oauth,
            encryption_key,
            runtime_owner_id: "sync-test-runtime-owner".to_owned(),
        })
    }

    async fn seed_user(pool: &SqlitePool, user_id: &str) {
        let now = "2026-03-06T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(user_id)
        .bind(30_215_105_i64 + i64::from(user_id.bytes().map(i16::from).sum::<i16>()))
        .bind(format!("user-{user_id}"))
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed user");
    }

    fn mock_notification(
        id: &str,
        subject_url: Option<&str>,
        repo_full_name: Option<&str>,
        subject_type: Option<&str>,
        updated_at: &str,
    ) -> GitHubNotification {
        GitHubNotification {
            id: id.to_owned(),
            unread: Some(true),
            reason: Some("state_change".to_owned()),
            updated_at: Some(updated_at.to_owned()),
            url: Some(format!("https://api.github.com/notifications/threads/{id}")),
            subject: NotificationSubject {
                title: Some(format!("Notification {id}")),
                subject_type: subject_type.map(str::to_owned),
                url: subject_url.map(str::to_owned),
            },
            repository: NotificationRepo {
                full_name: repo_full_name.map(str::to_owned),
            },
        }
    }
}
