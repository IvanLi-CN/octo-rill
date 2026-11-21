//! PostgreSQL implementation of the GitHub release webhook receiver.
//!
//! Mirrors `crate::webhook_push::receive` (the legacy SQLite runtime) while
//! recording deliveries and queueing one single-repository `sync.repo_release`
//! refresh per delivery.

use std::{future::Future, time::Duration};

use anyhow::Context;
use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::FromRow;
use tokio::time::Instant;
use tracing::warn;

use crate::{
    error::ApiError,
    local_id,
    postgres_server::PostgresServerState,
    postgres_write::contains_retryable_sqlstate,
    webhook_push::{
        DESIRED_ENABLED, ReceiverQuery, ReleasePayload, has_valid_signature_format,
        private_repo_scope_allows, should_enqueue_release, verify_signature,
    },
};

const RECEIVER_WRITE_DEADLINE: Duration = Duration::from_millis(750);
const RECEIVER_RECOVERY_RESERVE: Duration = Duration::from_millis(250);
const RECEIVER_RETRY_AFTER_SECONDS: u64 = 1;
const STALE_PROCESSING_MINUTES: i64 = 5;

#[derive(Debug, FromRow)]
struct ReceiverRow {
    user_id: String,
    include_own_releases: i64,
    webhook_push_desired_state: String,
    webhook_push_secret_ciphertext: Vec<u8>,
    webhook_push_secret_nonce: Vec<u8>,
    repo_id: i64,
    repo_full_name: String,
    status: String,
    is_private: Option<i64>,
    allows_private: Option<i64>,
}

/// Receive a GitHub release webhook delivery.
///
/// The route is POST-only, matching the legacy receiver. Unknown hook/callback
/// pairs, `ping` events, duplicates and filtered deliveries are acknowledged
/// with HTTP 200 and an explicit `reason`, exactly like the SQLite runtime.
pub(crate) async fn receive(
    State(state): State<PostgresServerState>,
    Query(query): Query<ReceiverQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let delivery = headers
        .get("x-github-delivery")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("X-GitHub-Delivery is required"))?;
    let event = headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("X-GitHub-Event is required"))?;
    let hook_id = headers
        .get("x-github-hook-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| ApiError::bad_request("X-GitHub-Hook-ID is required"))?;
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_signature",
                "X-Hub-Signature-256 is required",
            )
        })?;
    if !has_valid_signature_format(signature) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "X-Hub-Signature-256 is malformed",
        ));
    }

    let row = sqlx::query_as::<_, ReceiverRow>(
        r#"
        SELECT
          u.id AS user_id,
          u.include_own_releases,
          u.webhook_push_desired_state,
          u.webhook_push_secret_ciphertext,
          u.webhook_push_secret_nonce,
          wr.repo_id,
          wr.repo_full_name,
          wr.status,
          (SELECT ob.is_private
           FROM public.owned_repo_star_baselines ob
           WHERE ob.user_id = wr.user_id AND ob.repo_id = wr.repo_id
           LIMIT 1) AS is_private,
          (SELECT pat.webhook_push_allows_private_repos
           FROM public.reaction_pat_tokens pat
           WHERE pat.user_id = u.id
           LIMIT 1) AS allows_private
        FROM public.users u
        JOIN public.webhook_push_repos wr ON wr.user_id = u.id AND wr.hook_id = $1
        WHERE u.webhook_push_callback_key = $2
          AND u.webhook_push_secret_ciphertext IS NOT NULL
          AND u.webhook_push_secret_nonce IS NOT NULL
          AND EXISTS (
            SELECT 1 FROM public.reaction_pat_tokens pat
            WHERE pat.user_id = u.id
              AND pat.owner_github_user_id = wr.owner_github_user_id
              AND pat.owner_login IS NOT NULL
              AND lower(substr(wr.repo_full_name, 1, position('/' in wr.repo_full_name) - 1)) = lower(pat.owner_login)
          )
          AND EXISTS (
            SELECT 1 FROM public.owned_repo_star_baselines ob
            WHERE ob.user_id = wr.user_id
              AND ob.repo_id = wr.repo_id
              AND EXISTS (
                SELECT 1 FROM public.reaction_pat_tokens pat
                WHERE pat.user_id = u.id
                  AND pat.owner_login IS NOT NULL
                  AND lower(substr(ob.repo_full_name, 1, position('/' in ob.repo_full_name) - 1)) = lower(pat.owner_login)
              )
          )
        "#,
    )
    .bind(hook_id)
    .bind(&query.key)
    .fetch_optional(&state.runtime.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some(row) = row else {
        return Ok(Json(
            json!({"accepted": true, "queued": false, "reason": "unknown_hook"}),
        ));
    };

    let secret = state
        .encryption_key
        .decrypt_str(
            &row.webhook_push_secret_ciphertext,
            &row.webhook_push_secret_nonce,
        )
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_signature",
                "Webhook secret is invalid",
            )
        })?;
    if !verify_signature(&secret, signature, &body) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "Webhook signature is invalid",
        ));
    }
    if event == "ping" {
        return Ok(Json(
            json!({"accepted": true, "queued": false, "reason": "ping"}),
        ));
    }

    let payload = serde_json::from_slice::<ReleasePayload>(&body)
        .map_err(|_| ApiError::bad_request("invalid GitHub webhook payload"))?;
    let action = payload.action.as_deref().unwrap_or("");
    let release = payload.release.as_ref();
    let repo = payload.repository.as_ref();
    let should_queue = should_enqueue_release(
        row.include_own_releases != 0,
        row.webhook_push_desired_state == DESIRED_ENABLED,
        event,
        &payload,
        row.repo_id,
        &row.repo_full_name,
        &row.status,
    ) && private_repo_scope_allows(row.is_private, row.allows_private);

    let recovery_deadline = Instant::now() + RECEIVER_WRITE_DEADLINE + RECEIVER_RECOVERY_RESERVE;
    let write_deadline = recovery_deadline - RECEIVER_RECOVERY_RESERVE;

    let claimed = receiver_stage(
        "webhook_receiver_delivery_claim",
        write_deadline,
        claim_delivery(
            &state,
            delivery,
            hook_id,
            repo.map(|item| item.id),
            event,
            action,
            should_queue,
        ),
    )
    .await?;
    if !claimed {
        return Ok(Json(
            json!({"accepted": true, "queued": false, "reason": "duplicate"}),
        ));
    }
    if !should_queue {
        return Ok(Json(
            json!({"accepted": true, "queued": false, "reason": "ignored"}),
        ));
    }

    let repo = repo.expect("repo checked above");
    let queued_task_id = match receiver_stage(
        "webhook_receiver_release_enqueue",
        write_deadline,
        enqueue_repo_release_refresh(&state, &row.user_id, repo.id, &repo.full_name, delivery),
    )
    .await
    {
        Ok(task_id) => task_id,
        Err(error) => {
            if let Err(rollback_error) =
                reset_delivery_to_pending(&state, delivery, recovery_deadline).await
            {
                warn!(
                    event = "webhook.receiver_write",
                    operation = "webhook_receiver_delivery_rollback",
                    error = %rollback_error,
                    recovery = "processing_lease_expiry",
                    "GitHub release delivery rollback failed after enqueue failure"
                );
                return Err(receiver_retryable_error(
                    "webhook_receiver_delivery_rollback",
                    "rollback_failed",
                ));
            }
            return Err(error);
        }
    };
    if let Err(error) = receiver_stage(
        "webhook_receiver_delivery_queued",
        write_deadline,
        mark_delivery_queued(&state, delivery, &queued_task_id),
    )
    .await
    {
        if let Err(rollback_error) =
            reset_delivery_to_pending(&state, delivery, recovery_deadline).await
        {
            warn!(
                event = "webhook.receiver_write",
                operation = "webhook_receiver_delivery_rollback",
                error = %rollback_error,
                recovery = "processing_lease_expiry",
                "GitHub release delivery rollback failed after queue-state failure"
            );
            return Err(receiver_retryable_error(
                "webhook_receiver_delivery_rollback",
                "rollback_failed",
            ));
        }
        return Err(error);
    }

    Ok(Json(json!({
        "accepted": true,
        "queued": true,
        "reason": "release_sync_queued",
        "release_id": release.map(|item| item.id),
    })))
}

async fn claim_delivery(
    state: &PostgresServerState,
    delivery: &str,
    hook_id: i64,
    repo_id: Option<i64>,
    event: &str,
    action: &str,
    should_queue: bool,
) -> anyhow::Result<bool> {
    let now = Utc::now();
    let received_at = now.to_rfc3339();
    let stale_processing_cutoff =
        (now - chrono::Duration::minutes(STALE_PROCESSING_MINUTES)).to_rfc3339();
    let mut transaction = state
        .runtime
        .pool
        .begin()
        .await
        .context("begin GitHub release delivery claim")?;
    sqlx::query(
        "INSERT INTO public.webhook_push_deliveries (delivery_id, hook_id, repo_id, event, action, received_at, processing_state) VALUES ($1, $2, $3, $4, $5, $6, 'pending') ON CONFLICT (delivery_id) DO NOTHING",
    )
    .bind(delivery)
    .bind(hook_id)
    .bind(repo_id)
    .bind(event)
    .bind(action)
    .bind(&received_at)
    .execute(&mut *transaction)
    .await
    .context("insert GitHub release delivery")?;
    let claimed = sqlx::query(
        "UPDATE public.webhook_push_deliveries SET processing_state = 'processing', processing_started_at = $1 WHERE delivery_id = $2 AND (processing_state = 'pending' OR (processing_state = 'processing' AND processing_started_at < $3))",
    )
    .bind(&received_at)
    .bind(delivery)
    .bind(&stale_processing_cutoff)
    .execute(&mut *transaction)
    .await
    .context("claim GitHub release delivery")?;
    if claimed.rows_affected() == 0 {
        transaction
            .commit()
            .await
            .context("commit duplicate GitHub release delivery")?;
        return Ok(false);
    }
    if !should_queue {
        sqlx::query(
            "UPDATE public.webhook_push_deliveries SET processing_state = 'ignored', processing_started_at = NULL WHERE delivery_id = $1",
        )
        .bind(delivery)
        .execute(&mut *transaction)
        .await
        .context("mark GitHub release delivery ignored")?;
    }
    transaction
        .commit()
        .await
        .context("commit GitHub release delivery claim")?;
    Ok(true)
}

async fn enqueue_repo_release_refresh(
    state: &PostgresServerState,
    user_id: &str,
    repo_id: i64,
    repo_full_name: &str,
    delivery_id: &str,
) -> anyhow::Result<String> {
    let mut transaction = state
        .runtime
        .pool
        .begin()
        .await
        .context("begin repo release refresh enqueue")?;
    // Serialize enqueues per (user, repository) so concurrent deliveries reuse
    // one queued `sync.repo_release` task instead of stacking duplicate work.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("repo-release-refresh:{user_id}:{repo_id}"))
        .execute(&mut *transaction)
        .await
        .context("lock repo release refresh enqueue")?;
    let repo_id_key = repo_id.to_string();
    let existing = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id FROM public.job_tasks
        WHERE task_type = 'sync.repo_release'
          AND status IN ('queued', 'running')
          AND requested_by = $1
          AND CASE
                WHEN pg_input_is_valid(payload_json, 'jsonb')
                THEN payload_json::jsonb ->> 'repo_id' = $2
                ELSE false
              END
        ORDER BY created_at ASC, id ASC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(&repo_id_key)
    .fetch_optional(&mut *transaction)
    .await
    .context("load pending repo release refresh task")?;
    if let Some(task_id) = existing {
        transaction
            .commit()
            .await
            .context("commit reused repo release refresh task")?;
        return Ok(task_id);
    }
    let task_id = local_id::generate_local_id();
    let now = Utc::now();
    let payload = json!({
        "user_id": user_id,
        "repo_id": repo_id,
        "repo_full_name": repo_full_name,
        "reason": "webhook_release",
        "delivery_id": delivery_id,
    });
    let inserted = sqlx::query_scalar::<_, String>(
        r#"
        INSERT INTO public.job_tasks (
          id, task_type, status, source, requested_by, payload_json, created_at, updated_at
        )
        SELECT $1, 'sync.repo_release', 'queued', $2, $3, $4, $5, $5
        WHERE NOT EXISTS (
          SELECT 1
          FROM public.job_tasks pending
          WHERE pending.task_type = 'sync.repo_release'
            AND pending.status IN ('queued', 'running')
            AND pending.requested_by = $3
            AND CASE
                  WHEN pg_input_is_valid(pending.payload_json, 'jsonb')
                  THEN pending.payload_json::jsonb ->> 'repo_id' = $6
                  ELSE false
                END
        )
        RETURNING id
        "#,
    )
    .bind(&task_id)
    .bind("webhook.release")
    .bind(user_id)
    .bind(payload.to_string())
    .bind(now)
    .bind(&repo_id_key)
    .fetch_optional(&mut *transaction)
    .await
    .context("enqueue repo release refresh task")?;
    let Some(inserted_task_id) = inserted else {
        transaction
            .rollback()
            .await
            .context("rollback duplicate repo release refresh task")?;
        return Err(anyhow::anyhow!(
            "repo release refresh enqueue lost its single-flight guard"
        ));
    };
    transaction
        .commit()
        .await
        .context("commit repo release refresh task")?;
    Ok(inserted_task_id)
}

async fn mark_delivery_queued(
    state: &PostgresServerState,
    delivery: &str,
    task_id: &str,
) -> anyhow::Result<()> {
    let mut transaction = state
        .runtime
        .pool
        .begin()
        .await
        .context("begin GitHub release delivery queued state")?;
    sqlx::query(
        "UPDATE public.webhook_push_deliveries SET queued_task_id = $1, processing_state = 'queued', processing_started_at = NULL WHERE delivery_id = $2",
    )
    .bind(task_id)
    .bind(delivery)
    .execute(&mut *transaction)
    .await
    .context("mark GitHub release delivery queued")?;
    transaction
        .commit()
        .await
        .context("commit GitHub release delivery queued state")?;
    Ok(())
}

async fn reset_delivery_to_pending(
    state: &PostgresServerState,
    delivery: &str,
    deadline: Instant,
) -> Result<(), ApiError> {
    let reset = async {
        let mut transaction = state
            .runtime
            .pool
            .begin()
            .await
            .context("begin GitHub release delivery rollback")?;
        sqlx::query(
            "UPDATE public.webhook_push_deliveries SET processing_state = 'pending', processing_started_at = NULL WHERE delivery_id = $1 AND processing_state = 'processing'",
        )
        .bind(delivery)
        .execute(&mut *transaction)
        .await
        .context("reset GitHub release delivery to pending")?;
        transaction
            .commit()
            .await
            .context("commit GitHub release delivery rollback")?;
        Ok::<_, anyhow::Error>(())
    };
    receiver_stage("webhook_receiver_delivery_rollback", deadline, reset).await
}

async fn receiver_stage<T, Fut>(
    operation: &'static str,
    deadline: Instant,
    future: Fut,
) -> Result<T, ApiError>
where
    Fut: Future<Output = anyhow::Result<T>>,
{
    if Instant::now() >= deadline {
        return Err(receiver_retryable_error(operation, "deadline_exceeded"));
    }
    match tokio::time::timeout_at(deadline, future).await {
        Err(_) => Err(receiver_retryable_error(operation, "deadline_exceeded")),
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) if is_retryable_postgres_write_error(&error) => {
            warn!(
                event = "webhook.receiver_write",
                operation,
                error_kind = "postgres_retryable",
                error_chain = %crate::observability::error_chain_summary(error.as_ref()),
                "GitHub release delivery write hit PostgreSQL contention"
            );
            Err(receiver_retryable_error(operation, "postgres_retryable"))
        }
        Ok(Err(error)) => Err(ApiError::internal(error)),
    }
}

fn is_retryable_postgres_write_error(error: &anyhow::Error) -> bool {
    let error_chain = format!("{error:?}");
    contains_retryable_sqlstate(&error_chain)
        || error_chain.to_ascii_lowercase().contains("pool timed out")
}

fn receiver_retryable_error(operation: &'static str, error_kind: &'static str) -> ApiError {
    warn!(
        event = "webhook.receiver_write",
        operation,
        error_kind,
        retry_after_seconds = RECEIVER_RETRY_AFTER_SECONDS,
        "GitHub release delivery needs to be retried"
    );
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "webhook_receiver_retryable",
        "Webhook delivery could not be recorded; retry the delivery",
    )
    .with_retry_after(RECEIVER_RETRY_AFTER_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_postgres_sqlstate_is_classified_as_retryable() {
        let error = anyhow::anyhow!("error returned from database: 40001: serialization failure");
        assert!(is_retryable_postgres_write_error(&error));
        assert!(!is_retryable_postgres_write_error(&anyhow::anyhow!(
            "error returned from database: 23505: duplicate key"
        )));
    }

    #[test]
    fn receiver_retryable_error_matches_legacy_contract() {
        let error = receiver_retryable_error("test_stage", "test_kind");
        assert_eq!(error.code(), "webhook_receiver_retryable");
    }
}
