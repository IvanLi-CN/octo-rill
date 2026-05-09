use std::{fmt, time::Duration};

use async_trait::async_trait;
use tokio::time::Instant;
use tower_sessions::{
    ExpiredDeletion, SessionStore,
    session::{Id, Record},
    session_store,
};
use tower_sessions_sqlx_store::SqliteStore;
use tracing::{debug, warn};

use crate::sqlite_write::{SqliteWriteCoordinator, SqliteWritePriority};

#[derive(Clone)]
pub struct CoordinatedSqliteSessionStore {
    inner: SqliteStore,
    sqlite_writer: SqliteWriteCoordinator,
}

impl CoordinatedSqliteSessionStore {
    pub fn new(inner: SqliteStore, sqlite_writer: SqliteWriteCoordinator) -> Self {
        Self {
            inner,
            sqlite_writer,
        }
    }

    pub async fn migrate(&self) -> sqlx::Result<()> {
        self.inner.migrate().await
    }
}

impl fmt::Debug for CoordinatedSqliteSessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoordinatedSqliteSessionStore")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl SessionStore for CoordinatedSqliteSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let mut attempt = 1usize;
        loop {
            let _permit = self
                .sqlite_writer
                .acquire_with_priority("session_create", SqliteWritePriority::Foreground)
                .await
                .map_err(anyhow_session_error)?;
            let started = Instant::now();
            let result = self.inner.create(record).await;
            match result {
                Ok(()) => {
                    debug!(
                        sqlite_write_lane = "session_create",
                        sqlite_write_priority = SqliteWritePriority::Foreground.as_str(),
                        elapsed_ms = started.elapsed().as_millis(),
                        attempt,
                        "sqlite session write completed"
                    );
                    return Ok(());
                }
                Err(err) if session_error_is_busy(&err) && attempt < SESSION_WRITE_MAX_ATTEMPTS => {
                    warn!(
                        sqlite_write_lane = "session_create",
                        sqlite_write_priority = SqliteWritePriority::Foreground.as_str(),
                        elapsed_ms = started.elapsed().as_millis(),
                        attempt,
                        retry_after_ms = session_retry_delay(attempt).as_millis(),
                        err = %err,
                        "sqlite session write hit busy state; retrying"
                    );
                    tokio::time::sleep(session_retry_delay(attempt)).await;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let mut attempt = 1usize;
        loop {
            let _permit = self
                .sqlite_writer
                .acquire_with_priority("session_save", SqliteWritePriority::Foreground)
                .await
                .map_err(anyhow_session_error)?;
            let started = Instant::now();
            let result = self.inner.save(record).await;
            match result {
                Ok(()) => {
                    debug!(
                        sqlite_write_lane = "session_save",
                        sqlite_write_priority = SqliteWritePriority::Foreground.as_str(),
                        elapsed_ms = started.elapsed().as_millis(),
                        attempt,
                        "sqlite session write completed"
                    );
                    return Ok(());
                }
                Err(err) if session_error_is_busy(&err) && attempt < SESSION_WRITE_MAX_ATTEMPTS => {
                    warn!(
                        sqlite_write_lane = "session_save",
                        sqlite_write_priority = SqliteWritePriority::Foreground.as_str(),
                        elapsed_ms = started.elapsed().as_millis(),
                        attempt,
                        retry_after_ms = session_retry_delay(attempt).as_millis(),
                        err = %err,
                        "sqlite session write hit busy state; retrying"
                    );
                    tokio::time::sleep(session_retry_delay(attempt)).await;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        self.inner.load(session_id).await
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        let mut attempt = 1usize;
        loop {
            let _permit = self
                .sqlite_writer
                .acquire_with_priority("session_delete", SqliteWritePriority::Foreground)
                .await
                .map_err(anyhow_session_error)?;
            let started = Instant::now();
            let result = self.inner.delete(session_id).await;
            match result {
                Ok(()) => {
                    debug!(
                        sqlite_write_lane = "session_delete",
                        sqlite_write_priority = SqliteWritePriority::Foreground.as_str(),
                        elapsed_ms = started.elapsed().as_millis(),
                        attempt,
                        "sqlite session write completed"
                    );
                    return Ok(());
                }
                Err(err) if session_error_is_busy(&err) && attempt < SESSION_WRITE_MAX_ATTEMPTS => {
                    warn!(
                        sqlite_write_lane = "session_delete",
                        sqlite_write_priority = SqliteWritePriority::Foreground.as_str(),
                        elapsed_ms = started.elapsed().as_millis(),
                        attempt,
                        retry_after_ms = session_retry_delay(attempt).as_millis(),
                        err = %err,
                        "sqlite session write hit busy state; retrying"
                    );
                    tokio::time::sleep(session_retry_delay(attempt)).await;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }
}

#[async_trait]
impl ExpiredDeletion for CoordinatedSqliteSessionStore {
    async fn delete_expired(&self) -> session_store::Result<()> {
        let Some(_permit) = self
            .sqlite_writer
            .try_acquire_best_effort("session_delete_expired")
        else {
            debug!("skip session expiry cleanup because sqlite writer is busy");
            return Ok(());
        };
        if let Err(err) = self.inner.delete_expired().await {
            if session_error_is_busy(&err) {
                warn!(
                    err = %err,
                    "skip session expiry cleanup after sqlite busy state"
                );
                return Ok(());
            }
            return Err(err);
        }
        Ok(())
    }
}

fn anyhow_session_error(err: anyhow::Error) -> session_store::Error {
    session_store::Error::Backend(err.to_string())
}

const SESSION_WRITE_MAX_ATTEMPTS: usize = 5;
const SESSION_WRITE_BASE_DELAY: Duration = Duration::from_millis(25);
const SESSION_WRITE_MAX_DELAY: Duration = Duration::from_millis(500);

fn session_retry_delay(attempt: usize) -> Duration {
    let shift = attempt.saturating_sub(1).min(8);
    let multiplier = 1_u32.checked_shl(shift as u32).unwrap_or(u32::MAX);
    SESSION_WRITE_BASE_DELAY
        .saturating_mul(multiplier)
        .min(SESSION_WRITE_MAX_DELAY)
}

fn session_error_is_busy(err: &session_store::Error) -> bool {
    let normalized = err.to_string().to_ascii_lowercase();
    normalized.contains("database is locked")
        || normalized.contains("database table is locked")
        || normalized.contains("sqlite_busy")
        || normalized.contains("sqlstate")
            && (normalized.contains("database is busy") || normalized.contains("locked"))
}
