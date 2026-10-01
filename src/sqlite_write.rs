use std::{
    borrow::Cow,
    future::Future,
    ops::{Deref, DerefMut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use sqlx::{
    Sqlite, SqliteConnection, SqlitePool, Transaction, TransactionManager,
    sqlite::SqliteTransactionManager,
};
use tokio::{sync::Notify, time::Instant};
use tracing::{debug, warn};

use crate::observability;

pub const SQLITE_WRITE_BUSY_TIMEOUT: Duration = Duration::from_millis(100);
const FOREGROUND_DEADLINE: Duration = Duration::from_millis(900);
const BACKGROUND_DEADLINE: Duration = Duration::from_millis(2500);
const BEST_EFFORT_DEADLINE: Duration = BACKGROUND_DEADLINE;
const ROLLBACK_CLEANUP_TIMEOUT: Duration = Duration::from_millis(150);

#[derive(Clone, Debug)]
pub struct SqliteWriteCoordinator {
    state: Arc<Mutex<SqliteWriteState>>,
    notify: Arc<Notify>,
    write_pool: Option<SqlitePool>,
    deadlines: SqliteWriteDeadlines,
    retry: SqliteWriteRetryConfig,
    slow_threshold_ms: usize,
}

#[derive(Clone, Copy, Debug)]
struct SqliteWriteDeadlines {
    foreground: Duration,
    background: Duration,
    best_effort: Duration,
}

impl Default for SqliteWriteDeadlines {
    fn default() -> Self {
        Self {
            foreground: FOREGROUND_DEADLINE,
            background: BACKGROUND_DEADLINE,
            best_effort: BEST_EFFORT_DEADLINE,
        }
    }
}

#[derive(Debug, Default)]
struct SqliteWriteState {
    active: bool,
    waiting_foreground: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SqliteWriteRuntimeStatus {
    pub active: bool,
    pub waiting_foreground: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqliteWritePriority {
    Foreground,
    Background,
    BestEffort,
}

struct ForegroundWaiter {
    state: Arc<Mutex<SqliteWriteState>>,
    notify: Arc<Notify>,
    registered: bool,
}

impl ForegroundWaiter {
    fn new(state: Arc<Mutex<SqliteWriteState>>, notify: Arc<Notify>) -> Self {
        Self {
            state,
            notify,
            registered: false,
        }
    }

    fn register(&mut self, state: &mut SqliteWriteState) {
        if !self.registered {
            state.waiting_foreground += 1;
            self.registered = true;
        }
    }

    fn complete(&mut self, state: &mut SqliteWriteState) {
        if self.registered {
            state.waiting_foreground = state.waiting_foreground.saturating_sub(1);
            self.registered = false;
        }
    }
}

impl Drop for ForegroundWaiter {
    fn drop(&mut self) {
        if self.registered
            && let Ok(mut state) = self.state.lock()
        {
            state.waiting_foreground = state.waiting_foreground.saturating_sub(1);
            self.notify.notify_waiters();
        }
    }
}

impl SqliteWritePriority {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
            Self::BestEffort => "best_effort",
        }
    }

    fn waits_for_foreground(self) -> bool {
        matches!(self, Self::Background | Self::BestEffort)
    }
}

#[derive(Debug, thiserror::Error)]
#[error(
    "retryable sqlite write deadline exceeded: lane={lane}, priority={priority}, phase={phase}, deadline_ms={deadline_ms}"
)]
pub struct SqliteWriteDeadlineError {
    pub lane: &'static str,
    pub priority: &'static str,
    pub phase: &'static str,
    pub deadline_ms: u64,
}

impl SqliteWriteDeadlineError {
    fn new(
        lane: &'static str,
        priority: SqliteWritePriority,
        phase: &'static str,
        deadline: Duration,
    ) -> Self {
        Self {
            lane,
            priority: priority.as_str(),
            phase,
            deadline_ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
        }
    }
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
        Self::with_write_pool_and_settings(None, SqliteWriteDeadlines::default())
    }

    pub fn with_write_pool(write_pool: SqlitePool) -> Self {
        Self::with_write_pool_and_settings(Some(write_pool), SqliteWriteDeadlines::default())
    }

    fn with_write_pool_and_settings(
        write_pool: Option<SqlitePool>,
        deadlines: SqliteWriteDeadlines,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(SqliteWriteState::default())),
            notify: Arc::new(Notify::new()),
            write_pool,
            deadlines,
            retry: SqliteWriteRetryConfig {
                max_attempts: 4,
                base_delay: Duration::from_millis(25),
                max_delay: Duration::from_millis(100),
            },
            slow_threshold_ms: observability::logging_thresholds().sqlite_write_slow_ms,
        }
    }

    pub(crate) fn write_pool_or<'a>(&'a self, fallback: &'a SqlitePool) -> &'a SqlitePool {
        self.write_pool.as_ref().unwrap_or(fallback)
    }

    fn deadline_for(&self, priority: SqliteWritePriority) -> Duration {
        match priority {
            SqliteWritePriority::Foreground => self.deadlines.foreground,
            SqliteWritePriority::Background => self.deadlines.background,
            SqliteWritePriority::BestEffort => self.deadlines.best_effort,
        }
    }

    pub fn runtime_status(&self) -> SqliteWriteRuntimeStatus {
        self.state
            .lock()
            .map(|state| SqliteWriteRuntimeStatus {
                active: state.active,
                waiting_foreground: state.waiting_foreground,
            })
            .unwrap_or_default()
    }

    pub async fn write<T, Fut, Op>(&self, lane: &'static str, operation: Op) -> Result<T>
    where
        Op: FnMut(usize) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        self.write_with_priority(lane, SqliteWritePriority::Background, operation)
            .await
    }

    pub async fn write_foreground<T, Fut, Op>(&self, lane: &'static str, operation: Op) -> Result<T>
    where
        Op: FnMut(usize) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        self.write_with_priority(lane, SqliteWritePriority::Foreground, operation)
            .await
    }

    pub async fn write_with_priority<T, Fut, Op>(
        &self,
        lane: &'static str,
        priority: SqliteWritePriority,
        mut operation: Op,
    ) -> Result<T>
    where
        Op: FnMut(usize) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let deadline = self.deadline_for(priority);
        let deadline_at = Instant::now() + deadline;
        let mut attempt = 1usize;
        loop {
            let permit = self
                .acquire_until(lane, priority, deadline_at, deadline)
                .await?;

            if Instant::now() >= deadline_at {
                drop(permit);
                return Err(self.deadline_error(lane, priority, "callback_start", deadline));
            }

            let op_started = Instant::now();
            let result = operation(attempt).await;
            let elapsed = op_started.elapsed();
            let writer_wait_ms = permit.writer_wait_ms();
            drop(permit);

            match result {
                Ok(value) => {
                    let elapsed_ms = elapsed.as_millis();
                    let completed_after_deadline = Instant::now() >= deadline_at;
                    let deadline_overrun_ms = Instant::now()
                        .saturating_duration_since(deadline_at)
                        .as_millis();
                    if completed_after_deadline || elapsed_ms >= self.slow_threshold_ms as u128 {
                        warn!(
                            event = "sqlite.write",
                            operation = lane,
                            priority = priority.as_str(),
                            elapsed_ms,
                            completed_after_deadline,
                            deadline_overrun_ms,
                            attempt,
                            writer_wait_ms,
                            pool_wait_ms = 0_u128,
                            begin_ms = 0_u128,
                            transaction_ms = elapsed_ms,
                            deadline_ms = deadline.as_millis(),
                            threshold_ms = self.slow_threshold_ms,
                            "sqlite write completed slowly or after its deadline"
                        );
                    } else {
                        debug!(
                            event = "sqlite.write",
                            operation = lane,
                            priority = priority.as_str(),
                            elapsed_ms,
                            attempt,
                            writer_wait_ms,
                            pool_wait_ms = 0_u128,
                            begin_ms = 0_u128,
                            transaction_ms = elapsed_ms,
                            deadline_ms = deadline.as_millis(),
                            "sqlite write completed"
                        );
                    }
                    return Ok(value);
                }
                Err(err)
                    if is_sqlite_busy_error(err.as_ref()) && attempt < self.retry.max_attempts =>
                {
                    let delay = self.retry_delay(attempt);
                    let remaining = deadline_at.saturating_duration_since(Instant::now());
                    let can_retry = remaining > delay;
                    let error_kind = if can_retry {
                        "sqlite_busy"
                    } else {
                        "write_deadline"
                    };
                    let message = if can_retry {
                        "sqlite write hit busy state; retrying"
                    } else {
                        "sqlite write deadline expired before the next retry"
                    };
                    warn!(
                        event = "sqlite.write",
                        operation = lane,
                        priority = priority.as_str(),
                        elapsed_ms = elapsed.as_millis(),
                        attempt,
                        writer_wait_ms,
                        pool_wait_ms = 0_u128,
                        begin_ms = 0_u128,
                        transaction_ms = elapsed.as_millis(),
                        deadline_ms = deadline.as_millis(),
                        retry_after_ms = delay.as_millis(),
                        error_kind,
                        error_chain = %observability::error_chain_summary(err.as_ref()),
                        "{message}"
                    );
                    if !can_retry {
                        return Err(self.deadline_error(lane, priority, "retry_backoff", deadline));
                    }
                    tokio::time::sleep_until(Instant::now() + delay).await;
                    attempt += 1;
                }
                Err(err) if is_sqlite_busy_error(err.as_ref()) && Instant::now() >= deadline_at => {
                    warn!(
                        event = "sqlite.write",
                        operation = lane,
                        priority = priority.as_str(),
                        elapsed_ms = elapsed.as_millis(),
                        attempt,
                        writer_wait_ms,
                        pool_wait_ms = 0_u128,
                        begin_ms = 0_u128,
                        transaction_ms = elapsed.as_millis(),
                        deadline_ms = deadline.as_millis(),
                        error_kind = "write_deadline",
                        error_chain = %observability::error_chain_summary(err.as_ref()),
                        "sqlite write returned busy after its retry deadline"
                    );
                    return Err(self.deadline_error(lane, priority, "retry_backoff", deadline));
                }
                Err(err) if is_sqlite_write_deadline_error(err.as_ref()) => {
                    warn!(
                        event = "sqlite.write",
                        operation = lane,
                        priority = priority.as_str(),
                        elapsed_ms = elapsed.as_millis(),
                        attempt,
                        writer_wait_ms,
                        pool_wait_ms = 0_u128,
                        begin_ms = 0_u128,
                        transaction_ms = elapsed.as_millis(),
                        deadline_ms = deadline.as_millis(),
                        error_kind = "write_deadline",
                        "sqlite write deadline exceeded"
                    );
                    return Err(err);
                }
                Err(err) => {
                    if is_sqlite_busy_error(err.as_ref()) {
                        warn!(
                            event = "sqlite.write",
                            operation = lane,
                            priority = priority.as_str(),
                            elapsed_ms = elapsed.as_millis(),
                            attempt,
                            writer_wait_ms,
                            pool_wait_ms = 0_u128,
                            begin_ms = 0_u128,
                            transaction_ms = elapsed.as_millis(),
                            deadline_ms = deadline.as_millis(),
                            error_kind = "sqlite_busy",
                            error_chain = %observability::error_chain_summary(err.as_ref()),
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
        let deadline = self.deadline_for(SqliteWritePriority::BestEffort);
        let deadline_at = Instant::now() + deadline;
        let permit = match self.try_acquire(lane, SqliteWritePriority::BestEffort) {
            Some(permit) => permit,
            None => {
                debug!(
                    event = "sqlite.write",
                    operation = lane,
                    priority = SqliteWritePriority::BestEffort.as_str(),
                    writer_wait_ms = 0_u128,
                    pool_wait_ms = 0_u128,
                    begin_ms = 0_u128,
                    transaction_ms = 0_u128,
                    deadline_ms = deadline.as_millis(),
                    downgrade_reason = "sqlite_writer_busy",
                    "sqlite writer permit unavailable; skipping best-effort write"
                );
                return Ok(None);
            }
        };

        if Instant::now() >= deadline_at {
            drop(permit);
            return Ok(None);
        }

        let op_started = Instant::now();
        let result = operation().await;
        let elapsed = op_started.elapsed();
        let writer_wait_ms = permit.writer_wait_ms();
        drop(permit);

        match result {
            Ok(value) => {
                let elapsed_ms = elapsed.as_millis();
                let completed_after_deadline = Instant::now() >= deadline_at;
                let deadline_overrun_ms = Instant::now()
                    .saturating_duration_since(deadline_at)
                    .as_millis();
                if completed_after_deadline || elapsed_ms >= self.slow_threshold_ms as u128 {
                    warn!(
                        event = "sqlite.write",
                        operation = lane,
                        priority = SqliteWritePriority::BestEffort.as_str(),
                        elapsed_ms,
                        completed_after_deadline,
                        deadline_overrun_ms,
                        writer_wait_ms,
                        pool_wait_ms = 0_u128,
                        begin_ms = 0_u128,
                        transaction_ms = elapsed_ms,
                        deadline_ms = deadline.as_millis(),
                        threshold_ms = self.slow_threshold_ms,
                        "sqlite best-effort write completed slowly or after its deadline"
                    );
                } else {
                    debug!(
                        event = "sqlite.write",
                        operation = lane,
                        priority = SqliteWritePriority::BestEffort.as_str(),
                        elapsed_ms,
                        writer_wait_ms,
                        pool_wait_ms = 0_u128,
                        begin_ms = 0_u128,
                        transaction_ms = elapsed_ms,
                        deadline_ms = deadline.as_millis(),
                        "sqlite best-effort write completed"
                    );
                }
                Ok(Some(value))
            }
            Err(err) if is_sqlite_busy_error(err.as_ref()) && Instant::now() >= deadline_at => {
                warn!(
                    event = "sqlite.write",
                    operation = lane,
                    priority = SqliteWritePriority::BestEffort.as_str(),
                    elapsed_ms = elapsed.as_millis(),
                    writer_wait_ms,
                    pool_wait_ms = 0_u128,
                    begin_ms = 0_u128,
                    transaction_ms = elapsed.as_millis(),
                    deadline_ms = deadline.as_millis(),
                    error_kind = "write_deadline",
                    error_chain = %observability::error_chain_summary(err.as_ref()),
                    downgrade_reason = "deadline_overrun",
                    "sqlite best-effort write returned busy after its deadline"
                );
                Ok(None)
            }
            Err(err) => {
                let error_kind = if is_sqlite_write_deadline_error(err.as_ref()) {
                    "write_deadline"
                } else if is_sqlite_busy_error(err.as_ref()) {
                    "sqlite_busy"
                } else {
                    "best_effort_error"
                };
                warn!(
                    event = "sqlite.write",
                    operation = lane,
                    priority = SqliteWritePriority::BestEffort.as_str(),
                    elapsed_ms = elapsed.as_millis(),
                    writer_wait_ms,
                    pool_wait_ms = 0_u128,
                    begin_ms = 0_u128,
                    transaction_ms = elapsed.as_millis(),
                    deadline_ms = deadline.as_millis(),
                    error_kind,
                    error_chain = %observability::error_chain_summary(err.as_ref()),
                    downgrade_reason = "best_effort_failure",
                    "sqlite best-effort write skipped"
                );
                if is_sqlite_write_deadline_error(err.as_ref()) {
                    Ok(None)
                } else {
                    Err(err)
                }
            }
        }
    }

    pub fn try_acquire_best_effort(&self, lane: &'static str) -> Option<SqliteWritePermit> {
        self.try_acquire(lane, SqliteWritePriority::BestEffort)
    }

    pub async fn acquire_with_priority(
        &self,
        lane: &'static str,
        priority: SqliteWritePriority,
    ) -> Result<SqliteWritePermit> {
        let deadline = self.deadline_for(priority);
        let deadline_at = Instant::now() + deadline;
        self.acquire_until(lane, priority, deadline_at, deadline)
            .await
    }

    async fn acquire_until(
        &self,
        lane: &'static str,
        priority: SqliteWritePriority,
        deadline_at: Instant,
        deadline: Duration,
    ) -> Result<SqliteWritePermit> {
        let started = Instant::now();
        match tokio::time::timeout_at(deadline_at, self.acquire_without_deadline(lane, priority))
            .await
        {
            Ok(result) => result,
            Err(_) => {
                let writer_wait_ms = started.elapsed().as_millis();
                warn!(
                    event = "sqlite.write",
                    operation = lane,
                    priority = priority.as_str(),
                    writer_wait_ms,
                    pool_wait_ms = 0_u128,
                    begin_ms = 0_u128,
                    transaction_ms = 0_u128,
                    deadline_ms = deadline.as_millis(),
                    error_kind = "writer_queue_timeout",
                    "sqlite writer permit wait exceeded deadline"
                );
                Err(self.deadline_error(lane, priority, "writer_queue", deadline))
            }
        }
    }

    async fn acquire_without_deadline(
        &self,
        lane: &'static str,
        priority: SqliteWritePriority,
    ) -> Result<SqliteWritePermit> {
        let wait_started = Instant::now();
        let mut foreground_waiter = (priority == SqliteWritePriority::Foreground)
            .then(|| ForegroundWaiter::new(self.state.clone(), self.notify.clone()));
        loop {
            let notified = {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("sqlite writer coordinator poisoned"))?;
                if !state.active
                    && (!priority.waits_for_foreground() || state.waiting_foreground == 0)
                {
                    if let Some(waiter) = foreground_waiter.as_mut() {
                        waiter.complete(&mut state);
                    }
                    state.active = true;
                    break;
                }
                if let Some(waiter) = foreground_waiter.as_mut() {
                    waiter.register(&mut state);
                }
                self.notify.notified()
            };
            notified.await;
        }
        let waited = wait_started.elapsed();
        debug!(
            sqlite_write_lane = lane,
            sqlite_write_priority = priority.as_str(),
            wait_ms = waited.as_millis(),
            "sqlite writer permit acquired"
        );
        Ok(SqliteWritePermit {
            lane,
            priority,
            waited,
            acquired_at: Instant::now(),
            coordinator: self.clone(),
        })
    }

    fn deadline_error(
        &self,
        lane: &'static str,
        priority: SqliteWritePriority,
        phase: &'static str,
        deadline: Duration,
    ) -> anyhow::Error {
        anyhow::Error::new(SqliteWriteDeadlineError::new(
            lane, priority, phase, deadline,
        ))
    }

    pub async fn begin_immediate<'a>(
        &self,
        pool: &'a SqlitePool,
        lane: &'static str,
    ) -> Result<(SqliteWritePermit, SqliteWriteTransaction<'a>)> {
        self.begin_immediate_with_priority(pool, lane, SqliteWritePriority::Background)
            .await
    }

    pub async fn begin_immediate_with_priority<'a>(
        &self,
        pool: &'a SqlitePool,
        lane: &'static str,
        priority: SqliteWritePriority,
    ) -> Result<(SqliteWritePermit, SqliteWriteTransaction<'a>)> {
        let deadline = self.deadline_for(priority);
        let deadline_at = Instant::now() + deadline;
        let write_pool = self.write_pool_or(pool);
        let mut attempt = 1usize;
        loop {
            let permit = self
                .acquire_until(lane, priority, deadline_at, deadline)
                .await?;
            let pool_wait_started = Instant::now();
            let connection = tokio::time::timeout_at(deadline_at, write_pool.acquire()).await;
            let pool_wait = pool_wait_started.elapsed();
            let writer_wait_ms = permit.writer_wait_ms();
            let connection = match connection {
                Ok(Ok(connection)) => connection,
                Ok(Err(error)) => {
                    drop(permit);
                    return Err(anyhow::Error::new(error)
                        .context(format!("acquire sqlite write connection ({lane})")));
                }
                Err(_) => {
                    warn!(
                        event = "sqlite.write",
                        operation = lane,
                        priority = priority.as_str(),
                        writer_wait_ms,
                        pool_wait_ms = pool_wait.as_millis(),
                        begin_ms = 0_u128,
                        transaction_ms = 0_u128,
                        deadline_ms = deadline.as_millis(),
                        error_kind = "write_pool_timeout",
                        "sqlite write pool acquisition exceeded deadline"
                    );
                    drop(permit);
                    return Err(self.deadline_error(lane, priority, "write_pool", deadline));
                }
            };
            let begin_started = Instant::now();
            let result = match tokio::time::timeout_at(
                deadline_at,
                Transaction::begin(connection, Some(Cow::Borrowed("BEGIN IMMEDIATE"))),
            )
            .await
            {
                Ok(Ok(tx)) => Ok(tx),
                Ok(Err(error)) => Err(anyhow::Error::new(error))
                    .with_context(|| format!("begin sqlite write tx ({lane})")),
                Err(_) => Err(self.deadline_error(lane, priority, "begin_immediate", deadline)),
            };
            let mut begin_elapsed = begin_started.elapsed();

            match result {
                Ok(mut tx) => {
                    let deadline_std = deadline_at.into_std();
                    let progress_handler_active = Arc::new(AtomicBool::new(true));
                    let handler_active = Arc::clone(&progress_handler_active);
                    let setup_handler = async {
                        let mut handle = tx.lock_handle().await?;
                        handle.set_progress_handler(1000, move || {
                            !handler_active.load(Ordering::Relaxed)
                                || std::time::Instant::now() < deadline_std
                        });
                        Ok::<_, sqlx::Error>(())
                    };
                    match tokio::time::timeout_at(deadline_at, setup_handler).await {
                        Ok(Ok(handle)) => handle,
                        Ok(Err(error)) => {
                            let begin_elapsed = begin_started.elapsed();
                            progress_handler_active.store(false, Ordering::Relaxed);
                            cleanup_sqlite_write_transaction(tx, lane).await;
                            warn!(
                                event = "sqlite.write",
                                operation = lane,
                                priority = priority.as_str(),
                                writer_wait_ms = permit.writer_wait_ms(),
                                pool_wait_ms = pool_wait.as_millis(),
                                begin_ms = begin_elapsed.as_millis(),
                                transaction_ms = 0_u128,
                                deadline_ms = deadline.as_millis(),
                                error_kind = "writer_connection_error",
                                error = %error,
                                "sqlite writer connection setup failed"
                            );
                            drop(permit);
                            return Err(anyhow::Error::new(error)
                                .context("prepare sqlite write deadline handler"));
                        }
                        Err(_) => {
                            let begin_elapsed = begin_started.elapsed();
                            progress_handler_active.store(false, Ordering::Relaxed);
                            cleanup_sqlite_write_transaction(tx, lane).await;
                            warn!(
                                event = "sqlite.write",
                                operation = lane,
                                priority = priority.as_str(),
                                writer_wait_ms = permit.writer_wait_ms(),
                                pool_wait_ms = pool_wait.as_millis(),
                                begin_ms = begin_elapsed.as_millis(),
                                transaction_ms = 0_u128,
                                deadline_ms = deadline.as_millis(),
                                error_kind = "write_deadline",
                                "sqlite writer connection setup exceeded deadline"
                            );
                            drop(permit);
                            return Err(self.deadline_error(
                                lane,
                                priority,
                                "transaction_setup",
                                deadline,
                            ));
                        }
                    }
                    begin_elapsed = begin_started.elapsed();
                    debug!(
                        event = "sqlite.write",
                        operation = lane,
                        priority = priority.as_str(),
                        elapsed_ms = begin_elapsed.as_millis(),
                        attempt,
                        writer_wait_ms,
                        pool_wait_ms = pool_wait.as_millis(),
                        begin_ms = begin_elapsed.as_millis(),
                        transaction_ms = 0_u128,
                        deadline_ms = deadline.as_millis(),
                        "sqlite write transaction started"
                    );
                    let transaction = SqliteWriteTransaction {
                        inner: Some(tx),
                        lane,
                        priority,
                        writer_wait_ms,
                        pool_wait,
                        begin_elapsed,
                        transaction_started: Instant::now(),
                        deadline_at,
                        deadline,
                        progress_handler_active,
                        slow_threshold_ms: self.slow_threshold_ms,
                    };
                    return Ok((permit, transaction));
                }
                Err(err)
                    if is_sqlite_busy_error(err.as_ref()) && attempt < self.retry.max_attempts =>
                {
                    let delay = self.retry_delay(attempt);
                    let remaining = deadline_at.saturating_duration_since(Instant::now());
                    let can_retry = remaining > delay;
                    drop(permit);
                    let error_kind = if can_retry {
                        "sqlite_busy"
                    } else {
                        "write_deadline"
                    };
                    let message = if can_retry {
                        "sqlite write transaction hit busy state; retrying"
                    } else {
                        "sqlite write deadline expired before the next transaction retry"
                    };
                    warn!(
                        event = "sqlite.write",
                        operation = lane,
                        priority = priority.as_str(),
                        elapsed_ms = begin_elapsed.as_millis(),
                        error_kind,
                        sqlite_write_lane = lane,
                        sqlite_write_priority = priority.as_str(),
                        writer_wait_ms,
                        pool_wait_ms = pool_wait.as_millis(),
                        begin_ms = begin_elapsed.as_millis(),
                        transaction_ms = 0_u128,
                        deadline_ms = deadline.as_millis(),
                        attempt,
                        retry_after_ms = delay.as_millis(),
                        error_chain = %observability::error_chain_summary(err.as_ref()),
                        "{message}"
                    );
                    if !can_retry {
                        return Err(self.deadline_error(lane, priority, "retry_backoff", deadline));
                    }
                    tokio::time::sleep_until(Instant::now() + delay).await;
                    attempt += 1;
                }
                Err(err) => {
                    let writer_wait_ms = permit.writer_wait_ms();
                    drop(permit);
                    let deadline_error = is_sqlite_write_deadline_error(err.as_ref());
                    if is_sqlite_busy_error(err.as_ref()) || deadline_error {
                        warn!(
                            event = "sqlite.write",
                            operation = lane,
                            priority = priority.as_str(),
                            elapsed_ms = begin_elapsed.as_millis(),
                            sqlite_write_lane = lane,
                            sqlite_write_priority = priority.as_str(),
                            writer_wait_ms,
                            pool_wait_ms = pool_wait.as_millis(),
                            begin_ms = begin_elapsed.as_millis(),
                            transaction_ms = 0_u128,
                            deadline_ms = deadline.as_millis(),
                            attempt,
                            error_kind = if deadline_error {
                                "write_deadline"
                            } else {
                                "sqlite_busy"
                            },
                            error_chain = %observability::error_chain_summary(err.as_ref()),
                            "sqlite write transaction could not begin"
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

    fn try_acquire(
        &self,
        lane: &'static str,
        priority: SqliteWritePriority,
    ) -> Option<SqliteWritePermit> {
        let mut state = self.state.lock().ok()?;
        if state.active || (priority.waits_for_foreground() && state.waiting_foreground > 0) {
            return None;
        }
        state.active = true;
        debug!(
            sqlite_write_lane = lane,
            sqlite_write_priority = priority.as_str(),
            wait_ms = 0_u128,
            "sqlite writer permit acquired"
        );
        Some(SqliteWritePermit {
            lane,
            priority,
            waited: Duration::ZERO,
            acquired_at: Instant::now(),
            coordinator: self.clone(),
        })
    }

    fn release(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.active = false;
        }
        self.notify.notify_waiters();
    }
}

pub struct SqliteWriteTransaction<'a> {
    inner: Option<Transaction<'a, Sqlite>>,
    lane: &'static str,
    priority: SqliteWritePriority,
    writer_wait_ms: u128,
    pool_wait: Duration,
    begin_elapsed: Duration,
    transaction_started: Instant,
    deadline_at: Instant,
    deadline: Duration,
    progress_handler_active: Arc<AtomicBool>,
    slow_threshold_ms: usize,
}

impl<'a> SqliteWriteTransaction<'a> {
    pub fn as_transaction_mut(&mut self) -> &mut Transaction<'a, Sqlite> {
        self.inner
            .as_mut()
            .expect("sqlite write transaction already completed")
    }

    pub async fn commit(mut self) -> Result<()> {
        let mut tx = self
            .inner
            .take()
            .expect("sqlite write transaction already completed");
        if Instant::now() >= self.deadline_at {
            self.progress_handler_active.store(false, Ordering::Relaxed);
            cleanup_sqlite_write_transaction(tx, self.lane).await;
            self.log_transaction_end(
                "write_deadline",
                "sqlite write transaction rolled back at deadline",
            );
            return Err(anyhow::Error::new(SqliteWriteDeadlineError::new(
                self.lane,
                self.priority,
                "transaction",
                self.deadline,
            )));
        }

        let disable_progress_handler = async {
            let mut handle = tx.lock_handle().await?;
            handle.set_progress_handler(0, || true);
            Ok::<_, sqlx::Error>(())
        };
        match tokio::time::timeout_at(self.deadline_at, disable_progress_handler).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                self.progress_handler_active.store(false, Ordering::Relaxed);
                cleanup_sqlite_write_transaction(tx, self.lane).await;
                self.log_transaction_end(
                    "transaction_error",
                    "sqlite write progress handler could not be disabled before commit",
                );
                return Err(anyhow::Error::new(error)
                    .context("disable sqlite write progress handler before commit"));
            }
            Err(_) => {
                self.progress_handler_active.store(false, Ordering::Relaxed);
                cleanup_sqlite_write_transaction(tx, self.lane).await;
                self.log_transaction_end(
                    "write_deadline",
                    "sqlite write progress handler disable exceeded deadline",
                );
                return Err(anyhow::Error::new(SqliteWriteDeadlineError::new(
                    self.lane,
                    self.priority,
                    "commit_prepare",
                    self.deadline,
                )));
            }
        }
        self.progress_handler_active.store(false, Ordering::Relaxed);

        if Instant::now() >= self.deadline_at {
            cleanup_sqlite_write_transaction(tx, self.lane).await;
            self.log_transaction_end(
                "write_deadline",
                "sqlite write transaction rolled back before commit dispatch",
            );
            return Err(anyhow::Error::new(SqliteWriteDeadlineError::new(
                self.lane,
                self.priority,
                "commit_prepare",
                self.deadline,
            )));
        }

        // Once COMMIT is dispatched, wait for SQLite's actual outcome.
        match SqliteTransactionManager::commit(&mut *tx).await {
            Ok(()) => {
                drop(tx);
                self.log_transaction_end("ok", "sqlite write transaction committed");
                Ok(())
            }
            Err(error) => {
                let deadline_error = is_commit_deadline_error(&error, self.deadline_at);
                cleanup_sqlite_write_transaction(tx, self.lane).await;
                if deadline_error {
                    self.log_transaction_end(
                        "write_deadline",
                        "sqlite write commit returned busy after deadline and was rolled back",
                    );
                    Err(anyhow::Error::new(SqliteWriteDeadlineError::new(
                        self.lane,
                        self.priority,
                        "commit",
                        self.deadline,
                    )))
                } else {
                    let error_kind = if is_sqlite_busy_error(&error) {
                        "sqlite_busy"
                    } else {
                        "transaction_error"
                    };
                    self.log_transaction_end(
                        error_kind,
                        if error_kind == "sqlite_busy" {
                            "sqlite write commit hit busy state"
                        } else {
                            "sqlite write transaction commit failed"
                        },
                    );
                    Err(anyhow::Error::new(error).context("commit sqlite write transaction"))
                }
            }
        }
    }

    pub async fn rollback(mut self) -> Result<()> {
        self.progress_handler_active.store(false, Ordering::Relaxed);
        let tx = self
            .inner
            .take()
            .expect("sqlite write transaction already completed");
        let result = tokio::time::timeout(ROLLBACK_CLEANUP_TIMEOUT, tx.rollback()).await;
        match result {
            Ok(Ok(())) => {
                self.log_transaction_end("rollback", "sqlite write transaction rolled back");
                Ok(())
            }
            Ok(Err(error)) => {
                self.log_transaction_end(
                    "rollback_error",
                    "sqlite write transaction rollback failed",
                );
                Err(anyhow::Error::new(error).context("rollback sqlite write transaction"))
            }
            Err(error) => {
                self.log_transaction_end(
                    "rollback_timeout",
                    "sqlite write rollback exceeded cleanup timeout",
                );
                Err(anyhow::Error::new(error).context("rollback sqlite write transaction"))
            }
        }
    }

    fn log_transaction_end(&self, error_kind: &'static str, message: &'static str) {
        let transaction_ms = self.transaction_started.elapsed().as_millis();
        let completed_at = Instant::now();
        let completed_after_deadline = completed_at >= self.deadline_at;
        let deadline_overrun_ms = completed_at
            .saturating_duration_since(self.deadline_at)
            .as_millis();
        if completed_after_deadline
            || transaction_ms >= self.slow_threshold_ms as u128
            || error_kind != "ok"
        {
            warn!(
                event = "sqlite.write",
                operation = self.lane,
                priority = self.priority.as_str(),
                writer_wait_ms = self.writer_wait_ms,
                pool_wait_ms = self.pool_wait.as_millis(),
                begin_ms = self.begin_elapsed.as_millis(),
                transaction_ms,
                deadline_ms = self.deadline.as_millis(),
                completed_after_deadline,
                deadline_overrun_ms,
                error_kind,
                "{message}"
            );
        } else {
            debug!(
                event = "sqlite.write",
                operation = self.lane,
                priority = self.priority.as_str(),
                writer_wait_ms = self.writer_wait_ms,
                pool_wait_ms = self.pool_wait.as_millis(),
                begin_ms = self.begin_elapsed.as_millis(),
                transaction_ms,
                deadline_ms = self.deadline.as_millis(),
                completed_after_deadline,
                deadline_overrun_ms,
                "{message}"
            );
        }
    }
}

async fn cleanup_sqlite_write_transaction(tx: Transaction<'_, Sqlite>, lane: &'static str) {
    match tokio::time::timeout(ROLLBACK_CLEANUP_TIMEOUT, tx.rollback()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => warn!(
            event = "sqlite.write",
            operation = lane,
            error = %error,
            "sqlite write rollback cleanup failed"
        ),
        Err(error) => warn!(
            event = "sqlite.write",
            operation = lane,
            error = %error,
            "sqlite write rollback cleanup timed out"
        ),
    }
}

impl<'a> Deref for SqliteWriteTransaction<'a> {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.inner
            .as_deref()
            .expect("sqlite write transaction already completed")
    }
}

impl DerefMut for SqliteWriteTransaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
            .as_deref_mut()
            .expect("sqlite write transaction already completed")
    }
}

impl Drop for SqliteWriteTransaction<'_> {
    fn drop(&mut self) {
        self.progress_handler_active.store(false, Ordering::Relaxed);
        if self.inner.is_some() && Instant::now() >= self.deadline_at {
            self.log_transaction_end(
                "write_deadline",
                "sqlite write transaction was interrupted at deadline",
            );
        }
    }
}

pub fn is_sqlite_write_deadline_error(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(err) = current {
        if err.downcast_ref::<SqliteWriteDeadlineError>().is_some() {
            return true;
        }
        if let Some(sqlx::Error::Database(database_error)) = err.downcast_ref::<sqlx::Error>()
            && database_error.code().as_deref() == Some("9")
            && database_error.message().eq_ignore_ascii_case("interrupted")
        {
            return true;
        }
        current = err.source();
    }
    false
}

pub struct SqliteWritePermit {
    lane: &'static str,
    priority: SqliteWritePriority,
    waited: Duration,
    acquired_at: Instant,
    coordinator: SqliteWriteCoordinator,
}

impl SqliteWritePermit {
    pub(crate) fn writer_wait_ms(&self) -> u128 {
        self.waited.as_millis()
    }
}

impl Drop for SqliteWritePermit {
    fn drop(&mut self) {
        self.coordinator.release();
        debug!(
            sqlite_write_lane = self.lane,
            sqlite_write_priority = self.priority.as_str(),
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

fn is_commit_deadline_error(error: &sqlx::Error, deadline_at: Instant) -> bool {
    is_sqlite_busy_error(error) && Instant::now() >= deadline_at
}

fn sqlx_error_is_busy(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db_err) => {
            let code = db_err.code().map(|code| code.into_owned());
            code.as_deref().is_some_and(sqlite_code_is_busy_or_locked)
                || db_err
                    .message()
                    .to_ascii_lowercase()
                    .contains("database is locked")
        }
        _ => false,
    }
}

fn sqlite_code_is_busy_or_locked(code: &str) -> bool {
    code.parse::<u32>()
        .is_ok_and(|code| matches!(code & 0xff, 5 | 6))
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::PathBuf,
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::*;
    use serde_json::Value;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use tokio::sync::oneshot;
    use tracing_subscriber::fmt::MakeWriter;

    fn test_deadlines(
        foreground: Duration,
        background: Duration,
        best_effort: Duration,
    ) -> SqliteWriteDeadlines {
        SqliteWriteDeadlines {
            foreground,
            background,
            best_effort,
        }
    }

    async fn open_test_pool(
        database_path: &PathBuf,
        max_connections: u32,
        busy_timeout: Duration,
    ) -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(database_path)
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .busy_timeout(busy_timeout),
            )
            .await
            .expect("create sqlite test pool")
    }

    async fn remove_test_database(pool: SqlitePool, database_path: &PathBuf) {
        pool.close().await;
        let _ = std::fs::remove_file(database_path);
        let _ = std::fs::remove_file(database_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(database_path.with_extension("db-shm"));
    }

    #[derive(Clone, Default)]
    struct SharedLogBuffer {
        inner: Arc<StdMutex<Vec<u8>>>,
    }

    struct SharedLogWriter {
        inner: Arc<StdMutex<Vec<u8>>>,
    }

    impl io::Write for SharedLogWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.inner
                .lock()
                .expect("log buffer lock poisoned")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for SharedLogBuffer {
        type Writer = SharedLogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedLogWriter {
                inner: Arc::clone(&self.inner),
            }
        }
    }

    impl SharedLogBuffer {
        fn json_events(&self) -> Vec<Value> {
            let bytes = self.inner.lock().expect("log buffer lock poisoned").clone();
            String::from_utf8(bytes)
                .expect("captured logs should be utf-8")
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str(line).expect("captured line should be valid json"))
                .collect()
        }
    }

    #[derive(Debug)]
    struct TestDatabaseError {
        code: &'static str,
        message: &'static str,
    }

    impl std::fmt::Display for TestDatabaseError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "{}", self.message)
        }
    }

    impl std::error::Error for TestDatabaseError {}

    impl sqlx::error::DatabaseError for TestDatabaseError {
        fn message(&self) -> &str {
            self.message
        }

        fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
            Some(self.code.into())
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
    }

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
            .acquire_with_priority("held", SqliteWritePriority::Background)
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

    #[tokio::test(flavor = "current_thread")]
    async fn foreground_deadline_bounds_wait_for_writer_permit() {
        let deadline = Duration::from_millis(60);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            None,
            test_deadlines(deadline, deadline, Duration::from_millis(20)),
        );
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);
        let _held = coordinator
            .acquire_with_priority("held", SqliteWritePriority::Background)
            .await
            .expect("acquire held permit");
        let started = Instant::now();
        let callback_started = Arc::new(AtomicUsize::new(0));
        let callback_started_for_write = callback_started.clone();

        let error = coordinator
            .write_foreground("queue_timeout", move |_| {
                callback_started_for_write.fetch_add(1, Ordering::SeqCst);
                async { Ok::<_, anyhow::Error>(()) }
            })
            .await
            .expect_err("foreground write should hit its queue deadline");

        assert!(started.elapsed() < Duration::from_millis(250));
        let deadline_error = error
            .downcast_ref::<SqliteWriteDeadlineError>()
            .expect("typed deadline error");
        assert_eq!(deadline_error.phase, "writer_queue");
        assert_eq!(callback_started.load(Ordering::SeqCst), 0);
        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation") == Some(&Value::String("queue_timeout".to_owned()))
                && event.get("error_kind")
                    == Some(&Value::String("writer_queue_timeout".to_owned()))
                && event.get("writer_wait_ms").is_some()
                && event.get("pool_wait_ms").is_some()
                && event.get("begin_ms").is_some()
                && event.get("transaction_ms").is_some()
                && event.get("deadline_ms").is_some()
        }));
    }

    #[tokio::test]
    async fn write_callback_finishes_after_deadline_and_keeps_writer_permit() {
        let deadline = Duration::from_millis(60);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            None,
            test_deadlines(deadline, deadline, deadline),
        );
        let (started_tx, started_rx) = oneshot::channel();
        let first = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                let mut started_tx = Some(started_tx);
                coordinator
                    .write_foreground("callback_deadline_overrun", move |_| {
                        let started_tx = started_tx.take();
                        async move {
                            if let Some(started_tx) = started_tx {
                                let _ = started_tx.send(());
                            }
                            tokio::time::sleep(Duration::from_millis(120)).await;
                            Ok::<_, anyhow::Error>(7)
                        }
                    })
                    .await
            })
        };
        started_rx.await.expect("first callback started");

        let second_callback_started = Arc::new(AtomicUsize::new(0));
        let second_callback_started_for_write = second_callback_started.clone();
        let second_error = coordinator
            .write_foreground("callback_waits_for_first", move |_| {
                second_callback_started_for_write.fetch_add(1, Ordering::SeqCst);
                async { Ok::<_, anyhow::Error>(()) }
            })
            .await
            .expect_err("second write should expire while waiting for the first callback");
        assert_eq!(
            second_error
                .downcast_ref::<SqliteWriteDeadlineError>()
                .expect("typed queue deadline")
                .phase,
            "writer_queue"
        );
        assert_eq!(second_callback_started.load(Ordering::SeqCst), 0);

        assert_eq!(
            first
                .await
                .expect("join first write")
                .expect("late success"),
            7
        );
    }

    #[tokio::test]
    async fn best_effort_callback_started_before_deadline_returns_actual_result() {
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            None,
            test_deadlines(
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(20),
            ),
        );
        let started = Instant::now();

        let result = coordinator
            .try_write("best_effort_timeout", || async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok::<_, anyhow::Error>(7)
            })
            .await
            .expect("best-effort callback result");

        assert_eq!(result, Some(7));
        assert!(started.elapsed() >= Duration::from_millis(200));
    }

    #[tokio::test]
    async fn foreground_write_runs_before_queued_background_write() {
        let coordinator = SqliteWriteCoordinator::new();
        let held = coordinator
            .acquire_with_priority("held", SqliteWritePriority::Background)
            .await
            .expect("acquire held writer");
        let order = Arc::new(StdMutex::new(Vec::new()));

        let background = {
            let coordinator = coordinator.clone();
            let order = order.clone();
            tokio::spawn(async move {
                coordinator
                    .write("background", |_| {
                        let order = order.clone();
                        async move {
                            order.lock().expect("order lock").push("background");
                            Ok::<_, anyhow::Error>(())
                        }
                    })
                    .await
                    .expect("background write");
            })
        };
        tokio::task::yield_now().await;

        let foreground = {
            let coordinator = coordinator.clone();
            let order = order.clone();
            tokio::spawn(async move {
                coordinator
                    .write_foreground("foreground", |_| {
                        let order = order.clone();
                        async move {
                            order.lock().expect("order lock").push("foreground");
                            Ok::<_, anyhow::Error>(())
                        }
                    })
                    .await
                    .expect("foreground write");
            })
        };
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        drop(held);

        foreground.await.expect("join foreground");
        background.await.expect("join background");

        assert_eq!(
            order.lock().expect("order lock").as_slice(),
            ["foreground", "background"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn coordinator_telemetry_includes_lane_priority_wait_attempt_and_elapsed() {
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);

        SqliteWriteCoordinator::new()
            .write_foreground("telemetry", |_| async { Ok::<_, anyhow::Error>(()) })
            .await
            .expect("coordinated telemetry write");

        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("sqlite_write_lane") == Some(&Value::String("telemetry".to_owned()))
                && event.get("sqlite_write_priority")
                    == Some(&Value::String("foreground".to_owned()))
                && event.get("wait_ms").is_some()
        }));
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation") == Some(&Value::String("telemetry".to_owned()))
                && event.get("priority") == Some(&Value::String("foreground".to_owned()))
                && event.get("attempt").is_some()
                && event.get("writer_wait_ms").is_some()
                && event.get("elapsed_ms").is_some()
        }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn transaction_telemetry_includes_all_deadline_phases() {
        let database_path = test_database_path();
        let pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        sqlx::query("CREATE TABLE telemetry_probe (value INTEGER NOT NULL)")
            .execute(&pool)
            .await
            .expect("create telemetry probe");
        let writer_pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);

        let coordinator = SqliteWriteCoordinator::with_write_pool(writer_pool.clone());
        let (_permit, mut tx) = coordinator
            .begin_immediate(&pool, "transaction_telemetry")
            .await
            .expect("begin telemetry transaction");
        sqlx::query("INSERT INTO telemetry_probe (value) VALUES (1)")
            .execute(&mut *tx)
            .await
            .expect("insert telemetry probe");
        tx.commit().await.expect("commit telemetry transaction");

        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation")
                    == Some(&Value::String("transaction_telemetry".to_owned()))
                && event.get("writer_wait_ms").is_some()
                && event.get("pool_wait_ms").is_some()
                && event.get("begin_ms").is_some()
                && event.get("transaction_ms").is_some()
                && event.get("deadline_ms").is_some()
        }));

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(pool, &database_path).await;
    }

    #[test]
    fn busy_detection_matches_sqlite_locked_messages() {
        let err = anyhow::anyhow!("error returned from database: (code: 5) database is locked");
        assert!(is_sqlite_busy_error(err.as_ref()));
    }

    #[test]
    fn progress_handler_interruption_is_a_retryable_write_deadline() {
        let error = sqlx::Error::Database(Box::new(TestDatabaseError {
            code: "9",
            message: "interrupted",
        }));

        assert!(is_sqlite_write_deadline_error(&error));
    }

    #[test]
    fn sqlite_busy_detection_matches_primary_and_extended_codes() {
        assert!(sqlite_code_is_busy_or_locked("5"));
        assert!(sqlite_code_is_busy_or_locked("6"));
        assert!(sqlite_code_is_busy_or_locked("261"));
        assert!(sqlite_code_is_busy_or_locked("517"));
        assert!(!sqlite_code_is_busy_or_locked("19"));
    }

    #[test]
    fn non_busy_commit_errors_are_not_reclassified_after_deadline() {
        let deadline_at = Instant::now() - Duration::from_millis(1);
        let constraint_error = sqlx::Error::Database(Box::new(TestDatabaseError {
            code: "19",
            message: "constraint failed",
        }));
        let busy_error = sqlx::Error::Database(Box::new(TestDatabaseError {
            code: "5",
            message: "database is locked",
        }));

        assert!(!is_commit_deadline_error(&constraint_error, deadline_at));
        assert!(is_commit_deadline_error(&busy_error, deadline_at));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn busy_write_logs_deadline_when_retry_delay_will_not_fit() {
        let deadline = Duration::from_millis(250);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            None,
            test_deadlines(deadline, deadline, deadline),
        );
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);

        let (started_tx, started_rx) = oneshot::channel();
        let write = tokio::spawn(async move {
            let mut started_tx = Some(started_tx);
            coordinator
                .write_foreground("busy_deadline", move |_| {
                    if let Some(started_tx) = started_tx.take() {
                        let _ = started_tx.send(());
                    }
                    async {
                        tokio::time::sleep(Duration::from_millis(230)).await;
                        Err::<(), _>(anyhow::Error::new(sqlx::Error::Database(Box::new(
                            TestDatabaseError {
                                code: "5",
                                message: "database is locked",
                            },
                        ))))
                    }
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("busy callback should start before its deadline")
            .expect("receive busy callback start signal");
        let error = write
            .await
            .expect("join busy deadline write")
            .expect_err("deadline should prevent an undersized busy retry");

        assert!(is_sqlite_write_deadline_error(error.as_ref()));
        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation") == Some(&Value::String("busy_deadline".to_owned()))
                && event.get("error_kind") == Some(&Value::String("write_deadline".to_owned()))
                && event.get("attempt").and_then(Value::as_u64) == Some(1)
        }));
        assert!(!events.iter().any(|event| {
            event.get("operation") == Some(&Value::String("busy_deadline".to_owned()))
                && event.get("error_kind") == Some(&Value::String("sqlite_busy".to_owned()))
                && event.get("attempt").and_then(Value::as_u64) == Some(1)
        }));
    }

    #[test]
    fn busy_detection_matches_sqlx_primary_and_extended_codes() {
        for code in ["5", "6", "261", "262", "517", "19"] {
            let error = sqlx::Error::Database(Box::new(TestDatabaseError {
                code,
                message: "generic database failure",
            }));

            assert_eq!(is_sqlite_busy_error(&error), code != "19");
        }
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

    #[tokio::test]
    async fn dedicated_writer_pool_is_independent_of_reader_pool_capacity() {
        let database_path = test_database_path();
        let read_pool = open_test_pool(&database_path, 1, Duration::from_millis(50)).await;
        sqlx::query("CREATE TABLE isolated_writer_probe (value INTEGER NOT NULL)")
            .execute(&read_pool)
            .await
            .expect("create isolated writer probe");
        let writer_pool = open_test_pool(&database_path, 1, Duration::from_millis(50)).await;
        let held_reader = read_pool
            .acquire()
            .await
            .expect("hold reader pool connection");
        let coordinator = SqliteWriteCoordinator::with_write_pool(writer_pool.clone());

        let (_permit, mut tx) = coordinator
            .begin_immediate(&read_pool, "isolated_writer")
            .await
            .expect("writer pool should be available while reader pool is exhausted");
        sqlx::query("INSERT INTO isolated_writer_probe (value) VALUES (7)")
            .execute(&mut *tx)
            .await
            .expect("write using the dedicated pool");
        tx.commit()
            .await
            .expect("commit isolated writer transaction");
        drop(held_reader);

        let value: i64 = sqlx::query_scalar("SELECT value FROM isolated_writer_probe")
            .fetch_one(&read_pool)
            .await
            .expect("read committed writer value");
        assert_eq!(value, 7);

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test]
    async fn coordinated_callback_uses_writer_pool_when_reader_pool_is_exhausted() {
        let database_path = test_database_path();
        let read_pool = open_test_pool(&database_path, 1, Duration::from_millis(50)).await;
        sqlx::query("CREATE TABLE isolated_callback_probe (value INTEGER NOT NULL)")
            .execute(&read_pool)
            .await
            .expect("create isolated callback probe");
        let writer_pool = open_test_pool(&database_path, 1, Duration::from_millis(50)).await;
        let held_reader = read_pool
            .acquire()
            .await
            .expect("hold reader pool connection");
        let coordinator = SqliteWriteCoordinator::with_write_pool(writer_pool.clone());
        let callback_pool = coordinator.write_pool_or(&read_pool).clone();

        coordinator
            .write("isolated_callback", |_| {
                let callback_pool = callback_pool.clone();
                async move {
                    sqlx::query("INSERT INTO isolated_callback_probe (value) VALUES (11)")
                        .execute(&callback_pool)
                        .await
                        .map(|_| ())
                        .map_err(anyhow::Error::from)
                }
            })
            .await
            .expect("coordinated callback should use the independent writer pool");
        drop(held_reader);

        let value: i64 = sqlx::query_scalar("SELECT value FROM isolated_callback_probe")
            .fetch_one(&read_pool)
            .await
            .expect("read committed callback value");
        assert_eq!(value, 11);

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn writer_pool_acquisition_has_its_own_deadline_phase() {
        let database_path = test_database_path();
        let read_pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        let writer_pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        let held_writer = writer_pool
            .acquire()
            .await
            .expect("hold dedicated writer connection");
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            Some(writer_pool.clone()),
            test_deadlines(
                Duration::from_millis(50),
                Duration::from_millis(50),
                Duration::from_millis(20),
            ),
        );
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);
        let started = Instant::now();

        let error = match coordinator
            .begin_immediate(&read_pool, "pool_timeout")
            .await
        {
            Ok(_) => panic!("writer pool acquisition should time out"),
            Err(error) => error,
        };

        assert!(started.elapsed() < Duration::from_millis(250));
        assert_eq!(
            error
                .downcast_ref::<SqliteWriteDeadlineError>()
                .expect("typed deadline error")
                .phase,
            "write_pool"
        );
        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation") == Some(&Value::String("pool_timeout".to_owned()))
                && event.get("error_kind") == Some(&Value::String("write_pool_timeout".to_owned()))
                && event.get("writer_wait_ms").is_some()
                && event.get("pool_wait_ms").is_some()
                && event.get("begin_ms").is_some()
                && event.get("transaction_ms").is_some()
                && event.get("deadline_ms").is_some()
        }));
        drop(held_writer);
        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test]
    async fn expired_transaction_rolls_back_and_writer_connection_is_reusable() {
        let database_path = test_database_path();
        let read_pool = open_test_pool(&database_path, 2, Duration::from_millis(20)).await;
        sqlx::query("CREATE TABLE deadline_probe (value INTEGER NOT NULL)")
            .execute(&read_pool)
            .await
            .expect("create deadline probe");
        let writer_pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        let deadline = Duration::from_millis(80);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            Some(writer_pool.clone()),
            test_deadlines(deadline, deadline, Duration::from_millis(20)),
        );

        let (expired_permit, mut expired_tx) = coordinator
            .begin_immediate(&read_pool, "expired_transaction")
            .await
            .expect("begin transaction that expires");
        sqlx::query("INSERT INTO deadline_probe (value) VALUES (1)")
            .execute(&mut *expired_tx)
            .await
            .expect("insert into transaction that expires");
        tokio::time::sleep(deadline + Duration::from_millis(10)).await;
        let error = expired_tx
            .commit()
            .await
            .expect_err("expired transaction must not commit");
        assert!(is_sqlite_write_deadline_error(error.as_ref()));
        drop(expired_permit);

        let (_permit, mut next_tx) = coordinator
            .begin_immediate(&read_pool, "reusable_writer")
            .await
            .expect("writer connection should be reusable after rollback");
        sqlx::query("INSERT INTO deadline_probe (value) VALUES (2)")
            .execute(&mut *next_tx)
            .await
            .expect("insert on reusable writer connection");
        next_tx.commit().await.expect("commit after rollback");

        let values: Vec<i64> =
            sqlx::query_scalar("SELECT value FROM deadline_probe ORDER BY value")
                .fetch_all(&read_pool)
                .await
                .expect("read deadline probe rows");
        assert_eq!(values, [2]);

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn commit_dispatched_before_deadline_can_succeed_after_reader_releases() {
        let database_path = test_database_path();
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Delete)
            .busy_timeout(Duration::from_millis(250));
        let read_pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options.clone())
            .await
            .expect("create rollback-journal reader pool");
        let writer_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create rollback-journal writer pool");
        sqlx::query("CREATE TABLE commit_deadline_probe (value INTEGER NOT NULL)")
            .execute(&read_pool)
            .await
            .expect("create commit deadline probe");

        let mut reader = read_pool.begin().await.expect("begin reader transaction");
        sqlx::query("SELECT value FROM commit_deadline_probe")
            .fetch_all(&mut *reader)
            .await
            .expect("hold a shared read lock");

        let deadline = Duration::from_millis(40);
        let coordinator_deadline = Duration::from_secs(5);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            Some(writer_pool.clone()),
            test_deadlines(
                coordinator_deadline,
                coordinator_deadline,
                Duration::from_millis(20),
            ),
        );
        let (permit, mut tx) = coordinator
            .begin_immediate(&read_pool, "commit_reader_contention")
            .await
            .expect("begin write transaction while reader is active");
        sqlx::query("INSERT INTO commit_deadline_probe (value) VALUES (1)")
            .execute(&mut *tx)
            .await
            .expect("write before commit deadline");
        tx.deadline_at = Instant::now() + deadline;
        tx.deadline = deadline;

        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);
        let release_reader = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(90)).await;
            reader.rollback().await.expect("release reader transaction");
        });
        let started = Instant::now();
        tx.commit()
            .await
            .expect("commit should report SQLite success after the reader releases");
        let commit_elapsed = started.elapsed();
        drop(permit);
        release_reader.await.expect("join reader release");
        assert!(commit_elapsed >= deadline);
        let events = buffer.json_events();
        assert!(
            events.iter().any(|event| {
                event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                    && event.get("operation")
                        == Some(&Value::String("commit_reader_contention".to_owned()))
                    && event.get("error_kind") == Some(&Value::String("ok".to_owned()))
                    && event.get("completed_after_deadline") == Some(&Value::Bool(true))
                    && event
                        .get("deadline_overrun_ms")
                        .and_then(Value::as_str)
                        .and_then(|overrun| overrun.parse::<u64>().ok())
                        .is_some_and(|overrun| overrun > 0)
            }),
            "late commit telemetry was not captured: {events:?}"
        );

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM commit_deadline_probe")
            .fetch_one(&read_pool)
            .await
            .expect("count after late successful commit");
        assert_eq!(count, 1);

        let (_permit, mut next_tx) = coordinator
            .begin_immediate(&read_pool, "commit_after_late_success")
            .await
            .expect("writer connection is reusable after late successful commit");
        sqlx::query("INSERT INTO commit_deadline_probe (value) VALUES (2)")
            .execute(&mut *next_tx)
            .await
            .expect("write after late successful commit");
        next_tx.commit().await.expect("commit after late success");

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test]
    async fn non_busy_commit_error_preserves_sqlite_error() {
        let database_path = test_database_path();
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Delete)
            .busy_timeout(Duration::from_millis(250));
        let read_pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options.clone())
            .await
            .expect("create rollback-journal reader pool");
        let writer_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create rollback-journal writer pool");
        sqlx::query("CREATE TABLE commit_error_parent (id INTEGER PRIMARY KEY)")
            .execute(&read_pool)
            .await
            .expect("create deferred foreign-key parent");
        sqlx::query(
            "CREATE TABLE commit_error_child (parent_id INTEGER NOT NULL REFERENCES commit_error_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        )
        .execute(&read_pool)
        .await
        .expect("create deferred foreign-key child");

        let mut reader = read_pool.begin().await.expect("begin reader transaction");
        sqlx::query("SELECT COUNT(*) FROM commit_error_child")
            .fetch_one(&mut *reader)
            .await
            .expect("hold a shared read lock");

        let coordinator_deadline = Duration::from_secs(5);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            Some(writer_pool.clone()),
            test_deadlines(
                coordinator_deadline,
                coordinator_deadline,
                Duration::from_millis(20),
            ),
        );
        let (permit, mut tx) = coordinator
            .begin_immediate(&read_pool, "commit_deferred_constraint")
            .await
            .expect("begin write transaction while reader is active");
        sqlx::query("INSERT INTO commit_error_child (parent_id) VALUES (99)")
            .execute(&mut *tx)
            .await
            .expect("deferred foreign-key constraint allows the insert");

        reader.rollback().await.expect("release reader transaction");
        let error = tx
            .commit()
            .await
            .expect_err("deferred foreign-key violation should fail COMMIT");
        drop(permit);

        assert!(!is_sqlite_busy_error(error.as_ref()));
        assert!(!is_sqlite_write_deadline_error(error.as_ref()));
        let child_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM commit_error_child")
            .fetch_one(&read_pool)
            .await
            .expect("count after failed deferred constraint commit");
        assert_eq!(child_count, 0);

        let (_permit, mut next_tx) = coordinator
            .begin_immediate(&read_pool, "commit_after_constraint_error")
            .await
            .expect("writer connection is reusable after failed commit");
        sqlx::query("INSERT INTO commit_error_parent (id) VALUES (2)")
            .execute(&mut *next_tx)
            .await
            .expect("insert parent on reusable writer connection");
        sqlx::query("INSERT INTO commit_error_child (parent_id) VALUES (2)")
            .execute(&mut *next_tx)
            .await
            .expect("insert valid child on reusable writer connection");
        next_tx
            .commit()
            .await
            .expect("commit after constraint error");

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn commit_busy_telemetry_keeps_sqlite_busy_cause() {
        let database_path = test_database_path();
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Delete)
            .busy_timeout(Duration::from_millis(100));
        let read_pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options.clone())
            .await
            .expect("create rollback-journal reader pool");
        let writer_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create rollback-journal writer pool");
        sqlx::query("CREATE TABLE commit_busy_probe (value INTEGER NOT NULL)")
            .execute(&read_pool)
            .await
            .expect("create commit busy probe");

        let mut reader = read_pool.begin().await.expect("begin reader transaction");
        sqlx::query("SELECT value FROM commit_busy_probe")
            .fetch_all(&mut *reader)
            .await
            .expect("hold a shared read lock");
        let deadline = Duration::from_millis(500);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            Some(writer_pool.clone()),
            test_deadlines(deadline, deadline, Duration::from_millis(20)),
        );
        let (permit, mut tx) = coordinator
            .begin_immediate(&read_pool, "commit_busy_telemetry")
            .await
            .expect("begin write transaction while reader is active");
        sqlx::query("INSERT INTO commit_busy_probe (value) VALUES (1)")
            .execute(&mut *tx)
            .await
            .expect("write before busy commit");

        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);
        let error = tx
            .commit()
            .await
            .expect_err("reader lock should keep commit busy before deadline");
        drop(permit);

        assert!(is_sqlite_busy_error(error.as_ref()));
        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation")
                    == Some(&Value::String("commit_busy_telemetry".to_owned()))
                && event.get("error_kind") == Some(&Value::String("sqlite_busy".to_owned()))
        }));
        reader.rollback().await.expect("release reader transaction");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM commit_busy_probe")
            .fetch_one(&read_pool)
            .await
            .expect("count after busy commit");
        assert_eq!(count, 0);

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test]
    async fn begin_retry_logs_deadline_when_retry_delay_will_not_fit() {
        let database_path = test_database_path();
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_millis(1));
        let read_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .expect("create busy deadline reader pool");
        let writer_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create busy deadline writer pool");
        sqlx::query("CREATE TABLE busy_deadline_probe (value INTEGER NOT NULL)")
            .execute(&read_pool)
            .await
            .expect("create busy deadline probe");
        let holder = read_pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .expect("hold external writer lock");
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);
        let deadline = Duration::from_millis(170);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            Some(writer_pool.clone()),
            test_deadlines(deadline, deadline, Duration::from_millis(20)),
        );

        let error = match coordinator
            .begin_immediate(&read_pool, "begin_busy_deadline")
            .await
        {
            Ok(_) => panic!("deadline should prevent an undersized begin retry"),
            Err(error) => error,
        };

        assert!(is_sqlite_write_deadline_error(error.as_ref()));
        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation") == Some(&Value::String("begin_busy_deadline".to_owned()))
                && event.get("error_kind") == Some(&Value::String("write_deadline".to_owned()))
                && event
                    .get("attempt")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                    >= 2
        }));
        holder
            .rollback()
            .await
            .expect("release external writer lock");

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test]
    async fn coordinator_reports_bounded_busy_retry_exhaustion() {
        let database_path = test_database_path();
        let read_pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        let writer_pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        sqlx::query("CREATE TABLE busy_exhaustion_probe (value INTEGER NOT NULL)")
            .execute(&read_pool)
            .await
            .expect("create busy exhaustion probe");
        let holder = read_pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .expect("hold external writer lock");

        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);

        let deadline = Duration::from_millis(600);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            Some(writer_pool.clone()),
            test_deadlines(deadline, deadline, Duration::from_millis(20)),
        );
        let started = Instant::now();
        let error = match coordinator
            .begin_immediate(&read_pool, "busy_retry_exhaustion")
            .await
        {
            Ok(_) => panic!("persistent external lock must exhaust bounded retries"),
            Err(error) => error,
        };
        let elapsed = started.elapsed();

        assert!(is_sqlite_busy_error(error.as_ref()));
        assert!(elapsed >= Duration::from_millis(100));
        assert!(elapsed < Duration::from_millis(500));
        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation")
                    == Some(&Value::String("busy_retry_exhaustion".to_owned()))
                && event.get("error_kind") == Some(&Value::String("sqlite_busy".to_owned()))
                && event.get("attempt").and_then(Value::as_u64) == Some(4)
        }));

        holder
            .rollback()
            .await
            .expect("release external writer lock");
        let (_permit, tx) = coordinator
            .begin_immediate(&read_pool, "busy_retry_after_release")
            .await
            .expect("writer pool should recover after external lock release");
        tx.commit()
            .await
            .expect("commit after external lock release");

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test]
    async fn coordinator_uses_fallback_pool_for_in_memory_transactions() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(":memory:")
                    .create_if_missing(true)
                    .busy_timeout(Duration::from_millis(20)),
            )
            .await
            .expect("create in-memory sqlite pool");
        sqlx::query("CREATE TABLE memory_writer_probe (value INTEGER NOT NULL)")
            .execute(&pool)
            .await
            .expect("create in-memory writer probe");

        let coordinator = SqliteWriteCoordinator::new();
        let (_permit, mut tx) = coordinator
            .begin_immediate(&pool, "in_memory_writer")
            .await
            .expect("begin in-memory coordinator transaction");
        sqlx::query("INSERT INTO memory_writer_probe (value) VALUES (9)")
            .execute(&mut *tx)
            .await
            .expect("insert in-memory writer probe");
        tx.commit().await.expect("commit in-memory writer probe");

        let value: i64 = sqlx::query_scalar("SELECT value FROM memory_writer_probe")
            .fetch_one(&pool)
            .await
            .expect("read in-memory writer probe");
        assert_eq!(value, 9);
        pool.close().await;
    }

    #[tokio::test]
    async fn transaction_deadline_interrupts_long_sqlite_statement() {
        let database_path = test_database_path();
        let read_pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        let writer_pool = open_test_pool(&database_path, 1, Duration::from_millis(20)).await;
        let deadline = Duration::from_millis(60);
        let coordinator = SqliteWriteCoordinator::with_write_pool_and_settings(
            Some(writer_pool.clone()),
            test_deadlines(deadline, deadline, Duration::from_millis(20)),
        );
        let (permit, mut tx) = coordinator
            .begin_immediate(&read_pool, "long_statement_deadline")
            .await
            .expect("begin bounded transaction");
        let started = Instant::now();
        let result = sqlx::query_scalar::<_, i64>(
            "WITH RECURSIVE sequence(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 100000000) SELECT SUM(value) FROM sequence",
        )
        .fetch_one(&mut *tx)
        .await;

        let error = result.expect_err("long statement should be interrupted");
        assert!(is_sqlite_write_deadline_error(&error));
        assert!(started.elapsed() < Duration::from_millis(300));
        drop(tx);
        drop(permit);

        let (_permit, next_tx) = coordinator
            .begin_immediate(&read_pool, "after_statement_interrupt")
            .await
            .expect("writer connection should recover after interrupted statement");
        next_tx
            .commit()
            .await
            .expect("commit after interrupted statement");

        remove_test_database(writer_pool, &database_path).await;
        remove_test_database(read_pool, &database_path).await;
    }

    #[tokio::test]
    async fn foreground_writer_queue_p99_stays_inside_deadline_under_contention() {
        let coordinator = SqliteWriteCoordinator::new();
        let mut handles = Vec::new();
        for _ in 0..32 {
            let coordinator = coordinator.clone();
            handles.push(tokio::spawn(async move {
                let started = Instant::now();
                coordinator
                    .write_foreground("foreground_p99", |_| async {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                        Ok::<_, anyhow::Error>(())
                    })
                    .await
                    .expect("foreground write within deadline");
                started.elapsed()
            }));
        }

        let mut elapsed = Vec::with_capacity(handles.len());
        for handle in handles {
            elapsed.push(handle.await.expect("join foreground writer"));
        }
        elapsed.sort_unstable();
        let p99 = elapsed[(elapsed.len() * 99 / 100).min(elapsed.len() - 1)];
        assert!(p99 < FOREGROUND_DEADLINE, "foreground p99 was {p99:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn coordinator_retries_against_a_real_sqlite_busy_lock() {
        let database_path = test_database_path();
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_millis(1));
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .expect("create sqlite busy fixture db");
        sqlx::query("CREATE TABLE busy_probe (value INTEGER NOT NULL)")
            .execute(&pool)
            .await
            .expect("create busy probe table");

        let mut holder = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .expect("begin external writer transaction");
        sqlx::query("INSERT INTO busy_probe (value) VALUES (1)")
            .execute(&mut *holder)
            .await
            .expect("seed external writer transaction");

        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_target(false)
            .with_writer(buffer.clone())
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _default_guard = tracing::subscriber::set_default(subscriber);

        let coordinator = SqliteWriteCoordinator::new();
        let writer_pool = pool.clone();
        let writer = tokio::spawn(async move {
            coordinator
                .write("busy_fixture", move |_attempt| {
                    let writer_pool = writer_pool.clone();
                    async move {
                        sqlx::query("INSERT INTO busy_probe (value) VALUES (2)")
                            .execute(&writer_pool)
                            .await
                            .map(|_| ())
                            .map_err(anyhow::Error::from)
                    }
                })
                .await
        });

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if buffer.json_events().iter().any(|event| {
                    event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                        && event.get("operation") == Some(&Value::String("busy_fixture".to_owned()))
                        && event.get("error_kind") == Some(&Value::String("sqlite_busy".to_owned()))
                        && event.get("attempt").and_then(Value::as_u64) == Some(1)
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first SQLite insert should report its busy result");
        holder.commit().await.expect("release external writer lock");
        writer
            .await
            .expect("join busy fixture writer")
            .expect("coordinator should retry after sqlite busy");

        let events = buffer.json_events();
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation") == Some(&Value::String("busy_fixture".to_owned()))
                && event.get("error_kind") == Some(&Value::String("sqlite_busy".to_owned()))
                && event.get("attempt").and_then(Value::as_u64) == Some(1)
                && event.get("writer_wait_ms").is_some()
        }));
        assert!(events.iter().any(|event| {
            event.get("event") == Some(&Value::String("sqlite.write".to_owned()))
                && event.get("operation") == Some(&Value::String("busy_fixture".to_owned()))
                && event.get("attempt").and_then(Value::as_u64) == Some(2)
                && event.get("elapsed_ms").is_some()
        }));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM busy_probe")
            .fetch_one(&pool)
            .await
            .expect("count busy probe rows");
        assert_eq!(count, 2);

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
