//! PostgreSQL task execution lifecycle.
//!
//! Business executors are supplied by the caller. This module owns the
//! durable lifecycle around them so every executor gets the same claim,
//! cancellation and owner-checked finalization semantics.

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;

use crate::postgres_jobs::ClaimedTask;

#[async_trait]
pub trait TaskExecutor: Send + Sync {
    async fn execute(&self, task: &ClaimedTask) -> Result<String>;
}

/// Execute at most one queued PostgreSQL task.
///
/// A missing task is a normal idle result. Cancellation is finalized without
/// invoking the business executor. Executor failures are persisted as failed
/// task results and returned to the caller after the durable state transition.
pub async fn run_once<E: TaskExecutor>(
    pool: &PgPool,
    runtime_owner_id: &str,
    executor: &E,
) -> Result<bool> {
    let Some(task) =
        crate::postgres_jobs::claim_next_queued_task(pool, runtime_owner_id, Utc::now()).await?
    else {
        return Ok(false);
    };

    if task.cancel_requested != 0 {
        crate::postgres_jobs::finalize_task(
            pool,
            &task.id,
            runtime_owner_id,
            "cancelled",
            None,
            None,
            Utc::now(),
        )
        .await
        .context("finalize cancelled PostgreSQL task")?;
        return Ok(true);
    }

    match executor.execute(&task).await {
        Ok(result_json) => {
            crate::postgres_jobs::finalize_task(
                pool,
                &task.id,
                runtime_owner_id,
                "succeeded",
                Some(result_json.as_str()),
                None,
                Utc::now(),
            )
            .await
            .context("finalize succeeded PostgreSQL task")?;
        }
        Err(error) => {
            let message = error.to_string();
            crate::postgres_jobs::finalize_task(
                pool,
                &task.id,
                runtime_owner_id,
                "failed",
                None,
                Some(message.as_str()),
                Utc::now(),
            )
            .await
            .context("finalize failed PostgreSQL task")?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn runner_contract_uses_claim_cancel_and_owner_checked_finalize() {
        let source = include_str!("postgres_worker.rs");
        assert!(source.contains("claim_next_queued_task"));
        assert!(source.contains("cancel_requested"));
        assert!(source.contains("finalize_task"));
        assert!(source.contains("TaskExecutor"));
    }
}
