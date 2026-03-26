use std::{future::Future, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};

use crate::state::AppState;

pub const RUNTIME_LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const RUNTIME_LEASE_STALE_AFTER: Duration = Duration::from_secs(90);
pub const RUNTIME_LEASE_EXPIRED_ERROR: &str = "runtime_lease_expired";
pub const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeRecoveryMode {
    Startup,
    Sweep,
}

pub struct LeaseHeartbeat {
    stop_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

impl LeaseHeartbeat {
    pub fn disabled() -> Self {
        Self {
            stop_tx: None,
            join_handle: None,
        }
    }

    pub async fn stop(mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.await;
        }
    }
}

pub fn spawn_lease_heartbeat<F, Fut>(
    label: &'static str,
    interval: Duration,
    mut beat: F,
) -> LeaseHeartbeat
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let join_handle = tokio::spawn(async move {
        let mut ticker = time::interval(interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        ticker.tick().await;
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = ticker.tick() => {
                    if let Err(err) = beat().await {
                        tracing::warn!(?err, lease = label, "runtime lease heartbeat failed");
                    }
                }
            }
        }
    });

    LeaseHeartbeat {
        stop_tx: Some(stop_tx),
        join_handle: Some(join_handle),
    }
}

pub fn stale_cutoff_timestamp(now: DateTime<Utc>) -> String {
    let cutoff = chrono::Duration::from_std(RUNTIME_LEASE_STALE_AFTER)
        .unwrap_or_else(|_| chrono::Duration::seconds(90));
    (now - cutoff).to_rfc3339()
}

pub async fn register_runtime_owner(state: &AppState) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO runtime_owners (runtime_owner_id, lease_heartbeat_at, created_at, updated_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(runtime_owner_id) DO UPDATE SET
          lease_heartbeat_at = excluded.lease_heartbeat_at,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(state.runtime_owner_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&state.pool)
    .await
    .context("failed to register runtime owner")?;

    prune_stale_runtime_owners(state).await?;
    Ok(())
}

pub async fn unregister_runtime_owner(state: &AppState) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM runtime_owners
        WHERE runtime_owner_id = ?
        "#,
    )
    .bind(state.runtime_owner_id.as_str())
    .execute(&state.pool)
    .await
    .context("failed to unregister runtime owner")?;
    Ok(())
}

pub fn spawn_runtime_owner_heartbeat(state: Arc<AppState>) -> LeaseHeartbeat {
    spawn_lease_heartbeat(
        "runtime_owner",
        RUNTIME_LEASE_HEARTBEAT_INTERVAL,
        move || {
            let state = state.clone();
            async move { touch_runtime_owner_lease(state.as_ref()).await }
        },
    )
}

async fn touch_runtime_owner_lease(state: &AppState) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let updated_rows = sqlx::query(
        r#"
        UPDATE runtime_owners
        SET lease_heartbeat_at = ?, updated_at = ?
        WHERE runtime_owner_id = ?
        "#,
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(state.runtime_owner_id.as_str())
    .execute(&state.pool)
    .await
    .context("failed to heartbeat runtime owner")?
    .rows_affected();

    if updated_rows == 0 {
        register_runtime_owner(state).await?;
    }

    Ok(())
}

async fn prune_stale_runtime_owners(state: &AppState) -> Result<()> {
    let cutoff = stale_cutoff_timestamp(Utc::now());
    sqlx::query(
        r#"
        DELETE FROM runtime_owners
        WHERE runtime_owner_id != ?
          AND julianday(lease_heartbeat_at) <= julianday(?)
        "#,
    )
    .bind(state.runtime_owner_id.as_str())
    .bind(cutoff.as_str())
    .execute(&state.pool)
    .await
    .context("failed to prune stale runtime owners")?;
    Ok(())
}

#[cfg(test)]
pub async fn upsert_runtime_owner_for_tests(
    state: &AppState,
    runtime_owner_id: &str,
    lease_heartbeat_at: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO runtime_owners (runtime_owner_id, lease_heartbeat_at, created_at, updated_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(runtime_owner_id) DO UPDATE SET
          lease_heartbeat_at = excluded.lease_heartbeat_at,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(runtime_owner_id)
    .bind(lease_heartbeat_at)
    .bind(lease_heartbeat_at)
    .bind(lease_heartbeat_at)
    .execute(&state.pool)
    .await
    .context("failed to upsert runtime owner for tests")?;
    Ok(())
}
