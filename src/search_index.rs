use std::{
    ffi::CString,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
use tokio::task::AbortHandle;
use tower_sessions::Session;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    api,
    error::ApiError,
    sqlite_write::{
        SqliteWritePriority, is_sqlite_background_admission_error, is_sqlite_write_deadline_error,
    },
    state::AppState,
};

const INDEX_BATCH_SIZE: i64 = 25;
const DEFAULT_MIN_FREE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const BACKGROUND_BATCH_DELAY: Duration = Duration::from_millis(10);
const RETRY_DELAY: Duration = Duration::from_secs(5);
const PROJECTION_POLL_DELAY: Duration = Duration::from_millis(50);
const PROJECTION_PROGRESS_DELAY: Duration = Duration::from_millis(10);
const PROJECTION_IDLE_DELAY: Duration = Duration::from_secs(2);
const PROJECTION_RETRY_DELAY: Duration = Duration::from_secs(5);
const PROJECTION_TERMINAL_RETRY_DELAY: Duration = Duration::from_secs(30);
const PROJECTION_TERMINAL_RECOVERY_CADENCE: Duration = Duration::from_secs(5);
const PROJECTION_LEASE_SECONDS: i64 = 5;
const PROJECTION_MAX_ATTEMPTS: i64 = 8;
const PROJECTION_TRANSACTION_BUDGET: Duration = Duration::from_millis(250);
const PROJECTION_FANOUT_BATCH_SIZE: i64 = 100;
const MISSING_SEARCH_DOCUMENT_ERROR: &str = "missing_search_document";

const PHASES: &[&str] = &[
    "releases",
    "announcements",
    "notifications",
    "briefs",
    "repo_associations",
    "starred_repos",
    "content_projections",
    "translations",
    "metadata",
    "fts_documents",
    "fts_user_lanes",
];

#[cfg(test)]
pub(crate) const PHASE_COUNT: usize = PHASES.len();

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BatchOutcome {
    Progress,
    Idle,
    Paused,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ProjectionRunOutcome {
    Committed,
    Idle,
    AdmissionDenied,
    Deferred,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum ProjectionApplyOutcome {
    Complete,
    Continue { cursor: String },
    Deferred,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct BackfillState {
    phase: String,
    cursor: i64,
    status: String,
}

pub fn spawn_worker(state: Arc<AppState>) -> AbortHandle {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        loop {
            match run_batch_once(state.as_ref()).await {
                Ok(BatchOutcome::Ready) => tokio::time::sleep(RETRY_DELAY).await,
                Ok(BatchOutcome::Progress) => tokio::time::sleep(BACKGROUND_BATCH_DELAY).await,
                Ok(BatchOutcome::Idle | BatchOutcome::Paused) => {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Ok(BatchOutcome::Failed) => {
                    warn!(
                        event = "search.index.failed_terminal",
                        "search projection backfill stopped after a terminal failure"
                    );
                    break;
                }
                Err(error) => {
                    if is_transient_backfill_error(&error) {
                        warn!(
                            ?error,
                            event = "search.index.deferred",
                            retry_after_seconds = RETRY_DELAY.as_secs(),
                            "search projection backfill deferred by writer admission or deadline"
                        );
                        tokio::time::sleep(RETRY_DELAY).await;
                    } else {
                        warn!(
                            ?error,
                            event = "search.index.failed_terminal",
                            "search projection backfill stopped after a failed batch"
                        );
                        break;
                    }
                }
            }
        }
    })
    .abort_handle()
}

pub fn spawn_realtime_projection_worker(state: Arc<AppState>) -> AbortHandle {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut next_terminal_recovery_at = tokio::time::Instant::now();
        loop {
            let allow_terminal_recovery = tokio::time::Instant::now() >= next_terminal_recovery_at;
            let outcome = run_projection_once(state.as_ref(), allow_terminal_recovery).await;
            if allow_terminal_recovery {
                next_terminal_recovery_at =
                    tokio::time::Instant::now() + PROJECTION_TERMINAL_RECOVERY_CADENCE;
            }
            match outcome {
                Ok(ProjectionRunOutcome::Committed) => {
                    tokio::time::sleep(PROJECTION_PROGRESS_DELAY).await
                }
                Ok(ProjectionRunOutcome::Idle) => tokio::time::sleep(PROJECTION_IDLE_DELAY).await,
                Ok(ProjectionRunOutcome::AdmissionDenied) => {
                    warn!(
                        event = "search.projection_outbox.admission_denied",
                        retry_after_ms = PROJECTION_POLL_DELAY.as_millis(),
                        "search projection outbox deferred by background writer admission"
                    );
                    tokio::time::sleep(PROJECTION_POLL_DELAY).await
                }
                Ok(ProjectionRunOutcome::Deferred) => {
                    tokio::time::sleep(PROJECTION_RETRY_DELAY).await
                }
                Err(error) => {
                    let retry_after = if is_sqlite_background_admission_error(error.as_ref())
                        || is_sqlite_write_deadline_error(error.as_ref())
                    {
                        PROJECTION_POLL_DELAY
                    } else {
                        PROJECTION_RETRY_DELAY
                    };
                    warn!(
                        ?error,
                        event = "search.projection_outbox.failed",
                        retry_after_ms = retry_after.as_millis(),
                        "search projection outbox item failed; source data remains committed"
                    );
                    tokio::time::sleep(retry_after).await;
                }
            }
        }
    })
    .abort_handle()
}

#[cfg(test)]
pub(crate) async fn run_projection_for_test(state: &AppState) -> Result<bool> {
    Ok(matches!(
        run_projection_once(state, true).await?,
        ProjectionRunOutcome::Committed
    ))
}

#[cfg(test)]
pub(crate) async fn reactivate_terminal_projection_for_test(state: &AppState) -> Result<bool> {
    reactivate_terminal_projection_once(state).await
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ProjectionOutboxRow {
    projection_key: String,
    operation: String,
    resource_type: String,
    resource_id: String,
    previous_resource_id: Option<String>,
    canonical_resource_type: Option<String>,
    user_id: Option<String>,
    attempt_count: i64,
    fanout_cursor: Option<String>,
}

async fn run_projection_once(
    state: &AppState,
    allow_terminal_recovery: bool,
) -> Result<ProjectionRunOutcome> {
    if !projection_work_due(&state.pool).await? {
        return Ok(ProjectionRunOutcome::Idle);
    }
    let Some(lane_guard) = state
        .sqlite_writer
        .try_admit_background_lane("search_projection_outbox", PROJECTION_POLL_DELAY)
    else {
        return Ok(ProjectionRunOutcome::AdmissionDenied);
    };

    if allow_terminal_recovery {
        match reactivate_terminal_projection_once(state).await {
            Ok(true) => {}
            Ok(false) => {}
            Err(error) => {
                warn!(
                    ?error,
                    event = "search.projection_outbox.terminal_reactivation_failed",
                    "terminal search projection recovery was deferred"
                );
            }
        }
    }
    if !active_projection_work_due(&state.pool).await? {
        drop(lane_guard);
        return Ok(ProjectionRunOutcome::Idle);
    }
    let (claim_permit, mut claim_tx) = state
        .sqlite_writer
        .begin_immediate_with_priority_until(
            &state.pool,
            "search_projection_outbox",
            SqliteWritePriority::Background,
            tokio::time::Instant::now() + PROJECTION_TRANSACTION_BUDGET,
        )
        .await?;
    let row = sqlx::query_as::<_, ProjectionOutboxRow>(
        "SELECT projection_key, operation, resource_type, resource_id, previous_resource_id, canonical_resource_type, user_id, attempt_count, fanout_cursor
         FROM search_projection_outbox
         WHERE available_at <= CURRENT_TIMESTAMP
           AND (lease_expires_at IS NULL OR lease_expires_at <= CURRENT_TIMESTAMP)
         ORDER BY available_at ASC, created_at ASC, projection_key ASC
         LIMIT 1",
    )
    .fetch_optional(&mut **claim_tx.as_transaction_mut())
    .await?;
    let Some(row) = row else {
        claim_tx.rollback().await?;
        drop(claim_permit);
        drop(lane_guard);
        return Ok(ProjectionRunOutcome::Idle);
    };
    let lease_owner = format!("{}:{}", state.runtime_owner_id, Uuid::new_v4());
    sqlx::query(
        "UPDATE search_projection_outbox
         SET lease_owner = ?, lease_expires_at = datetime('now', '+' || ? || ' seconds'),
             attempt_count = CASE WHEN last_error IS NULL THEN 1 ELSE attempt_count + 1 END,
             updated_at = CURRENT_TIMESTAMP
         WHERE projection_key = ?",
    )
    .bind(&lease_owner)
    .bind(PROJECTION_LEASE_SECONDS)
    .bind(&row.projection_key)
    .execute(&mut **claim_tx.as_transaction_mut())
    .await?;
    claim_tx.commit().await?;
    drop(claim_permit);

    let apply_transaction = state
        .sqlite_writer
        .begin_immediate_with_priority_until(
            &state.pool,
            "search_projection_outbox_apply",
            SqliteWritePriority::Background,
            tokio::time::Instant::now() + PROJECTION_TRANSACTION_BUDGET,
        )
        .await;
    let (permit, mut tx) = match apply_transaction {
        Ok(transaction) => transaction,
        Err(error) => {
            drop(lane_guard);
            recover_projection_claim(state, &row.projection_key, &lease_owner, &error).await;
            return Err(error);
        }
    };
    let current_lease = sqlx::query_scalar::<_, Option<String>>(
        "SELECT lease_owner FROM search_projection_outbox WHERE projection_key = ?",
    )
    .bind(&row.projection_key)
    .fetch_optional(&mut **tx.as_transaction_mut())
    .await;
    let current_lease = match current_lease {
        Ok(value) => value.flatten(),
        Err(error) => {
            let error = anyhow::Error::from(error);
            let _ = tx.rollback().await;
            drop(permit);
            drop(lane_guard);
            recover_projection_claim(state, &row.projection_key, &lease_owner, &error).await;
            return Err(error);
        }
    };
    if current_lease.as_deref() != Some(lease_owner.as_str()) {
        tx.rollback().await?;
        drop(permit);
        drop(lane_guard);
        return Ok(ProjectionRunOutcome::Idle);
    }

    let apply_outcome = match apply_projection_outbox_item(tx.as_transaction_mut(), &row).await {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = tx.rollback().await;
            drop(permit);
            drop(lane_guard);
            recover_projection_claim(state, &row.projection_key, &lease_owner, &error).await;
            return Err(error);
        }
    };

    let deferred = matches!(&apply_outcome, ProjectionApplyOutcome::Deferred);
    match apply_outcome {
        ProjectionApplyOutcome::Continue { cursor } => {
            if let Err(error) = sqlx::query(
                "UPDATE search_projection_outbox
                 SET fanout_cursor = ?, available_at = CURRENT_TIMESTAMP,
                     lease_owner = NULL, lease_expires_at = NULL,
                     last_error = NULL, updated_at = CURRENT_TIMESTAMP
                 WHERE projection_key = ? AND lease_owner = ?",
            )
            .bind(cursor)
            .bind(&row.projection_key)
            .bind(&lease_owner)
            .execute(&mut **tx.as_transaction_mut())
            .await
            {
                let _ = tx.rollback().await;
                drop(permit);
                drop(lane_guard);
                let error = anyhow::Error::from(error)
                    .context("advance announcement projection fanout cursor");
                recover_projection_claim(state, &row.projection_key, &lease_owner, &error).await;
                return Err(error);
            }
        }
        ProjectionApplyOutcome::Deferred => {
            if let Err(error) = record_projection_failure_in_transaction(
                tx.as_transaction_mut(),
                &row.projection_key,
                &lease_owner,
                MISSING_SEARCH_DOCUMENT_ERROR,
            )
            .await
            {
                let _ = tx.rollback().await;
                drop(permit);
                drop(lane_guard);
                recover_projection_claim(state, &row.projection_key, &lease_owner, &error).await;
                return Err(error);
            }
        }
        ProjectionApplyOutcome::Complete => {
            if let Err(error) = sqlx::query(
                "DELETE FROM search_projection_outbox WHERE projection_key = ? AND lease_owner = ?",
            )
            .bind(&row.projection_key)
            .bind(&lease_owner)
            .execute(&mut **tx.as_transaction_mut())
            .await
            {
                let _ = tx.rollback().await;
                drop(permit);
                drop(lane_guard);
                let error = anyhow::Error::from(error);
                recover_projection_claim(state, &row.projection_key, &lease_owner, &error).await;
                return Err(error);
            }
        }
    }
    if let Err(error) = tx.commit().await {
        drop(permit);
        drop(lane_guard);
        recover_projection_claim(state, &row.projection_key, &lease_owner, &error).await;
        return Err(error);
    }
    drop(permit);
    drop(lane_guard);
    if deferred {
        info!(
            event = "search.projection_outbox.deferred",
            projection_key = %row.projection_key,
            reason = MISSING_SEARCH_DOCUMENT_ERROR,
            attempt = row.attempt_count + 1,
            "search projection outbox item deferred until its base search document exists"
        );
        return Ok(ProjectionRunOutcome::Deferred);
    }
    info!(
        event = "search.projection_outbox.committed",
        projection_key = %row.projection_key,
        operation = %row.operation,
        attempt = row.attempt_count + 1,
        "search projection outbox item committed"
    );
    Ok(ProjectionRunOutcome::Committed)
}

async fn projection_work_due(pool: &SqlitePool) -> Result<bool> {
    let due = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(
           SELECT 1 FROM search_projection_outbox
           WHERE available_at <= CURRENT_TIMESTAMP
             AND (lease_expires_at IS NULL OR lease_expires_at <= CURRENT_TIMESTAMP)
         )
         OR EXISTS(
           SELECT 1 FROM search_projection_outbox_failed
           WHERE failed_at <= datetime('now', '-' || ? || ' seconds')
         )",
    )
    .bind(PROJECTION_TERMINAL_RETRY_DELAY.as_secs() as i64)
    .fetch_one(pool)
    .await
    .context("check due search projection work")?;
    Ok(due != 0)
}

async fn active_projection_work_due(pool: &SqlitePool) -> Result<bool> {
    let due = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(
           SELECT 1 FROM search_projection_outbox
           WHERE available_at <= CURRENT_TIMESTAMP
             AND (lease_expires_at IS NULL OR lease_expires_at <= CURRENT_TIMESTAMP)
         )",
    )
    .fetch_one(pool)
    .await
    .context("check due active search projection work")?;
    Ok(due != 0)
}

async fn reactivate_terminal_projection_once(state: &AppState) -> Result<bool> {
    let due = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(
           SELECT 1 FROM search_projection_outbox_failed
           WHERE failed_at <= datetime('now', '-' || ? || ' seconds')
         )",
    )
    .bind(PROJECTION_TERMINAL_RETRY_DELAY.as_secs() as i64)
    .fetch_one(&state.pool)
    .await?;
    if due == 0 {
        return Ok(false);
    }
    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority_until(
            &state.pool,
            "search_projection_outbox_terminal_recovery",
            SqliteWritePriority::Background,
            tokio::time::Instant::now() + PROJECTION_TRANSACTION_BUDGET,
        )
        .await?;
    let failed = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            String,
        ),
    >(
        "SELECT projection_key, operation, resource_type, resource_id,
                previous_resource_id, canonical_resource_type, user_id,
                created_at, updated_at
         FROM search_projection_outbox_failed
         WHERE failed_at <= datetime('now', '-' || ? || ' seconds')
         ORDER BY failed_at ASC, projection_key ASC
         LIMIT 1",
    )
    .bind(PROJECTION_TERMINAL_RETRY_DELAY.as_secs() as i64)
    .fetch_optional(&mut **tx.as_transaction_mut())
    .await?;
    let Some((
        projection_key,
        operation,
        resource_type,
        resource_id,
        previous_resource_id,
        canonical_resource_type,
        user_id,
        created_at,
        updated_at,
    )) = failed
    else {
        tx.rollback().await?;
        drop(permit);
        return Ok(false);
    };
    sqlx::query(
        "INSERT INTO search_projection_outbox (
           projection_key, operation, resource_type, resource_id,
           previous_resource_id, canonical_resource_type, user_id,
           attempt_count, available_at, lease_owner, lease_expires_at,
           last_error, fanout_cursor, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, 0, CURRENT_TIMESTAMP, NULL, NULL, NULL, NULL, ?, ?)
         ON CONFLICT(projection_key) DO UPDATE SET
           operation = excluded.operation,
           resource_type = excluded.resource_type,
           resource_id = excluded.resource_id,
           previous_resource_id = excluded.previous_resource_id,
           canonical_resource_type = excluded.canonical_resource_type,
           user_id = excluded.user_id,
           attempt_count = 0,
           available_at = CURRENT_TIMESTAMP,
           lease_owner = NULL,
           lease_expires_at = NULL,
           last_error = NULL,
           fanout_cursor = NULL,
           updated_at = CURRENT_TIMESTAMP",
    )
    .bind(&projection_key)
    .bind(&operation)
    .bind(&resource_type)
    .bind(&resource_id)
    .bind(&previous_resource_id)
    .bind(&canonical_resource_type)
    .bind(&user_id)
    .bind(&created_at)
    .bind(&updated_at)
    .execute(&mut **tx.as_transaction_mut())
    .await?;
    sqlx::query("DELETE FROM search_projection_outbox_failed WHERE projection_key = ?")
        .bind(&projection_key)
        .execute(&mut **tx.as_transaction_mut())
        .await?;
    tx.commit().await?;
    drop(permit);
    info!(
        event = "search.projection_outbox.terminal_reactivated",
        projection_key = %projection_key,
        "terminal search projection returned to the retryable outbox"
    );
    Ok(true)
}

async fn recover_projection_claim(
    state: &AppState,
    projection_key: &str,
    lease_owner: &str,
    error: &anyhow::Error,
) {
    if let Err(background_error) = record_projection_failure_with_priority(
        state,
        projection_key,
        lease_owner,
        error,
        SqliteWritePriority::Background,
    )
    .await
    {
        warn!(
            ?background_error,
            projection_key,
            event = "search.projection_outbox.failure_record_deferred",
            "background projection retry state could not be recorded; escalating cleanup"
        );
        if let Err(foreground_error) = record_projection_failure_with_priority(
            state,
            projection_key,
            lease_owner,
            error,
            SqliteWritePriority::Foreground,
        )
        .await
        {
            warn!(
                ?foreground_error,
                projection_key,
                event = "search.projection_outbox.failure_record_failed",
                "failed to persist search projection retry state"
            );
        }
    }
}

async fn record_projection_failure_with_priority(
    state: &AppState,
    projection_key: &str,
    lease_owner: &str,
    error: &anyhow::Error,
    priority: SqliteWritePriority,
) -> Result<()> {
    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(&state.pool, "search_projection_outbox_failure", priority)
        .await?;
    let error_summary = redact_search_error_summary(&error.to_string());
    record_projection_failure_in_transaction(
        tx.as_transaction_mut(),
        projection_key,
        lease_owner,
        &error_summary,
    )
    .await?;
    tx.commit().await?;
    drop(permit);
    Ok(())
}

async fn record_projection_failure_in_transaction(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    projection_key: &str,
    lease_owner: &str,
    error_summary: &str,
) -> Result<()> {
    let attempt_count = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT attempt_count FROM search_projection_outbox WHERE projection_key = ? AND lease_owner = ?",
    )
    .bind(projection_key)
    .bind(lease_owner)
    .fetch_optional(&mut **tx)
    .await?
    .flatten();
    let Some(attempt_count) = attempt_count else {
        return Ok(());
    };
    if attempt_count >= PROJECTION_MAX_ATTEMPTS {
        sqlx::query(
            "INSERT INTO search_projection_outbox_failed (
               projection_key, operation, resource_type, resource_id, previous_resource_id,
               canonical_resource_type, user_id, attempt_count, last_error,
               created_at, updated_at, failed_at
             )
             SELECT projection_key, operation, resource_type, resource_id, previous_resource_id,
                    canonical_resource_type, user_id, attempt_count, ?, created_at,
                    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
             FROM search_projection_outbox
             WHERE projection_key = ? AND lease_owner = ?
             ON CONFLICT(projection_key) DO UPDATE SET
               operation = excluded.operation,
               resource_type = excluded.resource_type,
               resource_id = excluded.resource_id,
               previous_resource_id = excluded.previous_resource_id,
               canonical_resource_type = excluded.canonical_resource_type,
               user_id = excluded.user_id,
               attempt_count = excluded.attempt_count,
               last_error = excluded.last_error,
               updated_at = excluded.updated_at,
               failed_at = excluded.failed_at",
        )
        .bind(error_summary)
        .bind(projection_key)
        .bind(lease_owner)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "DELETE FROM search_projection_outbox WHERE projection_key = ? AND lease_owner = ?",
        )
        .bind(projection_key)
        .bind(lease_owner)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE search_projection_outbox
             SET available_at = datetime('now', '+' || ? || ' seconds'),
                 lease_owner = NULL, lease_expires_at = NULL,
                 last_error = ?, updated_at = CURRENT_TIMESTAMP
             WHERE projection_key = ? AND lease_owner = ?",
        )
        .bind(PROJECTION_RETRY_DELAY.as_secs() as i64)
        .bind(error_summary)
        .bind(projection_key)
        .bind(lease_owner)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn apply_projection_outbox_item(
    tx: &mut Transaction<'_, Sqlite>,
    row: &ProjectionOutboxRow,
) -> Result<ProjectionApplyOutcome> {
    if row.operation == "delete" {
        match row.resource_type.as_str() {
            "content" => {
                let canonical_type = row
                    .canonical_resource_type
                    .as_deref()
                    .context("content projection delete is missing canonical resource type")?;
                let _ = refresh_content_projection(tx, canonical_type, &row.resource_id).await?;
            }
            "announcement" | "notification" => {
                delete_search_document(tx, &row.projection_key).await?;
            }
            resource_type => {
                bail!("unsupported search projection resource type {resource_type}");
            }
        }
        return Ok(ProjectionApplyOutcome::Complete);
    }

    match row.resource_type.as_str() {
        "notification" => {
            apply_notification_projection(tx, row).await?;
            Ok(ProjectionApplyOutcome::Complete)
        }
        "announcement" => apply_announcement_projection(tx, row).await,
        "content" => {
            let canonical_type = row
                .canonical_resource_type
                .as_deref()
                .context("content projection is missing canonical resource type")?;
            if refresh_content_projection(tx, canonical_type, &row.resource_id).await? {
                Ok(ProjectionApplyOutcome::Complete)
            } else {
                Ok(ProjectionApplyOutcome::Deferred)
            }
        }
        resource_type => bail!("unsupported search projection resource type {resource_type}"),
    }
}

async fn apply_notification_projection(
    tx: &mut Transaction<'_, Sqlite>,
    row: &ProjectionOutboxRow,
) -> Result<()> {
    let user_id = row
        .user_id
        .as_deref()
        .context("notification projection is missing user id")?;
    let source = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        ),
    >(
        "SELECT user_id, repo_full_name, subject_title, subject_type, reason, updated_at, unread, html_url
         FROM notifications WHERE user_id = ? AND thread_id = ?",
    )
    .bind(user_id)
    .bind(&row.resource_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((
        source_user_id,
        repo_full_name,
        subject_title,
        subject_type,
        reason,
        updated_at,
        unread,
        html_url,
    )) = source
    else {
        delete_search_document(tx, &row.projection_key).await?;
        return Ok(());
    };
    let owner_login = repo_full_name.as_deref().map(|name| {
        name.split_once('/')
            .map(|(owner, _)| owner)
            .unwrap_or(name)
            .to_owned()
    });
    remove_conflicting_search_document(
        tx,
        &source_user_id,
        "notification",
        &row.resource_id,
        &row.projection_key,
    )
    .await?;
    sqlx::query(
        "INSERT INTO search_documents (
           id, user_id, resource_type, resource_id, repo_full_name, owner_login,
           title, body, source_time, unread, target_url, created_at, updated_at
         ) VALUES (?, ?, 'notification', ?, ?, ?, ?, ?, ?, ?, ?, COALESCE(?, CURRENT_TIMESTAMP), COALESCE(?, CURRENT_TIMESTAMP))
         ON CONFLICT(id) DO UPDATE SET
           user_id = excluded.user_id,
           resource_id = excluded.resource_id,
           repo_full_name = excluded.repo_full_name,
           owner_login = excluded.owner_login,
           title = excluded.title,
           body = excluded.body,
           source_time = excluded.source_time,
           unread = excluded.unread,
           target_url = excluded.target_url,
           updated_at = excluded.updated_at",
    )
    .bind(&row.projection_key)
    .bind(&source_user_id)
    .bind(&row.resource_id)
    .bind(&repo_full_name)
    .bind(owner_login)
    .bind(&subject_title)
    .bind(format!(
        "{} {}",
        subject_type.as_deref().unwrap_or_default(),
        reason.as_deref().unwrap_or_default()
    ))
    .bind(&updated_at)
    .bind(unread)
    .bind(&html_url)
    .bind(&updated_at)
    .bind(&updated_at)
    .execute(&mut **tx)
    .await?;
    refresh_user_lane(
        tx,
        &row.projection_key,
        &source_user_id,
        "notification",
        &row.resource_id,
        &row.resource_id,
    )
    .await
}

async fn apply_announcement_projection(
    tx: &mut Transaction<'_, Sqlite>,
    row: &ProjectionOutboxRow,
) -> Result<ProjectionApplyOutcome> {
    let source = sqlx::query_as::<
        _,
        (
            String,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<i64>,
            Option<String>,
            String,
            String,
        ),
    >(
        "SELECT user_id, repo_id, repo_full_name, title, body, occurred_at,
                discussion_number, html_url, created_at, updated_at
         FROM social_activity_events
         WHERE id = ? AND kind = 'announcement'",
    )
    .bind(&row.resource_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((
        user_id,
        repo_id,
        repo_full_name,
        title,
        body,
        occurred_at,
        discussion_number,
        html_url,
        created_at,
        updated_at,
    )) = source
    else {
        delete_search_document(tx, &row.projection_key).await?;
        return Ok(ProjectionApplyOutcome::Complete);
    };
    let resource_id = match (&repo_full_name, discussion_number) {
        (Some(repo), Some(number)) => format!("{}#{}", repo.to_ascii_lowercase(), number),
        _ => row.resource_id.clone(),
    };
    let owner_login = repo_full_name.as_deref().map(|name| {
        name.split_once('/')
            .map(|(owner, _)| owner)
            .unwrap_or(name)
            .to_owned()
    });
    let target_path = repo_full_name
        .as_deref()
        .zip(discussion_number)
        .map(|(repo, number)| format!("/{repo}/discussions/{number}"));
    remove_conflicting_search_document(
        tx,
        &user_id,
        "announcement",
        &resource_id,
        &row.projection_key,
    )
    .await?;
    sqlx::query(
        "INSERT INTO search_documents (
           id, user_id, resource_type, resource_id, repo_id, repo_full_name,
           owner_login, title, body, source_time, target_path, target_url,
           created_at, updated_at
         ) VALUES (?, ?, 'announcement', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           user_id = excluded.user_id,
           resource_id = excluded.resource_id,
           repo_id = excluded.repo_id,
           repo_full_name = excluded.repo_full_name,
           owner_login = excluded.owner_login,
           title = excluded.title,
           body = excluded.body,
           source_time = excluded.source_time,
           target_path = excluded.target_path,
           target_url = excluded.target_url,
           updated_at = excluded.updated_at",
    )
    .bind(&row.projection_key)
    .bind(&user_id)
    .bind(&resource_id)
    .bind(repo_id)
    .bind(&repo_full_name)
    .bind(owner_login)
    .bind(&title)
    .bind(&body)
    .bind(&occurred_at)
    .bind(target_path)
    .bind(&html_url)
    .bind(&created_at)
    .bind(&updated_at)
    .execute(&mut **tx)
    .await?;
    refresh_announcement_lanes_chunk_with_cursor(
        tx,
        &row.projection_key,
        &row.resource_id,
        &resource_id,
        row.previous_resource_id.as_deref(),
        row.fanout_cursor.as_deref(),
    )
    .await
}

async fn refresh_user_lane(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    user_id: &str,
    resource_type: &str,
    entity_id: &str,
    alternate_entity_id: &str,
) -> Result<()> {
    sqlx::query("DELETE FROM search_document_user_lanes WHERE document_id = ? AND user_id = ?")
        .bind(document_id)
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    let entity_pattern = format!("{resource_type}%");
    sqlx::query(
        "INSERT INTO search_document_user_lanes (
           document_id, user_id, translated_text, smart_text, updated_at
         )
         SELECT ?, ?,
           (SELECT trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, ''))
            FROM ai_translations t
            WHERE t.user_id = ? AND t.lang = 'zh-CN'
              AND t.status IN ('ready', 'disabled', 'missing')
              AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
              AND t.entity_type LIKE ? AND lower(t.entity_type) NOT LIKE '%smart%'
              AND lower(t.entity_id) IN (lower(?), lower(?))
            ORDER BY t.updated_at DESC, t.id DESC LIMIT 1),
           CASE WHEN ? = 1 THEN NULL ELSE (SELECT trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, ''))
            FROM ai_translations t
            WHERE t.user_id = ? AND t.lang = 'zh-CN'
              AND t.status IN ('ready', 'disabled', 'missing')
              AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
              AND t.entity_type LIKE ? AND lower(t.entity_type) LIKE '%smart%'
              AND lower(t.entity_id) IN (lower(?), lower(?))
            ORDER BY t.updated_at DESC, t.id DESC LIMIT 1) END,
           COALESCE((SELECT MAX(t.updated_at) FROM ai_translations t
                     WHERE t.user_id = ? AND t.lang = 'zh-CN'
                       AND t.status IN ('ready', 'disabled', 'missing')
                       AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
                       AND t.entity_type LIKE ?
                       AND lower(t.entity_id) IN (lower(?), lower(?))), CURRENT_TIMESTAMP)
         WHERE EXISTS (SELECT 1 FROM search_documents d WHERE d.id = ? AND d.user_id = ?)
           AND EXISTS (SELECT 1 FROM ai_translations t
                       WHERE t.user_id = ? AND t.lang = 'zh-CN'
                         AND t.status IN ('ready', 'disabled', 'missing')
                         AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
                         AND t.entity_type LIKE ?
                         AND lower(t.entity_id) IN (lower(?), lower(?))
                         AND (? = 0 OR lower(t.entity_type) NOT LIKE '%smart%'))",
    )
    .bind(document_id)
    .bind(user_id)
    .bind(user_id)
    .bind(&entity_pattern)
    .bind(entity_id)
    .bind(alternate_entity_id)
    .bind((resource_type == "release") as i64)
    .bind(user_id)
    .bind(&entity_pattern)
    .bind(entity_id)
    .bind(alternate_entity_id)
    .bind(user_id)
    .bind(&entity_pattern)
    .bind(entity_id)
    .bind(alternate_entity_id)
    .bind(document_id)
    .bind(user_id)
    .bind(user_id)
    .bind(&entity_pattern)
    .bind(entity_id)
    .bind(alternate_entity_id)
    .bind((resource_type == "release") as i64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn refresh_announcement_lanes_chunk_with_cursor(
    tx: &mut Transaction<'_, Sqlite>,
    document_id: &str,
    event_id: &str,
    resource_id: &str,
    previous_resource_id: Option<&str>,
    fanout_cursor: Option<&str>,
) -> Result<ProjectionApplyOutcome> {
    if fanout_cursor.is_none() {
        sqlx::query("DELETE FROM search_document_user_lanes WHERE document_id = ?")
            .bind(document_id)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query(
        "WITH user_ids AS (
           SELECT DISTINCT t.user_id
           FROM ai_translations t
           WHERE t.lang = 'zh-CN'
             AND t.status IN ('ready', 'disabled', 'missing')
             AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
             AND lower(t.entity_type) LIKE 'announcement%'
             AND lower(t.entity_id) IN (lower(?), lower(?), lower(?))
             AND (? IS NULL OR t.user_id > ?)
           ORDER BY t.user_id
           LIMIT ?
         ), candidates AS (
           SELECT t.user_id,
                  CASE WHEN lower(t.entity_type) LIKE '%smart%' THEN 1 ELSE 0 END AS is_smart,
                  trim(COALESCE(t.title, '') || ' ' || COALESCE(t.summary, '')) AS value,
                  t.updated_at,
                  t.id,
                  ROW_NUMBER() OVER (
                    PARTITION BY t.user_id, CASE WHEN lower(t.entity_type) LIKE '%smart%' THEN 1 ELSE 0 END
                    ORDER BY t.updated_at DESC, t.id DESC
                  ) AS lane_rank
           FROM ai_translations t
           JOIN user_ids u ON u.user_id = t.user_id
           WHERE t.lang = 'zh-CN'
             AND t.status IN ('ready', 'disabled', 'missing')
             AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
             AND lower(t.entity_type) LIKE 'announcement%'
             AND lower(t.entity_id) IN (lower(?), lower(?), lower(?))
         ), selected AS (
           SELECT user_id,
                  MAX(CASE WHEN is_smart = 0 AND lane_rank = 1 THEN value END) AS translated_text,
                  MAX(CASE WHEN is_smart = 1 AND lane_rank = 1 THEN value END) AS smart_text,
                  MAX(CASE WHEN lane_rank = 1 THEN updated_at END) AS updated_at
           FROM candidates
           GROUP BY user_id
         )
         INSERT INTO search_document_user_lanes (
           document_id, user_id, translated_text, smart_text, updated_at
         )
         SELECT ?, user_id, translated_text, smart_text,
                COALESCE(updated_at, CURRENT_TIMESTAMP)
         FROM selected
         WHERE 1
         ON CONFLICT(document_id, user_id) DO UPDATE SET
           translated_text = excluded.translated_text,
           smart_text = excluded.smart_text,
           updated_at = excluded.updated_at",
    )
    .bind(event_id)
    .bind(resource_id)
    .bind(previous_resource_id)
    .bind(fanout_cursor)
    .bind(fanout_cursor)
    .bind(PROJECTION_FANOUT_BATCH_SIZE)
    .bind(event_id)
    .bind(resource_id)
    .bind(previous_resource_id)
    .bind(document_id)
    .execute(&mut **tx)
    .await?;

    let last_cursor = sqlx::query_scalar::<_, Option<String>>(
        "SELECT MAX(user_id) FROM (
           SELECT DISTINCT t.user_id
           FROM ai_translations t
           WHERE t.lang = 'zh-CN'
             AND t.status IN ('ready', 'disabled', 'missing')
             AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
             AND lower(t.entity_type) LIKE 'announcement%'
             AND lower(t.entity_id) IN (lower(?), lower(?), lower(?))
             AND (? IS NULL OR t.user_id > ?)
           ORDER BY t.user_id
           LIMIT ?
         )",
    )
    .bind(event_id)
    .bind(resource_id)
    .bind(previous_resource_id)
    .bind(fanout_cursor)
    .bind(fanout_cursor)
    .bind(PROJECTION_FANOUT_BATCH_SIZE)
    .fetch_one(&mut **tx)
    .await?;
    let Some(last_cursor) = last_cursor else {
        return Ok(ProjectionApplyOutcome::Complete);
    };
    let has_more = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(
           SELECT 1
           FROM ai_translations t
           WHERE t.lang = 'zh-CN'
             AND t.status IN ('ready', 'disabled', 'missing')
             AND (t.title IS NOT NULL OR t.summary IS NOT NULL)
             AND lower(t.entity_type) LIKE 'announcement%'
             AND lower(t.entity_id) IN (lower(?), lower(?), lower(?))
             AND t.user_id > ?
         )",
    )
    .bind(event_id)
    .bind(resource_id)
    .bind(previous_resource_id)
    .bind(&last_cursor)
    .fetch_one(&mut **tx)
    .await?
        != 0;
    Ok(if has_more {
        ProjectionApplyOutcome::Continue {
            cursor: last_cursor,
        }
    } else {
        ProjectionApplyOutcome::Complete
    })
}

async fn remove_conflicting_search_document(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    resource_type: &str,
    resource_id: &str,
    keep_document_id: &str,
) -> Result<()> {
    let conflicting_ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM search_documents
         WHERE user_id = ? AND resource_type = ? AND resource_id = ? AND id <> ?",
    )
    .bind(user_id)
    .bind(resource_type)
    .bind(resource_id)
    .bind(keep_document_id)
    .fetch_all(&mut **tx)
    .await?;
    for document_id in conflicting_ids {
        delete_search_document(tx, &document_id).await?;
    }
    Ok(())
}

async fn refresh_content_projection(
    tx: &mut Transaction<'_, Sqlite>,
    resource_type: &str,
    resource_id: &str,
) -> Result<bool> {
    let document_exists = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(
           SELECT 1 FROM search_documents
           WHERE resource_type = ? AND resource_id = ?
         )",
    )
    .bind(resource_type)
    .bind(resource_id)
    .fetch_one(&mut **tx)
    .await?;
    if document_exists == 0 {
        return Ok(false);
    }

    let identity_upgrade_complete =
        crate::content_identity_upgrade::is_complete_in_transaction(tx).await?;
    let mut updated_any = false;
    for (pipeline, column) in [
        ("translation", "translated_text"),
        ("polishing", "smart_text"),
    ] {
        let public_release_smart = (resource_type == "release" && pipeline == "polishing") as i64;
        let projection_text_query = if identity_upgrade_complete {
            "SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_current_result_projections p JOIN content_work_identities i ON i.id = p.identity_id LEFT JOIN content_work_items active ON active.id = COALESCE(p.active_work_item_id, p.work_item_id) WHERE i.canonical_resource_type = ? AND i.canonical_resource_id = ? AND i.pipeline = ? AND i.target_lang = 'zh-CN' AND (? = 0 OR i.variant = ?) ORDER BY CASE WHEN active.status = 'ready' THEN 0 ELSE 1 END, julianday(p.published_at) DESC, p.published_at DESC, i.source_hash DESC LIMIT 1"
        } else {
            "SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = ? AND p.canonical_resource_id = ? AND p.pipeline = ? AND p.target_lang = 'zh-CN' AND (? = 0 OR p.variant = ?) ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1"
        };
        let text = sqlx::query_scalar::<_, Option<String>>(projection_text_query)
            .bind(resource_type)
            .bind(resource_id)
            .bind(pipeline)
            .bind(public_release_smart)
            .bind(crate::translations::RELEASE_READING_PUBLIC_VARIANT)
            .fetch_optional(&mut **tx)
            .await?
            .flatten();
        let query = format!(
            "UPDATE search_documents SET {column} = ?, updated_at = MAX(updated_at, CURRENT_TIMESTAMP) WHERE resource_type = ? AND resource_id = ?"
        );
        let result = sqlx::query(&query)
            .bind(text)
            .bind(resource_type)
            .bind(resource_id)
            .execute(&mut **tx)
            .await?;
        updated_any |= result.rows_affected() > 0;
    }
    if resource_type == "release" {
        // Legacy ai_translations triggers may have materialized a private
        // smart lane before the shared release projection was introduced.
        // Remove smart-only rows and clear the smart field on translated rows
        // so an access change cannot leave private compare text searchable.
        let deleted = sqlx::query(
            "DELETE FROM search_document_user_lanes WHERE translated_text IS NULL AND document_id IN (SELECT id FROM search_documents WHERE resource_type = ? AND resource_id = ?)",
        )
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await?;
        let cleared = sqlx::query(
            "UPDATE search_document_user_lanes SET smart_text = NULL, updated_at = MAX(updated_at, CURRENT_TIMESTAMP) WHERE document_id IN (SELECT id FROM search_documents WHERE resource_type = ? AND resource_id = ?)",
        )
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await?;
        updated_any |= deleted.rows_affected() > 0 || cleared.rows_affected() > 0;
    }
    Ok(updated_any)
}

async fn delete_search_document(tx: &mut Transaction<'_, Sqlite>, document_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM search_document_user_lanes WHERE document_id = ?")
        .bind(document_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM search_documents WHERE id = ?")
        .bind(document_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn run_batch(state: &AppState) -> Result<BatchOutcome> {
    let directory = database_directory(&state.config.database_url);
    let minimum_free_bytes = min_free_bytes();
    let free_bytes = available_bytes(&directory)
        .with_context(|| format!("check free space for {}", directory.display()))?;
    run_batch_with_free_bytes(state, free_bytes, minimum_free_bytes).await
}

async fn run_batch_once(state: &AppState) -> Result<BatchOutcome> {
    let result = run_batch(state).await;
    if let Err(error) = &result
        && !is_transient_backfill_error(error)
    {
        record_batch_failure(state, error).await;
    }
    result
}

fn is_transient_backfill_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        is_sqlite_background_admission_error(cause) || is_sqlite_write_deadline_error(cause)
    })
}

#[cfg(test)]
pub(crate) async fn run_batch_for_test(state: &AppState, free_bytes: u64) -> Result<()> {
    let result = run_batch_with_free_bytes(state, free_bytes, 1).await;
    if let Err(error) = &result
        && !is_transient_backfill_error(error)
    {
        record_batch_failure(state, error).await;
    }
    result.map(|_| ())
}

#[cfg(test)]
pub(crate) async fn resume_failed_for_test(state: &AppState) -> Result<bool> {
    resume_failed(state).await
}

async fn record_batch_failure(state: &AppState, error: &anyhow::Error) {
    warn!(?error, "search projection backfill batch failed");
    if let Err(mark_error) = mark_failed(state, &error.to_string()).await {
        warn!(
            ?mark_error,
            event = "search.index.failure_record_failed",
            "failed to persist terminal search projection backfill status"
        );
    }
}

async fn run_batch_with_free_bytes(
    state: &AppState,
    free_bytes: u64,
    minimum_free_bytes: u64,
) -> Result<BatchOutcome> {
    let current_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM search_projection_backfill_state WHERE id = 1",
    )
    .fetch_optional(&state.pool)
    .await
    .context("read search projection backfill status")?;
    if current_status.as_deref() == Some("failed") {
        return Ok(BatchOutcome::Failed);
    }
    let ready_queue_pending = if current_status.as_deref() == Some("ready") {
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM search_metadata_backfill_queue)")
            .fetch_one(&state.pool)
            .await
            .context("check pending search metadata work")?
            != 0
    } else {
        false
    };
    if current_status.as_deref() == Some("ready") && !ready_queue_pending {
        return Ok(BatchOutcome::Ready);
    }
    if free_bytes < minimum_free_bytes {
        if current_status.as_deref() != Some("paused_low_disk") {
            mark_status(state, "paused_low_disk", None).await?;
            warn!(
                event = "search.index.paused_low_disk",
                status = "paused_low_disk",
                free_bytes,
                minimum_free_bytes,
                retry_after_seconds = RETRY_DELAY.as_secs(),
                "search projection recovery paused below the free-space watermark"
            );
        }
        return Ok(BatchOutcome::Paused);
    }
    let was_paused = current_status.as_deref() == Some("paused_low_disk");

    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_projection_backfill",
            SqliteWritePriority::Background,
        )
        .await?;
    let transaction_result: Result<(BatchOutcome, bool, Option<String>, Option<i64>)> = async {
        let state_row = load_state(tx.as_transaction_mut()).await?;
        if state_row.status == "failed" {
            return Ok((BatchOutcome::Failed, false, None, None));
        }
        if state_row.status == "ready" {
            let rowids = load_rowids(tx.as_transaction_mut(), "metadata", 0).await?;
            if rowids.is_empty() {
                return Ok((BatchOutcome::Ready, false, None, None));
            }
            process_phase(tx.as_transaction_mut(), "metadata", &rowids).await?;
            return Ok((BatchOutcome::Progress, true, None, None));
        }
        let phase = phase_index(&state_row.phase)?;
        let rowids = load_rowids(
            tx.as_transaction_mut(),
            state_row.phase.as_str(),
            state_row.cursor,
        )
        .await?;
        if rowids.is_empty() {
            let next_phase = if phase + 1 >= PHASES.len() {
                if !load_rowids(tx.as_transaction_mut(), "metadata", 0)
                    .await?
                    .is_empty()
                {
                    Some("metadata")
                } else {
                    None
                }
            } else {
                Some(PHASES[phase + 1])
            };
            let next_phase_name = next_phase.unwrap_or("translations").to_owned();
            if let Some(next_phase) = next_phase {
                update_state(tx.as_transaction_mut(), next_phase, 0, "building", None).await?;
            } else {
                update_state(tx.as_transaction_mut(), "translations", 0, "ready", None).await?;
            }
            return Ok((BatchOutcome::Idle, true, Some(next_phase_name), Some(0)));
        }

        update_state(
            tx.as_transaction_mut(),
            state_row.phase.as_str(),
            state_row.cursor,
            "building",
            None,
        )
        .await?;
        process_phase(tx.as_transaction_mut(), state_row.phase.as_str(), &rowids).await?;
        let next_cursor = if state_row.phase == "metadata" {
            0
        } else {
            *rowids.last().expect("non-empty batch")
        };
        update_state(
            tx.as_transaction_mut(),
            state_row.phase.as_str(),
            next_cursor,
            "building",
            None,
        )
        .await?;
        Ok((
            BatchOutcome::Progress,
            true,
            Some(state_row.phase),
            Some(next_cursor),
        ))
    }
    .await;

    let (outcome, should_commit, resumed_phase, resumed_cursor) = match transaction_result {
        Ok(result) => result,
        Err(error) => {
            let rollback_result = tx.rollback().await;
            drop(permit);
            if let Err(rollback_error) = rollback_result {
                return Err(error.context(format!(
                    "rollback search projection batch after failure: {rollback_error:#}"
                )));
            }
            return Err(error);
        }
    };
    let transaction_result = if should_commit {
        tx.commit().await
    } else {
        tx.rollback().await
    };
    drop(permit);
    transaction_result?;
    if was_paused {
        info!(
            event = "search.index.resumed",
            phase = resumed_phase.as_deref().unwrap_or("unknown"),
            cursor = resumed_cursor.unwrap_or_default(),
            free_bytes,
            minimum_free_bytes,
            "search projection recovery resumed above the free-space watermark"
        );
    }
    Ok(outcome)
}

async fn mark_status(state: &AppState, status: &str, error: Option<&str>) -> Result<()> {
    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_projection_backfill_status",
            SqliteWritePriority::Background,
        )
        .await?;
    let current = load_state(tx.as_transaction_mut()).await?;
    update_state(
        tx.as_transaction_mut(),
        &current.phase,
        current.cursor,
        status,
        error,
    )
    .await?;
    tx.commit().await?;
    drop(permit);
    Ok(())
}

async fn mark_failed(state: &AppState, error: &str) -> Result<()> {
    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_projection_backfill_failure",
            SqliteWritePriority::Background,
        )
        .await?;
    let current = load_state(tx.as_transaction_mut()).await?;
    update_state(
        tx.as_transaction_mut(),
        &current.phase,
        current.cursor,
        "failed",
        Some(error),
    )
    .await?;
    tx.commit().await?;
    drop(permit);
    Ok(())
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchIndexStatusResponse {
    phase: String,
    cursor: i64,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    updated_at: String,
    metadata_pending: bool,
}

async fn status_response(pool: &SqlitePool) -> Result<SearchIndexStatusResponse> {
    let Some((phase, cursor, status, last_error, updated_at)) =
        sqlx::query_as::<_, (String, i64, String, Option<String>, String)>(
            "SELECT phase, cursor, status, last_error, updated_at FROM search_projection_backfill_state WHERE id = 1",
        )
        .fetch_optional(pool)
        .await
        .context("read search projection backfill status")?
    else {
        bail!("search projection backfill state is missing");
    };
    let metadata_pending =
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM search_metadata_backfill_queue)")
            .fetch_one(pool)
            .await
            .context("check pending search metadata work")?
            != 0;
    let projection_pending =
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM search_projection_outbox)")
            .fetch_one(pool)
            .await
            .context("check pending realtime search projection work")?
            != 0;
    let projection_failure = sqlx::query_scalar::<_, Option<String>>(
        "SELECT last_error FROM search_projection_outbox_failed ORDER BY failed_at DESC, projection_key DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .context("read terminal realtime search projection failure")?
    .flatten();
    let status = if projection_failure.is_some() {
        "failed".to_owned()
    } else if status == "ready" && (metadata_pending || projection_pending) {
        "building".to_owned()
    } else {
        status
    };
    Ok(SearchIndexStatusResponse {
        phase,
        cursor,
        status,
        last_error: last_error
            .or(projection_failure)
            .map(|error| redact_search_error_summary(&error)),
        updated_at,
        metadata_pending,
    })
}

fn redact_search_error_summary(error: &str) -> String {
    const SENSITIVE_MARKERS: &[&str] = &[
        "api_key",
        "api-key",
        "apikey",
        "authorization",
        "access_token",
        "refresh_token",
        "password",
        "secret",
        "private_key",
        "cookie",
    ];
    let mut summary = String::new();
    for (index, line) in error.lines().enumerate() {
        if index > 0 {
            summary.push('\n');
        }
        if SENSITIVE_MARKERS
            .iter()
            .any(|marker| line.to_ascii_lowercase().contains(marker))
        {
            summary.push_str("<redacted sensitive error>");
        } else {
            summary.push_str(line);
        }
        if summary.len() >= 500 {
            break;
        }
    }
    if summary.len() > 500 {
        let mut end = 500;
        while !summary.is_char_boundary(end) {
            end -= 1;
        }
        summary.truncate(end);
    }
    summary
}

pub async fn admin_get_status(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<SearchIndexStatusResponse>, ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    status_response(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::internal)
}

async fn resume_failed(state: &AppState) -> Result<bool> {
    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_projection_backfill_resume",
            SqliteWritePriority::Foreground,
        )
        .await?;
    let transaction_result: Result<bool> = async {
        let current = load_state(tx.as_transaction_mut()).await?;
        if current.status != "failed" {
            return Ok(false);
        }
        update_state(
            tx.as_transaction_mut(),
            &current.phase,
            current.cursor,
            "building",
            None,
        )
        .await?;
        Ok(true)
    }
    .await;
    match transaction_result {
        Ok(true) => {
            let commit_result = tx.commit().await;
            drop(permit);
            commit_result?;
            Ok(true)
        }
        Ok(false) => {
            let rollback_result = tx.rollback().await;
            drop(permit);
            rollback_result?;
            Ok(false)
        }
        Err(error) => {
            let rollback_result = tx.rollback().await;
            drop(permit);
            if let Err(rollback_error) = rollback_result {
                return Err(error.context(format!(
                    "rollback search projection resume after failure: {rollback_error:#}"
                )));
            }
            Err(error)
        }
    }
}

pub async fn admin_resume(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<SearchIndexStatusResponse>, ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    if !resume_failed(state.as_ref())
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "search_projection_backfill_state_conflict",
            "search projection backfill is not in a failed state",
        ));
    }
    spawn_worker(state.clone());
    status_response(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::internal)
}

pub(crate) async fn refresh_content_projection_phase(state: &AppState) -> Result<bool> {
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_content_projection_refresh",
            SqliteWritePriority::Background,
        )
        .await?;
    let current = load_state(tx.as_transaction_mut()).await?;
    let current_phase = phase_index(&current.phase)?;
    let content_phase = phase_index("content_projections")?;
    if current_phase < content_phase {
        tx.rollback().await?;
        return Ok(true);
    }
    if current.status == "paused_low_disk" {
        tx.rollback().await?;
        return Ok(false);
    }
    update_state(
        tx.as_transaction_mut(),
        "content_projections",
        0,
        "building",
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

async fn load_state(tx: &mut Transaction<'_, Sqlite>) -> Result<BackfillState> {
    sqlx::query_as::<_, BackfillState>(
        "SELECT phase, cursor, status FROM search_projection_backfill_state WHERE id = 1",
    )
    .fetch_one(&mut **tx)
    .await
    .context("load search projection backfill state")
}

async fn update_state(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    cursor: i64,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE search_projection_backfill_state SET phase = ?, cursor = ?, status = ?, last_error = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
    )
    .bind(phase)
    .bind(cursor)
    .bind(status)
    .bind(error)
    .execute(&mut **tx)
    .await
    .context("update search projection backfill state")?;
    Ok(())
}

fn phase_index(phase: &str) -> Result<usize> {
    PHASES
        .iter()
        .position(|candidate| *candidate == phase)
        .with_context(|| format!("unknown search projection phase {phase}"))
}

async fn load_rowids(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    cursor: i64,
) -> Result<Vec<i64>> {
    let query = match phase {
        "releases" => "SELECT rowid FROM repo_releases WHERE rowid > ? ORDER BY rowid LIMIT ?",
        "announcements" => {
            "SELECT rowid FROM social_activity_events WHERE kind = 'announcement' AND rowid > ? ORDER BY rowid LIMIT ?"
        }
        "notifications" => "SELECT rowid FROM notifications WHERE rowid > ? ORDER BY rowid LIMIT ?",
        "briefs" => "SELECT rowid FROM briefs WHERE rowid > ? ORDER BY rowid LIMIT ?",
        "repo_associations" => {
            "SELECT rowid FROM user_repo_associations WHERE rowid > ? ORDER BY rowid LIMIT ?"
        }
        "starred_repos" => "SELECT rowid FROM starred_repos WHERE rowid > ? ORDER BY rowid LIMIT ?",
        "content_projections" => {
            "SELECT rowid FROM content_result_projections WHERE pipeline IN ('translation', 'polishing') AND target_lang = 'zh-CN' AND rowid > ? ORDER BY rowid LIMIT ?"
        }
        "translations" => {
            "SELECT rowid FROM ai_translations WHERE lang = 'zh-CN' AND status IN ('ready', 'disabled', 'missing') AND (title IS NOT NULL OR summary IS NOT NULL) AND rowid > ? ORDER BY rowid LIMIT ?"
        }
        "metadata" => {
            "SELECT repo_id AS rowid FROM search_metadata_backfill_queue ORDER BY updated_at, repo_id LIMIT 1"
        }
        "fts_documents" => {
            "SELECT rowid FROM search_documents WHERE rowid > ? ORDER BY rowid LIMIT ?"
        }
        "fts_user_lanes" => {
            "SELECT rowid FROM search_document_user_lanes WHERE rowid > ? ORDER BY rowid LIMIT ?"
        }
        _ => bail!("unknown search projection phase {phase}"),
    };
    let rows = if phase == "metadata" {
        sqlx::query(query).fetch_all(&mut **tx).await
    } else {
        sqlx::query(query)
            .bind(cursor)
            .bind(INDEX_BATCH_SIZE)
            .fetch_all(&mut **tx)
            .await
    }
    .with_context(|| format!("load rowids for search phase {phase}"))?;
    Ok(rows.into_iter().map(|row| row.get("rowid")).collect())
}

async fn process_phase(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    rowids: &[i64],
) -> Result<()> {
    match phase {
        "releases" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "WITH batch_releases AS (
                   SELECT release_id, repo_id, tag_name, name, body, html_url,
                          published_at, created_at, updated_at
                   FROM repo_releases
                   WHERE rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(
                ")),
                visible_repo_candidates AS (
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
                  UNION ALL
                  SELECT sr.repo_id, sr.full_name, sr.owner_login,
                         2 AS source_priority, sr.user_id
                  FROM starred_repos sr
                  JOIN batch_releases br ON br.repo_id = sr.repo_id
                  WHERE NOT EXISTS (
                    SELECT 1
                    FROM user_repo_associations ura
                    WHERE ura.user_id = sr.user_id
                      AND ura.repo_full_name_lower = lower(sr.full_name)
                  )
                  UNION ALL
                  SELECT ob.repo_id, ob.repo_full_name,
                         CASE
                           WHEN instr(ob.repo_full_name, '/') > 0
                             THEN substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)
                           ELSE ob.repo_full_name
                         END AS owner_login,
                         3 AS source_priority, ob.user_id
                  FROM owned_repo_star_baselines ob
                  JOIN batch_releases br ON br.repo_id = ob.repo_id
                  JOIN users u ON u.id = ob.user_id
                  WHERE u.include_own_releases != 0
                    AND NOT EXISTS (
                      SELECT 1
                      FROM user_repo_associations ura
                      WHERE ura.user_id = ob.user_id
                        AND ura.repo_full_name_lower = lower(ob.repo_full_name)
                    )
                    AND NOT EXISTS (
                      SELECT 1
                      FROM starred_repos sr
                      WHERE sr.user_id = ob.user_id
                        AND sr.repo_id = ob.repo_id
                    )
                ),
                visible_repo_metadata AS (
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
                ),
                associated_repo_metadata AS (
                  SELECT ura.repo_id, ura.repo_full_name, ura.owner_login
                  FROM user_repo_associations ura
                  JOIN batch_releases br ON br.repo_id = ura.repo_id
                  GROUP BY ura.repo_id
                ),
                starred_repo_metadata AS (
                  SELECT sr.repo_id, sr.full_name, sr.owner_login
                  FROM starred_repos sr
                  JOIN batch_releases br ON br.repo_id = sr.repo_id
                  GROUP BY sr.repo_id
                )
                INSERT INTO search_documents (
                  id, user_id, resource_type, resource_id, repo_id, repo_full_name,
                  owner_login, title, body, source_time, target_path, target_url,
                  created_at, updated_at
                )
                SELECT
                  'release:' || br.release_id,
                  NULL,
                  'release',
                  CAST(br.release_id AS TEXT),
                  br.repo_id,
                  COALESCE(rwi.repo_full_name, vr.full_name, ura.repo_full_name, sr.full_name),
                  COALESCE(
                    CASE
                      WHEN instr(rwi.repo_full_name, '/') > 0
                        THEN substr(rwi.repo_full_name, 1, instr(rwi.repo_full_name, '/') - 1)
                      ELSE rwi.repo_full_name
                    END,
                    vr.owner_login,
                    ura.owner_login,
                    sr.owner_login
                  ),
                  COALESCE(br.name, br.tag_name),
                  br.body,
                  COALESCE(br.published_at, br.created_at, br.updated_at),
                  CASE
                    WHEN COALESCE(rwi.repo_full_name, vr.full_name, ura.repo_full_name, sr.full_name) IS NOT NULL
                      THEN '/' || COALESCE(rwi.repo_full_name, vr.full_name, ura.repo_full_name, sr.full_name)
                           || '/releases/tag/' || br.tag_name
                  END,
                  br.html_url,
                  br.updated_at,
                  br.updated_at
                FROM batch_releases br
                LEFT JOIN repo_release_work_items rwi ON rwi.repo_id = br.repo_id
                LEFT JOIN visible_repo_metadata vr ON vr.repo_id = br.repo_id
                LEFT JOIN associated_repo_metadata ura ON ura.repo_id = br.repo_id
                LEFT JOIN starred_repo_metadata sr ON sr.repo_id = br.repo_id
                ON CONFLICT(id) DO UPDATE SET
                  repo_id = excluded.repo_id,
                  repo_full_name = excluded.repo_full_name,
                  owner_login = excluded.owner_login,
                  title = excluded.title,
                  body = excluded.body,
                  source_time = excluded.source_time,
                  target_path = excluded.target_path,
                  target_url = excluded.target_url,
                  updated_at = excluded.updated_at",
            );
            query.build().execute(&mut **tx).await?;
        }
        "announcements" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,unread,target_path,target_url,created_at,updated_at) SELECT 'announcement:'||e.id,e.user_id,'announcement',CASE WHEN e.repo_full_name IS NOT NULL AND e.discussion_number IS NOT NULL THEN lower(e.repo_full_name)||'#'||e.discussion_number ELSE e.id END,e.repo_id,e.repo_full_name,CASE WHEN instr(COALESCE(e.repo_full_name,''),'/')>0 THEN substr(e.repo_full_name,1,instr(e.repo_full_name,'/')-1) ELSE e.repo_full_name END,e.title,e.body,e.occurred_at,NULL,CASE WHEN e.repo_full_name IS NOT NULL AND e.discussion_number IS NOT NULL THEN '/'||e.repo_full_name||'/discussions/'||e.discussion_number END,e.html_url,e.created_at,e.updated_at FROM social_activity_events e WHERE e.kind='announcement' AND e.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "notifications" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_full_name,owner_login,title,body,source_time,unread,target_url,created_at,updated_at) SELECT 'notification:'||n.user_id||':'||n.thread_id,n.user_id,'notification',n.thread_id,n.repo_full_name,CASE WHEN instr(COALESCE(n.repo_full_name,''),'/')>0 THEN substr(n.repo_full_name,1,instr(n.repo_full_name,'/')-1) ELSE n.repo_full_name END,n.subject_title,COALESCE(n.subject_type,'')||' '||COALESCE(n.reason,''),n.updated_at,n.unread,n.html_url,COALESCE(n.updated_at,CURRENT_TIMESTAMP),COALESCE(n.updated_at,CURRENT_TIMESTAMP) FROM notifications n WHERE n.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,unread=excluded.unread,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "briefs" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,title,body,source_time,target_path,created_at,updated_at) SELECT 'brief:'||b.id,b.user_id,'brief',b.id,'日报 '||b.date,b.content_markdown,COALESCE(b.window_end_utc,b.created_at),'/briefs?brief='||b.id,b.created_at,b.updated_at FROM briefs b WHERE b.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "repo_associations" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at) SELECT 'repository:'||u.user_id||':'||u.repo_full_name_lower,u.user_id,'repository',u.repo_full_name_lower,u.repo_id,u.repo_full_name,u.owner_login,u.repo_name,u.description,u.updated_at,'/focus/repo/'||u.owner_login||'/'||u.repo_name,u.html_url,u.created_at,u.updated_at FROM user_repo_associations u WHERE u.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "starred_repos" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at) SELECT 'repository:'||s.user_id||':'||lower(s.full_name),s.user_id,'repository',lower(s.full_name),s.repo_id,s.full_name,s.owner_login,s.name,s.description,s.updated_at,'/focus/repo/'||s.owner_login||'/'||s.name,s.html_url,s.updated_at,s.updated_at FROM starred_repos s WHERE s.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") AND NOT EXISTS (SELECT 1 FROM user_repo_associations u WHERE u.user_id=s.user_id AND u.repo_full_name_lower=lower(s.full_name)) ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "content_projections" => apply_content_projection_batch(tx, rowids).await?,
        "translations" => apply_translation_batch(tx, rowids).await?,
        "metadata" => apply_metadata_batch(tx, rowids).await?,
        "fts_documents" | "fts_user_lanes" => {}
        _ => bail!("unknown search projection phase {phase}"),
    }

    if matches!(phase, "fts_documents" | "fts_user_lanes") {
        refresh_fts_recovery_batch(tx, phase, rowids).await?;
    }
    Ok(())
}

async fn apply_metadata_batch(tx: &mut Transaction<'_, Sqlite>, repo_ids: &[i64]) -> Result<()> {
    for repo_id in repo_ids {
        let cursor = sqlx::query_scalar::<_, i64>(
            "SELECT release_cursor FROM search_metadata_backfill_queue WHERE repo_id = ?",
        )
        .bind(repo_id)
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or_default();
        let rowids = sqlx::query(
            "SELECT rowid FROM search_documents WHERE resource_type = 'release' AND repo_id = ? AND rowid > ? ORDER BY rowid LIMIT ?",
        )
        .bind(repo_id)
        .bind(cursor)
        .bind(INDEX_BATCH_SIZE)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|row| row.get::<i64, _>("rowid"))
        .collect::<Vec<_>>();
        if rowids.is_empty() {
            sqlx::query("DELETE FROM search_metadata_backfill_queue WHERE repo_id = ?")
                .bind(repo_id)
                .execute(&mut **tx)
                .await?;
            continue;
        }
        let mut update = QueryBuilder::<Sqlite>::new(
            "UPDATE search_documents SET repo_full_name = (SELECT repo_full_name FROM search_release_metadata WHERE id = search_documents.id), owner_login = (SELECT owner_login FROM search_release_metadata WHERE id = search_documents.id), target_path = (SELECT target_path FROM search_release_metadata WHERE id = search_documents.id) WHERE rowid IN (",
        );
        push_rowids(&mut update, &rowids);
        update.push(")");
        update.build().execute(&mut **tx).await?;
        let next_cursor = *rowids.last().expect("non-empty metadata batch");
        if rowids.len() < INDEX_BATCH_SIZE as usize {
            sqlx::query("DELETE FROM search_metadata_backfill_queue WHERE repo_id = ?")
                .bind(repo_id)
                .execute(&mut **tx)
                .await?;
        } else {
            sqlx::query(
                "UPDATE search_metadata_backfill_queue SET release_cursor = ? WHERE repo_id = ?",
            )
            .bind(next_cursor)
            .bind(repo_id)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

fn push_rowids(query: &mut QueryBuilder<'_, Sqlite>, rowids: &[i64]) {
    for (index, rowid) in rowids.iter().enumerate() {
        if index > 0 {
            query.push(",");
        }
        query.push_bind(*rowid);
    }
}

async fn apply_content_projection_batch(
    tx: &mut Transaction<'_, Sqlite>,
    rowids: &[i64],
) -> Result<()> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT canonical_resource_type, canonical_resource_id, pipeline FROM content_result_projections WHERE rowid IN (",
    );
    push_rowids(&mut query, rowids);
    query.push(")");
    let rows = query.build().fetch_all(&mut **tx).await?;
    let identity_upgrade_complete =
        crate::content_identity_upgrade::is_complete_in_transaction(tx).await?;
    for row in rows {
        let resource_type: String = row.get("canonical_resource_type");
        let resource_id: String = row.get("canonical_resource_id");
        let pipeline: String = row.get("pipeline");
        let projection_text_query = if identity_upgrade_complete {
            "SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_current_result_projections p JOIN content_work_identities i ON i.id = p.identity_id LEFT JOIN content_work_items active ON active.id = COALESCE(p.active_work_item_id, p.work_item_id) WHERE i.canonical_resource_type = ? AND i.canonical_resource_id = ? AND i.pipeline = ? AND i.target_lang = 'zh-CN' AND (? = 0 OR i.variant = ?) ORDER BY CASE WHEN active.status = 'ready' THEN 0 ELSE 1 END, julianday(p.published_at) DESC, p.published_at DESC, i.source_hash DESC LIMIT 1"
        } else {
            "SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = ? AND p.canonical_resource_id = ? AND p.pipeline = ? AND p.target_lang = 'zh-CN' AND (? = 0 OR p.variant = ?) ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1"
        };
        let public_release_smart = (resource_type == "release" && pipeline == "polishing") as i64;
        let text = sqlx::query_scalar::<_, Option<String>>(projection_text_query)
            .bind(&resource_type)
            .bind(&resource_id)
            .bind(&pipeline)
            .bind(public_release_smart)
            .bind(crate::translations::RELEASE_READING_PUBLIC_VARIANT)
            .fetch_optional(&mut **tx)
            .await?;
        let column = if pipeline == "polishing" {
            "smart_text"
        } else {
            "translated_text"
        };
        let sql = format!(
            "UPDATE search_documents SET {column} = ?, updated_at = MAX(updated_at, CURRENT_TIMESTAMP) WHERE resource_type = ? AND resource_id = ?"
        );
        sqlx::query(&sql)
            .bind(text.flatten())
            .bind(resource_type)
            .bind(resource_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn apply_translation_batch(tx: &mut Transaction<'_, Sqlite>, rowids: &[i64]) -> Result<()> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT rowid, user_id, entity_type, entity_id, title, summary, updated_at FROM ai_translations WHERE rowid IN (",
    );
    push_rowids(&mut query, rowids);
    query.push(")");
    let rows = query.build().fetch_all(&mut **tx).await?;
    for row in rows {
        let user_id: String = row.get("user_id");
        let entity_type: String = row.get("entity_type");
        let entity_id: String = row.get("entity_id");
        let title: Option<String> = row.get("title");
        let summary: Option<String> = row.get("summary");
        let updated_at: String = row.get("updated_at");
        let is_smart = entity_type.to_ascii_lowercase().contains("smart");
        let resource_type = if entity_type.to_ascii_lowercase().starts_with("release") {
            "release"
        } else if entity_type.to_ascii_lowercase().starts_with("announcement") {
            "announcement"
        } else if entity_type.to_ascii_lowercase().starts_with("notification") {
            "notification"
        } else {
            continue;
        };
        let resource_id = if resource_type == "announcement" {
            entity_id.to_ascii_lowercase()
        } else {
            entity_id.clone()
        };
        let document_id = sqlx::query_scalar::<_, Option<String>>(
            "SELECT d.id FROM search_documents d WHERE d.resource_type = ? AND (d.resource_id = ? OR (d.resource_type = 'announcement' AND (lower(d.resource_id) = lower(?) OR d.id = 'announcement:' || ?))) AND (d.user_id = ? OR d.user_id IS NULL) ORDER BY CASE WHEN d.user_id = ? THEN 0 ELSE 1 END, d.id LIMIT 1",
        )
        .bind(resource_type)
        .bind(&resource_id)
        .bind(&entity_id)
        .bind(&entity_id)
        .bind(&user_id)
        .bind(&user_id)
        .fetch_one(&mut **tx)
        .await?;
        let Some(document_id) = document_id else {
            continue;
        };
        let text = title
            .into_iter()
            .chain(summary)
            .collect::<Vec<_>>()
            .join(" ");
        let (translated, smart) = if is_smart {
            (None, Some(text))
        } else {
            (Some(text), None)
        };
        sqlx::query(
            "INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at) VALUES (?,?,?,?,?) ON CONFLICT(document_id,user_id) DO UPDATE SET translated_text=COALESCE(excluded.translated_text,search_document_user_lanes.translated_text), smart_text=COALESCE(excluded.smart_text,search_document_user_lanes.smart_text), updated_at=MAX(search_document_user_lanes.updated_at,excluded.updated_at)",
        )
        .bind(document_id)
        .bind(user_id)
        .bind(translated)
        .bind(smart)
        .bind(updated_at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn refresh_fts_recovery_batch(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    rowids: &[i64],
) -> Result<()> {
    let mut query = match phase {
        "fts_documents" => QueryBuilder::<Sqlite>::new(
            "INSERT INTO search_documents_fts (doc_id,title,body,repo_full_name,translated_text,smart_text) SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE rowid IN (",
        ),
        "fts_user_lanes" => QueryBuilder::<Sqlite>::new(
            "INSERT INTO search_document_user_lanes_fts (doc_id,user_id,translated_text,smart_text) SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE rowid IN (",
        ),
        _ => bail!("unknown FTS recovery phase {phase}"),
    };
    push_rowids(&mut query, rowids);
    query.push(")");
    query.build().execute(&mut **tx).await?;
    Ok(())
}

fn database_directory(database_url: &str) -> PathBuf {
    let path = database_url
        .strip_prefix("sqlite:")
        .unwrap_or(database_url)
        .trim_start_matches("//")
        .split('?')
        .next()
        .unwrap_or("");
    Path::new(path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_owned()
}

fn min_free_bytes() -> u64 {
    std::env::var("OCTORILL_SEARCH_INDEX_MIN_FREE_BYTES")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|value: &u64| *value > 0)
        .unwrap_or(DEFAULT_MIN_FREE_BYTES)
}

fn available_bytes(path: &Path) -> io::Result<u64> {
    #[cfg(unix)]
    {
        let path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid database path"))?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
        let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        let stats = unsafe { stats.assume_init() };
        u128::from(stats.f_bavail)
            .checked_mul(u128::from(stats.f_frsize))
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or_else(|| io::Error::other("free-space value overflow"))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn projection_due_probe_only_admits_when_work_is_due() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect sqlite test pool");
        sqlx::query(
            "CREATE TABLE search_projection_outbox (
               available_at TEXT NOT NULL,
               lease_expires_at TEXT
             )",
        )
        .execute(&pool)
        .await
        .expect("create active projection fixture");
        sqlx::query(
            "CREATE TABLE search_projection_outbox_failed (
               failed_at TEXT NOT NULL
             )",
        )
        .execute(&pool)
        .await
        .expect("create terminal projection fixture");

        assert!(
            !projection_work_due(&pool)
                .await
                .expect("probe empty projection queues")
        );

        sqlx::query(
            "INSERT INTO search_projection_outbox (available_at, lease_expires_at)
             VALUES (datetime('now', '-1 second'), NULL)",
        )
        .execute(&pool)
        .await
        .expect("seed active projection work");
        assert!(
            projection_work_due(&pool)
                .await
                .expect("probe active projection queue")
        );
    }

    #[test]
    fn database_directory_handles_sqlite_urls() {
        assert_eq!(
            database_directory("sqlite:./.data/octo-rill.db"),
            PathBuf::from("./.data")
        );
        assert_eq!(database_directory("sqlite::memory:"), PathBuf::from("."));
    }

    #[test]
    fn free_space_is_nonzero_for_workspace() {
        assert!(available_bytes(Path::new(".")).expect("statvfs") > 0);
    }
}
