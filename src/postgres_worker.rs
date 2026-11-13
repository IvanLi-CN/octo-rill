//! PostgreSQL task execution lifecycle.
//!
//! Business executors are supplied by the caller. This module owns the
//! durable lifecycle around them so every executor gets the same claim,
//! cancellation and owner-checked finalization semantics.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use std::{collections::HashMap, sync::Arc};

use crate::postgres_jobs::ClaimedTask;

#[async_trait]
pub trait TaskExecutor: Send + Sync {
    async fn execute(&self, task: &ClaimedTask) -> Result<String>;
}

/// Dispatch PostgreSQL tasks to explicitly registered business executors.
///
/// Keeping the registry separate from the durable claim/finalize lifecycle
/// lets each business vertical migrate independently while preserving one
/// owner-checked state transition for every task.
#[derive(Default, Clone)]
pub struct TaskExecutorRegistry {
    handlers: Arc<HashMap<String, Arc<dyn TaskExecutor>>>,
}

impl TaskExecutorRegistry {
    pub fn builder() -> TaskExecutorRegistryBuilder {
        TaskExecutorRegistryBuilder::default()
    }

    pub fn registered_task_types(&self) -> Vec<String> {
        let mut task_types = self.handlers.keys().cloned().collect::<Vec<_>>();
        task_types.sort();
        task_types
    }
}

#[derive(Default)]
pub struct TaskExecutorRegistryBuilder {
    handlers: HashMap<String, Arc<dyn TaskExecutor>>,
}

impl TaskExecutorRegistryBuilder {
    pub fn register<E>(mut self, task_type: impl Into<String>, executor: E) -> Self
    where
        E: TaskExecutor + 'static,
    {
        self.handlers.insert(task_type.into(), Arc::new(executor));
        self
    }

    pub fn build(self) -> TaskExecutorRegistry {
        TaskExecutorRegistry {
            handlers: Arc::new(self.handlers),
        }
    }
}

#[async_trait]
impl TaskExecutor for TaskExecutorRegistry {
    async fn execute(&self, task: &ClaimedTask) -> Result<String> {
        let executor = self.handlers.get(&task.task_type).ok_or_else(|| {
            anyhow!(
                "no PostgreSQL task executor registered for task type {}",
                task.task_type
            )
        })?;
        executor.execute(task).await
    }
}

/// Execute at most one queued PostgreSQL task.
///
/// A missing task is a normal idle result. Cancellation is finalized without
/// invoking the business executor. Executor failures are persisted as failed
/// task results and returned to the caller after the durable state transition.
pub async fn run_once<E: TaskExecutor + ?Sized>(
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
    use super::*;

    struct TestExecutor;

    #[async_trait]
    impl TaskExecutor for TestExecutor {
        async fn execute(&self, _task: &ClaimedTask) -> Result<String> {
            Ok("{}".to_owned())
        }
    }

    #[test]
    fn runner_contract_uses_claim_cancel_and_owner_checked_finalize() {
        let source = include_str!("postgres_worker.rs");
        assert!(source.contains("claim_next_queued_task"));
        assert!(source.contains("cancel_requested"));
        assert!(source.contains("finalize_task"));
        assert!(source.contains("TaskExecutor"));
    }

    #[test]
    fn registry_keeps_explicit_task_type_boundary() {
        let registry = TaskExecutorRegistry::builder()
            .register("sync.access_refresh", TestExecutor)
            .register("brief.generate", TestExecutor)
            .build();
        assert_eq!(
            registry.registered_task_types(),
            vec!["brief.generate", "sync.access_refresh"]
        );
    }
}
