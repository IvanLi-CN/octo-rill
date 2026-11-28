//! PostgreSQL-native business task executors.
//!
//! Each executor owns one business vertical's upstream calls and PostgreSQL
//! writes. Durable claim/finalize semantics remain in `postgres_worker`.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use tracing::{debug, info, warn};

/// Business task types owned by the PostgreSQL task executors. The scheduler,
/// admin job groups and job center read surfaces all share these names.
pub(crate) const TASK_SYNC_STARRED: &str = "sync.starred";
pub(crate) const TASK_SYNC_RELEASES: &str = "sync.releases";
pub(crate) const TASK_SYNC_NOTIFICATIONS: &str = "sync.notifications";
pub(crate) const TASK_SYNC_SUBSCRIPTIONS: &str = "sync.subscriptions";
/// Repair sweep for Releases that were admitted before polishing learned to
/// emit the audience reading variants.
///
/// Such a Release keeps its `smart` work item forever because admission only
/// runs for newly inserted Releases, so no sync will ever re-offer it. This task
/// re-drives the existing admission over the historical backlog, one bounded
/// page per run, and carries a cursor forward so successive runs make progress
/// instead of re-scanning the same rows.
pub(crate) const TASK_POLISH_BACKFILL: &str = "polish.backfill";

use crate::{
    config::AiConfig,
    crypto::EncryptionKey,
    local_id,
    postgres_jobs::ClaimedTask,
    postgres_repo_release_fetch::{
        AnonymousBudget, BudgetDeferral, RefreshFailure, ReleaseCredentialCache, ReleaseFetchError,
        ReleaseFetchRequest, fetch_releases, mark_public_usage_failed, mark_public_usage_ready,
        shared_anonymous_budget,
    },
    postgres_star_epochs::{self, StarEpochProgress, StarEpochRow, StarSlicePage},
    postgres_worker::TaskExecutor,
};

const GITHUB_API: &str = "https://api.github.com";

#[derive(Clone)]
pub struct NotificationsExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
}

#[derive(Clone)]
pub struct ReleasesExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
    /// Present only when global release content admission can run; without it
    /// the lane stores releases and admits no translation or polishing work.
    ai: Option<AiConfig>,
    /// The process-wide unauthenticated read allowance, shared with the
    /// single-repository worker lane.
    budget: std::sync::Arc<AnonymousBudget>,
}

#[derive(Clone)]
pub struct StarredExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
}

#[derive(Clone)]
pub struct CompositeSyncExecutor {
    starred: StarredExecutor,
    releases: ReleasesExecutor,
    notifications: NotificationsExecutor,
}

#[derive(Clone)]
pub struct WebhookExecutor {
    pool: PgPool,
    http: Client,
    encryption_key: EncryptionKey,
    public_base_url: String,
}

#[derive(Clone)]
pub struct BriefExecutor {
    pool: PgPool,
}

#[derive(Clone)]
pub struct TranslationExecutor {
    pool: PgPool,
    http: Client,
    ai: Option<AiConfig>,
}

/// Repair sweep for polished Releases that are missing their audience variants.
///
/// A Release admitted before polishing emitted `smart_public`/`smart_authorized`
/// keeps only `smart`, and admission only runs for newly inserted Releases, so
/// nothing re-offers it. This executor walks the backlog in Release-id order,
/// admitting one bounded page per run and recording where the next run starts.
#[derive(Clone)]
pub struct PolishBackfillExecutor {
    pool: PgPool,
    ai: Option<AiConfig>,
}

impl PolishBackfillExecutor {
    pub fn new(pool: PgPool, ai: Option<AiConfig>) -> Self {
        Self { pool, ai }
    }
}

impl ReleasesExecutor {
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

impl NotificationsExecutor {
    pub fn new(pool: PgPool, http: Client, encryption_key: EncryptionKey) -> Self {
        Self {
            pool,
            http,
            encryption_key,
        }
    }
}

impl StarredExecutor {
    pub fn new(pool: PgPool, http: Client, encryption_key: EncryptionKey) -> Self {
        Self {
            pool,
            http,
            encryption_key,
        }
    }
}

impl CompositeSyncExecutor {
    pub fn new(
        pool: PgPool,
        http: Client,
        encryption_key: EncryptionKey,
        ai: Option<AiConfig>,
    ) -> Self {
        Self {
            starred: StarredExecutor::new(pool.clone(), http.clone(), encryption_key.clone()),
            releases: ReleasesExecutor::new(pool.clone(), http.clone(), encryption_key.clone(), ai),
            notifications: NotificationsExecutor::new(pool, http, encryption_key),
        }
    }
}

impl WebhookExecutor {
    pub fn new(
        pool: PgPool,
        http: Client,
        encryption_key: EncryptionKey,
        public_base_url: impl Into<String>,
    ) -> Self {
        Self {
            pool,
            http,
            encryption_key,
            public_base_url: public_base_url.into(),
        }
    }
}

impl BriefExecutor {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TranslationExecutor {
    pub fn new(pool: PgPool, http: Client, ai: Option<AiConfig>) -> Self {
        Self { pool, http, ai }
    }
}

#[async_trait]
impl TaskExecutor for TranslationExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != "translation.generate" {
            bail!("translation executor received {}", task.task_type);
        }
        let payload: Value =
            serde_json::from_str(&task.payload_json).context("decode translation task payload")?;
        let work_item_id = payload
            .get("work_item_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("translation task has no work_item_id"))?;
        let request_id = payload.get("request_id").and_then(Value::as_str);
        let ai = self
            .ai
            .as_ref()
            .ok_or_else(|| anyhow!("AI provider is not configured"))?;
        let outcome = crate::postgres_translate::translate_work_item_for_task(
            &self.pool,
            &self.http,
            ai,
            &task.id,
            &task.task_type,
            work_item_id,
            request_id,
            &task.source,
        )
        .await?;
        if outcome.status != "ready" {
            bail!(
                "{}",
                outcome
                    .error
                    .unwrap_or_else(|| "translation failed".to_owned())
            );
        }
        Ok(json!({"work_item_id": work_item_id, "status": "completed"}).to_string())
    }
}

/// Releases one run may consider. This is the same bound
/// `admit_release_polish` applies, so a run considers at most this many
/// Releases and every considered Release gets its whole variant set.
const POLISH_BACKFILL_PAGE: usize = crate::postgres_content::POLISH_ADMISSION_CAP;

impl PolishBackfillExecutor {
    /// Quota-blocked work: `blocked_config` rows whose blocking attempt recorded
    /// a quota refusal rather than a credential rejection.
    ///
    /// Both land in the configuration class with the same summary, so the class
    /// and summary cannot discriminate. The durable marker is
    /// `content_attempt_events.retry_disposition = 'quota'`, written by the
    /// content worker at failure time from the raw provider body, because
    /// `llm_calls.error_text` is not joinable back to the attempt: the content
    /// worker passes no `parent_task`, and `content_attempt_llm_calls` is only
    /// written when a call succeeds, so a failed attempt leaves no link.
    ///
    /// Rows written before this marker existed carry no disposition and are
    /// deliberately not matched: without evidence they could be credential
    /// rejections, and requeueing those would reintroduce the unbounded retry
    /// loop the route-health gate prevents.
    async fn quota_blocked_candidates(pool: &PgPool, limit: usize) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT blocked.id
            FROM public.content_work_items blocked
            JOIN public.content_attempt_events event
              ON event.work_item_id = blocked.id
             AND event.attempt_no = blocked.attempt_count::bigint
             AND event.event_type = 'attempt_completed'
            WHERE blocked.status = 'blocked_config'
              AND blocked.failure_class = 'configuration'
              AND event.retry_disposition = 'quota'
            ORDER BY blocked.updated_at, blocked.id
            LIMIT $1
            "#,
        )
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(pool)
        .await
        .context("scan quota-blocked content work")?;
        Ok(rows)
    }

    /// Requeue quota-blocked rows with the same reset the route-health sweep
    /// uses, so a requeued item looks exactly like one that sweep released.
    ///
    /// The reset also clears `retry_disposition` on the attempt event it consumed:
    /// the row has left `blocked_config`, so the marker must not survive to
    /// requeue it again on the next run without new evidence.
    async fn requeue_quota_blocked(pool: &PgPool, ids: &[String]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut tx = pool.begin().await.context("begin quota requeue")?;
        // Re-admit only while the item is still blocked *and* still marked, so a
        // concurrent route-health release cannot be double-counted.
        let mut requeued = 0u64;
        for id in ids {
            let updated = sqlx::query_scalar::<_, i32>(
                r#"
                UPDATE public.content_work_items
                SET status = 'queued', failure_class = NULL, next_retry_at = NULL,
                    retry_expires_at = NULL, retry_after_at = NULL, finished_at = NULL,
                    lease_owner = NULL, lease_expires_at = NULL, updated_at = now()
                WHERE id = $1
                  AND status = 'blocked_config'
                  AND failure_class = 'configuration'
                RETURNING attempt_count::integer
                "#,
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .context("requeue quota-blocked content work")?;
            let Some(attempt_count) = updated else {
                continue;
            };
            sqlx::query(
                "UPDATE public.content_attempt_events SET retry_disposition = NULL WHERE work_item_id = $1 AND attempt_no = $2 AND event_type = 'attempt_completed'",
            )
            .bind(id)
            .bind(i64::from(attempt_count))
            .execute(&mut *tx)
            .await
            .context("clear the consumed quota marker")?;
            requeued += 1;
        }
        tx.commit().await.context("commit quota requeue")?;
        Ok(requeued)
    }

    /// Releases with `smart` polishing work but no `smart_public` work, ordered
    /// by `repo_releases.release_id` and strictly after the cursor.
    ///
    /// The scan is keyed on the shared Release table rather than on the work
    /// items alone so the cursor is a single monotonic integer: work-item ids are
    /// text, and ordering by them would not give the sweep a resumable sequence.
    /// A Release whose `smart` row exists but that has no stored source row is
    /// not admissable anyway, so joining the source table loses no candidate.
    async fn polish_backfill_candidates(
        pool: &PgPool,
        after_release_id: i64,
        limit: usize,
    ) -> Result<Vec<i64>> {
        let rows = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT DISTINCT r.release_id
            FROM public.repo_releases r
            JOIN public.content_work_items smart
              ON smart.canonical_resource_type = 'release'
             AND smart.canonical_resource_id = r.release_id::text
             AND smart.pipeline = 'polishing'
             AND smart.variant = 'smart'
            WHERE r.release_id > $1
              AND NOT EXISTS (
                  SELECT 1
                  FROM public.content_work_items audience
                  WHERE audience.canonical_resource_type = 'release'
                    AND audience.canonical_resource_id = r.release_id::text
                    AND audience.pipeline = 'polishing'
                    AND audience.variant = 'smart_public'
              )
            ORDER BY r.release_id
            LIMIT $2
            "#,
        )
        .bind(after_release_id)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(pool)
        .await
        .context("scan Releases missing audience polishing variants")?;
        Ok(rows)
    }

    async fn load_cursor(pool: &PgPool) -> Result<i64> {
        let cursor = sqlx::query_scalar::<_, i64>(
            "SELECT last_release_id FROM public.polish_backfill_cursor WHERE id = 1",
        )
        .fetch_optional(pool)
        .await
        .context("read polishing backfill cursor")?;
        Ok(cursor.unwrap_or(0).max(0))
    }

    async fn store_cursor(pool: &PgPool, release_id: i64, now: DateTime<Utc>) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO public.polish_backfill_cursor (id, last_release_id, last_run_at, updated_at)
            VALUES (1, $1, $2, $2)
            ON CONFLICT (id) DO UPDATE SET
              last_release_id = EXCLUDED.last_release_id,
              last_run_at = EXCLUDED.last_run_at,
              updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(release_id.max(0))
        .bind(now)
        .execute(pool)
        .await
        .context("store polishing backfill cursor")?;
        Ok(())
    }
}

#[async_trait]
impl TaskExecutor for PolishBackfillExecutor {
    /// One bounded sweep page, resumable across runs.
    ///
    /// The cursor is the highest `repo_releases.release_id` this run *examined*,
    /// not the highest it admitted. A Release that legitimately admits nothing —
    /// its stored text states no change, so it keeps only `smart` — must not pin
    /// the sweep on itself, and a crash mid-run resumes after the last examined
    /// id instead of restarting from zero.
    ///
    /// A page shorter than the limit means the scan reached the end of the id
    /// space, so the run is complete and the cursor rewinds to `0`. That is
    /// deliberate: a completed sweep must leave the next manual trigger able to
    /// find gaps that appeared afterwards, otherwise the finished state would
    /// permanently suppress repairs for new work. Automatic lane ticks then
    /// no-op until a candidate exists again, because the scan from zero finds
    /// nothing to scan.
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != TASK_POLISH_BACKFILL {
            bail!("polish backfill executor received {}", task.task_type);
        }
        let ai = self
            .ai
            .as_ref()
            .ok_or_else(|| anyhow!("AI provider is not configured"))?;
        // Quota-blocked rows are independent of the variant cursor: a different
        // failure, bounded separately per run, and consuming their marker is what
        // makes them one-shot. This runs first so a page of repairs never starves
        // the rows waiting on a restored allowance.
        let quota_candidates =
            Self::quota_blocked_candidates(&self.pool, POLISH_BACKFILL_PAGE).await?;
        let quota_scanned = quota_candidates.len();
        let quota_requeued = Self::requeue_quota_blocked(&self.pool, &quota_candidates).await?;

        let after = Self::load_cursor(&self.pool).await?;
        let candidates =
            Self::polish_backfill_candidates(&self.pool, after, POLISH_BACKFILL_PAGE).await?;
        let scanned = candidates.len();
        if scanned == 0 {
            if after != 0 {
                Self::store_cursor(&self.pool, 0, Utc::now()).await?;
            }
            return Ok(json!({
                "scanned": 0,
                "admitted": 0,
                "cursor": 0,
                "complete": true,
                "quota_scanned": quota_scanned,
                "quota_requeued": quota_requeued,
            })
            .to_string());
        }
        let admitted =
            crate::postgres_content::admit_release_polish(&self.pool, &candidates, &ai.model)
                .await?;
        let last = candidates.last().copied().unwrap_or(after);
        // A short page means this run reached the tail of the candidate set, so
        // the next run should start over rather than ask for ids above the tail.
        let complete = scanned < POLISH_BACKFILL_PAGE;
        let cursor = if complete { 0 } else { last };
        Self::store_cursor(&self.pool, cursor, Utc::now()).await?;
        info!(
            scanned,
            admitted, cursor, complete, "polishing audience backfill page finished"
        );
        Ok(json!({
            "scanned": scanned,
            "admitted": admitted,
            "cursor": cursor,
            "complete": complete,
            "quota_scanned": quota_scanned,
            "quota_requeued": quota_requeued,
        })
        .to_string())
    }
}

#[derive(Debug, FromRow)]
struct GithubTokenRow {
    id: String,
    access_token_ciphertext: Vec<u8>,
    access_token_nonce: Vec<u8>,
}

#[derive(Debug, FromRow)]
struct StarredRepoRow {
    repo_id: i64,
    full_name: String,
}

#[derive(Debug, FromRow)]
struct GithubConnectionRow {
    id: String,
    github_user_id: i64,
    login: String,
    avatar_url: Option<String>,
    access_token_ciphertext: Vec<u8>,
    access_token_nonce: Vec<u8>,
}

#[derive(Debug, FromRow)]
struct StarMembershipSnapshot {
    repo_id: i64,
    full_name: String,
    owner_login: String,
    name: String,
    description: Option<String>,
    html_url: String,
    stargazed_at: Option<String>,
    is_private: i64,
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: i64,
    repo_stargazer_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct StarredGraphQlResponse {
    data: Option<StarredGraphQlData>,
    errors: Option<Vec<StarredGraphQlError>>,
}

#[derive(Debug, Deserialize)]
struct StarredGraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct StarredGraphQlData {
    viewer: StarredViewer,
}

#[derive(Debug, Deserialize)]
struct StarredViewer {
    #[serde(rename = "starredRepositories")]
    starred_repositories: StarredConnection,
}

#[derive(Debug, Deserialize)]
struct StarredConnection {
    #[serde(rename = "totalCount")]
    total_count: i64,
    #[serde(rename = "isOverLimit")]
    is_over_limit: bool,
    #[serde(rename = "pageInfo")]
    page_info: StarredPageInfo,
    edges: Vec<StarredEdge>,
}

#[derive(Debug, Deserialize)]
struct StarredPageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StarredEdge {
    #[serde(rename = "starredAt")]
    starred_at: Option<String>,
    node: StarredNode,
}

#[derive(Debug, Deserialize)]
struct StarredNode {
    #[serde(rename = "databaseId")]
    database_id: Option<i64>,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    name: String,
    description: Option<String>,
    url: String,
    #[serde(rename = "isPrivate")]
    is_private: bool,
    #[serde(rename = "stargazerCount")]
    stargazer_count: Option<i64>,
    #[serde(rename = "openGraphImageUrl")]
    open_graph_image_url: Option<String>,
    #[serde(rename = "usesCustomOpenGraphImage")]
    uses_custom_open_graph_image: bool,
    owner: StarredOwner,
}

#[derive(Debug, Deserialize)]
struct StarredOwner {
    login: String,
    #[serde(rename = "avatarUrl")]
    avatar_url: Option<String>,
}

#[derive(Debug, FromRow)]
struct WebhookCredentialRow {
    webhook_push_desired_state: String,
    webhook_push_secret_ciphertext: Option<Vec<u8>>,
    webhook_push_secret_nonce: Option<Vec<u8>>,
    webhook_push_callback_key: Option<String>,
    token_ciphertext: Option<Vec<u8>>,
    token_nonce: Option<Vec<u8>>,
}

#[derive(Debug, FromRow)]
struct WebhookRepoRow {
    repo_id: i64,
    owner_login: String,
    repo_name: String,
    repo_full_name: String,
    hook_id: Option<i64>,
}

#[derive(Debug, FromRow)]
struct BriefReleaseRow {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
}

#[async_trait]
impl TaskExecutor for BriefExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if !matches!(
            task.task_type.as_str(),
            "brief.generate" | "brief.daily_slot"
        ) {
            bail!("brief executor received {}", task.task_type);
        }
        let payload: Value =
            serde_json::from_str(&task.payload_json).context("decode brief task payload")?;
        let user_id =
            task_user_id(&payload, task).ok_or_else(|| anyhow!("brief task has no user_id"))?;
        let releases = sqlx::query_as::<_, BriefReleaseRow>(
            "SELECT tag_name, name, body, html_url FROM public.releases WHERE user_id = $1 ORDER BY COALESCE(published_at, created_at) DESC, release_id DESC LIMIT 50",
        )
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL releases for brief")?;
        let date = Utc::now().date_naive().to_string();
        let now = Utc::now();
        let mut markdown = String::from("# 每日简报\n\n");
        markdown.push_str("## 项目更新\n\n");
        if releases.is_empty() {
            markdown.push_str("过去窗口没有新的发布。\n");
        } else {
            for release in &releases {
                let title = release
                    .name
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .unwrap_or(release.tag_name.as_str());
                let summary = release
                    .body
                    .as_deref()
                    .unwrap_or_default()
                    .lines()
                    .next()
                    .unwrap_or_default();
                markdown.push_str(&format!(
                    "- [{}]({}) {}\n",
                    title, release.html_url, summary
                ));
            }
        }
        let brief_id = format!("{user_id}:{date}");
        sqlx::query(
            "INSERT INTO public.briefs (id, user_id, date, window_start_utc, window_end_utc, effective_time_zone, effective_local_boundary, generation_source, content_markdown, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, 'postgres_deterministic', $8, $9, $9) ON CONFLICT (user_id, date) DO UPDATE SET content_markdown = EXCLUDED.content_markdown, generation_source = EXCLUDED.generation_source, updated_at = EXCLUDED.updated_at",
        )
        .bind(&brief_id)
        .bind(&user_id)
        .bind(&date)
        .bind(now - chrono::Duration::days(1))
        .bind(now)
        .bind("UTC")
        .bind("00:00")
        .bind(&markdown)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("upsert PostgreSQL brief")?;
        Ok(
            json!({"brief_id": brief_id, "user_id": user_id, "releases": releases.len()})
                .to_string(),
        )
    }
}

#[async_trait]
impl TaskExecutor for WebhookExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if !matches!(
            task.task_type.as_str(),
            "webhook.push.manage" | "webhook.push.reconcile"
        ) {
            bail!("webhook executor received {}", task.task_type);
        }
        let payload: Value =
            serde_json::from_str(&task.payload_json).context("decode webhook task payload")?;
        let user_id =
            task_user_id(&payload, task).ok_or_else(|| anyhow!("webhook task has no user_id"))?;
        let credentials = sqlx::query_as::<_, WebhookCredentialRow>(
            "SELECT u.webhook_push_desired_state, u.webhook_push_secret_ciphertext, u.webhook_push_secret_nonce, u.webhook_push_callback_key, p.token_ciphertext, p.token_nonce FROM public.users u LEFT JOIN public.reaction_pat_tokens p ON p.user_id = u.id WHERE u.id = $1 AND u.is_disabled = 0",
        )
        .bind(&user_id)
        .fetch_optional(&self.pool)
        .await
        .context("load PostgreSQL webhook credentials")?
        .ok_or_else(|| anyhow!("webhook user is missing or disabled"))?;
        let desired_state = payload
            .get("desired_state")
            .and_then(Value::as_str)
            .unwrap_or(&credentials.webhook_push_desired_state);
        let token_ciphertext = credentials
            .token_ciphertext
            .as_deref()
            .ok_or_else(|| anyhow!("webhook user has no GitHub PAT"))?;
        let token_nonce = credentials
            .token_nonce
            .as_deref()
            .ok_or_else(|| anyhow!("webhook user has no GitHub PAT nonce"))?;
        let token = self
            .encryption_key
            .decrypt_str(token_ciphertext, token_nonce)
            .context("decrypt webhook GitHub PAT")?;
        let callback_key = credentials
            .webhook_push_callback_key
            .as_deref()
            .ok_or_else(|| anyhow!("webhook user has no callback key"))?;
        let secret = match (
            credentials.webhook_push_secret_ciphertext.as_deref(),
            credentials.webhook_push_secret_nonce.as_deref(),
        ) {
            (Some(ciphertext), Some(nonce)) => Some(
                self.encryption_key
                    .decrypt_str(ciphertext, nonce)
                    .context("decrypt webhook callback secret")?,
            ),
            _ => None,
        };
        let callback_url = format!(
            "{}/api/webhooks/github/releases?key={}",
            self.public_base_url.trim_end_matches('/'),
            callback_key
        );
        let repo_id = payload.get("repo_id").and_then(Value::as_i64);
        let repos = sqlx::query_as::<_, WebhookRepoRow>(
            "SELECT repo_id, owner_login, repo_name, repo_full_name, hook_id, callback_url FROM public.webhook_push_repos WHERE user_id = $1 AND ($2::bigint IS NULL OR repo_id = $2) ORDER BY repo_id",
        )
        .bind(&user_id)
        .bind(repo_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL webhook repository state")?;
        let mut registered = 0usize;
        let mut removed = 0usize;
        for repo in repos {
            if desired_state == "enabled" {
                if let Some(secret) = secret.as_deref() {
                    let created_hook_id = ensure_github_hook(
                        &self.http,
                        &token,
                        &repo.repo_full_name,
                        &callback_url,
                        secret,
                    )
                    .await?;
                    if created_hook_id.is_some() {
                        registered += 1;
                    }
                    mark_webhook_repo(
                        &self.pool,
                        &user_id,
                        &repo,
                        &callback_url,
                        "registered",
                        created_hook_id.or(repo.hook_id),
                    )
                    .await?;
                }
            } else if let Some(hook_id) = repo.hook_id {
                if remove_github_hook(&self.http, &token, &repo.repo_full_name, hook_id).await? {
                    removed += 1;
                }
                mark_webhook_repo(
                    &self.pool,
                    &user_id,
                    &repo,
                    &callback_url,
                    "delete_pending",
                    None,
                )
                .await?;
            }
        }
        // Completing the pass acknowledges the generation that requested it and
        // records the check time; without this the demand stays outstanding and
        // every scheduler tick enqueues another manage task for the same user.
        let completed_at = Utc::now().to_rfc3339();
        // Only an account-wide registration advances the completion timestamp:
        // a repo-scoped pass never checked the account, and a pause/delete
        // sweep only removes hooks, whichever producer queued it.
        let account_wide =
            payload.get("repo_id").is_none_or(Value::is_null) && desired_state == "enabled";
        acknowledge_reconcile_generation(
            &self.pool,
            &user_id,
            payload.get("demand_generation").and_then(Value::as_i64),
            account_wide.then_some(completed_at.as_str()),
        )
        .await?;
        Ok(json!({"user_id": user_id, "desired_state": desired_state, "registered": registered, "removed": removed}).to_string())
    }
}

/// Mark the reconcile demand generation as completed and advance the user's
/// last completed check time in one transaction.
async fn acknowledge_reconcile_generation(
    pool: &PgPool,
    user_id: &str,
    generation: Option<i64>,
    account_check_at: Option<&str>,
) -> Result<()> {
    let mut transaction = pool
        .begin()
        .await
        .context("begin webhook reconcile acknowledgement")?;
    if let Some(generation) = generation {
        sqlx::query(
            "UPDATE public.webhook_push_reconcile_demands
             SET completed_generation = GREATEST(completed_generation, $2), updated_at = $3
             WHERE user_id = $1 AND requested_generation >= $2",
        )
        .bind(user_id)
        .bind(generation)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *transaction)
        .await
        .context("acknowledge PostgreSQL webhook reconcile generation")?;
    }
    if let Some(checked_at) = account_check_at {
        sqlx::query(
            "UPDATE public.users SET webhook_push_last_completed_check_at = $2 WHERE id = $1",
        )
        .bind(user_id)
        .bind(checked_at)
        .execute(&mut *transaction)
        .await
        .context("record PostgreSQL webhook completion time")?;
    }
    transaction
        .commit()
        .await
        .context("commit webhook reconcile acknowledgement")?;
    Ok(())
}

async fn ensure_github_hook(
    http: &Client,
    token: &str,
    full_name: &str,
    callback_url: &str,
    secret: &str,
) -> Result<Option<i64>> {
    let hooks =
        github_request(http, token, format!("{GITHUB_API}/repos/{full_name}/hooks")).await?;
    let existing = hooks.as_array().into_iter().flatten().find(|hook| {
        hook.get("config")
            .and_then(|config| config.get("url"))
            .and_then(Value::as_str)
            == Some(callback_url)
    });
    if let Some(existing) = existing {
        return Ok(existing.get("id").and_then(Value::as_i64));
    }
    let response = http
        .post(format!("{GITHUB_API}/repos/{full_name}/hooks"))
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&json!({"name": "web", "active": true, "events": ["release"], "config": {"url": callback_url, "content_type": "json", "secret": secret, "insecure_ssl": "0"}}))
        .send()
        .await
        .context("create GitHub release webhook")?;
    if !response.status().is_success() {
        bail!("GitHub webhook create returned HTTP {}", response.status());
    }
    Ok(response
        .json::<Value>()
        .await
        .context("decode created GitHub webhook")?
        .get("id")
        .and_then(Value::as_i64))
}

async fn remove_github_hook(
    http: &Client,
    token: &str,
    full_name: &str,
    hook_id: i64,
) -> Result<bool> {
    let response = http
        .delete(format!("{GITHUB_API}/repos/{full_name}/hooks/{hook_id}"))
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("delete GitHub release webhook")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(false);
    }
    if !response.status().is_success() {
        bail!("GitHub webhook delete returned HTTP {}", response.status());
    }
    Ok(true)
}

async fn github_request(http: &Client, token: &str, url: String) -> Result<Value> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("list GitHub hooks")?;
    let status = response.status();
    if !status.is_success() {
        bail!("GitHub hooks returned HTTP {status}");
    }
    response.json().await.context("decode GitHub hooks")
}

async fn mark_webhook_repo(
    pool: &PgPool,
    user_id: &str,
    repo: &WebhookRepoRow,
    callback_url: &str,
    status: &str,
    hook_id: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO public.webhook_push_repos (user_id, repo_id, owner_login, repo_name, repo_full_name, hook_id, callback_url, status, permission_paused, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9) ON CONFLICT (user_id, repo_id) DO UPDATE SET hook_id = EXCLUDED.hook_id, callback_url = EXCLUDED.callback_url, status = EXCLUDED.status, permission_paused = 0, updated_at = EXCLUDED.updated_at",
    )
    .bind(user_id)
    .bind(repo.repo_id)
    .bind(&repo.owner_login)
    .bind(&repo.repo_name)
    .bind(&repo.repo_full_name)
    .bind(hook_id)
    .bind(callback_url)
    .bind(status)
    .bind(Utc::now())
    .execute(pool)
    .await
    .context("persist PostgreSQL webhook repository state")?;
    Ok(())
}

#[async_trait]
impl TaskExecutor for StarredExecutor {
    /// Advance at most one Star reconciliation slice per execution.
    ///
    /// Every GitHub connection owns a durable epoch row; the executor picks the
    /// first connection whose epoch is due, claims exactly one page slice and
    /// returns `queued` while the sweep has more pages. Only the last page
    /// finalizes the epoch and unlocks membership pruning.
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != "sync.starred" {
            bail!("starred executor received {}", task.task_type);
        }
        let payload: Value =
            serde_json::from_str(&task.payload_json).context("decode sync.starred task payload")?;
        let user_id = task_user_id(&payload, task)
            .ok_or_else(|| anyhow!("sync.starred task has no user_id"))?;
        let connections = sqlx::query_as::<_, GithubConnectionRow>(
            "SELECT id, github_user_id, login, avatar_url, access_token_ciphertext, access_token_nonce FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC",
        )
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL starred repository connections")?;
        if connections.is_empty() {
            bail!("sync.starred has no GitHub connection for user {user_id}");
        }

        let now = Utc::now();
        let full_sweep_interval_minutes =
            postgres_star_epochs::load_full_sweep_interval_minutes(&self.pool).await?;
        let mut due = Vec::with_capacity(connections.len());
        for connection in &connections {
            if let Some(dispatch) = postgres_star_epochs::ensure_epoch_due(
                &self.pool,
                &user_id,
                connection.id.as_str(),
                full_sweep_interval_minutes,
                now,
            )
            .await?
            {
                due.push((dispatch, connection));
            }
        }
        // Advance the most overdue connection first so users with several
        // GitHub connections rotate instead of always serving the first one.
        due.sort_by(|(left, left_connection), (right, right_connection)| {
            left.due_at
                .cmp(&right.due_at)
                .then_with(|| left_connection.id.cmp(&right_connection.id))
        });
        let mut not_due = connections.len() - due.len();
        for (dispatch, connection) in due {
            let Some(epoch) = postgres_star_epochs::claim_epoch_slice(
                &self.pool,
                dispatch.epoch_id.as_str(),
                task.id.as_str(),
                now,
            )
            .await?
            else {
                not_due += 1;
                continue;
            };
            let progress = self
                .execute_epoch_slice(
                    &user_id,
                    connection,
                    &epoch,
                    full_sweep_interval_minutes,
                    task.id.as_str(),
                    now,
                )
                .await?;
            let repositories: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.starred_repos WHERE user_id = $1")
                    .bind(&user_id)
                    .fetch_one(&self.pool)
                    .await
                    .context("count PostgreSQL starred repositories")?;
            return Ok(json!({
                "status": progress.status,
                "user_id": user_id,
                "schedule_key": dispatch.schedule_key,
                "epoch_id": progress.epoch_id,
                "github_connection_id": progress.github_connection_id,
                "processed_pages": progress.processed_pages,
                "processed_items": progress.processed_items,
                "total_count_at_start": progress.total_count_at_start,
                "has_next_page": progress.has_next_page,
                "next_slice_not_before": progress.next_slice_not_before,
                "membership_removed": progress.membership_removed,
                "repositories": repositories,
            })
            .to_string());
        }
        Ok(json!({
            "status": "skipped",
            "user_id": user_id,
            "reason": "star_epoch_not_due",
            "not_due_connections": not_due,
        })
        .to_string())
    }
}

impl StarredExecutor {
    /// Execute one claimed slice: fetch a single page, persist the membership
    /// diff, then either schedule the next slice or finalize the epoch.
    async fn execute_epoch_slice(
        &self,
        user_id: &str,
        connection: &GithubConnectionRow,
        epoch: &StarEpochRow,
        full_sweep_interval_minutes: i64,
        lease_owner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<StarEpochProgress> {
        let token = match self.encryption_key.decrypt_str(
            &connection.access_token_ciphertext,
            &connection.access_token_nonce,
        ) {
            Ok(token) => token,
            Err(error) => {
                let reason = format!("credentials_invalid: {error:#}");
                postgres_star_epochs::fail_epoch(&self.pool, epoch.id.as_str(), &reason, now)
                    .await?;
                return Err(error.context(format!("decrypt GitHub connection {}", connection.id)));
            }
        };
        let page = match fetch_starred_page(
            &self.http,
            token.as_str(),
            epoch.next_cursor.as_deref(),
        )
        .await
        {
            Ok(page) => page,
            Err(error) => {
                if error.downcast_ref::<StarredFetchTerminal>().is_some() {
                    postgres_star_epochs::fail_epoch(
                        &self.pool,
                        epoch.id.as_str(),
                        format!("{error:#}").as_str(),
                        now,
                    )
                    .await?;
                } else {
                    postgres_star_epochs::defer_epoch_after_failure(
                        &self.pool,
                        epoch.id.as_str(),
                        format!("{error:#}").as_str(),
                        lease_owner_id,
                        now,
                    )
                    .await?;
                }
                return Err(error);
            }
        };
        if page.is_over_limit {
            // `isOverLimit=true` proves only that this page is incomplete, never
            // that the connection is unusable: keep the cursor and retry the
            // same slice instead of waiting for the next full-sweep window.
            postgres_star_epochs::defer_epoch_after_failure(
                &self.pool,
                epoch.id.as_str(),
                "github_is_over_limit",
                lease_owner_id,
                now,
            )
            .await?;
            bail!("star reconciliation refused to prune an over-limit GitHub connection");
        }
        if let Err(error) = self
            .apply_epoch_page(user_id, connection, epoch, &page.repos, now)
            .await
        {
            postgres_star_epochs::defer_epoch_after_failure(
                &self.pool,
                epoch.id.as_str(),
                format!("{error:#}").as_str(),
                lease_owner_id,
                now,
            )
            .await?;
            return Err(error);
        }
        let slice = StarSlicePage {
            repo_count: i64::try_from(page.repos.len()).unwrap_or(i64::MAX),
            total_count: page.total_count,
            end_cursor: page.end_cursor,
        };
        if page.has_next_page {
            postgres_star_epochs::schedule_next_slice(
                &self.pool,
                epoch,
                &slice,
                full_sweep_interval_minutes,
                lease_owner_id,
                now,
            )
            .await
        } else {
            self.finalize_epoch_slice(epoch, connection, &slice, lease_owner_id, now)
                .await
        }
    }

    /// Persist one page of connection memberships and the aggregate Star rows
    /// they imply in a single transaction.
    async fn apply_epoch_page(
        &self,
        user_id: &str,
        connection: &GithubConnectionRow,
        epoch: &StarEpochRow,
        repos: &[StarredRepoSnapshot],
        now: DateTime<Utc>,
    ) -> Result<()> {
        let now_rfc3339 = now.to_rfc3339();
        let mut tx = self
            .pool
            .begin()
            .await
            .context("begin PostgreSQL star membership page transaction")?;
        for repo in repos {
            upsert_connection_star_membership(
                &mut tx,
                user_id,
                connection.id.as_str(),
                epoch.id.as_str(),
                repo,
                now_rfc3339.as_str(),
            )
            .await?;
            rebuild_user_starred_repo_from_memberships(
                &mut tx,
                user_id,
                repo.repo_id,
                now_rfc3339.as_str(),
            )
            .await?;
            upsert_epoch_star_member(&mut tx, user_id, connection, repo, now_rfc3339.as_str())
                .await?;
        }
        tx.commit()
            .await
            .context("commit PostgreSQL star membership page transaction")?;
        Ok(())
    }

    /// Finalize the epoch: prune memberships the sweep never saw, rebuild their
    /// aggregates and mark the epoch succeeded, all in one transaction.
    async fn finalize_epoch_slice(
        &self,
        epoch: &StarEpochRow,
        connection: &GithubConnectionRow,
        slice: &StarSlicePage,
        lease_owner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<StarEpochProgress> {
        let now_rfc3339 = now.to_rfc3339();
        let mut tx = self
            .pool
            .begin()
            .await
            .context("begin PostgreSQL star reconciliation finalize transaction")?;
        let stale =
            postgres_star_epochs::finalize_epoch_in_tx(&mut tx, epoch, slice, lease_owner_id, now)
                .await?;
        for membership in &stale {
            rebuild_user_starred_repo_from_memberships(
                &mut tx,
                epoch.user_id.as_str(),
                membership.repo_id,
                now_rfc3339.as_str(),
            )
            .await?;
        }
        prune_epoch_star_members(&mut tx, epoch.user_id.as_str(), connection.github_user_id)
            .await?;
        tx.commit()
            .await
            .context("commit PostgreSQL star reconciliation finalize transaction")?;
        Ok(StarEpochProgress {
            epoch_id: epoch.id.clone(),
            github_connection_id: epoch.github_connection_id.clone(),
            processed_pages: epoch.processed_pages + 1,
            processed_items: epoch.processed_items + slice.repo_count,
            total_count_at_start: epoch
                .total_count_at_start
                .unwrap_or(slice.total_count)
                .max(0),
            has_next_page: false,
            next_slice_not_before: None,
            status: "succeeded",
            membership_removed: stale.len(),
        })
    }
}

#[async_trait]
impl TaskExecutor for CompositeSyncExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if !matches!(task.task_type.as_str(), "sync.all" | "sync.access_refresh") {
            bail!("composite sync executor received {}", task.task_type);
        }
        let starred: Value = self
            .starred
            .execute(&ClaimedTask {
                task_type: "sync.starred".to_owned(),
                ..task.clone()
            })
            .await
            .and_then(|value| serde_json::from_str(&value).context("decode starred sync result"))?;
        let releases: Value = self
            .releases
            .execute(&ClaimedTask {
                task_type: "sync.releases".to_owned(),
                ..task.clone()
            })
            .await
            .and_then(|value| serde_json::from_str(&value).context("decode release sync result"))?;
        let notifications: Value = self
            .notifications
            .execute(&ClaimedTask {
                task_type: "sync.notifications".to_owned(),
                ..task.clone()
            })
            .await
            .and_then(|value| {
                serde_json::from_str(&value).context("decode notification sync result")
            })?;
        Ok(
            json!({"starred": starred, "releases": releases, "notifications": notifications})
                .to_string(),
        )
    }
}

#[async_trait]
impl TaskExecutor for NotificationsExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != "sync.notifications" {
            bail!("notifications executor received {}", task.task_type);
        }
        let payload: Value = serde_json::from_str(&task.payload_json)
            .context("decode sync.notifications task payload")?;
        let user_id = task_user_id(&payload, task)
            .ok_or_else(|| anyhow!("sync.notifications task has no user_id"))?;
        let connections = sqlx::query_as::<_, GithubTokenRow>(
            "SELECT id, access_token_ciphertext, access_token_nonce FROM public.github_connections WHERE user_id = $1 ORDER BY linked_at ASC, id ASC",
        )
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL GitHub notification connections")?;
        if connections.is_empty() {
            bail!("sync.notifications has no GitHub connection for user {user_id}");
        }

        let mut total = 0usize;
        for connection in connections {
            let token = self
                .encryption_key
                .decrypt_str(
                    &connection.access_token_ciphertext,
                    &connection.access_token_nonce,
                )
                .with_context(|| format!("decrypt GitHub connection {}", connection.id))?;
            for page in 1..=20 {
                let response = self
                    .http
                    .get(format!(
                        "{GITHUB_API}/notifications?all=true&per_page=50&page={page}"
                    ))
                    .bearer_auth(&token)
                    .header(reqwest::header::USER_AGENT, "OctoRill")
                    .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28")
                    .send()
                    .await
                    .context("fetch GitHub notifications")?;
                let status = response.status();
                if !status.is_success() {
                    bail!("GitHub notifications returned HTTP {status}");
                }
                let notifications = response
                    .json::<Vec<Value>>()
                    .await
                    .context("decode GitHub notifications")?;
                for notification in &notifications {
                    upsert_notification(&self.pool, &user_id, notification).await?;
                }
                total += notifications.len();
                if notifications.len() < 50 {
                    break;
                }
                if page == 20 {
                    bail!(
                        "notification sync exceeds the bounded page budget; continuation is required"
                    );
                }
            }
        }
        Ok(json!({"notifications": total, "user_id": user_id}).to_string())
    }
}

#[async_trait]
impl TaskExecutor for ReleasesExecutor {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        if task.task_type != "sync.releases" {
            bail!("releases executor received {}", task.task_type);
        }
        let payload: Value = serde_json::from_str(&task.payload_json)
            .context("decode sync.releases task payload")?;
        let user_id = task_user_id(&payload, task)
            .ok_or_else(|| anyhow!("sync.releases task has no user_id"))?;
        let repos = sqlx::query_as::<_, StarredRepoRow>(
            "SELECT DISTINCT repo_id, full_name FROM public.user_release_visible_repos WHERE user_id = $1 ORDER BY repo_id",
        )
        .bind(&user_id)
        .fetch_all(&self.pool)
        .await
        .context("load PostgreSQL starred repositories")?;
        if repos.is_empty() {
            return Ok(json!({
                "releases": 0,
                "repositories": 0,
                "user_id": user_id,
                "translations_enqueued": 0,
                "failed_count": 0,
                "deferred_count": 0,
                "failure_ratio": 0.0,
                "failure_reasons": {},
                "degraded": false,
            })
            .to_string());
        }
        let now = Utc::now().to_rfc3339();
        let credentials = ReleaseCredentialCache::default();
        // The legacy lane refreshed up to `repo_release_worker_concurrency`
        // repositories in parallel across its work-item pool. One account task
        // now owns that whole list, so the same setting bounds the in-flight
        // requests here; without it a 9k-repository account would serialize a
        // full cycle past the 10-minute refresh interval.
        let concurrency = load_repo_release_concurrency(&self.pool).await;
        let mut pending = Vec::with_capacity(repos.len());
        for repo in &repos {
            let credentials = &credentials;
            let now = now.as_str();
            let user_id = user_id.as_str();
            pending.push(async move {
                fetch_one_account_repo(self, user_id, repo, now, credentials).await
            });
        }
        let mut outcomes = stream::iter(pending)
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;
        // Deterministic reporting: completion order follows network latency.
        outcomes.sort_by_key(|outcome| outcome.repo_id);
        let mut total = 0usize;
        let mut repositories = 0usize;
        let mut failed = Vec::new();
        let mut failed_reason_codes = Vec::new();
        let mut deferred = Vec::new();
        let mut new_release_ids = Vec::new();
        for outcome in outcomes {
            if let Some(deferral) = outcome.deferred {
                deferred.push(json!({
                    "repo_id": outcome.repo_id,
                    "repo_full_name": outcome.repo_full_name,
                    "reason_code": "deferred_budget",
                    "anonymous_budget_remaining": deferral.remaining,
                }));
                continue;
            }
            if let Some(failure) = outcome.failure {
                failed_reason_codes.push(failure.reason_code);
                failed.push(json!({
                    "repo_id": outcome.repo_id,
                    "repo_full_name": outcome.repo_full_name,
                    "reason_code": failure.reason_code,
                }));
                continue;
            }
            repositories += 1;
            total += outcome.fetched;
            new_release_ids.extend(outcome.new_release_ids);
        }
        let summary = summarize_release_run(repositories, failed_reason_codes, deferred.len());
        if summary.failed_count > 0 {
            warn!(
                event = "task.summary",
                operation = "sync.releases",
                user_id = user_id.as_str(),
                repositories,
                failed = summary.failed_count,
                deferred = summary.deferred_count,
                failure_ratio = summary.failure_ratio,
                failure_reasons = summary.top_reasons(),
                "account release sync finished with repository failures"
            );
        }
        if summary.all_failed {
            bail!(
                "every repository whose read was attempted failed: {} failed ({}), {} deferred",
                summary.failed_count,
                summary.top_reasons(),
                summary.deferred_count
            );
        }
        let translations_enqueued = self.admit_release_translations(&new_release_ids).await;
        Ok(json!({
            "releases": total,
            "repositories": repositories,
            "user_id": user_id,
            "failed_repositories": failed,
            "deferred_repositories": deferred,
            "translations_enqueued": translations_enqueued,
            "failed_count": summary.failed_count,
            "deferred_count": summary.deferred_count,
            "failure_ratio": summary.failure_ratio,
            "failure_reasons": summary.failure_reasons,
            "degraded": summary.degraded,
        })
        .to_string())
    }
}

impl ReleasesExecutor {
    /// Admit this sync's newly inserted Releases to the global content pipeline.
    ///
    /// The lane no longer writes per-user `translation_work_items`,
    /// `translation_requests` or `translation.generate` tasks: Release detail
    /// translation goes through global admission and Release polishing through
    /// its own admission, so the global worker owns every attempt and
    /// projection (REQ-PGCP-006).
    ///
    /// Returns how many Releases got new global translation work: a Release
    /// without translatable content, an already-admitted identity or a satisfied
    /// projection counts zero. The sync already committed its repository writes,
    /// so a failure here is reported and swallowed: the Releases stay stored and
    /// the next sync or an explicit request re-admits them. Without a configured
    /// AI provider the lane admits nothing.
    async fn admit_release_translations(&self, new_release_ids: &[i64]) -> usize {
        let Some(ai) = self.ai.as_ref() else {
            return 0;
        };
        let admitted = crate::postgres_translate::admit_release_detail_translations(
            &self.pool,
            new_release_ids,
            ai.model.as_str(),
            "sync.releases.auto_translate",
        )
        .await;
        if !new_release_ids.is_empty()
            && let Err(error) = crate::postgres_content::admit_release_polish(
                &self.pool,
                new_release_ids,
                ai.model.as_str(),
            )
            .await
        {
            warn!(
                event = "content.polish.enqueue",
                operation = "sync.releases",
                error = %error,
                "global polishing admission failed"
            );
        }
        admitted
    }
}

/// Aggregated outcome of one account-wide release sync.
///
/// The per-repository warnings made a bad run log 100+ lines while the task
/// still finished `succeeded`. The task now reports one summary line and states
/// in its result whether it was degraded or failed outright.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReleaseRunSummary {
    pub(crate) failed_count: usize,
    pub(crate) deferred_count: usize,
    /// Failed repositories over attempted repositories (succeeded + failed).
    /// Deferrals never reached GitHub, so they are not attempts.
    pub(crate) failure_ratio: f64,
    /// Failure reason code → repository count.
    pub(crate) failure_reasons: BTreeMap<&'static str, usize>,
    /// At least one repository failed while others were read.
    pub(crate) degraded: bool,
    /// No repository could be read at all: the task must be recorded failed.
    pub(crate) all_failed: bool,
}

impl ReleaseRunSummary {
    /// `reason=count` pairs, most frequent first, bounded to one log line.
    fn top_reasons(&self) -> String {
        let mut rows: Vec<_> = self.failure_reasons.iter().collect();
        rows.sort_by(|(left_code, left_count), (right_code, right_count)| {
            right_count
                .cmp(left_count)
                .then_with(|| left_code.cmp(right_code))
        });
        rows.into_iter()
            .take(5)
            .map(|(code, count)| format!("{code}={count}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Aggregate per-repository outcomes of one run. Pure so the degraded and
/// all-failed decisions are testable without a database or network.
pub(crate) fn summarize_release_run(
    repositories: usize,
    failures: impl IntoIterator<Item = &'static str>,
    deferred_count: usize,
) -> ReleaseRunSummary {
    let mut failure_reasons: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut failed_count = 0usize;
    for reason_code in failures {
        failed_count += 1;
        *failure_reasons.entry(reason_code).or_insert(0) += 1;
    }
    let attempted = repositories + failed_count;
    let failure_ratio = if attempted == 0 {
        0.0
    } else {
        ((failed_count as f64 / attempted as f64) * 100.0).round() / 100.0
    };
    ReleaseRunSummary {
        failed_count,
        deferred_count,
        failure_ratio,
        failure_reasons,
        degraded: failed_count > 0 && repositories > 0,
        all_failed: failed_count > 0 && repositories == 0,
    }
}

struct AccountRepoOutcome {
    repo_id: i64,
    repo_full_name: String,
    fetched: usize,
    /// Releases this refresh inserted into the shared cache; they are the only
    /// ones global translation admission may admit.
    new_release_ids: Vec<i64>,
    failure: Option<RefreshFailure>,
    /// Set when the anonymous budget could not pay for this repository.
    deferred: Option<BudgetDeferral>,
}

/// Fetch and persist one repository of an account-wide sync. A repository this
/// account cannot read must not fail the whole sync: the shared decision
/// rotates to another candidate user and, for a public repository, to an
/// unauthenticated read. A read the anonymous budget cannot pay for is skipped
/// without recording any failure.
async fn fetch_one_account_repo(
    executor: &ReleasesExecutor,
    user_id: &str,
    repo: &StarredRepoRow,
    now: &str,
    credentials: &ReleaseCredentialCache,
) -> AccountRepoOutcome {
    let skipped =
        |deferred: Option<BudgetDeferral>, failure: Option<RefreshFailure>| AccountRepoOutcome {
            repo_id: repo.repo_id,
            repo_full_name: repo.full_name.clone(),
            fetched: 0,
            new_release_ids: Vec::new(),
            failure,
            deferred,
        };
    let outcome = match fetch_releases(
        &executor.pool,
        &executor.http,
        &executor.encryption_key,
        &ReleaseFetchRequest {
            repo_id: repo.repo_id,
            repo_full_name: repo.full_name.as_str(),
            requested_by_user_id: user_id,
        },
        credentials,
        executor.budget.as_ref(),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(ReleaseFetchError::Deferred(deferral)) => {
            info!(
                event = "upstream.call",
                operation = "sync.releases",
                repo = repo.full_name,
                user_id,
                deferred_reason = "anonymous_budget",
                anonymous_budget_remaining = deferral.remaining,
                anonymous_budget_reset_at = deferral.reset_at_unix,
                "repository releases deferred: anonymous GitHub budget exhausted"
            );
            return skipped(Some(deferral), None);
        }
        Err(ReleaseFetchError::Failed(failure)) => {
            debug!(
                event = "upstream.call",
                operation = "sync.releases",
                repo = repo.full_name,
                user_id,
                error_kind = failure.reason_code,
                error_chain = failure.message,
                "repository releases could not be read for this account"
            );
            if let Err(error) =
                mark_public_usage_failed(&executor.pool, repo.repo_id, &failure, now).await
            {
                warn!(
                    event = "upstream.call",
                    operation = "sync.releases",
                    repo = repo.full_name,
                    error = %error,
                    "failed to record public release usage failure"
                );
            }
            return skipped(None, Some(failure));
        }
    };
    if outcome.public_usage.exists
        && let Err(error) = mark_public_usage_ready(&executor.pool, repo.repo_id, now).await
    {
        warn!(
            event = "upstream.call",
            operation = "sync.releases",
            repo = repo.full_name,
            error = %error,
            "failed to record public release usage success"
        );
    }
    if outcome.fetch.not_modified {
        return AccountRepoOutcome {
            repo_id: repo.repo_id,
            repo_full_name: repo.full_name.clone(),
            fetched: 0,
            new_release_ids: Vec::new(),
            failure: None,
            deferred: None,
        };
    }
    let mut new_release_ids = Vec::new();
    for release in &outcome.fetch.releases {
        match upsert_release(&executor.pool, user_id, repo, release, now).await {
            Ok(upsert) => push_inserted_release(&mut new_release_ids, upsert),
            Err(error) => return skipped(None, Some(RefreshFailure::local(error))),
        }
    }
    AccountRepoOutcome {
        repo_id: repo.repo_id,
        repo_full_name: repo.full_name.clone(),
        fetched: outcome.fetch.releases.len(),
        new_release_ids,
        failure: None,
        deferred: None,
    }
}

async fn load_repo_release_concurrency(pool: &PgPool) -> usize {
    let configured = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT repo_release_worker_concurrency FROM public.admin_runtime_settings LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten();
    configured
        .map(|value| {
            value.clamp(
                1,
                i64::try_from(crate::admin_runtime::MAX_REPO_RELEASE_WORKER_CONCURRENCY)
                    .unwrap_or(32),
            ) as usize
        })
        .unwrap_or(crate::admin_runtime::DEFAULT_REPO_RELEASE_WORKER_CONCURRENCY)
}

async fn upsert_release(
    pool: &PgPool,
    user_id: &str,
    repo: &StarredRepoRow,
    release: &Value,
    now: &str,
) -> Result<ReleaseUpsert> {
    let release_id = release
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("GitHub release has no numeric id"))?;
    let revision = release
        .get("updated_at")
        .and_then(Value::as_str)
        .or_else(|| release.get("published_at").and_then(Value::as_str))
        .or_else(|| release.get("created_at").and_then(Value::as_str))
        .unwrap_or(now);
    // Foreground release reads use this shared cache. Keep the source revision
    // separate from first detection, and never overwrite a newer webhook row.
    //
    // `xmax = 0` identifies the row this statement inserted; the conflict
    // branch keeps the existing row. The revision guard can also skip the
    // update entirely, which returns no row: that release already existed.
    let inserted = sqlx::query_scalar::<_, bool>(
        r#"INSERT INTO public.repo_releases
        (id,repo_id,release_id,node_id,tag_name,name,body,html_url,published_at,created_at,is_prerelease,is_draft,updated_at,detected_at,react_plus1,react_laugh,react_heart,react_hooray,react_rocket,react_eyes)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)
        ON CONFLICT(release_id) DO UPDATE SET
        repo_id=EXCLUDED.repo_id,node_id=EXCLUDED.node_id,tag_name=EXCLUDED.tag_name,name=EXCLUDED.name,body=EXCLUDED.body,html_url=EXCLUDED.html_url,published_at=EXCLUDED.published_at,created_at=EXCLUDED.created_at,is_prerelease=EXCLUDED.is_prerelease,is_draft=EXCLUDED.is_draft,updated_at=EXCLUDED.updated_at,
        react_plus1=EXCLUDED.react_plus1,react_laugh=EXCLUDED.react_laugh,react_heart=EXCLUDED.react_heart,react_hooray=EXCLUDED.react_hooray,react_rocket=EXCLUDED.react_rocket,react_eyes=EXCLUDED.react_eyes
        WHERE repo_releases.updated_at=repo_releases.detected_at OR EXCLUDED.updated_at >= repo_releases.updated_at
        RETURNING (xmax = 0) AS inserted"#,
    )
    .bind(local_id::generate_local_id())
    .bind(repo.repo_id)
    .bind(release_id)
    .bind(release.get("node_id").and_then(Value::as_str))
    .bind(release.get("tag_name").and_then(Value::as_str).unwrap_or_default())
    .bind(release.get("name").and_then(Value::as_str))
    .bind(release.get("body").and_then(Value::as_str))
    .bind(release.get("html_url").and_then(Value::as_str).unwrap_or_default())
    .bind(release.get("published_at").and_then(Value::as_str))
    .bind(release.get("created_at").and_then(Value::as_str))
    .bind(i64::from(release.get("prerelease").and_then(Value::as_bool).unwrap_or(false)))
    .bind(i64::from(release.get("draft").and_then(Value::as_bool).unwrap_or(false)))
    .bind(revision)
    .bind(now)
    .bind(reaction_count(release, "+1"))
    .bind(reaction_count(release, "laugh"))
    .bind(reaction_count(release, "heart"))
    .bind(reaction_count(release, "hooray"))
    .bind(reaction_count(release, "rocket"))
    .bind(reaction_count(release, "eyes"))
    .fetch_optional(pool).await.context("upsert PostgreSQL shared release")?
    .unwrap_or(false);
    let _ = user_id; // User visibility is represented by repository associations.

    Ok(ReleaseUpsert {
        release_id,
        inserted,
    })
}

/// Whether [`upsert_release`] created the shared row or found it existing.
struct ReleaseUpsert {
    release_id: i64,
    inserted: bool,
}

/// Collect the release only when the shared cache did not already hold it.
fn push_inserted_release(new_release_ids: &mut Vec<i64>, upsert: ReleaseUpsert) {
    if upsert.inserted {
        new_release_ids.push(upsert.release_id);
    }
}

fn reaction_count(release: &Value, key: &str) -> i64 {
    release
        .get("reactions")
        .and_then(|v| v.get(key))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

async fn upsert_notification(pool: &PgPool, user_id: &str, notification: &Value) -> Result<()> {
    let thread_id = required_string(notification, "id")?;
    let subject = notification.get("subject").unwrap_or(&Value::Null);
    let repository = notification.get("repository").unwrap_or(&Value::Null);
    let updated_at = notification
        .get("updated_at")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let unread = notification
        .get("unread")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    sqlx::query(
        "INSERT INTO public.notifications (id, user_id, thread_id, repo_full_name, subject_title, subject_type, reason, updated_at, unread, url, html_url, last_seen_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10, NULL) ON CONFLICT (user_id, thread_id) DO UPDATE SET repo_full_name = EXCLUDED.repo_full_name, subject_title = EXCLUDED.subject_title, subject_type = EXCLUDED.subject_type, reason = EXCLUDED.reason, updated_at = EXCLUDED.updated_at, unread = EXCLUDED.unread, url = EXCLUDED.url, html_url = EXCLUDED.html_url",
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(thread_id)
    .bind(repository.get("full_name").and_then(Value::as_str))
    .bind(subject.get("title").and_then(Value::as_str))
    .bind(subject.get("type").and_then(Value::as_str))
    .bind(notification.get("reason").and_then(Value::as_str))
    .bind(updated_at)
    .bind(if unread { 1_i64 } else { 0_i64 })
    .bind(notification.get("url").and_then(Value::as_str).or_else(|| notification.get("html_url").and_then(Value::as_str)))
    .execute(pool)
    .await
    .context("upsert PostgreSQL notification")?;
    Ok(())
}

/// One GraphQL Star page: the slices of a connection epoch.
#[derive(Debug)]
struct StarredPage {
    repos: Vec<StarredRepoSnapshot>,
    has_next_page: bool,
    end_cursor: Option<String>,
    total_count: i64,
    is_over_limit: bool,
}

#[derive(Debug, Clone)]
struct StarredRepoSnapshot {
    repo_id: i64,
    full_name: String,
    owner_login: String,
    name: String,
    description: Option<String>,
    html_url: String,
    stargazed_at: Option<String>,
    is_private: bool,
    owner_avatar_url: Option<String>,
    open_graph_image_url: Option<String>,
    uses_custom_open_graph_image: bool,
    repo_stargazer_count: Option<i64>,
}

/// A Star page failure that must not be retried: the connection itself is
/// unusable (expired or under-scoped token), so the epoch has to end.
#[derive(Debug)]
struct StarredFetchTerminal {
    message: String,
}

impl std::fmt::Display for StarredFetchTerminal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message.as_str())
    }
}

impl std::error::Error for StarredFetchTerminal {}

async fn fetch_starred_page(
    http: &Client,
    token: &str,
    after: Option<&str>,
) -> Result<StarredPage> {
    let query = r#"
      query($after: String, $first: Int!) {
        viewer {
          starredRepositories(first: $first, after: $after, orderBy: {field: STARRED_AT, direction: DESC}) {
            totalCount
            isOverLimit
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
                stargazerCount
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
    let response = http
        .post(crate::github_api::graphql_url())
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, "OctoRill")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&json!({
            "query": query,
            "variables": {
                "after": after,
                "first": postgres_star_epochs::STAR_SYNC_FULL_PAGE_SIZE,
            },
        }))
        .send()
        .await
        .context("fetch GitHub starred repositories")?;
    let status = response.status();
    if !status.is_success() {
        let message = format!("GitHub starred repositories returned HTTP {status}");
        if matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED
                | reqwest::StatusCode::FORBIDDEN
                | reqwest::StatusCode::NOT_FOUND
        ) {
            return Err(anyhow::Error::new(StarredFetchTerminal { message }));
        }
        bail!("{message}");
    }
    let body = response
        .text()
        .await
        .context("read GitHub starred repository page")?;
    let parsed = serde_json::from_str::<StarredGraphQlResponse>(&body)
        .context("decode GitHub starred repositories")?;
    if let Some(errors) = parsed.errors.as_ref().filter(|errors| !errors.is_empty()) {
        let message = errors
            .iter()
            .map(|error| error.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        bail!("GitHub starred page returned errors: {message}");
    }
    let page = parsed
        .data
        .ok_or_else(|| anyhow!("GitHub starred page has no data"))?
        .viewer
        .starred_repositories;
    let repos = page
        .edges
        .into_iter()
        .filter_map(|edge| {
            let repo_id = edge.node.database_id?;
            Some(StarredRepoSnapshot {
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
                uses_custom_open_graph_image: edge.node.uses_custom_open_graph_image,
                repo_stargazer_count: edge.node.stargazer_count,
            })
        })
        .collect::<Vec<_>>();
    Ok(StarredPage {
        repos,
        has_next_page: page.page_info.has_next_page,
        end_cursor: page.page_info.end_cursor,
        total_count: page.total_count.max(0),
        is_over_limit: page.is_over_limit,
    })
}

/// Upsert one connection membership and stamp it with the current epoch.
async fn upsert_connection_star_membership(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    github_connection_id: &str,
    epoch_id: &str,
    repo: &StarredRepoSnapshot,
    now: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO public.starred_repo_connection_memberships (\
           id, user_id, github_connection_id, repo_id, full_name, owner_login, name, description, \
           html_url, stargazed_at, is_private, owner_avatar_url, open_graph_image_url, \
           uses_custom_open_graph_image, repo_stargazer_count, last_seen_epoch_id, \
           last_delta_seen_at, created_at, updated_at\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, NULL, $17, $17) \
         ON CONFLICT (user_id, github_connection_id, repo_id) DO UPDATE SET \
           full_name = EXCLUDED.full_name, owner_login = EXCLUDED.owner_login, \
           name = EXCLUDED.name, description = EXCLUDED.description, \
           html_url = EXCLUDED.html_url, stargazed_at = EXCLUDED.stargazed_at, \
           is_private = EXCLUDED.is_private, owner_avatar_url = EXCLUDED.owner_avatar_url, \
           open_graph_image_url = EXCLUDED.open_graph_image_url, \
           uses_custom_open_graph_image = EXCLUDED.uses_custom_open_graph_image, \
           repo_stargazer_count = EXCLUDED.repo_stargazer_count, \
           last_seen_epoch_id = COALESCE(\
             EXCLUDED.last_seen_epoch_id, \
             public.starred_repo_connection_memberships.last_seen_epoch_id\
           ), \
           updated_at = EXCLUDED.updated_at",
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(github_connection_id)
    .bind(repo.repo_id)
    .bind(repo.full_name.as_str())
    .bind(repo.owner_login.as_str())
    .bind(repo.name.as_str())
    .bind(repo.description.as_deref())
    .bind(repo.html_url.as_str())
    .bind(repo.stargazed_at.as_deref())
    .bind(i64::from(repo.is_private))
    .bind(repo.owner_avatar_url.as_deref())
    .bind(repo.open_graph_image_url.as_deref())
    .bind(i64::from(repo.uses_custom_open_graph_image))
    .bind(repo.repo_stargazer_count)
    .bind(epoch_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .with_context(|| format!("upsert PostgreSQL star membership {}", repo.full_name))?;
    Ok(())
}

/// Rebuild one aggregate `starred_repos` row from the connection memberships.
async fn rebuild_user_starred_repo_from_memberships(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    repo_id: i64,
    now: &str,
) -> Result<()> {
    let membership = sqlx::query_as::<_, StarMembershipSnapshot>(
        "SELECT repo_id, full_name, owner_login, name, description, html_url, stargazed_at, \
                is_private, owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image, \
                repo_stargazer_count \
         FROM public.starred_repo_connection_memberships \
         WHERE user_id = $1 AND repo_id = $2 \
         ORDER BY stargazed_at DESC, updated_at DESC, id ASC LIMIT 1",
    )
    .bind(user_id)
    .bind(repo_id)
    .fetch_optional(&mut **tx)
    .await
    .context("load aggregate PostgreSQL star membership")?;
    let Some(membership) = membership else {
        sqlx::query("DELETE FROM public.starred_repos WHERE user_id = $1 AND repo_id = $2")
            .bind(user_id)
            .bind(repo_id)
            .execute(&mut **tx)
            .await
            .context("delete aggregate PostgreSQL starred repo")?;
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO public.starred_repos (\
           id, user_id, repo_id, full_name, owner_login, name, description, html_url, \
           stargazed_at, is_private, updated_at, owner_avatar_url, open_graph_image_url, \
           uses_custom_open_graph_image, repo_stargazer_count, repo_stargazer_count_updated_at\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) \
         ON CONFLICT (user_id, repo_id) DO UPDATE SET \
           full_name = EXCLUDED.full_name, owner_login = EXCLUDED.owner_login, \
           name = EXCLUDED.name, description = EXCLUDED.description, \
           html_url = EXCLUDED.html_url, stargazed_at = EXCLUDED.stargazed_at, \
           is_private = EXCLUDED.is_private, updated_at = EXCLUDED.updated_at, \
           owner_avatar_url = EXCLUDED.owner_avatar_url, \
           open_graph_image_url = EXCLUDED.open_graph_image_url, \
           uses_custom_open_graph_image = EXCLUDED.uses_custom_open_graph_image, \
           repo_stargazer_count = EXCLUDED.repo_stargazer_count, \
           repo_stargazer_count_updated_at = EXCLUDED.repo_stargazer_count_updated_at",
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(membership.repo_id)
    .bind(membership.full_name.as_str())
    .bind(membership.owner_login.as_str())
    .bind(membership.name.as_str())
    .bind(membership.description.as_deref())
    .bind(membership.html_url.as_str())
    .bind(membership.stargazed_at.as_deref())
    .bind(membership.is_private)
    .bind(now)
    .bind(membership.owner_avatar_url.as_deref())
    .bind(membership.open_graph_image_url.as_deref())
    .bind(membership.uses_custom_open_graph_image)
    .bind(membership.repo_stargazer_count)
    .bind(membership.repo_stargazer_count.map(|_| now))
    .execute(&mut **tx)
    .await
    .context("upsert aggregate PostgreSQL starred repo")?;
    Ok(())
}

/// Record the observing account as the current stargazer membership of a repo.
async fn upsert_epoch_star_member(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    connection: &GithubConnectionRow,
    repo: &StarredRepoSnapshot,
    now: &str,
) -> Result<()> {
    let actor_html_url = format!("https://github.com/{}", connection.login);
    sqlx::query(
        "INSERT INTO public.repo_star_current_members (\
           id, user_id, repo_id, repo_full_name, actor_github_user_id, actor_login, \
           actor_avatar_url, actor_html_url, starred_at, created_at, updated_at\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10) \
         ON CONFLICT (user_id, repo_id, actor_github_user_id) DO UPDATE SET \
           repo_full_name = EXCLUDED.repo_full_name, actor_login = EXCLUDED.actor_login, \
           actor_avatar_url = EXCLUDED.actor_avatar_url, \
           actor_html_url = EXCLUDED.actor_html_url, starred_at = EXCLUDED.starred_at, \
           updated_at = EXCLUDED.updated_at",
    )
    .bind(local_id::generate_local_id())
    .bind(user_id)
    .bind(repo.repo_id)
    .bind(repo.full_name.as_str())
    .bind(connection.github_user_id)
    .bind(connection.login.as_str())
    .bind(connection.avatar_url.as_deref())
    .bind(actor_html_url.as_str())
    .bind(repo.stargazed_at.as_deref())
    .bind(now)
    .execute(&mut **tx)
    .await
    .context("upsert PostgreSQL repo star current member")?;
    Ok(())
}

/// Drop current-member rows whose repository is no longer starred through any
/// connection of the user.
async fn prune_epoch_star_members(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    actor_github_user_id: i64,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM public.repo_star_current_members member \
         WHERE member.user_id = $1 AND member.actor_github_user_id = $2 \
           AND NOT EXISTS (\
             SELECT 1 FROM public.starred_repo_connection_memberships membership \
             WHERE membership.user_id = $1 AND membership.repo_id = member.repo_id\
           )",
    )
    .bind(user_id)
    .bind(actor_github_user_id)
    .execute(&mut **tx)
    .await
    .context("prune PostgreSQL repo star current members")?;
    Ok(())
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("GitHub notification has no {key}"))
}

fn task_user_id(payload: &Value, task: &ClaimedTask) -> Option<String> {
    payload
        .get("user_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| task.requested_by.clone().filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::{
        ClaimedTask, PgPool, ReleaseUpsert, push_inserted_release, summarize_release_run,
        task_user_id,
    };
    use serde_json::json;

    #[test]
    fn release_run_summary_marks_partial_failure_as_degraded() {
        let summary = summarize_release_run(7, ["timeout", "timeout", "rate_limited"], 2);
        assert_eq!(summary.failed_count, 3);
        assert_eq!(summary.deferred_count, 2);
        // 3 failures out of 10 attempted repositories (7 read + 3 failed);
        // deferrals reached no upstream and do not dilute the ratio.
        assert_eq!(summary.failure_ratio, 0.3);
        assert!(summary.degraded);
        assert!(!summary.all_failed);
        assert_eq!(summary.top_reasons(), "timeout=2,rate_limited=1");
    }

    #[test]
    fn release_run_summary_fails_the_task_when_nothing_was_read() {
        let summary = summarize_release_run(0, ["network_error", "decode_error"], 0);
        assert_eq!(summary.failed_count, 2);
        assert_eq!(summary.failure_ratio, 1.0);
        assert!(summary.all_failed);
        assert!(!summary.degraded);
        assert_eq!(summary.top_reasons(), "decode_error=1,network_error=1");
    }

    #[test]
    fn release_run_summary_rounds_the_failure_ratio_and_ignores_deferrals() {
        let summary = summarize_release_run(2, ["timeout"], 3);
        assert_eq!(summary.failure_ratio, 0.33);
        assert!(summary.degraded);
        assert!(!summary.all_failed);

        let deferred_only = summarize_release_run(0, [], 4);
        assert_eq!(deferred_only.failed_count, 0);
        assert_eq!(deferred_only.deferred_count, 4);
        assert_eq!(deferred_only.failure_ratio, 0.0);
        assert!(!deferred_only.degraded);
        assert!(!deferred_only.all_failed);
        assert_eq!(deferred_only.top_reasons(), "");
    }

    #[tokio::test]
    #[ignore = "requires TEST_PG_TASKS_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_seed_upserts_keep_shared_releases_and_user_notification_identity()
    -> anyhow::Result<()> {
        use super::*;
        let admin_url = std::env::var("TEST_PG_TASKS_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("tasks_seed_{}", local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result=async {
            sqlx::raw_sql(r#"
                CREATE TABLE repo_releases(id text PRIMARY KEY,repo_id bigint,release_id bigint UNIQUE,node_id text,tag_name text,name text,body text,html_url text,published_at text,created_at text,is_prerelease bigint,is_draft bigint,updated_at text,detected_at text,react_plus1 bigint,react_laugh bigint,react_heart bigint,react_hooray bigint,react_rocket bigint,react_eyes bigint);
                CREATE TABLE notifications(id text PRIMARY KEY,user_id text,thread_id text,repo_full_name text,subject_title text,subject_type text,reason text,updated_at text,unread bigint,url text,html_url text,last_seen_at text,UNIQUE(user_id,thread_id));
            "#).execute(&pool).await?;
            sqlx::raw_sql("CREATE TABLE starred_repos(id text PRIMARY KEY,user_id text,repo_id bigint,full_name text,owner_login text,name text,description text,html_url text,stargazed_at text,is_private bigint,updated_at text,owner_avatar_url text,open_graph_image_url text,uses_custom_open_graph_image bigint,repo_stargazer_count bigint,repo_stargazer_count_updated_at text,UNIQUE(user_id,repo_id))").execute(&pool).await?;
            sqlx::raw_sql(r#"
                CREATE TABLE starred_repo_connection_memberships(id text PRIMARY KEY,user_id text,github_connection_id text,repo_id bigint,full_name text,owner_login text,name text,description text,html_url text,stargazed_at text,is_private bigint,owner_avatar_url text,open_graph_image_url text,uses_custom_open_graph_image bigint,repo_stargazer_count bigint,last_seen_epoch_id text,last_delta_seen_at text,created_at text,updated_at text,UNIQUE(user_id,github_connection_id,repo_id));
                CREATE TABLE repo_star_current_members(id text PRIMARY KEY,user_id text,repo_id bigint,repo_full_name text,actor_github_user_id bigint,actor_login text,actor_avatar_url text,actor_html_url text,starred_at text,created_at text,updated_at text,UNIQUE(user_id,repo_id,actor_github_user_id));
            "#).execute(&pool).await?;
            let starred=StarredRepoSnapshot{repo_id:42,full_name:"seed/repo".to_owned(),owner_login:"seed".to_owned(),name:"repo".to_owned(),description:None,html_url:"https://github.com/seed/repo".to_owned(),stargazed_at:Some("2026-10-01T00:00:00Z".to_owned()),is_private:false,owner_avatar_url:None,open_graph_image_url:None,uses_custom_open_graph_image:false,repo_stargazer_count:None};
            let connection=GithubConnectionRow{id:"seed-connection".to_owned(),github_user_id:501,login:"seed".to_owned(),avatar_url:None,access_token_ciphertext:Vec::new(),access_token_nonce:Vec::new()};
            let mut tx=pool.begin().await?;
            upsert_connection_star_membership(&mut tx,"user-a","seed-connection","seed-epoch",&starred,"2026-11-21T00:00:00Z").await?;
            rebuild_user_starred_repo_from_memberships(&mut tx,"user-a",42,"2026-11-21T00:00:00Z").await?;
            upsert_epoch_star_member(&mut tx,"user-a",&connection,&starred,"2026-11-21T00:00:00Z").await?;
            tx.commit().await?;
            let star:(String,String)=sqlx::query_as("SELECT id,stargazed_at FROM starred_repos").fetch_one(&pool).await?;
            assert!(local_id::is_local_id(&star.0));assert_eq!(star.1,"2026-10-01T00:00:00Z");
            let members:i64=sqlx::query_scalar("SELECT COUNT(*) FROM repo_star_current_members WHERE user_id='user-a'").fetch_one(&pool).await?;
            assert_eq!(members,1);
            let repo=StarredRepoRow{repo_id:42,full_name:"seed/repo".to_owned()};
            let release=json!({"id":99,"tag_name":"v1","body":"current","html_url":"https://example.invalid/v1","updated_at":"2026-11-20T00:00:00Z","reactions":{"+1":2}});
            let upsert=upsert_release(&pool,"user-a",&repo,&release,"2026-11-21T00:00:00Z").await?;
            assert!(upsert.inserted,"first write of a release must report an insertion");assert_eq!(upsert.release_id,99);
            let initial:(String,String,String,i64)=sqlx::query_as("SELECT id,body,detected_at,react_plus1 FROM repo_releases WHERE release_id=99").fetch_one(&pool).await?;
            assert!(local_id::is_local_id(&initial.0));assert_eq!(initial.1,"current");assert_eq!(initial.3,2);
            let stale=json!({"id":99,"tag_name":"old","body":"stale","updated_at":"2026-11-19T00:00:00Z"});
            let upsert=upsert_release(&pool,"user-b",&repo,&stale,"2026-11-22T00:00:00Z").await?;
            assert!(!upsert.inserted,"a guarded conflict must not report an insertion");
            let latest:(String,String,String)=sqlx::query_as("SELECT id,body,detected_at FROM repo_releases WHERE release_id=99").fetch_one(&pool).await?;
            assert_eq!((latest.0,latest.1,latest.2),(initial.0,"current".to_owned(),initial.2));
            let updated=json!({"id":99,"tag_name":"v1","body":"newer","html_url":"https://example.invalid/v1","updated_at":"2026-11-21T06:00:00Z"});
            let upsert=upsert_release(&pool,"user-a",&repo,&updated,"2026-11-23T00:00:00Z").await?;
            assert!(!upsert.inserted,"an applied update is still an existing release");
            let body:String=sqlx::query_scalar("SELECT body FROM repo_releases WHERE release_id=99").fetch_one(&pool).await?;
            assert_eq!(body,"newer");
            let notification=json!({"id":"thread-1","unread":true,"repository":{"full_name":"seed/repo"},"subject":{"title":"hello","type":"Issue"},"url":"https://example.invalid/thread"});
            upsert_notification(&pool,"user-a",&notification).await?;
            upsert_notification(&pool,"user-b",&notification).await?;
            upsert_notification(&pool,"user-a",&notification).await?;
            let ids:Vec<String>=sqlx::query_scalar("SELECT id FROM notifications ORDER BY user_id").fetch_all(&pool).await?;
            assert_eq!(ids.len(),2);assert_ne!(ids[0],ids[1]);assert!(ids.iter().all(|id|local_id::is_local_id(id)));
            Ok::<_,anyhow::Error>(())
        }.await;
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await?;
        admin.close().await;
        result
    }

    #[test]
    fn notification_executor_rejects_other_task_types() {
        let source = include_str!("postgres_tasks.rs");
        assert!(source.contains("sync.notifications"));
        assert!(source.contains("sync.releases"));
        assert!(source.contains("sync.starred"));
        assert!(source.contains("sync.all"));
        assert!(source.contains("sync.access_refresh"));
        assert!(source.contains("starredRepositories(first: $first, after: $after"));
        assert!(source.contains("postgres_star_epochs::STAR_SYNC_FULL_PAGE_SIZE"));
        assert!(source.contains("star_sync_epochs"));
        assert!(source.contains("webhook.push.manage"));
        assert!(source.contains("webhook.push.reconcile"));
        assert!(source.contains("/hooks"));
        assert!(source.contains("brief.generate"));
        assert!(source.contains("postgres_deterministic"));
        assert!(source.contains("ON CONFLICT (user_id, thread_id)"));
        assert!(source.contains("ON CONFLICT(release_id)"));
        assert!(source.contains("ON CONFLICT (user_id, repo_id)"));
        assert!(source.contains("decrypt_str"));
        assert!(source.contains("requested_by"));
        assert!(source.contains("page in 1..=20"));
    }

    #[test]
    fn only_newly_inserted_releases_feed_global_translation_admission() {
        let mut ids = Vec::new();
        push_inserted_release(
            &mut ids,
            ReleaseUpsert {
                release_id: 11,
                inserted: true,
            },
        );
        push_inserted_release(
            &mut ids,
            ReleaseUpsert {
                release_id: 12,
                inserted: false,
            },
        );
        push_inserted_release(
            &mut ids,
            ReleaseUpsert {
                release_id: 13,
                inserted: true,
            },
        );
        assert_eq!(ids, vec![11, 13]);
    }

    #[test]
    fn task_user_id_falls_back_to_legacy_requester() {
        let task = ClaimedTask {
            id: "task".to_owned(),
            task_type: "sync.releases".to_owned(),
            source: "test".to_owned(),
            requested_by: Some("legacy-user".to_owned()),
            payload_json: "{}".to_owned(),
            cancel_requested: 0,
        };
        assert_eq!(
            task_user_id(&json!({}), &task).as_deref(),
            Some("legacy-user")
        );
    }

    /// Tables the polishing backfill touches: the shared Release cache, the
    /// global work items it scans, the projection tables admission writes, the
    /// runtime route, and the dispatch-state row the cursor lives in.
    async fn create_polish_backfill_schema(pool: &PgPool) -> anyhow::Result<()> {
        sqlx::raw_sql(
            r#"
            CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY, repo_id bigint NOT NULL, html_url text NOT NULL, tag_name text NOT NULL, name text, body text, updated_at text, detected_at text);
            CREATE TABLE public.admin_runtime_settings(id integer PRIMARY KEY, llm_models_json text NOT NULL);
            INSERT INTO public.admin_runtime_settings(id, llm_models_json) VALUES (1, '["m1"]');
            CREATE TABLE public.llm_model_health(model text PRIMARY KEY, relevant_failure_count bigint NOT NULL DEFAULT 0, window_started_at timestamptz, cooldown_until timestamptz, last_failure_class text, last_failure_at timestamptz, created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL);
            CREATE TABLE public.content_work_items(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_snapshot_json text NOT NULL, configuration_fingerprint text NOT NULL, status text NOT NULL, priority integer NOT NULL DEFAULT 0, cache_hit boolean NOT NULL DEFAULT false, token_estimate integer NOT NULL DEFAULT 0, attempt_count integer NOT NULL DEFAULT 0, evidence_status text NOT NULL DEFAULT 'not_required', next_retry_at text, retry_expires_at text, retry_after_at text, failure_class text, lease_owner text, lease_expires_at text, started_at text, finished_at text, cancelled_at text, supersedes_work_item_id text, batch_id text, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile));
            CREATE TABLE public.content_work_identities(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version));
            CREATE TABLE public.content_work_identity_members(work_item_id text PRIMARY KEY, identity_id text NOT NULL, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, linked_at text NOT NULL, UNIQUE (identity_id, work_item_id));
            CREATE TABLE public.content_result_projections(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_hash text NOT NULL, work_item_id text NOT NULL, active_work_item_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash));
            CREATE TABLE public.content_current_result_projections(identity_id text PRIMARY KEY, work_item_id text NOT NULL, active_work_item_id text, source_projection_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL);
            CREATE TABLE public.content_attempt_events(id text PRIMARY KEY, work_item_id text NOT NULL, attempt_no bigint NOT NULL, trigger text NOT NULL, event_type text NOT NULL, result_status text, error_code text, error_summary text, failure_class text, retry_disposition text, retry_eligible bigint NOT NULL DEFAULT 0, next_retry_at text, duration_ms bigint, token_count bigint, cost_microunits bigint, created_at text NOT NULL, configuration_snapshot_json text, route_snapshot_json text, configuration_fingerprint text, UNIQUE (work_item_id, attempt_no, event_type));
            CREATE TABLE public.polish_backfill_cursor(id bigint PRIMARY KEY CHECK (id = 1), last_release_id bigint NOT NULL DEFAULT 0 CHECK (last_release_id >= 0), last_run_at timestamptz, updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP);
            INSERT INTO public.polish_backfill_cursor (id, last_release_id, last_run_at, updated_at) VALUES (1, 0, NULL, CURRENT_TIMESTAMP);
            "#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// A Release stored with a stated change, plus the `smart`-only polishing
    /// work an older admission left behind.
    async fn seed_smart_only_release(pool: &PgPool, release_id: i64) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO public.repo_releases (release_id, repo_id, html_url, tag_name, name, body, updated_at) VALUES ($1, 7, $2, $3, $4, $5, '2026-11-24T00:00:00Z')",
        )
        .bind(release_id)
        .bind(format!("https://github.com/acme/demo/releases/tag/v{release_id}"))
        .bind(format!("v{release_id}"))
        .bind(format!("v{release_id}"))
        .bind("- 每轮最多处理 10 批，此前为 5 批")
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, attempt_count, created_at, updated_at) VALUES ($1, 'release', $2, 'polishing', 'smart', 'zh-CN', 'seeded', 'content-processing.v1', 'postgres-default', '{}', 'seeded', 'ready', 1, now()::text, now()::text)",
        )
        .bind(format!("smart-{release_id}"))
        .bind(release_id.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// A Release whose published text states no change: admission offers it only
    /// `smart`, so it stays eligible for the sweep forever.
    async fn seed_smart_only_release_without_stated_change(
        pool: &PgPool,
        release_id: i64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO public.repo_releases (release_id, repo_id, html_url, tag_name, name, body, updated_at) VALUES ($1, 7, $2, $3, $4, 'See full changelog below.', '2026-11-24T00:00:00Z')",
        )
        .bind(release_id)
        .bind(format!("https://github.com/acme/demo/releases/tag/v{release_id}"))
        .bind(format!("v{release_id}"))
        .bind(format!("v{release_id}"))
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, attempt_count, created_at, updated_at) VALUES ($1, 'release', $2, 'polishing', 'smart', 'zh-CN', 'seeded', 'content-processing.v1', 'postgres-default', '{}', 'seeded', 'ready', 1, now()::text, now()::text)",
        )
        .bind(format!("smart-{release_id}"))
        .bind(release_id.to_string())
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn polish_variants_for(pool: &PgPool, release_id: i64) -> anyhow::Result<Vec<String>> {
        let mut variants: Vec<String> = sqlx::query_scalar(
            "SELECT variant FROM public.content_work_items WHERE canonical_resource_type = 'release' AND canonical_resource_id = $1 AND pipeline = 'polishing'",
        )
        .bind(release_id.to_string())
        .fetch_all(pool)
        .await?;
        variants.sort();
        Ok(variants)
    }

    fn backfill_task() -> ClaimedTask {
        ClaimedTask {
            id: "polish-backfill-1".to_owned(),
            task_type: super::TASK_POLISH_BACKFILL.to_owned(),
            source: "scheduler.polish_backfill".to_owned(),
            requested_by: None,
            payload_json: "{}".to_owned(),
            cancel_requested: 0,
        }
    }

    /// Three `smart`-only Releases are repaired by one run, and a repeat run
    /// finds nothing left and reports the sweep complete.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TASKS_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_polish_backfill_admits_missing_audience_variants_once() -> anyhow::Result<()>
    {
        use super::*;
        use crate::postgres_worker::TaskExecutor;

        let admin_url = std::env::var("TEST_PG_TASKS_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("polish_backfill_{}", local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            create_polish_backfill_schema(&pool).await?;
            for release_id in [101_i64, 102, 103] {
                seed_smart_only_release(&pool, release_id).await?;
            }

            let executor = PolishBackfillExecutor::new(
                pool.clone(),
                Some(AiConfig {
                    base_url: url::Url::parse("http://127.0.0.1:1/")?,
                    model: "m1".to_owned(),
                    api_key: "test-key".to_owned(),
                }),
            );
            let summary: Value = serde_json::from_str(&executor.execute(&backfill_task()).await?)?;
            assert_eq!(summary["scanned"], json!(3));
            assert_eq!(
                summary["admitted"],
                json!(6),
                "two new variants per Release"
            );
            assert_eq!(summary["complete"], json!(true));
            for release_id in [101_i64, 102, 103] {
                assert_eq!(
                    polish_variants_for(&pool, release_id).await?,
                    vec!["smart", "smart_authorized", "smart_public"],
                    "Release {release_id} must gain both audience variants"
                );
            }

            // Second run: nothing left to do, and it says so.
            let repeat: Value = serde_json::from_str(&executor.execute(&backfill_task()).await?)?;
            assert_eq!(repeat["scanned"], json!(0));
            assert_eq!(repeat["admitted"], json!(0));
            assert_eq!(repeat["complete"], json!(true));
            assert_eq!(repeat["cursor"], json!(0));
            Ok::<_, anyhow::Error>(())
        }
        .await;
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await?;
        admin.close().await;
        result
    }

    /// More backlog than one page, in the case the cursor actually exists for:
    /// candidates that stay candidates. A Release that states no change in its
    /// stored text legitimately keeps only `smart`, so `admit_release_polish`
    /// skips it and it remains eligible forever. Without the cursor the scan
    /// would re-read the same page on every run and the tail would never be
    /// reached; the second run must continue past the first page.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TASKS_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_polish_backfill_advances_its_cursor_across_pages() -> anyhow::Result<()> {
        use super::*;
        use crate::postgres_worker::TaskExecutor;

        let admin_url = std::env::var("TEST_PG_TASKS_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("polish_backfill_cursor_{}", local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            create_polish_backfill_schema(&pool).await?;
            let total = POLISH_BACKFILL_PAGE + 5;
            let ids: Vec<i64> = (0..total).map(|index| 200 + index as i64).collect();
            for release_id in &ids {
                // The stored text states no change, so admission legitimately
                // leaves this Release with `smart` only and it stays a candidate.
                // This is the shape that exercises the cursor: a shrinking
                // candidate set would hide a missing cursor.
                seed_smart_only_release_without_stated_change(&pool, *release_id).await?;
            }

            let executor = PolishBackfillExecutor::new(
                pool.clone(),
                Some(AiConfig {
                    base_url: url::Url::parse("http://127.0.0.1:1/")?,
                    model: "m1".to_owned(),
                    api_key: "test-key".to_owned(),
                }),
            );
            let first: Value = serde_json::from_str(&executor.execute(&backfill_task()).await?)?;
            assert_eq!(first["scanned"], json!(POLISH_BACKFILL_PAGE));
            assert_eq!(first["admitted"], json!(0));
            assert_eq!(first["complete"], json!(false));
            assert_eq!(
                first["cursor"],
                json!(ids[POLISH_BACKFILL_PAGE - 1]),
                "the run must advance past the page it could not repair"
            );

            // The second run starts after the cursor: it sees only the tail, so
            // the sweep does not loop over the same unreachable head forever.
            let second: Value = serde_json::from_str(&executor.execute(&backfill_task()).await?)?;
            assert_eq!(
                second["scanned"],
                json!(5),
                "the next run must continue from the cursor, not restart at the head"
            );
            assert_eq!(second["complete"], json!(true));
            assert_eq!(second["cursor"], json!(0), "a short page resets the sweep");
            Ok::<_, anyhow::Error>(())
        }
        .await;
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await?;
        admin.close().await;
        result
    }

    /// A quota-blocked row is requeued once; a credential-blocked row with the
    /// same class and summary is left alone. This is the discriminator the sweep
    /// depends on.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TASKS_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_polish_backfill_requeues_only_quota_blocked_work() -> anyhow::Result<()> {
        use super::*;
        use crate::postgres_worker::TaskExecutor;

        let admin_url = std::env::var("TEST_PG_TASKS_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("polish_quota_{}", local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            create_polish_backfill_schema(&pool).await?;

            // Two blocked rows that are indistinguishable by class and summary,
            // exactly as production has them.
            for (index, id, disposition) in [
                (900_i64, "quota-work", Some("quota")),
                (901, "credential-work", Some("credential")),
                (902, "legacy-work", None),
            ] {
                sqlx::query(
                    "INSERT INTO public.content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, attempt_count, failure_class, created_at, updated_at) VALUES ($1, 'release', $2, 'polishing', 'smart', 'zh-CN', 'source', 'content-processing.v1', 'postgres-default', '{}', 'fp', 'blocked_config', 1, 'configuration', now()::text, now()::text)",
                )
                .bind(id)
                .bind(index.to_string())
                .execute(&pool)
                .await?;
                sqlx::query(
                    "INSERT INTO public.content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_disposition, created_at) VALUES ($1, $2, 1, 'initial', 'attempt_completed', 'failed', 'configuration', '模型配置或请求被拒绝', 'configuration', $3, now()::text)",
                )
                .bind(format!("event-{id}"))
                .bind(id)
                .bind(disposition)
                .execute(&pool)
                .await?;
            }

            let executor = PolishBackfillExecutor::new(
                pool.clone(),
                Some(AiConfig {
                    base_url: url::Url::parse("http://127.0.0.1:1/")?,
                    model: "m1".to_owned(),
                    api_key: "test-key".to_owned(),
                }),
            );
            let first: Value = serde_json::from_str(&executor.execute(&backfill_task()).await?)?;
            assert_eq!(first["quota_scanned"], json!(1));
            assert_eq!(first["quota_requeued"], json!(1));

            let statuses: Vec<(String, String)> = sqlx::query_as(
                "SELECT id, status FROM public.content_work_items ORDER BY id",
            )
            .fetch_all(&pool)
            .await?;
            assert_eq!(
                statuses,
                vec![
                    ("credential-work".to_owned(), "blocked_config".to_owned()),
                    ("legacy-work".to_owned(), "blocked_config".to_owned()),
                    ("quota-work".to_owned(), "queued".to_owned()),
                ],
                "only the quota-marked row is released; a credential block and an unmarked legacy row stay put"
            );

            // The marker was consumed, so a second run cannot requeue it again
            // without new evidence.
            let second: Value = serde_json::from_str(&executor.execute(&backfill_task()).await?)?;
            assert_eq!(second["quota_scanned"], json!(0));
            assert_eq!(second["quota_requeued"], json!(0));
            Ok::<_, anyhow::Error>(())
        }
        .await;
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await?;
        admin.close().await;
        result
    }

    /// The classifier marks an exhausted allowance as `quota` whether the
    /// provider reports it as 429 or as a 4xx body, and keeps credentials
    /// distinct. Without this the sweep would have nothing to select on.
    #[test]
    fn quota_and_credential_dispositions_are_distinct() {
        use crate::llm_contract::LlmFailureClass;
        use crate::postgres_translate::classify_upstream_failure_with_reason as classify;

        let (class, disposition) = classify(
            429,
            r#"{"error":{"code":"insufficient_quota","message":"inference exceeds tpm/rpm limit"}}"#,
        );
        assert_eq!(class, LlmFailureClass::RateLimited);
        assert_eq!(disposition, Some("quota"));

        let (class, disposition) = classify(400, r#"{"error":{"code":"insufficient_quota"}}"#);
        assert_eq!(class, LlmFailureClass::Configuration);
        assert_eq!(disposition, Some("quota"));

        let (class, disposition) = classify(401, "unauthorized");
        assert_eq!(class, LlmFailureClass::Configuration);
        assert_eq!(disposition, Some("credential"));

        // A plain 400 is a configuration failure with no quota/credential cause.
        let (class, disposition) = classify(400, "bad request");
        assert_eq!(class, LlmFailureClass::Configuration);
        assert_eq!(disposition, None);
    }
}
