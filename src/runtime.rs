use std::{future::Future, time::Duration};

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};

pub const RUNTIME_LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const RUNTIME_LEASE_STALE_AFTER: Duration = Duration::from_secs(90);
pub const RUNTIME_LEASE_EXPIRED_ERROR: &str = "runtime_lease_expired";
pub const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

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
