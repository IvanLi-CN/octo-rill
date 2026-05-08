use std::{future::Future, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use sqlx::{Sqlite, SqlitePool, Transaction};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use tracing::{debug, warn};

#[derive(Clone, Debug)]
pub struct SqliteWriteCoordinator {
    semaphore: Arc<Semaphore>,
    retry: SqliteWriteRetryConfig,
}

#[derive(Clone, Debug)]
struct SqliteWriteRetryConfig {
    max_attempts: usize,
    base_delay: Duration,
    max_delay: Duration,
}

impl Default for SqliteWriteCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl SqliteWriteCoordinator {
    pub fn new() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(1)),
            retry: SqliteWriteRetryConfig {
                max_attempts: 5,
                base_delay: Duration::from_millis(25),
                max_delay: Duration::from_millis(500),
            },
        }
    }

    pub async fn write<T, Fut, Op>(&self, lane: &'static str, mut operation: Op) -> Result<T>
    where
        Op: FnMut(usize) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut attempt = 1usize;
        loop {
            let wait_started = Instant::now();
            let permit = self
                .semaphore
                .clone()
                .acquire_owned()
                .await
                .context("sqlite writer coordinator closed")?;
            let waited = wait_started.elapsed();
            if !waited.is_zero() {
                debug!(
                    sqlite_write_lane = lane,
                    wait_ms = waited.as_millis(),
                    attempt,
                    "sqlite writer permit acquired"
                );
            }

            let op_started = Instant::now();
            let result = operation(attempt).await;
            let elapsed = op_started.elapsed();
            drop(permit);

            match result {
                Ok(value) => {
                    debug!(
                        sqlite_write_lane = lane,
                        elapsed_ms = elapsed.as_millis(),
                        attempt,
                        "sqlite write completed"
                    );
                    return Ok(value);
                }
                Err(err)
                    if is_sqlite_busy_error(err.as_ref()) && attempt < self.retry.max_attempts =>
                {
                    let delay = self.retry_delay(attempt);
                    warn!(
                        sqlite_write_lane = lane,
                        elapsed_ms = elapsed.as_millis(),
                        attempt,
                        retry_after_ms = delay.as_millis(),
                        err = %err,
                        "sqlite write hit busy state; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
                Err(err) => {
                    if is_sqlite_busy_error(err.as_ref()) {
                        warn!(
                            sqlite_write_lane = lane,
                            elapsed_ms = elapsed.as_millis(),
                            attempt,
                            err = %err,
                            "sqlite write exhausted busy retries"
                        );
                    }
                    return Err(err);
                }
            }
        }
    }

    pub async fn try_write<T, Fut, Op>(
        &self,
        lane: &'static str,
        operation: Op,
    ) -> Result<Option<T>>
    where
        Op: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let permit = match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(tokio::sync::TryAcquireError::NoPermits) => {
                debug!(
                    sqlite_write_lane = lane,
                    "sqlite writer permit unavailable; skipping best-effort write"
                );
                return Ok(None);
            }
            Err(tokio::sync::TryAcquireError::Closed) => {
                anyhow::bail!("sqlite writer coordinator closed");
            }
        };

        let op_started = Instant::now();
        let result = operation().await;
        let elapsed = op_started.elapsed();
        drop(permit);

        match result {
            Ok(value) => {
                debug!(
                    sqlite_write_lane = lane,
                    elapsed_ms = elapsed.as_millis(),
                    "sqlite best-effort write completed"
                );
                Ok(Some(value))
            }
            Err(err) => {
                if is_sqlite_busy_error(err.as_ref()) {
                    warn!(
                        sqlite_write_lane = lane,
                        elapsed_ms = elapsed.as_millis(),
                        err = %err,
                        "sqlite best-effort write hit busy state"
                    );
                }
                Err(err)
            }
        }
    }

    pub async fn acquire(&self, lane: &'static str) -> Result<SqliteWritePermit> {
        let wait_started = Instant::now();
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .context("sqlite writer coordinator closed")?;
        let waited = wait_started.elapsed();
        debug!(
            sqlite_write_lane = lane,
            wait_ms = waited.as_millis(),
            "sqlite writer permit acquired"
        );
        Ok(SqliteWritePermit {
            lane,
            acquired_at: Instant::now(),
            _permit: permit,
        })
    }

    pub async fn begin_immediate<'a>(
        &self,
        pool: &'a SqlitePool,
        lane: &'static str,
    ) -> Result<(SqliteWritePermit, Transaction<'a, Sqlite>)> {
        let mut attempt = 1usize;
        loop {
            let permit = self.acquire(lane).await?;
            let started = Instant::now();
            let result = pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .with_context(|| format!("begin sqlite write tx ({lane})"));
            let elapsed = started.elapsed();

            match result {
                Ok(tx) => {
                    debug!(
                        sqlite_write_lane = lane,
                        elapsed_ms = elapsed.as_millis(),
                        attempt,
                        "sqlite write transaction started"
                    );
                    return Ok((permit, tx));
                }
                Err(err)
                    if is_sqlite_busy_error(err.as_ref()) && attempt < self.retry.max_attempts =>
                {
                    let delay = self.retry_delay(attempt);
                    drop(permit);
                    warn!(
                        sqlite_write_lane = lane,
                        elapsed_ms = elapsed.as_millis(),
                        attempt,
                        retry_after_ms = delay.as_millis(),
                        err = %err,
                        "sqlite write transaction hit busy state; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
                Err(err) => {
                    drop(permit);
                    if is_sqlite_busy_error(err.as_ref()) {
                        warn!(
                            sqlite_write_lane = lane,
                            elapsed_ms = elapsed.as_millis(),
                            attempt,
                            err = %err,
                            "sqlite write transaction exhausted busy retries"
                        );
                    }
                    return Err(err);
                }
            }
        }
    }

    fn retry_delay(&self, attempt: usize) -> Duration {
        let shift = attempt.saturating_sub(1).min(8);
        let multiplier = 1_u32.checked_shl(shift as u32).unwrap_or(u32::MAX);
        self.retry
            .base_delay
            .saturating_mul(multiplier)
            .min(self.retry.max_delay)
    }
}

pub struct SqliteWritePermit {
    lane: &'static str,
    acquired_at: Instant,
    _permit: OwnedSemaphorePermit,
}

impl Drop for SqliteWritePermit {
    fn drop(&mut self) {
        debug!(
            sqlite_write_lane = self.lane,
            elapsed_ms = self.acquired_at.elapsed().as_millis(),
            "sqlite writer permit released"
        );
    }
}

pub fn is_sqlite_busy_error(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(err) = current {
        if let Some(sqlx_err) = err.downcast_ref::<sqlx::Error>()
            && sqlx_error_is_busy(sqlx_err)
        {
            return true;
        }
        let normalized = err.to_string().to_ascii_lowercase();
        if normalized.contains("database is locked")
            || normalized.contains("database table is locked")
            || normalized.contains("sqlite_busy")
            || normalized.contains("sqlite_busy_snapshot")
        {
            return true;
        }
        current = err.source();
    }
    false
}

fn sqlx_error_is_busy(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db_err) => {
            let code = db_err.code().map(|code| code.into_owned());
            matches!(code.as_deref(), Some("5" | "517" | "261"))
                || db_err
                    .message()
                    .to_ascii_lowercase()
                    .contains("database is locked")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

    #[tokio::test]
    async fn write_coordinator_serializes_concurrent_operations() {
        let coordinator = SqliteWriteCoordinator::new();
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();

        for _ in 0..16 {
            let coordinator = coordinator.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            handles.push(tokio::spawn(async move {
                coordinator
                    .write("test", |_| {
                        let active = active.clone();
                        let max_active = max_active.clone();
                        async move {
                            let now_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                            max_active.fetch_max(now_active, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(2)).await;
                            active.fetch_sub(1, Ordering::SeqCst);
                            Ok::<_, anyhow::Error>(())
                        }
                    })
                    .await
                    .expect("coordinated write");
            }));
        }

        for handle in handles {
            handle.await.expect("join write task");
        }

        assert_eq!(max_active.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn try_write_skips_when_writer_is_busy() {
        let coordinator = SqliteWriteCoordinator::new();
        let _permit = coordinator
            .acquire("held")
            .await
            .expect("acquire held permit");
        let ran = Arc::new(AtomicUsize::new(0));
        let result = coordinator
            .try_write("best_effort", || {
                let ran = ran.clone();
                async move {
                    ran.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, anyhow::Error>(())
                }
            })
            .await
            .expect("try write");

        assert!(result.is_none());
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn busy_detection_matches_sqlite_locked_messages() {
        let err = anyhow::anyhow!("error returned from database: (code: 5) database is locked");
        assert!(is_sqlite_busy_error(err.as_ref()));
    }

    #[tokio::test]
    async fn write_coordinator_serializes_wal_writes_across_pool_connections() {
        let database_path = test_database_path();
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_millis(50));
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .expect("create sqlite test db");
        sqlx::query(
            r#"
            CREATE TABLE writer_probe (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                lane TEXT NOT NULL,
                value INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create writer probe table");

        let coordinator = SqliteWriteCoordinator::new();
        let mut handles = Vec::new();
        for value in 0..32_i64 {
            let coordinator = coordinator.clone();
            let pool = pool.clone();
            handles.push(tokio::spawn(async move {
                let (_sqlite_write, mut tx) =
                    coordinator.begin_immediate(&pool, "test_wal_tx").await?;
                sqlx::query("INSERT INTO writer_probe (lane, value) VALUES (?, ?)")
                    .bind("test_wal_tx")
                    .bind(value)
                    .execute(&mut *tx)
                    .await?;
                tokio::time::sleep(Duration::from_millis(2)).await;
                tx.commit().await?;
                Ok::<_, anyhow::Error>(())
            }));
        }

        for handle in handles {
            handle
                .await
                .expect("join sqlite writer task")
                .expect("coordinated sqlite write");
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM writer_probe")
            .fetch_one(&pool)
            .await
            .expect("count writer probes");
        assert_eq!(count, 32);

        pool.close().await;
        let _ = std::fs::remove_file(&database_path);
        let _ = std::fs::remove_file(database_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(database_path.with_extension("db-shm"));
    }

    fn test_database_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "octo-rill-writer-coordinator-{}.db",
            crate::local_id::generate_local_id()
        ))
    }
}
