//! PostgreSQL executor for single-repository release refresh.
//!
//! Mirrors the legacy `repo_release_work_items` lane (`crate::sync`) for one
//! repository at a time: claim the shared work item, read the repository's
//! releases through the shared credential-rotation decision
//! (`crate::postgres_repo_release_fetch`), persist the shared `repo_releases`
//! cache, record `repo_release_sync_state`, and update the work item plus its
//! watchers. Newly discovered releases are flagged for the content pipeline
//! with the legacy pending marker.
//!
//! Durable task lifecycle stays in `crate::postgres_worker`; this module only
//! owns the business writes for one `sync.repo_release` task.

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool};
use tracing::{info, warn};

use crate::{
    config::AiConfig,
    crypto::EncryptionKey,
    local_id,
    postgres_jobs::ClaimedTask,
    postgres_repo_release_fetch::{
        AnonymousBudget, BudgetDeferral, FetchVia, PublicReleaseUsageAccess, RefreshFailure,
        ReleaseCredentialCache, ReleaseFetchError, ReleaseFetchOutcome, ReleaseFetchRequest,
        RepositoryFetch, fetch_releases, mark_public_usage_failed, mark_public_usage_ready,
        shared_anonymous_budget,
    },
    postgres_worker::TaskExecutor,
};

pub(crate) const TASK_TYPE: &str = "sync.repo_release";
/// Legacy marker carried by `repo_release_work_items.error_text` while the
/// releases named in `last_new_release_ids_json` still need content work.
pub(crate) const PENDING_CONTENT_MARKER: &str = "content_processing_enqueue_pending";
const RETRY_BACKOFF_MINUTES: i64 = 10;
/// Lease and hard deadline recorded on the work item while this task owns it.
/// The queued-claim deadline of the legacy lane is replaced by the task queue,
/// so the work item only needs to bound a single execution.
const WORK_ITEM_LEASE_MINUTES: i64 = 10;

#[derive(Clone)]
pub struct RepoReleaseRefreshExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
    /// Present only when automatic release translation can run; without it the
    /// refresh stores releases and queues no translation work.
    ai: Option<AiConfig>,
    /// The process-wide unauthenticated read allowance, shared with the
    /// account lane.
    budget: std::sync::Arc<AnonymousBudget>,
}

impl RepoReleaseRefreshExecutor {
    pub fn new(
        pool: PgPool,
        http: Client,
        encryption_key: EncryptionKey,
        ai: Option<AiConfig>,
    ) -> Self {
        Self {
            pool,
            http,
            encryption_key,
            ai,
            budget: shared_anonymous_budget(),
        }
    }
}

#[derive(Debug)]
struct RepoReleaseTaskPayload {
    user_id: String,
    repo_id: i64,
    repo_full_name: String,
    reason: String,
    delivery_id: Option<String>,
}

impl RepoReleaseTaskPayload {
    fn from_task(task: &ClaimedTask) -> Result<Self> {
        let payload: Value = serde_json::from_str(&task.payload_json)
            .context("decode sync.repo_release task payload")?;
        let user_id = payload
            .get("user_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| task.requested_by.clone().filter(|value| !value.is_empty()))
            .ok_or_else(|| anyhow!("sync.repo_release task has no user_id"))?;
        let repo_id = payload
            .get("repo_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("sync.repo_release task has no repo_id"))?;
        let repo_full_name = payload
            .get("repo_full_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("sync.repo_release task has no repo_full_name"))?;
        let reason = payload
            .get("reason")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("repo_release_refresh")
            .to_owned();
        let delivery_id = payload
            .get("delivery_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Ok(Self {
            user_id,
            repo_id,
            repo_full_name,
            reason,
            delivery_id,
        })
    }
}

#[derive(Debug, FromRow)]
struct WorkItemRow {
    id: String,
    status: String,
    runtime_owner_id: Option<String>,
    deadline_at: String,
}

#[derive(Debug, FromRow)]
struct ExistingReleaseRow {
    release_id: i64,
    node_id: Option<String>,
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    created_at: Option<String>,
    updated_at: String,
    is_prerelease: i64,
    is_draft: i64,
    react_plus1: i64,
    react_laugh: i64,
    react_heart: i64,
    react_hooray: i64,
    react_rocket: i64,
    react_eyes: i64,
}

/// One execution's claim on the shared `repo_release_work_items` row.
struct WorkItemLease {
    id: String,
    /// The `sync.repo_release` task id recorded as the work item's lease owner.
    task_id: String,
    started_at: String,
}

/// What a `sync.repo_release` task may do with the shared work item.
enum WorkItemClaim {
    /// This task owns the work item and must refresh the repository.
    Leased(WorkItemLease),
    /// Another task is already refreshing this repository; the legacy
    /// `reused_running` freshness decision applies instead of a second fetch.
    ReusedRunning { work_item_id: String },
}

#[derive(Debug, Default)]
struct RefreshStats {
    work_item_id: String,
    not_modified: bool,
    pages_fetched: usize,
    stopped_reason: String,
    fetched_count: usize,
    inserted_count: usize,
    updated_count: usize,
    unchanged_count: usize,
    new_release_ids: Vec<i64>,
    /// Access path that produced the read (`user`, `candidate` or `public`).
    via: Option<FetchVia>,
    candidate_user_id: Option<String>,
}

impl RefreshStats {
    fn into_result_json(
        self,
        payload: &RepoReleaseTaskPayload,
        translations_enqueued: usize,
    ) -> String {
        let content_pending = !self.new_release_ids.is_empty();
        json!({
            "task": TASK_TYPE,
            "user_id": payload.user_id,
            "repo_id": payload.repo_id,
            "repo_full_name": payload.repo_full_name,
            "reason": payload.reason,
            "delivery_id": payload.delivery_id,
            "work_item_id": self.work_item_id,
            "not_modified": self.not_modified,
            "pages_fetched": self.pages_fetched,
            "stopped_reason": self.stopped_reason,
            "fetched_count": self.fetched_count,
            "inserted_count": self.inserted_count,
            "updated_count": self.updated_count,
            "unchanged_count": self.unchanged_count,
            "new_release_ids": self.new_release_ids,
            "content_pending": content_pending,
            "translations_enqueued": translations_enqueued,
            "via": self.via.map(FetchVia::as_str),
        })
        .to_string()
    }
}

#[async_trait]
impl TaskExecutor for RepoReleaseRefreshExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != TASK_TYPE {
            bail!("repo release executor received {}", task.task_type);
        }
        let payload = RepoReleaseTaskPayload::from_task(task)?;
        let claim = self
            .begin_work_item(task, &payload)
            .await
            .context("claim repo release work item")?;
        let lease = match claim {
            WorkItemClaim::Leased(lease) => lease,
            WorkItemClaim::ReusedRunning { work_item_id } => {
                info!(
                    event = "worker.repo_release",
                    task_id = task.id,
                    repo_id = payload.repo_id,
                    repo = payload.repo_full_name,
                    work_item_id,
                    "repository release refresh already running; reusing it"
                );
                return Ok(json!({
                    "task": TASK_TYPE,
                    "user_id": payload.user_id,
                    "repo_id": payload.repo_id,
                    "repo_full_name": payload.repo_full_name,
                    "reason": payload.reason,
                    "delivery_id": payload.delivery_id,
                    "work_item_id": work_item_id,
                    "reused_running": true,
                })
                .to_string());
            }
        };

        let result = self.refresh(&payload, &lease).await;
        match result {
            Ok(stats) => {
                info!(
                    event = "worker.repo_release",
                    task_id = task.id,
                    repo_id = payload.repo_id,
                    repo = payload.repo_full_name,
                    via = stats.via.map(FetchVia::as_str),
                    candidate_user_id = stats.candidate_user_id.as_deref(),
                    inserted = stats.inserted_count,
                    updated = stats.updated_count,
                    not_modified = stats.not_modified,
                    "single-repository release refresh completed"
                );
                let translations_enqueued = self.enqueue_translations(&payload, &stats).await;
                Ok(stats.into_result_json(&payload, translations_enqueued))
            }
            Err(ReleaseFetchError::Deferred(deferral)) => {
                if let Err(persist_error) = self.record_deferred(&payload, &lease, deferral).await {
                    warn!(
                        event = "worker.repo_release",
                        task_id = task.id,
                        repo_id = payload.repo_id,
                        error = %persist_error,
                        "failed to persist repo release deferral state"
                    );
                }
                info!(
                    event = "worker.repo_release",
                    task_id = task.id,
                    repo_id = payload.repo_id,
                    repo = payload.repo_full_name,
                    deferred_reason = "anonymous_budget",
                    anonymous_budget_remaining = deferral.remaining,
                    anonymous_budget_reset_at = deferral.reset_at_unix,
                    "repository release refresh deferred: anonymous GitHub budget exhausted"
                );
                Ok(json!({
                    "task": TASK_TYPE,
                    "user_id": payload.user_id,
                    "repo_id": payload.repo_id,
                    "repo_full_name": payload.repo_full_name,
                    "reason": payload.reason,
                    "delivery_id": payload.delivery_id,
                    "work_item_id": lease.id,
                    "deferred_budget": true,
                    "anonymous_budget_remaining": deferral.remaining,
                })
                .to_string())
            }
            Err(ReleaseFetchError::Failed(failure)) => {
                if let Err(persist_error) = self.record_failure(&payload, &lease, &failure).await {
                    warn!(
                        event = "worker.repo_release",
                        task_id = task.id,
                        repo_id = payload.repo_id,
                        error = %persist_error,
                        "failed to persist repo release failure state"
                    );
                }
                Err(anyhow!(failure.summary()))
            }
        }
    }
}

impl RepoReleaseRefreshExecutor {
    /// Queue automatic release translation for the releases this refresh
    /// inserted. `content_pending` keeps describing the stored new ids; the
    /// enqueue only turns them into `translation.generate` tasks.
    ///
    /// The refresh already committed before this runs, so a failure is reported
    /// and swallowed instead of failing the sync task. Without a configured AI
    /// provider the lane writes nothing.
    async fn enqueue_translations(
        &self,
        payload: &RepoReleaseTaskPayload,
        stats: &RefreshStats,
    ) -> usize {
        if self.ai.is_none() || stats.new_release_ids.is_empty() {
            return 0;
        }
        match crate::postgres_translate::enqueue_release_translations(
            &self.pool,
            &payload.user_id,
            &stats.new_release_ids,
            "sync.repo_release.auto_translate",
        )
        .await
        {
            Ok(count) => {
                info!(
                    event = "translation.enqueue",
                    operation = TASK_TYPE,
                    user_id = payload.user_id,
                    repo_id = payload.repo_id,
                    translations_enqueued = count,
                    "automatic release translation queued"
                );
                count
            }
            Err(error) => {
                warn!(
                    event = "translation.enqueue",
                    operation = TASK_TYPE,
                    user_id = payload.user_id,
                    repo_id = payload.repo_id,
                    error = %error,
                    "automatic release translation enqueue failed"
                );
                0
            }
        }
    }

    /// Upsert the shared work item and this task's watcher row.
    ///
    /// The `(task_id, work_item_id)` watcher row is the durable link between the
    /// queued `sync.repo_release` task and the repository refresh it performed.
    async fn begin_work_item(
        &self,
        task: &ClaimedTask,
        payload: &RepoReleaseTaskPayload,
    ) -> Result<WorkItemClaim> {
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let deadline_at = (now + chrono::Duration::minutes(WORK_ITEM_LEASE_MINUTES)).to_rfc3339();
        let mut transaction = self
            .pool
            .begin()
            .await
            .context("begin repo release work item claim")?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("repo-release-work-item:{}", payload.repo_id))
            .execute(&mut *transaction)
            .await
            .context("lock repo release work item")?;
        let existing = sqlx::query_as::<_, WorkItemRow>(
            "SELECT id, status, runtime_owner_id, deadline_at FROM public.repo_release_work_items WHERE repo_id = $1 FOR UPDATE",
        )
        .bind(payload.repo_id)
        .fetch_optional(&mut *transaction)
        .await
        .context("load repo release work item")?;
        if let Some(row) = existing.as_ref()
            && reuse_in_flight_run(row, &task.id, &now_text)
        {
            record_reused_running_watcher(
                &mut transaction,
                row.id.as_str(),
                task,
                payload,
                &now_text,
            )
            .await?;
            transaction
                .commit()
                .await
                .context("commit reused repo release work item claim")?;
            return Ok(WorkItemClaim::ReusedRunning {
                work_item_id: row.id.clone(),
            });
        }
        let (work_item_id, is_new_repo) = match existing {
            Some(row) => {
                let work_item_id = row.id;
                sqlx::query(
                    r#"
                    UPDATE public.repo_release_work_items
                    SET repo_full_name = $2,
                        status = 'running',
                        request_origin = 'interactive',
                        priority = GREATEST(priority, 2),
                        has_new_repo_watchers = 0,
                        deadline_at = $3,
                        error_text = NULL,
                        started_at = $4,
                        finished_at = NULL,
                        updated_at = $4,
                        runtime_owner_id = $5,
                        lease_heartbeat_at = $4
                    WHERE id = $1
                    "#,
                )
                .bind(&work_item_id)
                .bind(&payload.repo_full_name)
                .bind(&deadline_at)
                .bind(&now_text)
                .bind(&task.id)
                .execute(&mut *transaction)
                .await
                .context("mark repo release work item running")?;
                (work_item_id, false)
            }
            None => {
                let work_item_id = local_id::generate_local_id();
                sqlx::query(
                    r#"
                    INSERT INTO public.repo_release_work_items (
                      id, repo_id, repo_full_name, status, request_origin, priority,
                      has_new_repo_watchers, deadline_at, last_release_count,
                      last_candidate_failures, last_success_at, error_text, created_at,
                      started_at, finished_at, updated_at, runtime_owner_id, lease_heartbeat_at
                    )
                    VALUES ($1, $2, $3, 'running', 'interactive', 2, 0, $4, 0, 0, NULL, NULL, $5, $5, NULL, $5, $6, $5)
                    "#,
                )
                .bind(&work_item_id)
                .bind(payload.repo_id)
                .bind(&payload.repo_full_name)
                .bind(&deadline_at)
                .bind(&now_text)
                .bind(&task.id)
                .execute(&mut *transaction)
                .await
                .context("insert repo release work item")?;
                (work_item_id, true)
            }
        };
        sqlx::query(
            r#"
            INSERT INTO public.repo_release_watchers (
              id, work_item_id, task_id, user_id, origin, priority, reason, is_new_repo,
              status, error_text, created_at, updated_at, reused_fresh
            )
            VALUES ($1, $2, $3, $4, 'interactive', 2, $5, $6, 'pending', NULL, $7, $7, 0)
            ON CONFLICT (task_id, work_item_id) DO UPDATE
            SET status = 'pending',
                error_text = NULL,
                reason = EXCLUDED.reason,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(local_id::generate_local_id())
        .bind(&work_item_id)
        .bind(&task.id)
        .bind(&payload.user_id)
        .bind(&payload.reason)
        .bind(if is_new_repo { 1_i64 } else { 0_i64 })
        .bind(&now_text)
        .execute(&mut *transaction)
        .await
        .context("record repo release watcher")?;
        transaction
            .commit()
            .await
            .context("commit repo release work item claim")?;
        Ok(WorkItemClaim::Leased(WorkItemLease {
            id: work_item_id,
            task_id: task.id.clone(),
            started_at: now_text,
        }))
    }

    /// Read the repository's releases and persist the outcome.
    ///
    /// The credential decision (payload user, rotated candidate users,
    /// unauthenticated public read) belongs to
    /// `crate::postgres_repo_release_fetch`; this lane only owns the work item
    /// and shared-cache writes.
    async fn refresh(
        &self,
        payload: &RepoReleaseTaskPayload,
        lease: &WorkItemLease,
    ) -> std::result::Result<RefreshStats, ReleaseFetchError> {
        let outcome = fetch_releases(
            &self.pool,
            &self.http,
            &self.encryption_key,
            &ReleaseFetchRequest {
                repo_id: payload.repo_id,
                repo_full_name: payload.repo_full_name.as_str(),
                requested_by_user_id: payload.user_id.as_str(),
            },
            &ReleaseCredentialCache::default(),
            self.budget.as_ref(),
        )
        .await?;
        self.persist_success(payload, lease, outcome)
            .await
            .map_err(|error| ReleaseFetchError::Failed(RefreshFailure::local(error)))
    }

    /// Persist the fetched snapshot, the work item outcome and the sync state in
    /// one transaction so a partially written refresh can never be observed.
    async fn persist_success(
        &self,
        payload: &RepoReleaseTaskPayload,
        lease: &WorkItemLease,
        outcome: ReleaseFetchOutcome,
    ) -> Result<RefreshStats> {
        let now = Utc::now().to_rfc3339();
        let public_usage: PublicReleaseUsageAccess = outcome.public_usage;
        let fetch = outcome.fetch;
        let mut transaction = self
            .pool
            .begin()
            .await
            .context("begin repo release refresh finalize")?;
        ensure_lease_held(&mut transaction, lease, payload.repo_id, &now).await?;

        let mut stats = RefreshStats {
            work_item_id: lease.id.clone(),
            not_modified: fetch.not_modified,
            pages_fetched: fetch.pages_fetched,
            stopped_reason: fetch.stopped_reason.clone(),
            via: Some(outcome.via),
            candidate_user_id: outcome.candidate_user_id,
            ..RefreshStats::default()
        };
        if !fetch.not_modified {
            stats.fetched_count = fetch.releases.len();
            apply_releases(
                &mut transaction,
                payload.repo_id,
                &fetch.releases,
                &now,
                &mut stats,
            )
            .await?;
        }

        let new_release_ids_json =
            serde_json::to_string(&stats.new_release_ids).context("serialize new release ids")?;
        let error_text = if stats.new_release_ids.is_empty() {
            None
        } else {
            Some(PENDING_CONTENT_MARKER)
        };
        let updated = sqlx::query(
            r#"
            UPDATE public.repo_release_work_items
            SET status = 'succeeded',
                priority = 0,
                has_new_repo_watchers = 0,
                deadline_at = $2,
                last_release_count = $3,
                last_candidate_failures = 0,
                last_fetched_count = $4,
                last_inserted_count = $5,
                last_updated_count = $6,
                last_unchanged_count = $7,
                last_pages_fetched = $8,
                last_stopped_reason = $9,
                last_new_release_ids_json = $10,
                last_success_at = $2,
                error_text = $11,
                finished_at = $2,
                updated_at = $2,
                runtime_owner_id = NULL,
                lease_heartbeat_at = NULL
            WHERE id = $1
              AND status = 'running'
              AND started_at = $12
              AND runtime_owner_id = $13
            "#,
        )
        .bind(&lease.id)
        .bind(&now)
        .bind(count_i64(stats.inserted_count + stats.updated_count))
        .bind(count_i64(stats.fetched_count))
        .bind(count_i64(stats.inserted_count))
        .bind(count_i64(stats.updated_count))
        .bind(count_i64(stats.unchanged_count))
        .bind(count_i64(stats.pages_fetched))
        .bind(&stats.stopped_reason)
        .bind(&new_release_ids_json)
        .bind(error_text)
        .bind(&lease.started_at)
        .bind(&lease.task_id)
        .execute(&mut *transaction)
        .await
        .context("finalize repo release work item")?
        .rows_affected();
        if updated == 0 {
            return Err(anyhow!(
                "repo release work item lease lost for {}",
                payload.repo_full_name
            ));
        }
        mark_watchers(&mut transaction, &lease.id, "succeeded", None, &now).await?;
        record_sync_success(&mut transaction, payload.repo_id, &fetch, &stats, &now).await?;
        if public_usage.exists {
            mark_public_usage_ready(&mut *transaction, payload.repo_id, &now)
                .await
                .context("mark public release usage ready")?;
        }
        transaction
            .commit()
            .await
            .context("commit repo release refresh finalize")?;
        Ok(stats)
    }

    /// Release the work item for a read the anonymous budget could not pay for.
    ///
    /// Nothing failed upstream, so this must not write `failed` anywhere: no
    /// `repo_release_sync_state.backoff_until`, no `last_error_text`, and no
    /// `public_repo_release_usage.last_sync_status` change. The row keeps its
    /// last success and the periodic scheduler re-queues the repository.
    async fn record_deferred(
        &self,
        payload: &RepoReleaseTaskPayload,
        lease: &WorkItemLease,
        deferral: BudgetDeferral,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let error_text = deferral.summary();
        let mut transaction = self
            .pool
            .begin()
            .await
            .context("begin repo release deferral finalize")?;
        let updated = sqlx::query(
            r#"
            UPDATE public.repo_release_work_items
            SET status = 'deferred',
                priority = 0,
                has_new_repo_watchers = 0,
                deadline_at = $2,
                error_text = $3,
                finished_at = $2,
                updated_at = $2,
                runtime_owner_id = NULL,
                lease_heartbeat_at = NULL
            WHERE id = $1
              AND status = 'running'
              AND started_at = $4
              AND runtime_owner_id = $5
            "#,
        )
        .bind(&lease.id)
        .bind(&now)
        .bind(&error_text)
        .bind(&lease.started_at)
        .bind(&lease.task_id)
        .execute(&mut *transaction)
        .await
        .context("defer repo release work item")?
        .rows_affected();
        if updated == 0 {
            warn!(
                event = "worker.repo_release",
                repo_id = payload.repo_id,
                work_item_id = lease.id,
                "repo release work item lease lost before deferral finalize"
            );
            sqlx::query(
                "UPDATE public.repo_release_watchers SET status = 'deferred', error_text = $2, updated_at = $3 WHERE task_id = $1 AND status = 'pending'",
            )
            .bind(&lease.task_id)
            .bind(&error_text)
            .bind(&now)
            .execute(&mut *transaction)
            .await
            .context("defer this task's repo release watcher")?;
        } else {
            mark_watchers(
                &mut transaction,
                &lease.id,
                "deferred",
                Some(error_text.as_str()),
                &now,
            )
            .await?;
        }
        transaction
            .commit()
            .await
            .context("commit repo release deferral finalize")?;
        Ok(())
    }

    /// Classify and persist the failure, including the retry backoff the
    /// scheduler and dashboard read from `repo_release_sync_state`.
    async fn record_failure(
        &self,
        payload: &RepoReleaseTaskPayload,
        lease: &WorkItemLease,
        failure: &RefreshFailure,
    ) -> Result<()> {
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let backoff_until = failure
            .retryable
            .then(|| (now + chrono::Duration::minutes(RETRY_BACKOFF_MINUTES)).to_rfc3339());
        let error_text = failure.summary();
        let mut transaction = self
            .pool
            .begin()
            .await
            .context("begin repo release failure finalize")?;
        let updated = sqlx::query(
            r#"
            UPDATE public.repo_release_work_items
            SET status = 'failed',
                priority = 0,
                has_new_repo_watchers = 0,
                deadline_at = $2,
                error_text = $3,
                finished_at = $2,
                updated_at = $2,
                runtime_owner_id = NULL,
                lease_heartbeat_at = NULL
            WHERE id = $1
              AND status = 'running'
              AND started_at = $4
              AND runtime_owner_id = $5
            "#,
        )
        .bind(&lease.id)
        .bind(&now_text)
        .bind(&error_text)
        .bind(&lease.started_at)
        .bind(&lease.task_id)
        .execute(&mut *transaction)
        .await
        .context("fail repo release work item")?
        .rows_affected();
        if updated == 0 {
            // Another run already owns the row; keep the sync-state failure
            // record but do not overwrite the newer work item outcome.
            warn!(
                event = "worker.repo_release",
                repo_id = payload.repo_id,
                work_item_id = lease.id,
                "repo release work item lease lost before failure finalize"
            );
            sqlx::query(
                "UPDATE public.repo_release_watchers SET status = 'failed', error_text = $2, updated_at = $3 WHERE task_id = $1 AND status = 'pending'",
            )
            .bind(&lease.task_id)
            .bind(&error_text)
            .bind(&now_text)
            .execute(&mut *transaction)
            .await
            .context("fail this task's repo release watcher")?;
        } else {
            mark_watchers(
                &mut transaction,
                &lease.id,
                "failed",
                Some(error_text.as_str()),
                &now_text,
            )
            .await?;
        }
        sqlx::query(
            r#"
            INSERT INTO public.repo_release_sync_state (
              repo_id, last_attempt_at, last_error_text, backoff_until, updated_at
            )
            VALUES ($1, $2, $3, $4, $2)
            ON CONFLICT (repo_id) DO UPDATE SET
              last_attempt_at = EXCLUDED.last_attempt_at,
              last_error_text = EXCLUDED.last_error_text,
              backoff_until = EXCLUDED.backoff_until,
              updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(payload.repo_id)
        .bind(&now_text)
        .bind(&error_text)
        .bind(backoff_until.as_deref())
        .execute(&mut *transaction)
        .await
        .context("record repo release sync failure")?;
        mark_public_usage_failed(&mut *transaction, payload.repo_id, failure, &now_text)
            .await
            .context("record public release usage failure")?;
        transaction
            .commit()
            .await
            .context("commit repo release failure finalize")?;
        Ok(())
    }
}

/// Decide whether another task's in-flight refresh must be reused.
///
/// Mirrors the legacy `reused_running` freshness decision: a work item that is
/// `running` under a different task with an unexpired deadline is already doing
/// the upstream read, so this task must not steal the lease.
fn reuse_in_flight_run(row: &WorkItemRow, task_id: &str, now: &str) -> bool {
    row.status == "running"
        && row
            .runtime_owner_id
            .as_deref()
            .is_some_and(|owner| !owner.is_empty() && owner != task_id)
        && row.deadline_at.as_str() > now
}

async fn record_reused_running_watcher(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    work_item_id: &str,
    task: &ClaimedTask,
    payload: &RepoReleaseTaskPayload,
    now: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO public.repo_release_watchers (
          id, work_item_id, task_id, user_id, origin, priority, reason, is_new_repo,
          status, error_text, created_at, updated_at, reused_fresh, freshness_decision
        )
        VALUES ($1, $2, $3, $4, 'interactive', 2, $5, 0, 'succeeded', NULL, $6, $6, 0, 'reused_running')
        ON CONFLICT (task_id, work_item_id) DO UPDATE
        SET status = 'succeeded',
            error_text = NULL,
            reason = EXCLUDED.reason,
            updated_at = EXCLUDED.updated_at,
            freshness_decision = EXCLUDED.freshness_decision
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(work_item_id)
    .bind(&task.id)
    .bind(&payload.user_id)
    .bind(&payload.reason)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .context("record reused repo release watcher")?;
    Ok(())
}

async fn ensure_lease_held(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    lease: &WorkItemLease,
    repo_id: i64,
    now: &str,
) -> Result<()> {
    let held = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM public.repo_release_work_items WHERE id = $1 AND repo_id = $2 AND status = 'running' AND started_at = $3 AND runtime_owner_id = $4 AND deadline_at > $5)",
    )
    .bind(&lease.id)
    .bind(repo_id)
    .bind(&lease.started_at)
    .bind(&lease.task_id)
    .bind(now)
    .fetch_one(&mut **transaction)
    .await
    .context("validate repo release work item lease")?;
    if !held {
        return Err(anyhow!("repo release work item lease expired"));
    }
    Ok(())
}

async fn mark_watchers(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    work_item_id: &str,
    status: &str,
    error_text: Option<&str>,
    now: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE public.repo_release_watchers SET status = $2, error_text = $3, updated_at = $4 WHERE work_item_id = $1 AND status = 'pending'",
    )
    .bind(work_item_id)
    .bind(status)
    .bind(error_text)
    .bind(now)
    .execute(&mut **transaction)
    .await
    .context("update repo release watchers")?;
    Ok(())
}

async fn record_sync_success(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    repo_id: i64,
    fetch: &RepositoryFetch,
    stats: &RefreshStats,
    now: &str,
) -> Result<()> {
    let not_modified_at = fetch.not_modified.then_some(now);
    sqlx::query(
        r#"
        INSERT INTO public.repo_release_sync_state (
          repo_id, etag, last_modified, last_success_at, last_attempt_at,
          last_not_modified_at, last_error_text, backoff_until, last_page_count,
          last_fetched_count, last_inserted_count, last_updated_count,
          last_unchanged_count, last_stopped_reason, updated_at
        )
        VALUES ($1, $2, $3, $4, $4, $5, NULL, NULL, $6, $7, $8, $9, $10, $11, $4)
        ON CONFLICT (repo_id) DO UPDATE SET
          etag = COALESCE(EXCLUDED.etag, public.repo_release_sync_state.etag),
          last_modified = COALESCE(EXCLUDED.last_modified, public.repo_release_sync_state.last_modified),
          last_success_at = EXCLUDED.last_success_at,
          last_attempt_at = EXCLUDED.last_attempt_at,
          last_not_modified_at = COALESCE(EXCLUDED.last_not_modified_at, public.repo_release_sync_state.last_not_modified_at),
          last_error_text = NULL,
          backoff_until = NULL,
          last_page_count = CASE
            WHEN EXCLUDED.last_page_count > 0 THEN EXCLUDED.last_page_count
            ELSE public.repo_release_sync_state.last_page_count
          END,
          last_fetched_count = EXCLUDED.last_fetched_count,
          last_inserted_count = EXCLUDED.last_inserted_count,
          last_updated_count = EXCLUDED.last_updated_count,
          last_unchanged_count = EXCLUDED.last_unchanged_count,
          last_stopped_reason = EXCLUDED.last_stopped_reason,
          updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(repo_id)
    .bind(fetch.etag.as_deref())
    .bind(fetch.last_modified.as_deref())
    .bind(now)
    .bind(not_modified_at)
    .bind(count_i64(stats.pages_fetched))
    .bind(count_i64(stats.fetched_count))
    .bind(count_i64(stats.inserted_count))
    .bind(count_i64(stats.updated_count))
    .bind(count_i64(stats.unchanged_count))
    .bind(&stats.stopped_reason)
    .execute(&mut **transaction)
    .await
    .context("record repo release sync success")?;
    Ok(())
}

/// Insert or refresh every fetched release in the shared `repo_releases` cache.
///
/// A release row is only overwritten when the incoming source revision is not
/// older than the stored `updated_at`, so a late webhook delivery can never
/// replace newer cached content.
async fn apply_releases(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    repo_id: i64,
    releases: &[Value],
    now: &str,
    stats: &mut RefreshStats,
) -> Result<()> {
    let mut release_ids = Vec::with_capacity(releases.len());
    for release in releases {
        let release_id = release
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("GitHub release has no numeric id"))?;
        release_ids.push(release_id);
    }
    let existing = sqlx::query_as::<_, ExistingReleaseRow>(
        r#"
        SELECT release_id, node_id, tag_name, name, body, html_url, published_at,
               created_at, updated_at, is_prerelease, is_draft, react_plus1,
               react_laugh, react_heart, react_hooray, react_rocket, react_eyes
        FROM public.repo_releases
        WHERE release_id = ANY($1)
        "#,
    )
    .bind(&release_ids)
    .fetch_all(&mut **transaction)
    .await
    .context("load cached repository releases")?;
    let mut existing = existing
        .into_iter()
        .map(|row| (row.release_id, row))
        .collect::<std::collections::HashMap<_, _>>();

    for release in releases {
        let release_id = release
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("GitHub release has no numeric id"))?;
        let tag_name = release
            .get("tag_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = release.get("name").and_then(Value::as_str);
        let body = release.get("body").and_then(Value::as_str);
        let html_url = release
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let published_at = release.get("published_at").and_then(Value::as_str);
        let created_at = release.get("created_at").and_then(Value::as_str);
        let is_prerelease = i64::from(
            release
                .get("prerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
        let is_draft = i64::from(
            release
                .get("draft")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        );
        let revision = release
            .get("updated_at")
            .and_then(Value::as_str)
            .or(published_at)
            .or(created_at)
            .filter(|value| !value.is_empty())
            .unwrap_or(now);
        let reactions = release.get("reactions");
        let reaction = |key: &str| {
            reactions
                .and_then(|value| value.get(key))
                .and_then(Value::as_i64)
                .unwrap_or(0)
        };

        match existing.remove(&release_id) {
            None => {
                sqlx::query(
                    r#"
                    INSERT INTO public.repo_releases (
                      id, repo_id, release_id, node_id, tag_name, name, body, html_url,
                      published_at, created_at, is_prerelease, is_draft, updated_at,
                      detected_at, react_plus1, react_laugh, react_heart, react_hooray,
                      react_rocket, react_eyes
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                            $15, $16, $17, $18, $19, $20)
                    "#,
                )
                .bind(local_id::generate_local_id())
                .bind(repo_id)
                .bind(release_id)
                .bind(release.get("node_id").and_then(Value::as_str))
                .bind(tag_name)
                .bind(name)
                .bind(body)
                .bind(html_url)
                .bind(published_at)
                .bind(created_at)
                .bind(is_prerelease)
                .bind(is_draft)
                .bind(revision)
                .bind(now)
                .bind(reaction("+1"))
                .bind(reaction("laugh"))
                .bind(reaction("heart"))
                .bind(reaction("hooray"))
                .bind(reaction("rocket"))
                .bind(reaction("eyes"))
                .execute(&mut **transaction)
                .await
                .with_context(|| format!("insert cached release {tag_name}"))?;
                stats.inserted_count += 1;
                stats.new_release_ids.push(release_id);
            }
            Some(current) => {
                let unchanged = current.node_id.as_deref()
                    == release.get("node_id").and_then(Value::as_str)
                    && current.tag_name == tag_name
                    && current.name.as_deref() == name
                    && current.body.as_deref() == body
                    && current.html_url == html_url
                    && current.published_at.as_deref() == published_at
                    && current.created_at.as_deref() == created_at
                    && current.is_prerelease == is_prerelease
                    && current.is_draft == is_draft
                    && current.react_plus1 == reaction("+1")
                    && current.react_laugh == reaction("laugh")
                    && current.react_heart == reaction("heart")
                    && current.react_hooray == reaction("hooray")
                    && current.react_rocket == reaction("rocket")
                    && current.react_eyes == reaction("eyes");
                if revision < current.updated_at.as_str() {
                    // The stored row came from a newer source revision.
                    stats.unchanged_count += 1;
                    continue;
                }
                if unchanged && revision == current.updated_at {
                    stats.unchanged_count += 1;
                    continue;
                }
                sqlx::query(
                    r#"
                    UPDATE public.repo_releases
                    SET repo_id = $2, node_id = $3, tag_name = $4, name = $5, body = $6,
                        html_url = $7, published_at = $8, created_at = $9,
                        is_prerelease = $10, is_draft = $11, updated_at = $12,
                        react_plus1 = $13, react_laugh = $14, react_heart = $15,
                        react_hooray = $16, react_rocket = $17, react_eyes = $18
                    WHERE release_id = $1
                    "#,
                )
                .bind(release_id)
                .bind(repo_id)
                .bind(release.get("node_id").and_then(Value::as_str))
                .bind(tag_name)
                .bind(name)
                .bind(body)
                .bind(html_url)
                .bind(published_at)
                .bind(created_at)
                .bind(is_prerelease)
                .bind(is_draft)
                .bind(revision)
                .bind(reaction("+1"))
                .bind(reaction("laugh"))
                .bind(reaction("heart"))
                .bind(reaction("hooray"))
                .bind(reaction("rocket"))
                .bind(reaction("eyes"))
                .execute(&mut **transaction)
                .await
                .with_context(|| format!("update cached release {tag_name}"))?;
                stats.updated_count += 1;
            }
        }
    }
    Ok(())
}

fn count_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(task_type: &str, payload: Value) -> ClaimedTask {
        ClaimedTask {
            id: "task-1".to_owned(),
            task_type: task_type.to_owned(),
            source: "webhook.release".to_owned(),
            requested_by: Some("fallback-user".to_owned()),
            payload_json: payload.to_string(),
            cancel_requested: 0,
        }
    }

    #[test]
    fn payload_requires_the_repository_scope() {
        let payload = RepoReleaseTaskPayload::from_task(&task(
            TASK_TYPE,
            json!({
                "user_id": "user-a",
                "repo_id": 4242,
                "repo_full_name": "owner/repo",
                "reason": "webhook_release",
                "delivery_id": "delivery-1",
            }),
        ))
        .expect("valid payload");
        assert_eq!(payload.user_id, "user-a");
        assert_eq!(payload.repo_id, 4242);
        assert_eq!(payload.repo_full_name, "owner/repo");
        assert_eq!(payload.reason, "webhook_release");
        assert_eq!(payload.delivery_id.as_deref(), Some("delivery-1"));

        let error = RepoReleaseTaskPayload::from_task(&task(TASK_TYPE, json!({"user_id": "u"})))
            .expect_err("missing repo scope");
        assert!(error.to_string().contains("repo_id"));
    }

    #[test]
    fn payload_falls_back_to_the_requesting_user() {
        let payload = RepoReleaseTaskPayload::from_task(&task(
            TASK_TYPE,
            json!({"repo_id": 7, "repo_full_name": "owner/repo"}),
        ))
        .expect("valid payload");
        assert_eq!(payload.user_id, "fallback-user");
        assert_eq!(payload.reason, "repo_release_refresh");
        assert!(payload.delivery_id.is_none());
    }

    #[test]
    fn task_type_matches_the_registered_webhook_target() {
        assert_eq!(TASK_TYPE, "sync.repo_release");
        assert_eq!(PENDING_CONTENT_MARKER, "content_processing_enqueue_pending");
    }

    #[test]
    fn refresh_stats_flag_pending_content_for_new_releases() {
        let payload = RepoReleaseTaskPayload {
            user_id: "user-a".to_owned(),
            repo_id: 4242,
            repo_full_name: "owner/repo".to_owned(),
            reason: "webhook_release".to_owned(),
            delivery_id: None,
        };
        let stats = RefreshStats {
            work_item_id: "work-1".to_owned(),
            inserted_count: 1,
            new_release_ids: vec![9001],
            via: Some(FetchVia::Candidate),
            candidate_user_id: Some("user-b".to_owned()),
            ..RefreshStats::default()
        };
        let result: Value =
            serde_json::from_str(&stats.into_result_json(&payload, 1)).expect("result json");
        assert_eq!(result["task"], TASK_TYPE);
        assert_eq!(result["repo_id"], 4242);
        assert_eq!(result["content_pending"], true);
        assert_eq!(result["new_release_ids"], json!([9001]));
        assert_eq!(result["translations_enqueued"], 1);
        assert_eq!(result["via"], "candidate");
        // The rotated account is a service-side fact: the requester may not
        // learn which other account's connection was used.
        assert!(result.get("candidate_user_id").is_none(), "{result}");
    }

    #[test]
    fn refresh_stats_report_no_enqueued_translations_without_new_releases() {
        let payload = RepoReleaseTaskPayload {
            user_id: "user-a".to_owned(),
            repo_id: 4242,
            repo_full_name: "owner/repo".to_owned(),
            reason: "webhook_release".to_owned(),
            delivery_id: None,
        };
        let stats = RefreshStats {
            work_item_id: "work-1".to_owned(),
            not_modified: true,
            ..RefreshStats::default()
        };
        let result: Value =
            serde_json::from_str(&stats.into_result_json(&payload, 0)).expect("result json");
        assert_eq!(result["content_pending"], false);
        assert_eq!(result["new_release_ids"], json!([]));
        assert_eq!(result["translations_enqueued"], 0);
    }

    #[test]
    fn in_flight_refresh_is_reused_instead_of_stolen() {
        let now = "2026-11-21T00:00:00Z";
        let running = WorkItemRow {
            id: "work-1".to_owned(),
            status: "running".to_owned(),
            runtime_owner_id: Some("task-other".to_owned()),
            deadline_at: "2026-11-21T00:10:00Z".to_owned(),
        };
        assert!(reuse_in_flight_run(&running, "task-mine", now));
        assert!(!reuse_in_flight_run(&running, "task-other", now));
        let expired = WorkItemRow {
            deadline_at: "2026-11-20T23:00:00Z".to_owned(),
            ..running
        };
        assert!(!reuse_in_flight_run(&expired, "task-mine", now));
        let idle = WorkItemRow {
            status: "succeeded".to_owned(),
            runtime_owner_id: None,
            deadline_at: "2026-11-21T00:10:00Z".to_owned(),
            ..expired
        };
        assert!(!reuse_in_flight_run(&idle, "task-mine", now));
    }

    #[test]
    fn worker_contract_keeps_single_repository_scope() {
        let source = include_str!("postgres_repo_release_worker.rs");
        assert!(source.contains("repo_release_work_items"));
        assert!(source.contains("repo_release_watchers"));
        assert!(source.contains("repo_release_sync_state"));
        assert!(source.contains("last_new_release_ids_json"));
        assert!(source.contains("backoff_until"));
        // The credential decision moved to the shared fetch module, so this
        // lane only delegates and records the outcome.
        assert!(source.contains("postgres_repo_release_fetch"));
        assert!(source.contains("mark_public_usage_ready"));
        assert!(source.contains("mark_public_usage_failed"));
        assert!(source.contains("candidate_user_id"));
    }
}
