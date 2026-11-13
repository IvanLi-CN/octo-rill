//! PostgreSQL task coordination primitives.
//!
//! This module is deliberately independent from the legacy SQLite worker. It
//! provides the transaction boundary that the PostgreSQL application runtime
//! will use while the remaining request handlers are being moved over.

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool, Postgres};
use tokio::time::{Duration, sleep};

const MAX_TRANSACTION_RETRIES: usize = 3;

#[derive(Debug, Clone, Serialize, FromRow, PartialEq, Eq)]
pub struct ClaimedTask {
    pub id: String,
    pub task_type: String,
    pub source: String,
    pub requested_by: Option<String>,
    pub payload_json: String,
    pub cancel_requested: i64,
}

/// Claim one eligible queued task atomically.
///
/// The complete select/update/event sequence runs in one transaction. Row
/// locks are skipped so concurrent workers do not wait behind a task another
/// worker is already claiming. Serialization failures and deadlocks retry the
/// whole transaction, never an individual statement.
pub async fn claim_next_queued_task(
    pool: &PgPool,
    runtime_owner_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<ClaimedTask>> {
    claim_next_queued_task_for_types(pool, runtime_owner_id, now, None).await
}

/// Claim one queued task whose type is present in `allowed_task_types`.
///
/// An empty allow-list returns no task. This prevents a partially migrated
/// worker from claiming business tasks that do not yet have a PostgreSQL
/// executor.
pub async fn claim_next_queued_task_for_types(
    pool: &PgPool,
    runtime_owner_id: &str,
    now: DateTime<Utc>,
    allowed_task_types: Option<&[String]>,
) -> Result<Option<ClaimedTask>> {
    if allowed_task_types.is_some_and(<[String]>::is_empty) {
        return Ok(None);
    }
    let allowed_task_types = allowed_task_types.map(|types| types.to_vec());
    for attempt in 0..=MAX_TRANSACTION_RETRIES {
        match claim_once(pool, runtime_owner_id, now, allowed_task_types.as_deref()).await {
            Ok(task) => return Ok(task),
            Err(error)
                if attempt < MAX_TRANSACTION_RETRIES
                    && error
                        .downcast_ref::<sqlx::Error>()
                        .is_some_and(is_retryable_sqlx_error) =>
            {
                sleep(Duration::from_millis(25 * (attempt as u64 + 1))).await;
            }
            Err(error) => return Err(error),
        }
    }
    Err(anyhow!("PostgreSQL task claim retry loop exhausted"))
}

/// Finalize a task only while the current runtime owner still owns its lease.
/// The status update and terminal event are committed atomically.
pub async fn finalize_task(
    pool: &PgPool,
    task_id: &str,
    runtime_owner_id: &str,
    status: &str,
    result_json: Option<&str>,
    error_message: Option<&str>,
    now: DateTime<Utc>,
) -> Result<bool> {
    if !matches!(status, "succeeded" | "failed" | "cancelled") {
        return Err(anyhow!("invalid terminal PostgreSQL task status: {status}"));
    }
    for attempt in 0..=MAX_TRANSACTION_RETRIES {
        match finalize_once(
            pool,
            task_id,
            runtime_owner_id,
            status,
            result_json,
            error_message,
            now,
        )
        .await
        {
            Ok(finalized) => return Ok(finalized),
            Err(error)
                if attempt < MAX_TRANSACTION_RETRIES
                    && error
                        .downcast_ref::<sqlx::Error>()
                        .is_some_and(is_retryable_sqlx_error) =>
            {
                sleep(Duration::from_millis(25 * (attempt as u64 + 1))).await;
            }
            Err(error) => return Err(error),
        }
    }
    Err(anyhow!("PostgreSQL task finalize retry loop exhausted"))
}

async fn finalize_once(
    pool: &PgPool,
    task_id: &str,
    runtime_owner_id: &str,
    status: &str,
    result_json: Option<&str>,
    error_message: Option<&str>,
    now: DateTime<Utc>,
) -> Result<bool> {
    let mut tx = pool
        .begin()
        .await
        .context("begin PostgreSQL task finalize")?;
    let updated = sqlx::query(
        "UPDATE public.job_tasks SET status = $1, result_json = $2, error_message = $3, finished_at = $4, updated_at = $4 WHERE id = $5 AND status = 'running' AND runtime_owner_id = $6",
    )
    .bind(status)
    .bind(result_json)
    .bind(error_message)
    .bind(now)
    .bind(task_id)
    .bind(runtime_owner_id)
    .execute(&mut *tx)
    .await
    .context("finalize PostgreSQL task")?;
    if updated.rows_affected() == 0 {
        tx.rollback()
            .await
            .context("rollback lost PostgreSQL task finalize")?;
        return Ok(false);
    }
    let payload = serde_json::json!({
        "task_id": task_id,
        "status": status,
        "error": error_message,
    });
    sqlx::query(
        "INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at) VALUES ($1, $2, 'task.completed', $3, $4)",
    )
    .bind(crate::local_id::generate_local_id())
    .bind(task_id)
    .bind(serde_json::to_string(&payload).context("serialize PostgreSQL task completion event")?)
    .bind(now)
    .execute(&mut *tx)
    .await
    .context("record PostgreSQL task completion event")?;
    tx.commit()
        .await
        .context("commit PostgreSQL task finalize")?;
    Ok(true)
}

async fn claim_once(
    pool: &PgPool,
    runtime_owner_id: &str,
    now: DateTime<Utc>,
    allowed_task_types: Option<&[String]>,
) -> Result<Option<ClaimedTask>> {
    let mut tx = pool.begin().await.context("begin PostgreSQL task claim")?;
    let task = sqlx::query_as::<Postgres, ClaimedTask>(
        r#"
        SELECT id, task_type, source, requested_by, payload_json, cancel_requested
        FROM job_tasks AS candidate
        WHERE candidate.status = 'queued'
          AND (candidate.available_at IS NULL OR candidate.available_at <= $1)
          AND (
            candidate.task_type NOT IN ('sync.subscriptions', 'retry.recent_failures')
            OR NOT EXISTS (
              SELECT 1
              FROM job_tasks AS running
              WHERE running.task_type = candidate.task_type
                AND running.status = 'running'
            )
          )
          AND ($2::text[] IS NULL OR candidate.task_type = ANY($2))
        ORDER BY candidate.created_at ASC, candidate.id ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#,
    )
    .bind(now)
    .bind(allowed_task_types)
    .fetch_optional(&mut *tx)
    .await
    .context("select PostgreSQL queued task")?;

    let Some(task) = task else {
        tx.commit().await.context("commit empty PostgreSQL claim")?;
        return Ok(None);
    };

    let claimed = sqlx::query_as::<Postgres, ClaimedTask>(
        r#"
        UPDATE job_tasks
        SET status = 'running',
            started_at = $1,
            runtime_owner_id = $2,
            lease_heartbeat_at = $1,
            updated_at = $1
        WHERE id = $3 AND status = 'queued'
        RETURNING id, task_type, source, requested_by, payload_json, cancel_requested
        "#,
    )
    .bind(now)
    .bind(runtime_owner_id)
    .bind(&task.id)
    .fetch_optional(&mut *tx)
    .await
    .context("claim PostgreSQL queued task")?;

    let Some(claimed) = claimed else {
        tx.commit().await.context("commit lost PostgreSQL claim")?;
        return Ok(None);
    };

    let event_payload = serde_json::json!({
        "task_id": claimed.id,
        "status": "running",
    });
    sqlx::query(
        r#"
        INSERT INTO job_task_events (id, task_id, event_type, payload_json, created_at)
        VALUES ($1, $2, 'task.running', $3, $4)
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(&claimed.id)
    .bind(serde_json::to_string(&event_payload).context("serialize task event")?)
    .bind(now)
    .execute(&mut *tx)
    .await
    .context("record PostgreSQL task claim event")?;

    tx.commit().await.context("commit PostgreSQL task claim")?;
    Ok(Some(claimed))
}

fn is_retryable_sqlx_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.code().is_some_and(|code| code == "40001" || code == "40P01"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn claim_contract_uses_skip_locked_and_returning() {
        let source = include_str!("postgres_jobs.rs");
        assert!(source.contains("FOR UPDATE SKIP LOCKED"));
        assert!(source.contains("RETURNING id, task_type"));
        assert!(source.contains("40001") && source.contains("40P01"));
        assert!(source.contains("runtime_owner_id = $6"));
        assert!(source.contains("task.completed"));
        assert!(source.contains("task_type = ANY($2)"));
    }
}
