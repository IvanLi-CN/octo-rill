use std::{
    borrow::Cow,
    cell::RefCell,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::Request,
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;
use time::OffsetDateTime;
use tokio::time::Instant;
use tower::{Layer, Service};
use tower_cookies::{Cookie, CookieManager, Cookies, cookie::SameSite};
use tower_sessions::{
    ExpiredDeletion, Expiry, Session, SessionStore,
    session::{Id, Record},
    session_store,
};
use tower_sessions_sqlx_store::SqliteStore;
use tracing::{debug, warn};

use crate::error::ApiError;
use crate::observability;
use crate::sqlite_write::{
    SqliteWriteCoordinator, SqliteWritePriority, is_sqlite_busy_error,
    is_sqlite_write_deadline_error,
};

const SESSION_TABLE_NAME: &str = "tower_sessions";
const SESSION_ACTIVITY_TOUCH_KEY: &str = "activity_touched_at";
const SESSION_ACTIVITY_REFRESH_FAILURE_PREFIX: &str = "sqlite session activity refresh failed";
const SESSION_WRITE_MAX_ATTEMPTS: usize = 4;

tokio::task_local! {
    static SESSION_BASELINE: RefCell<Option<Record>>;
}

#[derive(Clone)]
pub struct CoordinatedSqliteSessionStore {
    inner: SqliteStore,
    reader_pool: SqlitePool,
    sqlite_writer: SqliteWriteCoordinator,
}

impl CoordinatedSqliteSessionStore {
    pub fn new(
        inner: SqliteStore,
        reader_pool: SqlitePool,
        sqlite_writer: SqliteWriteCoordinator,
    ) -> Self {
        Self {
            inner,
            reader_pool,
            sqlite_writer,
        }
    }

    pub async fn migrate(&self) -> sqlx::Result<()> {
        self.inner.migrate().await
    }

    fn map_write_error(
        &self,
        lane: &'static str,
        activity_only: bool,
        deadline: Duration,
        error: anyhow::Error,
    ) -> session_store::Error {
        map_coordinated_write_error(&self.sqlite_writer, lane, activity_only, deadline, error)
    }

    fn retry_delay(&self, attempt: usize, deadline_at: Instant) -> Option<Duration> {
        if attempt >= SESSION_WRITE_MAX_ATTEMPTS {
            return None;
        }
        let delay = self.sqlite_writer.retry_delay_for_attempt(attempt);
        (deadline_at.saturating_duration_since(Instant::now()) > delay).then_some(delay)
    }

    async fn retry_after_busy(
        &self,
        lane: &'static str,
        attempt: usize,
        deadline_at: Instant,
        error: &anyhow::Error,
    ) -> bool {
        let Some(delay) = self.retry_delay(attempt, deadline_at) else {
            return false;
        };
        warn!(
            event = "sqlite.write",
            operation = lane,
            priority = SqliteWritePriority::Foreground.as_str(),
            attempt,
            retry_after_ms = delay.as_millis(),
            error_kind = "sqlite_busy",
            error_chain = %error,
            "sqlite session write hit busy state; retrying"
        );
        tokio::time::sleep(delay).await;
        true
    }

    fn log_success(&self, lane: &'static str, started: Instant, attempt: usize) {
        let elapsed_ms = started.elapsed().as_millis();
        let threshold_ms = observability::logging_thresholds().sqlite_write_slow_ms;
        if elapsed_ms >= threshold_ms as u128 {
            warn!(
                event = "sqlite.write",
                operation = lane,
                priority = SqliteWritePriority::Foreground.as_str(),
                elapsed_ms,
                attempt,
                threshold_ms,
                "sqlite session write completed slowly"
            );
        } else {
            debug!(
                event = "sqlite.write",
                operation = lane,
                priority = SqliteWritePriority::Foreground.as_str(),
                elapsed_ms,
                attempt,
                "sqlite session write completed"
            );
        }
    }
}

fn task_session_baseline_for(session_id: &Id) -> Option<Record> {
    SESSION_BASELINE
        .try_with(|baseline| baseline.borrow().clone())
        .ok()
        .flatten()
        .filter(|record| &record.id == session_id)
}

fn set_task_session_baseline(record: Option<&Record>) {
    let _ = SESSION_BASELINE.try_with(|baseline| {
        *baseline.borrow_mut() = record.cloned();
    });
}

pub(crate) async fn with_session_baseline_scope<F>(future: F) -> F::Output
where
    F: Future,
{
    SESSION_BASELINE.scope(RefCell::new(None), future).await
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
        let lane = "session_create";
        let deadline = self
            .sqlite_writer
            .deadline_for_priority(SqliteWritePriority::Foreground);
        let deadline_at = Instant::now() + deadline;
        let started = Instant::now();
        let mut attempt = 1usize;
        let mut encoded = rmp_serde::to_vec(record)
            .map_err(|error| session_store::Error::Encode(error.to_string()))?;

        loop {
            let (permit, mut transaction) = self
                .sqlite_writer
                .begin_immediate_with_priority_until(
                    &self.reader_pool,
                    lane,
                    SqliteWritePriority::Foreground,
                    deadline_at,
                )
                .await
                .map_err(|error| self.map_write_error(lane, false, deadline, error))?;

            let result = sqlx::query(&format!(
                "INSERT OR ABORT INTO {SESSION_TABLE_NAME} (id, data, expiry_date) VALUES (?, ?, ?)"
            ))
            .bind(record.id.to_string())
            .bind(encoded.clone())
            .bind(record.expiry_date)
            .execute(&mut *transaction)
            .await;

            match result {
                Ok(_) => match transaction.commit().await {
                    Ok(()) => {
                        drop(permit);
                        set_task_session_baseline(Some(record));
                        self.log_success(lane, started, attempt);
                        return Ok(());
                    }
                    Err(error) if is_sqlite_busy_error(error.as_ref()) => {
                        drop(permit);
                        if self
                            .retry_after_busy(lane, attempt, deadline_at, &error)
                            .await
                        {
                            attempt += 1;
                        } else {
                            return Err(self.map_write_error(lane, false, deadline, error));
                        }
                    }
                    Err(error) => {
                        return Err(self.map_write_error(lane, false, deadline, error));
                    }
                },
                Err(error) if matches!(&error, sqlx::Error::Database(database_error) if database_error.is_unique_violation()) =>
                {
                    transaction.rollback().await.map_err(|rollback_error| {
                        self.map_write_error(
                            lane,
                            false,
                            deadline,
                            rollback_error.context("rollback sqlite session create transaction"),
                        )
                    })?;
                    record.id = Id::default();
                    encoded = rmp_serde::to_vec(record).map_err(|encode_error| {
                        session_store::Error::Encode(encode_error.to_string())
                    })?;
                }
                Err(error) => {
                    let busy = is_sqlite_busy_error(&error);
                    let error = anyhow::Error::new(error).context("insert sqlite session");
                    if let Err(rollback_error) = transaction.rollback().await {
                        warn!(
                            event = "sqlite.write",
                            operation = lane,
                            priority = SqliteWritePriority::Foreground.as_str(),
                            error_kind = "rollback_error",
                            error_chain = %rollback_error,
                            "sqlite session write rollback failed; preserving the original error"
                        );
                    }
                    if busy {
                        drop(permit);
                        if self
                            .retry_after_busy(lane, attempt, deadline_at, &error)
                            .await
                        {
                            attempt += 1;
                            continue;
                        }
                        return Err(self.map_write_error(lane, false, deadline, error));
                    }
                    drop(permit);
                    return Err(self.map_write_error(lane, false, deadline, error));
                }
            }
        }
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let lane = "session_save";
        let deadline = self
            .sqlite_writer
            .deadline_for_priority(SqliteWritePriority::Foreground);
        let deadline_at = Instant::now() + deadline;
        let started = Instant::now();
        let mut attempt = 1usize;
        let previous = task_session_baseline_for(&record.id);
        let activity_only = previous
            .as_ref()
            .is_some_and(|previous| session_record_changed_only_by_activity(previous, record));
        let desired_encoded = rmp_serde::to_vec(record)
            .map_err(|error| session_store::Error::Encode(error.to_string()))?;

        loop {
            let (permit, mut transaction) = self
                .sqlite_writer
                .begin_immediate_with_priority_until(
                    &self.reader_pool,
                    lane,
                    SqliteWritePriority::Foreground,
                    deadline_at,
                )
                .await
                .map_err(|error| self.map_write_error(lane, activity_only, deadline, error))?;

            let current_result = sqlx::query_as::<_, (Vec<u8>,)>(&format!(
                "SELECT data FROM {SESSION_TABLE_NAME} WHERE id = ?"
            ))
            .bind(record.id.to_string())
            .fetch_optional(&mut *transaction)
            .await;
            let (encoded, committed_record) = match current_result {
                Ok(Some((data,))) => {
                    let current = match rmp_serde::from_slice::<Record>(&data) {
                        Ok(current) => current,
                        Err(error) => {
                            if let Err(rollback_error) = transaction.rollback().await {
                                warn!(
                                    event = "sqlite.write",
                                    operation = lane,
                                    priority = SqliteWritePriority::Foreground.as_str(),
                                    error_kind = "rollback_error",
                                    error_chain = %rollback_error,
                                    "sqlite session write rollback failed; preserving the original error"
                                );
                            }
                            drop(permit);
                            return Err(session_store::Error::Decode(error.to_string()));
                        }
                    };
                    let Some(previous) = previous.as_ref() else {
                        let error = anyhow::anyhow!(
                            "retryable sqlite session conflict: request baseline unavailable"
                        );
                        if let Err(rollback_error) = transaction.rollback().await {
                            warn!(
                                event = "sqlite.write",
                                operation = lane,
                                priority = SqliteWritePriority::Foreground.as_str(),
                                error_kind = "rollback_error",
                                error_chain = %rollback_error,
                                "sqlite session write rollback failed; preserving the original error"
                            );
                        }
                        drop(permit);
                        return Err(self.map_write_error(lane, false, deadline, error));
                    };
                    let merged = merge_session_record_changes(previous, &current, record);
                    match rmp_serde::to_vec(&merged) {
                        Ok(encoded) => (encoded, merged),
                        Err(error) => {
                            if let Err(rollback_error) = transaction.rollback().await {
                                warn!(
                                    event = "sqlite.write",
                                    operation = lane,
                                    priority = SqliteWritePriority::Foreground.as_str(),
                                    error_kind = "rollback_error",
                                    error_chain = %rollback_error,
                                    "sqlite session write rollback failed; preserving the original error"
                                );
                            }
                            drop(permit);
                            return Err(session_store::Error::Encode(error.to_string()));
                        }
                    }
                }
                Ok(None) => (desired_encoded.clone(), record.clone()),
                Err(error) => {
                    let busy = is_sqlite_busy_error(&error);
                    let error =
                        anyhow::Error::new(error).context("load sqlite session before upsert");
                    if let Err(rollback_error) = transaction.rollback().await {
                        warn!(
                            event = "sqlite.write",
                            operation = lane,
                            priority = SqliteWritePriority::Foreground.as_str(),
                            error_kind = "rollback_error",
                            error_chain = %rollback_error,
                            "sqlite session write rollback failed; preserving the original error"
                        );
                    }
                    if busy {
                        drop(permit);
                        if self
                            .retry_after_busy(lane, attempt, deadline_at, &error)
                            .await
                        {
                            attempt += 1;
                            continue;
                        }
                        return Err(self.map_write_error(lane, activity_only, deadline, error));
                    }
                    drop(permit);
                    return Err(self.map_write_error(lane, activity_only, deadline, error));
                }
            };

            let result = sqlx::query(&format!(
                "INSERT INTO {SESSION_TABLE_NAME} (id, data, expiry_date) VALUES (?, ?, ?) \
                 ON CONFLICT(id) DO UPDATE SET data = excluded.data, expiry_date = excluded.expiry_date"
            ))
            .bind(committed_record.id.to_string())
            .bind(encoded.clone())
            .bind(committed_record.expiry_date)
            .execute(&mut *transaction)
            .await;

            match result {
                Ok(_) => match transaction.commit().await {
                    Ok(()) => {
                        drop(permit);
                        set_task_session_baseline(Some(&committed_record));
                        self.log_success(lane, started, attempt);
                        return Ok(());
                    }
                    Err(error) if is_sqlite_busy_error(error.as_ref()) => {
                        drop(permit);
                        if self
                            .retry_after_busy(lane, attempt, deadline_at, &error)
                            .await
                        {
                            attempt += 1;
                        } else {
                            return Err(self.map_write_error(lane, activity_only, deadline, error));
                        }
                    }
                    Err(error) => {
                        return Err(self.map_write_error(lane, activity_only, deadline, error));
                    }
                },
                Err(error) => {
                    let busy = is_sqlite_busy_error(&error);
                    let error = anyhow::Error::new(error).context("upsert sqlite session");
                    if let Err(rollback_error) = transaction.rollback().await {
                        warn!(
                            event = "sqlite.write",
                            operation = lane,
                            priority = SqliteWritePriority::Foreground.as_str(),
                            error_kind = "rollback_error",
                            error_chain = %rollback_error,
                            "sqlite session write rollback failed; preserving the original error"
                        );
                    }
                    if busy {
                        drop(permit);
                        if self
                            .retry_after_busy(lane, attempt, deadline_at, &error)
                            .await
                        {
                            attempt += 1;
                            continue;
                        }
                        return Err(self.map_write_error(lane, activity_only, deadline, error));
                    }
                    drop(permit);
                    return Err(self.map_write_error(lane, activity_only, deadline, error));
                }
            }
        }
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let record = self.inner.load(session_id).await?;
        set_task_session_baseline(record.as_ref());
        Ok(record)
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        let lane = "session_delete";
        let deadline = self
            .sqlite_writer
            .deadline_for_priority(SqliteWritePriority::Foreground);
        let deadline_at = Instant::now() + deadline;
        let started = Instant::now();
        let mut attempt = 1usize;

        loop {
            let (permit, mut transaction) = self
                .sqlite_writer
                .begin_immediate_with_priority_until(
                    &self.reader_pool,
                    lane,
                    SqliteWritePriority::Foreground,
                    deadline_at,
                )
                .await
                .map_err(|error| self.map_write_error(lane, false, deadline, error))?;
            let result = sqlx::query(&format!("DELETE FROM {SESSION_TABLE_NAME} WHERE id = ?"))
                .bind(session_id.to_string())
                .execute(&mut *transaction)
                .await;

            match result {
                Ok(_) => match transaction.commit().await {
                    Ok(()) => {
                        drop(permit);
                        set_task_session_baseline(None);
                        self.log_success(lane, started, attempt);
                        return Ok(());
                    }
                    Err(error) if is_sqlite_busy_error(error.as_ref()) => {
                        drop(permit);
                        if self
                            .retry_after_busy(lane, attempt, deadline_at, &error)
                            .await
                        {
                            attempt += 1;
                        } else {
                            return Err(self.map_write_error(lane, false, deadline, error));
                        }
                    }
                    Err(error) => {
                        return Err(self.map_write_error(lane, false, deadline, error));
                    }
                },
                Err(error) => {
                    let busy = is_sqlite_busy_error(&error);
                    let error = anyhow::Error::new(error).context("delete sqlite session");
                    if let Err(rollback_error) = transaction.rollback().await {
                        warn!(
                            event = "sqlite.write",
                            operation = lane,
                            priority = SqliteWritePriority::Foreground.as_str(),
                            error_kind = "rollback_error",
                            error_chain = %rollback_error,
                            "sqlite session write rollback failed; preserving the original error"
                        );
                    }
                    if busy {
                        drop(permit);
                        if self
                            .retry_after_busy(lane, attempt, deadline_at, &error)
                            .await
                        {
                            attempt += 1;
                            continue;
                        }
                        return Err(self.map_write_error(lane, false, deadline, error));
                    }
                    drop(permit);
                    return Err(self.map_write_error(lane, false, deadline, error));
                }
            }
        }
    }
}

fn map_coordinated_write_error(
    sqlite_writer: &SqliteWriteCoordinator,
    lane: &'static str,
    activity_only: bool,
    deadline: Duration,
    error: anyhow::Error,
) -> session_store::Error {
    let error =
        if is_sqlite_busy_error(error.as_ref()) || is_sqlite_write_deadline_error(error.as_ref()) {
            sqlite_writer.deadline_error_for(
                lane,
                SqliteWritePriority::Foreground,
                "statement",
                deadline,
            )
        } else {
            error
        };
    let message = error.to_string();
    if activity_only && is_retryable_session_text(&message) {
        session_store::Error::Backend(format!(
            "{SESSION_ACTIVITY_REFRESH_FAILURE_PREFIX}: {message}"
        ))
    } else {
        session_store::Error::Backend(message)
    }
}

#[async_trait]
impl ExpiredDeletion for CoordinatedSqliteSessionStore {
    async fn delete_expired(&self) -> session_store::Result<()> {
        let writer = self.sqlite_writer.clone();
        let pool = writer.write_pool_or(&self.reader_pool).clone();
        let result = writer
            .write_with_priority(
                "session_delete_expired",
                SqliteWritePriority::BestEffort,
                move |_| {
                    let pool = pool.clone();
                    async move {
                        sqlx::query(&format!(
                            "DELETE FROM {SESSION_TABLE_NAME} WHERE datetime(expiry_date) < datetime('now')"
                        ))
                        .execute(&pool)
                        .await
                        .map(|_| ())
                        .map_err(|error| anyhow::Error::new(error).context("delete expired sqlite sessions"))
                    }
                },
            )
            .await;

        match result {
            Ok(()) => Ok(()),
            Err(error)
                if is_sqlite_busy_error(error.as_ref())
                    || is_sqlite_write_deadline_error(error.as_ref()) =>
            {
                debug!(
                    event = "sqlite.write",
                    operation = "session_delete_expired",
                    priority = SqliteWritePriority::BestEffort.as_str(),
                    error_kind = "sqlite_busy",
                    error_chain = %error,
                    "skip session expiry cleanup after sqlite writer pressure"
                );
                Ok(())
            }
            Err(error) => Err(session_store::Error::Backend(error.to_string())),
        }
    }
}

fn session_record_changed_only_by_activity(previous: &Record, next: &Record) -> bool {
    let mut changed = false;
    for (key, value) in &previous.data {
        if next.data.get(key) != Some(value) {
            if key != SESSION_ACTIVITY_TOUCH_KEY {
                return false;
            }
            changed = true;
        }
    }
    for key in next.data.keys() {
        if !previous.data.contains_key(key) {
            if key != SESSION_ACTIVITY_TOUCH_KEY {
                return false;
            }
            changed = true;
        }
    }
    changed
}

fn merge_session_record_changes(previous: &Record, current: &Record, desired: &Record) -> Record {
    let mut merged = current.clone();
    for key in previous.data.keys() {
        if previous.data.get(key) != desired.data.get(key) {
            match desired.data.get(key) {
                Some(value) => {
                    merged.data.insert(key.clone(), value.clone());
                }
                None => {
                    merged.data.remove(key);
                }
            }
        }
    }
    for (key, value) in &desired.data {
        if !previous.data.contains_key(key) {
            merged.data.insert(key.clone(), value.clone());
        }
    }
    merged.id = desired.id;
    merged.expiry_date = current.expiry_date.max(desired.expiry_date);
    merged
}

fn is_retryable_session_text(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("retryable sqlite write deadline exceeded")
        || normalized.contains("sqlite write capacity is busy")
        || normalized.contains("database is locked")
        || normalized.contains("database table is locked")
        || normalized.contains("sqlite_busy")
        || (normalized.contains("code: 9") && normalized.contains("interrupted"))
        || (normalized.contains("code: \"9\"") && normalized.contains("interrupted"))
}

pub(crate) fn is_activity_refresh_session_error(error: &tower_sessions::session::Error) -> bool {
    match error {
        tower_sessions::session::Error::Store(session_store::Error::Backend(message)) => {
            message.starts_with(SESSION_ACTIVITY_REFRESH_FAILURE_PREFIX)
                && is_retryable_session_text(message)
        }
        _ => false,
    }
}

#[derive(Debug, Clone)]
struct CoordinatedSessionConfig {
    name: Cow<'static, str>,
    http_only: bool,
    same_site: SameSite,
    expiry: Option<Expiry>,
    secure: bool,
    path: Cow<'static, str>,
    domain: Option<Cow<'static, str>>,
    always_save: bool,
}

impl Default for CoordinatedSessionConfig {
    fn default() -> Self {
        Self {
            name: "id".into(),
            http_only: true,
            same_site: SameSite::Strict,
            expiry: None,
            secure: true,
            path: "/".into(),
            domain: None,
            always_save: false,
        }
    }
}

impl CoordinatedSessionConfig {
    fn build_cookie(self, session_id: Id, expiry: Option<Expiry>) -> Cookie<'static> {
        let mut cookie_builder = Cookie::build((self.name, session_id.to_string()))
            .http_only(self.http_only)
            .same_site(self.same_site)
            .secure(self.secure)
            .path(self.path);

        cookie_builder = match expiry {
            Some(Expiry::OnInactivity(duration)) => cookie_builder.max_age(duration),
            Some(Expiry::AtDateTime(datetime)) => {
                cookie_builder.max_age(datetime - OffsetDateTime::now_utc())
            }
            Some(Expiry::OnSessionEnd) | None => cookie_builder,
        };

        if let Some(domain) = self.domain {
            cookie_builder = cookie_builder.domain(domain);
        }

        cookie_builder.build()
    }
}

#[derive(Debug, Clone)]
pub struct CoordinatedSessionLayer<Store: SessionStore> {
    session_store: Arc<Store>,
    session_config: CoordinatedSessionConfig,
}

impl<Store: SessionStore> CoordinatedSessionLayer<Store> {
    pub fn new(session_store: Store) -> Self {
        Self {
            session_store: Arc::new(session_store),
            session_config: Default::default(),
        }
    }

    pub fn with_name<N: Into<Cow<'static, str>>>(mut self, name: N) -> Self {
        self.session_config.name = name.into();
        self
    }

    pub fn with_same_site(mut self, same_site: SameSite) -> Self {
        self.session_config.same_site = same_site;
        self
    }

    pub fn with_expiry(mut self, expiry: Expiry) -> Self {
        self.session_config.expiry = Some(expiry);
        self
    }

    pub fn with_secure(mut self, secure: bool) -> Self {
        self.session_config.secure = secure;
        self
    }
}

impl<S, Store> Layer<S> for CoordinatedSessionLayer<Store>
where
    Store: SessionStore,
{
    type Service = CookieManager<CoordinatedSessionManager<S, Store>>;

    fn layer(&self, inner: S) -> Self::Service {
        CookieManager::new(CoordinatedSessionManager {
            inner,
            session_store: self.session_store.clone(),
            session_config: self.session_config.clone(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CoordinatedSessionManager<S, Store: SessionStore> {
    inner: S,
    session_store: Arc<Store>,
    session_config: CoordinatedSessionConfig,
}

impl<S, Store> Service<Request<Body>> for CoordinatedSessionManager<S, Store>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Send + 'static,
    Store: SessionStore,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let session_store = self.session_store.clone();
        let session_config = self.session_config.clone();
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(with_session_baseline_scope(async move {
            let Some(cookies) = req.extensions().get::<Cookies>().cloned() else {
                return Ok(ApiError::internal("missing cookies request extension").into_response());
            };

            let session_cookie = cookies.get(&session_config.name).map(Cookie::into_owned);
            let session_id = session_cookie.as_ref().and_then(|cookie| {
                cookie.value().parse::<Id>().ok().or_else(|| {
                    tracing::warn!("possibly suspicious activity: malformed session id");
                    None
                })
            });
            let session = Session::new(session_id, session_store, session_config.expiry);
            req.extensions_mut().insert(session.clone());

            let response = inner.call(req).await?;
            let modified = session.is_modified();
            let empty = session.is_empty().await;

            match session_cookie {
                Some(mut cookie) if empty => {
                    cookie.set_path(session_config.path.clone());
                    if let Some(domain) = session_config.domain.clone() {
                        cookie.set_domain(domain);
                    }
                    cookies.remove(cookie);
                    Ok(response)
                }
                _ if (modified || session_config.always_save)
                    && !empty
                    && !response.status().is_server_error() =>
                {
                    match session.save().await {
                        Ok(()) => {
                            let Some(session_id) = session.id() else {
                                return Ok(ApiError::internal("missing session id").into_response());
                            };
                            cookies.add(session_config.build_cookie(session_id, session.expiry()));
                            Ok(response)
                        }
                        Err(error) if is_activity_refresh_session_error(&error) => {
                            warn!(
                                error = %error,
                                "skipping activity-only session refresh after sqlite writer pressure"
                            );
                            Ok(response)
                        }
                        Err(error) => Ok(ApiError::internal(&error).into_response()),
                    }
                }
                _ => Ok(response),
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};
    use serde_json::json;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU8, Ordering},
        },
        time::Duration as StdDuration,
    };
    use tower::ServiceExt;

    const FAILURE_NONE: u8 = 0;
    const FAILURE_ACTIVITY: u8 = 1;
    const FAILURE_CRITICAL: u8 = 2;

    #[derive(Debug, Clone)]
    struct TestSessionStore {
        records: Arc<Mutex<HashMap<Id, Record>>>,
        failure_mode: Arc<AtomicU8>,
    }

    impl TestSessionStore {
        fn new() -> Self {
            Self {
                records: Arc::new(Mutex::new(HashMap::new())),
                failure_mode: Arc::new(AtomicU8::new(FAILURE_NONE)),
            }
        }

        fn set_failure_mode(&self, mode: u8) {
            self.failure_mode.store(mode, Ordering::Release);
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

    #[async_trait]
    impl SessionStore for TestSessionStore {
        async fn create(&self, record: &mut Record) -> session_store::Result<()> {
            let mut records = self.records.lock().expect("lock test session records");
            while records.contains_key(&record.id) {
                record.id = Id::default();
            }
            records.insert(record.id, record.clone());
            Ok(())
        }

        async fn save(&self, record: &Record) -> session_store::Result<()> {
            let mode = self.failure_mode.load(Ordering::Acquire);
            if mode == FAILURE_ACTIVITY && record.data.contains_key(SESSION_ACTIVITY_TOUCH_KEY) {
                return Err(session_store::Error::Backend(format!(
                    "{SESSION_ACTIVITY_REFRESH_FAILURE_PREFIX}: retryable sqlite write deadline exceeded"
                )));
            }
            if mode == FAILURE_CRITICAL && record.data.contains_key("critical") {
                return Err(session_store::Error::Backend(
                    "retryable sqlite write deadline exceeded".to_owned(),
                ));
            }
            self.records
                .lock()
                .expect("lock test session records")
                .insert(record.id, record.clone());
            Ok(())
        }

        async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
            Ok(self
                .records
                .lock()
                .expect("lock test session records")
                .get(session_id)
                .filter(|record| record.expiry_date > OffsetDateTime::now_utc())
                .cloned())
        }

        async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
            self.records
                .lock()
                .expect("lock test session records")
                .remove(session_id);
            Ok(())
        }
    }

    async fn seed_session(session: Session) -> axum::http::StatusCode {
        session
            .insert("user_id", "test-user")
            .await
            .expect("seed test session");
        axum::http::StatusCode::NO_CONTENT
    }

    async fn touch_session(session: Session) -> axum::http::StatusCode {
        session
            .insert(SESSION_ACTIVITY_TOUCH_KEY, 123_i64)
            .await
            .expect("touch test session");
        axum::http::StatusCode::NO_CONTENT
    }

    async fn critical_session_write(session: Session) -> axum::http::StatusCode {
        session
            .insert("critical", json!(true))
            .await
            .expect("mutate test session");
        axum::http::StatusCode::NO_CONTENT
    }

    fn request(path: &str, cookie: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().uri(path);
        if let Some(cookie) = cookie {
            builder = builder.header(axum::http::header::COOKIE, cookie);
        }
        builder.body(Body::empty()).expect("build test request")
    }

    #[tokio::test]
    async fn activity_refresh_failure_preserves_response_and_cookie_state() {
        let store = TestSessionStore::new();
        let app = Router::new()
            .route("/seed", get(seed_session))
            .route("/touch", get(touch_session))
            .route("/critical", get(critical_session_write))
            .layer(
                CoordinatedSessionLayer::new(store.clone())
                    .with_secure(false)
                    .with_same_site(SameSite::Lax)
                    .with_expiry(Expiry::OnInactivity(time::Duration::days(30))),
            );

        let response = app
            .clone()
            .oneshot(request("/seed", None))
            .await
            .expect("seed request");
        assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
        let cookie = response
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .expect("seed response cookie")
            .to_str()
            .expect("valid seed response cookie")
            .to_owned();
        assert!(cookie.contains("Max-Age=2592000"));

        store.set_failure_mode(FAILURE_ACTIVITY);
        let response = app
            .clone()
            .oneshot(request("/touch", Some(&cookie)))
            .await
            .expect("activity refresh request");
        assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
        assert!(
            response
                .headers()
                .get(axum::http::header::SET_COOKIE)
                .is_none()
        );

        store.set_failure_mode(FAILURE_CRITICAL);
        let response = app
            .oneshot(request("/critical", Some(&cookie)))
            .await
            .expect("critical session request");
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(response.headers()[axum::http::header::RETRY_AFTER], "1");
    }

    #[test]
    fn interrupted_sqlite_statement_maps_to_retryable_session_error() {
        let error = anyhow::Error::new(sqlx::Error::Database(Box::new(TestDatabaseError {
            code: "9",
            message: "interrupted",
        })));
        let mapped = map_coordinated_write_error(
            &SqliteWriteCoordinator::new(),
            "session_save",
            false,
            Duration::from_millis(900),
            error,
        );

        match mapped {
            session_store::Error::Backend(message) => {
                assert!(message.starts_with("retryable sqlite write deadline exceeded"));
            }
            other => panic!("interrupted sqlite statement mapped to {other:?}"),
        }
    }

    #[test]
    fn activity_only_classification_ignores_expiry_refresh() {
        assert!(is_retryable_session_text(
            "retryable sqlite write deadline exceeded"
        ));
        let previous = Record {
            id: Id::default(),
            data: HashMap::from([(String::from("user_id"), json!("user"))]),
            expiry_date: OffsetDateTime::now_utc(),
        };
        let mut activity = previous.clone();
        activity
            .data
            .insert(SESSION_ACTIVITY_TOUCH_KEY.to_owned(), json!(123_i64));
        activity.expiry_date += time::Duration::minutes(30);
        assert!(session_record_changed_only_by_activity(
            &previous, &activity
        ));

        let mut critical = activity.clone();
        critical.data.insert("critical".to_owned(), json!(true));
        assert!(!session_record_changed_only_by_activity(
            &previous, &critical
        ));

        let mut current = previous.clone();
        current.data.insert("critical".to_owned(), json!(true));
        let merged = merge_session_record_changes(&previous, &current, &activity);
        assert_eq!(merged.data.get("critical"), Some(&json!(true)));
        assert_eq!(
            merged.data.get(SESSION_ACTIVITY_TOUCH_KEY),
            Some(&json!(123_i64))
        );
        assert_eq!(merged.expiry_date, activity.expiry_date);

        let mut stale_expiry = activity.clone();
        stale_expiry.expiry_date = previous.expiry_date + time::Duration::minutes(5);
        let merged_stale_expiry = merge_session_record_changes(&previous, &activity, &stale_expiry);
        assert_eq!(merged_stale_expiry.expiry_date, activity.expiry_date);
    }

    #[tokio::test]
    async fn stale_activity_save_preserves_concurrent_session_fields() {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-session-merge-{}.db",
            crate::local_id::generate_local_id()
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(StdDuration::from_millis(100));
        let reader_pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.clone())
            .await
            .expect("create reader pool");
        let writer_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create writer pool");
        let coordinator = SqliteWriteCoordinator::with_write_pool(writer_pool.clone());
        let first_store = CoordinatedSqliteSessionStore::new(
            SqliteStore::new(reader_pool.clone()),
            reader_pool.clone(),
            coordinator.clone(),
        );
        let second_store = CoordinatedSqliteSessionStore::new(
            SqliteStore::new(reader_pool.clone()),
            reader_pool.clone(),
            coordinator.clone(),
        );
        let shared_store = first_store.clone();
        first_store.migrate().await.expect("migrate session store");

        let mut record = Record {
            id: Id::default(),
            data: HashMap::from([(String::from("user_id"), json!("user"))]),
            expiry_date: OffsetDateTime::now_utc() + time::Duration::hours(1),
        };
        first_store
            .create(&mut record)
            .await
            .expect("create session record");

        let baseline_conflict = SESSION_BASELINE
            .scope(RefCell::new(None), first_store.save(&record))
            .await;
        match baseline_conflict {
            Err(session_store::Error::Backend(message)) => {
                assert!(message.contains("retryable sqlite session conflict"));
            }
            other => panic!("missing request baseline returned {other:?}"),
        }

        SESSION_BASELINE
            .scope(RefCell::new(None), async {
                let mut first_update = first_store
                    .load(&record.id)
                    .await
                    .expect("load first session snapshot")
                    .expect("first session snapshot exists");
                first_update.data.insert("critical".to_owned(), json!(true));
                first_store
                    .save(&first_update)
                    .await
                    .expect("save critical session update");
            })
            .await;

        SESSION_BASELINE
            .scope(RefCell::new(None), async {
                let mut stale_activity_update = second_store
                    .load(&record.id)
                    .await
                    .expect("load stale session snapshot")
                    .expect("stale session snapshot exists");
                stale_activity_update
                    .data
                    .insert(SESSION_ACTIVITY_TOUCH_KEY.to_owned(), json!(123_i64));
                second_store
                    .save(&stale_activity_update)
                    .await
                    .expect("save stale activity update");
            })
            .await;

        let saved = first_store
            .load(&record.id)
            .await
            .expect("reload merged session")
            .expect("merged session exists");
        assert_eq!(saved.data.get("critical"), Some(&json!(true)));
        assert_eq!(
            saved.data.get(SESSION_ACTIVITY_TOUCH_KEY),
            Some(&json!(123_i64))
        );

        let shared_base = SESSION_BASELINE
            .scope(RefCell::new(None), shared_store.load(&record.id))
            .await
            .expect("load shared session snapshot")
            .expect("shared session snapshot exists");
        let mut shared_update = shared_base.clone();
        shared_update
            .data
            .insert("critical_two".to_owned(), json!(true));
        SESSION_BASELINE
            .scope(RefCell::new(Some(shared_base.clone())), async {
                shared_store.save(&shared_update).await
            })
            .await
            .expect("save shared critical session update");
        let mut shared_stale = shared_base.clone();
        shared_stale
            .data
            .insert(SESSION_ACTIVITY_TOUCH_KEY.to_owned(), json!(456_i64));
        SESSION_BASELINE
            .scope(RefCell::new(Some(shared_base)), async {
                shared_store.save(&shared_stale).await
            })
            .await
            .expect("save shared stale activity update");

        let shared_saved = shared_store
            .load(&record.id)
            .await
            .expect("reload shared merged session")
            .expect("shared merged session exists");
        assert_eq!(shared_saved.data.get("critical_two"), Some(&json!(true)));
        assert_eq!(
            shared_saved.data.get(SESSION_ACTIVITY_TOUCH_KEY),
            Some(&json!(456_i64))
        );

        let stale_before_follow_up_updates = shared_saved.clone();
        for index in 0..6 {
            SESSION_BASELINE
                .scope(RefCell::new(None), async {
                    let mut update = shared_store
                        .load(&record.id)
                        .await
                        .expect("load follow-up session snapshot")
                        .expect("follow-up session snapshot exists");
                    update
                        .data
                        .insert(format!("critical_follow_up_{index}"), json!(true));
                    shared_store
                        .save(&update)
                        .await
                        .expect("save follow-up session update");
                })
                .await;
        }
        let mut stale_after_follow_up_updates = stale_before_follow_up_updates.clone();
        stale_after_follow_up_updates
            .data
            .insert(SESSION_ACTIVITY_TOUCH_KEY.to_owned(), json!(789_i64));
        SESSION_BASELINE
            .scope(
                RefCell::new(Some(stale_before_follow_up_updates)),
                shared_store.save(&stale_after_follow_up_updates),
            )
            .await
            .expect("save stale activity after follow-up updates");
        let saved_after_follow_up = shared_store
            .load(&record.id)
            .await
            .expect("reload follow-up merged session")
            .expect("follow-up merged session exists");
        for index in 0..6 {
            assert_eq!(
                saved_after_follow_up
                    .data
                    .get(&format!("critical_follow_up_{index}")),
                Some(&json!(true))
            );
        }

        drop(first_store);
        drop(second_store);
        drop(coordinator);
        reader_pool.close().await;
        writer_pool.close().await;
        let _ = std::fs::remove_file(&database_path);
        let _ = std::fs::remove_file(database_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(database_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn load_uses_reader_pool_while_save_waits_for_writer_permit() {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-session-store-{}.db",
            crate::local_id::generate_local_id()
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(StdDuration::from_millis(100));
        let reader_pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options.clone())
            .await
            .expect("create reader pool");
        let writer_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create writer pool");
        let coordinator = SqliteWriteCoordinator::with_write_pool(writer_pool.clone());
        let store = CoordinatedSqliteSessionStore::new(
            SqliteStore::new(reader_pool.clone()),
            reader_pool.clone(),
            coordinator.clone(),
        );
        store.migrate().await.expect("migrate session store");

        let mut record = Record {
            id: Id::default(),
            data: HashMap::from([(String::from("user_id"), json!("user"))]),
            expiry_date: OffsetDateTime::now_utc() + time::Duration::hours(1),
        };
        store
            .create(&mut record)
            .await
            .expect("create session record");
        let mut updated = SESSION_BASELINE
            .scope(RefCell::new(None), store.load(&record.id))
            .await
            .expect("load session record")
            .expect("session record exists");
        let save_baseline = updated.clone();
        updated.data.insert("critical".to_owned(), json!(true));

        let permit = coordinator
            .acquire_with_priority("session_store_test_hold", SqliteWritePriority::Foreground)
            .await
            .expect("hold writer permit");
        let loaded_while_writer_held =
            tokio::time::timeout(StdDuration::from_millis(200), store.load(&record.id))
                .await
                .expect("reader load should not wait for writer permit")
                .expect("reader load should succeed")
                .expect("session record should remain present");
        assert_eq!(loaded_while_writer_held.id, record.id);

        let save_store = store.clone();
        let save_task = tokio::spawn(async move {
            SESSION_BASELINE
                .scope(RefCell::new(Some(save_baseline)), save_store.save(&updated))
                .await
        });
        tokio::time::timeout(StdDuration::from_millis(200), async {
            loop {
                if coordinator.runtime_status().waiting_foreground > 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("save should wait for held writer permit");
        drop(permit);
        save_task
            .await
            .expect("join session save")
            .expect("save session record");

        let saved = store
            .load(&record.id)
            .await
            .expect("reload session record")
            .expect("saved session record exists");
        assert_eq!(saved.data.get("critical"), Some(&json!(true)));

        drop(store);
        drop(coordinator);
        reader_pool.close().await;
        writer_pool.close().await;
        let _ = std::fs::remove_file(&database_path);
        let _ = std::fs::remove_file(database_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(database_path.with_extension("db-shm"));
    }
}
