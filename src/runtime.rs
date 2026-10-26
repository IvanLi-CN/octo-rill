use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};

use crate::{admin_runtime, state::AppState};

pub const RUNTIME_LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const RUNTIME_LEASE_STALE_AFTER: Duration = Duration::from_secs(90);
pub const RUNTIME_LEASE_EXPIRED_ERROR: &str = "runtime_lease_expired";
pub const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const RUNTIME_SETTINGS_SYNC_TIMEOUT: Duration = Duration::from_secs(30);
const LEASE_HEARTBEAT_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(250),
    Duration::from_millis(500),
];

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

impl Drop for LeaseHeartbeat {
    fn drop(&mut self) {
        self.stop_tx.take();
        if let Some(join_handle) = self.join_handle.take() {
            join_handle.abort();
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
                    let mut retry_count = 0;
                    loop {
                        match beat().await {
                            Ok(()) => {
                                if retry_count > 0 {
                                    tracing::info!(
                                        event = "runtime.lease_heartbeat.recovered",
                                        lease = label,
                                        retry_count,
                                        "runtime lease heartbeat recovered after sqlite backpressure"
                                    );
                                }
                                break;
                            }
                            Err(err)
                                if is_sqlite_heartbeat_backpressure(&err)
                                    && retry_count < LEASE_HEARTBEAT_RETRY_DELAYS.len() =>
                            {
                                let retry_after = LEASE_HEARTBEAT_RETRY_DELAYS[retry_count];
                                retry_count += 1;
                                tracing::warn!(
                                    event = "runtime.lease_heartbeat.retry",
                                    lease = label,
                                    error_kind = sqlite_heartbeat_error_kind(&err),
                                    retry_count,
                                    max_retries = LEASE_HEARTBEAT_RETRY_DELAYS.len(),
                                    retry_after_ms = retry_after.as_millis(),
                                    ?err,
                                    "runtime lease heartbeat deferred by sqlite backpressure"
                                );
                                tokio::select! {
                                    _ = &mut stop_rx => return,
                                    _ = time::sleep(retry_after) => {}
                                }
                            }
                            Err(err) => {
                                tracing::warn!(
                                    event = "runtime.lease_heartbeat.failed",
                                    lease = label,
                                    error_kind = sqlite_heartbeat_error_kind(&err),
                                    retry_count,
                                    ?err,
                                    "runtime lease heartbeat failed"
                                );
                                break;
                            }
                        }
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

fn is_sqlite_heartbeat_backpressure(error: &anyhow::Error) -> bool {
    crate::sqlite_write::is_sqlite_background_admission_error(error.as_ref())
        || crate::sqlite_write::is_sqlite_busy_error(error.as_ref())
        || crate::sqlite_write::is_sqlite_write_deadline_error(error.as_ref())
}

fn sqlite_heartbeat_error_kind(error: &anyhow::Error) -> &'static str {
    if crate::sqlite_write::is_sqlite_background_admission_error(error.as_ref()) {
        "background_admission"
    } else if crate::sqlite_write::is_sqlite_busy_error(error.as_ref()) {
        "busy"
    } else if crate::sqlite_write::is_sqlite_write_deadline_error(error.as_ref()) {
        "writer_deadline"
    } else {
        "other"
    }
}

pub fn stale_cutoff_timestamp(now: DateTime<Utc>) -> String {
    let cutoff = chrono::Duration::from_std(RUNTIME_LEASE_STALE_AFTER)
        .unwrap_or_else(|_| chrono::Duration::seconds(90));
    (now - cutoff).to_rfc3339()
}

pub async fn register_runtime_owner(state: &AppState) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    state
        .sqlite_writer
        .write("runtime_owner_register", |_| async {
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
            .execute(state.sqlite_writer.write_pool_or(&state.pool))
            .await
            .context("failed to register runtime owner")?;
            Ok::<(), anyhow::Error>(())
        })
        .await?;

    prune_stale_runtime_owners(state).await?;
    Ok(())
}

pub async fn unregister_runtime_owner(state: &AppState) -> Result<()> {
    state
        .sqlite_writer
        .write("runtime_owner_unregister", |_| async {
            sqlx::query(
                r#"
                DELETE FROM runtime_owners
                WHERE runtime_owner_id = ?
                "#,
            )
            .bind(state.runtime_owner_id.as_str())
            .execute(state.sqlite_writer.write_pool_or(&state.pool))
            .await
            .context("failed to unregister runtime owner")?;
            Ok::<(), anyhow::Error>(())
        })
        .await?;
    Ok(())
}

pub fn spawn_runtime_owner_heartbeat(state: Arc<AppState>) -> LeaseHeartbeat {
    let settings_sync_in_flight = Arc::new(AtomicBool::new(false));
    spawn_lease_heartbeat(
        "runtime_owner",
        RUNTIME_LEASE_HEARTBEAT_INTERVAL,
        move || {
            let state = state.clone();
            let settings_sync_in_flight = settings_sync_in_flight.clone();
            async move {
                touch_runtime_owner_lease(state.as_ref()).await?;
                if settings_sync_in_flight
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    tokio::spawn(async move {
                        let result = time::timeout(RUNTIME_SETTINGS_SYNC_TIMEOUT, async {
                            let _runtime_settings_guard =
                                admin_runtime::runtime_settings_update_lock().lock().await;
                            admin_runtime::sync_persisted_runtime_settings(state).await
                        })
                        .await;
                        settings_sync_in_flight.store(false, Ordering::Release);
                        match result {
                            Ok(Ok(_)) => {}
                            Ok(Err(err)) => tracing::warn!(
                                event = "runtime.settings_sync",
                                error_chain = %crate::observability::error_chain_summary(err.as_ref()),
                                "runtime settings reconciliation failed"
                            ),
                            Err(_) => tracing::warn!(
                                event = "runtime.settings_sync",
                                timeout_ms = RUNTIME_SETTINGS_SYNC_TIMEOUT.as_millis(),
                                "runtime settings reconciliation timed out"
                            ),
                        }
                    });
                }
                Ok(())
            }
        },
    )
}

async fn touch_runtime_owner_lease(state: &AppState) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let updated_rows = state
        .sqlite_writer
        .write("runtime_owner_heartbeat", |_| async {
            Ok::<_, anyhow::Error>(
                sqlx::query(
                    r#"
                UPDATE runtime_owners
                SET lease_heartbeat_at = ?, updated_at = ?
                WHERE runtime_owner_id = ?
                "#,
                )
                .bind(now.as_str())
                .bind(now.as_str())
                .bind(state.runtime_owner_id.as_str())
                .execute(state.sqlite_writer.write_pool_or(&state.pool))
                .await
                .context("failed to heartbeat runtime owner")?
                .rows_affected(),
            )
        })
        .await?;

    if updated_rows == 0 {
        register_runtime_owner(state).await?;
    }

    Ok(())
}

async fn prune_stale_runtime_owners(state: &AppState) -> Result<()> {
    let cutoff = stale_cutoff_timestamp(Utc::now());
    let result = state
        .sqlite_writer
        .try_write("runtime_owner_prune", || async {
            sqlx::query(
                r#"
                DELETE FROM runtime_owners
                WHERE runtime_owner_id != ?
                  AND julianday(lease_heartbeat_at) <= julianday(?)
                "#,
            )
            .bind(state.runtime_owner_id.as_str())
            .bind(cutoff.as_str())
            .execute(state.sqlite_writer.write_pool_or(&state.pool))
            .await
            .context("failed to prune stale runtime owners")?;
            Ok::<(), anyhow::Error>(())
        })
        .await;
    if let Err(err) = result {
        tracing::warn!(?err, "skipped best-effort stale runtime owner prune");
    }
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

#[cfg(test)]
pub async fn touch_runtime_owner_lease_for_tests(state: &AppState) -> Result<()> {
    touch_runtime_owner_lease(state).await
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tokio::sync::Notify;

    use super::{Duration, LEASE_HEARTBEAT_RETRY_DELAYS, spawn_lease_heartbeat};

    type ErrorFactory = Arc<dyn Fn() -> anyhow::Error + Send + Sync>;

    struct DropProbe(Arc<Notify>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.notify_one();
        }
    }

    #[tokio::test]
    async fn dropping_lease_heartbeat_aborts_an_active_beat() {
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(Notify::new());
        let heartbeat = {
            let started = started.clone();
            let dropped = dropped.clone();
            spawn_lease_heartbeat("test", Duration::from_millis(1), move || {
                let started = started.clone();
                let dropped = dropped.clone();
                async move {
                    started.notify_one();
                    let _probe = DropProbe(dropped);
                    std::future::pending::<()>().await;
                    Ok(())
                }
            })
        };

        tokio::time::timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("heartbeat beat should start");
        drop(heartbeat);
        tokio::time::timeout(Duration::from_secs(1), dropped.notified())
            .await
            .expect("dropping heartbeat should abort its active beat");
    }

    #[tokio::test]
    async fn lease_heartbeat_retries_sqlite_backpressure_and_stays_alive() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let recovered = Arc::new(Notify::new());
        let heartbeat = {
            let attempts = attempts.clone();
            let recovered = recovered.clone();
            spawn_lease_heartbeat("test", Duration::from_millis(1), move || {
                let attempts = attempts.clone();
                let recovered = recovered.clone();
                async move {
                    if attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                        return Err(anyhow::Error::new(
                            crate::sqlite_write::SqliteBackgroundAdmissionError {
                                lane: "heartbeat_test",
                                waiting_foreground: 1,
                                waiting_background: 1,
                                queue_limit: 1,
                            },
                        ));
                    }
                    recovered.notify_one();
                    Ok(())
                }
            })
        };

        tokio::time::timeout(Duration::from_secs(1), recovered.notified())
            .await
            .expect("heartbeat should recover after sqlite backpressure");
        assert!(attempts.load(Ordering::Acquire) >= 2);
        heartbeat.stop().await;
    }

    #[tokio::test]
    async fn lease_heartbeat_bounds_each_sqlite_backpressure_retry_class() {
        let error_factories: [(&str, ErrorFactory); 3] = [
            (
                "background_admission",
                Arc::new(|| {
                    anyhow::Error::new(crate::sqlite_write::SqliteBackgroundAdmissionError {
                        lane: "heartbeat_test",
                        waiting_foreground: 1,
                        waiting_background: 1,
                        queue_limit: 1,
                    })
                }),
            ),
            (
                "busy",
                Arc::new(|| anyhow::anyhow!("error returned from database: database is locked")),
            ),
            (
                "writer_deadline",
                Arc::new(|| {
                    anyhow::Error::new(crate::sqlite_write::SqliteWriteDeadlineError {
                        lane: "heartbeat_test",
                        priority: "background",
                        phase: "acquire",
                        deadline_ms: 100,
                    })
                }),
            ),
        ];

        for (expected_kind, error_factory) in error_factories {
            let error = error_factory();
            assert!(
                super::is_sqlite_heartbeat_backpressure(&error),
                "{expected_kind} should be retryable backpressure"
            );
            assert_eq!(
                super::sqlite_heartbeat_error_kind(&error),
                expected_kind,
                "{expected_kind} should keep its classification"
            );

            let attempts = Arc::new(AtomicUsize::new(0));
            let exhausted = Arc::new(Notify::new());
            let heartbeat = {
                let attempts = attempts.clone();
                let exhausted = exhausted.clone();
                spawn_lease_heartbeat("test", Duration::from_secs(2), move || {
                    let attempts = attempts.clone();
                    let exhausted = exhausted.clone();
                    let error_factory = error_factory.clone();
                    async move {
                        let attempt = attempts.fetch_add(1, Ordering::AcqRel) + 1;
                        if attempt == LEASE_HEARTBEAT_RETRY_DELAYS.len() + 1 {
                            exhausted.notify_one();
                        }
                        Err(error_factory())
                    }
                })
            };

            tokio::time::timeout(Duration::from_secs(4), exhausted.notified())
                .await
                .unwrap_or_else(|_| {
                    panic!("{expected_kind} heartbeat should exhaust its bounded retries")
                });
            assert_eq!(
                attempts.load(Ordering::Acquire),
                LEASE_HEARTBEAT_RETRY_DELAYS.len() + 1,
                "{expected_kind} heartbeat should stop after the initial attempt plus bounded retries"
            );
            heartbeat.stop().await;
        }
    }

    #[tokio::test]
    async fn lease_heartbeat_does_not_retry_non_sqlite_errors_within_one_tick() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(Notify::new());
        let heartbeat = {
            let attempts = attempts.clone();
            let failed = failed.clone();
            spawn_lease_heartbeat("test", Duration::from_millis(100), move || {
                let attempts = attempts.clone();
                let failed = failed.clone();
                async move {
                    attempts.fetch_add(1, Ordering::AcqRel);
                    failed.notify_one();
                    Err(anyhow::anyhow!("non-sqlite heartbeat failure"))
                }
            })
        };

        tokio::time::timeout(Duration::from_secs(1), failed.notified())
            .await
            .expect("heartbeat should report the non-sqlite failure");
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(attempts.load(Ordering::Acquire), 1);
        heartbeat.stop().await;
    }
}
