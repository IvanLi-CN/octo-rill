use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    future::Future,
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
const SUBSCRIPTION_RETRY_LIMIT: usize = 3;
const SUBSCRIPTION_RETRY_BACKOFF_MS: [u64; 3] = [500, 1_000, 2_000];
const SUBSCRIPTION_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const REPO_RELEASE_WORKERS: usize = 5;
const REPO_RELEASE_QUEUE_POLL_INTERVAL: Duration = Duration::from_millis(450);
const REPO_RELEASE_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(150);
const REPO_RELEASE_FRESHNESS_WINDOW: Duration = Duration::from_secs(30 * 60);
const REPO_RELEASE_PRIORITY_SYSTEM: i64 = 1;
const REPO_RELEASE_PRIORITY_INTERACTIVE: i64 = 2;

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
}

#[derive(Debug, Serialize)]
pub struct SyncNotificationsResult {
    pub notifications: usize,
    pub since: Option<String>,
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

#[derive(Debug, Serialize, Clone)]
pub struct SyncSubscriptionsResult {
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub star: SyncSubscriptionStarSummary,
    pub release: SyncSubscriptionReleaseSummary,
    pub releases_written: usize,
    pub critical_events: usize,
}

pub fn skipped_subscription_result(_schedule_key: &str, skip_reason: &str) -> Value {
    json!({
        "skipped": true,
        "skip_reason": skip_reason,
        "star": SyncSubscriptionStarSummary::default(),
        "release": SyncSubscriptionReleaseSummary::default(),
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
struct Viewer {
    #[serde(rename = "starredRepositories")]
    starred_repositories: StarredRepositories,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StarredRepositories {
    page_info: PageInfo,
    edges: Vec<StarredEdge>,
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
struct RepoNode {
    database_id: Option<i64>,
    name_with_owner: String,
    name: String,
    description: Option<String>,
    url: String,
    is_private: bool,
    owner: RepoOwner,
}

#[derive(Debug, Deserialize)]
struct RepoOwner {
    login: String,
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
    unread: bool,
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
    html_url: Option<String>,
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
    let demand = attach_and_wait_for_user_release_demand(
        state,
        None,
        user_id,
        RepoReleaseOrigin::Interactive,
        "manual_release_sync",
    )
    .await?;

    Ok(SyncReleasesResult {
        repos: demand.repos,
        releases: demand.releases,
    })
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

    Ok(SyncAccessRefreshResult { starred, release })
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
          COALESCE(SUM(last_release_count), 0) AS release_count,
          COALESCE(SUM(last_candidate_failures), 0) AS candidate_failures
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

    let result = SyncSubscriptionsResult {
        skipped: false,
        skip_reason: None,
        star: star_summary,
        release: release_summary,
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
                  deadline_at = NULL,
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
                  deadline_at = NULL,
                  error_text = ?,
                  finished_at = ?,
                  updated_at = ?,
                  runtime_owner_id = NULL,
                  lease_heartbeat_at = NULL
                WHERE id = ?
                "#,
            )
            .bind(jobs::STATUS_FAILED)
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
              deadline_at = NULL,
              error_text = ?,
              finished_at = ?,
              updated_at = ?,
              runtime_owner_id = NULL,
              lease_heartbeat_at = NULL
            WHERE id = ?
            "#,
        )
        .bind(jobs::STATUS_FAILED)
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
                owner { login }
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
            all.push(StarredRepoSnapshot {
                repo_id,
                full_name: edge.node.name_with_owner,
                owner_login: edge.node.owner.login,
                name: edge.node.name,
                description: edge.node.description,
                html_url: edge.node.url,
                stargazed_at: edge.starred_at,
                is_private: edge.node.is_private,
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
              id, user_id, repo_id, full_name, owner_login, name, description, html_url, stargazed_at, is_private, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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

    let since_key = "notifications_since";
    let since = sqlx::query_scalar::<_, String>(
        r#"SELECT value FROM sync_state WHERE user_id = ? AND key = ?"#,
    )
    .bind(user_id)
    .bind(since_key)
    .fetch_optional(&state.pool)
    .await
    .context("failed to query notifications since")?;

    let mut url = format!("{REST_API_BASE}/notifications?all=true&per_page=50");
    if let Some(ref since) = since {
        url.push_str("&since=");
        url.push_str(&urlencoding::encode(since));
    }

    let res = state
        .http
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
        .context("github notifications json decode failed")?;

    let now = chrono::Utc::now().to_rfc3339();
    for n in &res {
        sqlx::query(
            r#"
            INSERT INTO notifications (
              id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason,
              updated_at, unread, url, html_url, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(&n.id)
        .bind(n.repository.full_name.as_deref())
        .bind(n.subject.title.as_deref())
        .bind(n.subject.subject_type.as_deref())
        .bind(n.reason.as_deref())
        .bind(n.updated_at.as_deref())
        .bind(n.unread as i64)
        .bind(n.url.as_deref().or(n.subject.url.as_deref()))
        .bind(n.repository.html_url.as_deref())
        .bind(&now)
        .execute(&state.pool)
        .await
        .context("failed to upsert notification")?;
    }

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
    .bind(since_key)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await
    .context("failed to update notifications since")?;

    Ok(SyncNotificationsResult {
        notifications: res.len(),
        since,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering as AtomicTestOrdering},
        },
    };

    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use url::Url;

    use super::{
        EligibleUserRow, StarPhaseSuccess, StarredRepoSnapshot, SubscriptionRunContext,
        SyncRequestError, aggregate_repos, cmp_last_active_desc,
        subscription_event_counts_as_critical, subscription_timeout_error,
        sync_starred_for_user_with_fetch,
    };
    use crate::{
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        jobs,
        state::{AppState, build_oauth_client},
    };

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
            ai_model_context_limit: None,
            ai_daily_at_local: None,
        };
        let oauth = build_oauth_client(&config).expect("build oauth client");
        Arc::new(AppState {
            llm_scheduler: Arc::new(crate::ai::LlmScheduler::new(config.ai_max_concurrency)),
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
}
