use std::{
    collections::HashSet,
    fmt, fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use prometheus_client::{
    encoding::{EncodeLabelSet, text::encode},
    metrics::{counter::Counter, family::Family, gauge::Gauge, histogram::Histogram},
    registry::Registry,
};
use sqlx::{Row, SqlitePool};
use tokio::{
    net::TcpListener,
    sync::Mutex,
    task::AbortHandle,
    time::{MissedTickBehavior, interval, timeout_at},
};

const MAX_EXPOSITION_BYTES: usize = 4 * 1024 * 1024;
const MAX_SERIES: u64 = 20_000;
const FIXED_SERIES_BUDGET: u64 = 32;
const SAMPLE_TIMEOUT: Duration = Duration::from_millis(500);
const STALE_AFTER: Duration = Duration::from_secs(30);
const HTTP_BUCKETS: [f64; 16] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
];
const SSE_BUCKETS: [f64; 8] = [1.0, 5.0, 15.0, 30.0, 60.0, 300.0, 900.0, 3600.0];
const HTTP_HISTOGRAM_SERIES: usize = HTTP_BUCKETS.len() + 3;
const SSE_HISTOGRAM_SERIES: usize = SSE_BUCKETS.len() + 3;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct HttpLabels {
    method: String,
    route: String,
    status_class: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct RouteLabels {
    route: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct BodyLabels {
    route: String,
    outcome: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct SqliteLabels {
    phase: String,
    operation: String,
    priority: String,
    outcome: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct QueueLabels {
    executor: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct UpstreamLabels {
    provider: String,
    operation: String,
    outcome: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct WorkerLabels {
    executor: String,
    phase: String,
    outcome: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ExecutorLabels {
    executor: String,
}

type HistogramFamily<L> = Family<L, Histogram, fn() -> Histogram>;

fn http_histogram() -> Histogram {
    Histogram::new(HTTP_BUCKETS)
}

fn sse_histogram() -> Histogram {
    Histogram::new(SSE_BUCKETS)
}

fn normalize_method(method: &str) -> String {
    match method {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" => method.to_owned(),
        _ => "OTHER".to_owned(),
    }
}

pub fn normalize_route(path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    match path {
        "/api/health" | "/api/version" | "/health" | "/version" => path.to_owned(),
        _ if path.starts_with("/api/auth") => "/api/auth/*".to_owned(),
        _ if path.starts_with("/api/sync") => "/api/sync/*".to_owned(),
        _ if path.starts_with("/api/admin") => "/api/admin/*".to_owned(),
        _ if path.starts_with("/api/translate") => "/api/translate/*".to_owned(),
        _ if path.starts_with("/api") => "/api/*".to_owned(),
        _ if path.starts_with("/auth") => "/auth/*".to_owned(),
        _ if path == "/" => "/".to_owned(),
        _ => "/other".to_owned(),
    }
}

fn status_class(status: u16) -> String {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    }
    .to_owned()
}

fn normalize_sqlite_operation(operation: &str) -> String {
    let operation = operation.trim();
    match operation {
        value if value.starts_with("session") => "session".to_owned(),
        value if value.starts_with("job") => "job".to_owned(),
        value if value.starts_with("repo_release") => "repo_release".to_owned(),
        value if value.starts_with("translation") => "translation".to_owned(),
        value if value.starts_with("content") => "content".to_owned(),
        value if value.starts_with("webhook") => "webhook".to_owned(),
        value if value.starts_with("public_metrics") => "public_metrics".to_owned(),
        value if value.starts_with("api_key") => "api_key".to_owned(),
        value if value.starts_with("admin") => "admin".to_owned(),
        value if value.starts_with("sync") => "sync".to_owned(),
        value if value.starts_with("search") => "search".to_owned(),
        value if value.starts_with("runtime") => "runtime".to_owned(),
        value if value.starts_with("migration") => "migration".to_owned(),
        _ => "other".to_owned(),
    }
}

fn normalize_upstream_operation(operation: &str) -> String {
    let operation = operation.trim();
    match operation {
        "scheduler_wait" | "chat_completions" | "webhook" => operation.to_owned(),
        value if value.contains("graphql") => "graphql".to_owned(),
        value if value.contains("release") => "releases".to_owned(),
        value if value.contains("notification") => "notifications".to_owned(),
        value if value.contains("star") => "stars".to_owned(),
        value if value.contains("social") => "social".to_owned(),
        value if value.contains("discussion") => "discussions".to_owned(),
        _ => "other".to_owned(),
    }
}

fn normalize_upstream_outcome(outcome: &str) -> String {
    match outcome {
        "success" | "rate_limited" | "forbidden" | "timeout" | "http_4xx" | "http_5xx"
        | "transport_error" | "decode_error" | "graphql_error" | "fallback" | "queued"
        | "admitted" => outcome.to_owned(),
        _ => "other".to_owned(),
    }
}

#[derive(Debug)]
pub struct Metrics {
    registry: Registry,
    http_requests: Family<HttpLabels, Counter>,
    http_duration: HistogramFamily<HttpLabels>,
    http_in_flight: Gauge,
    http_body_events: Family<BodyLabels, Counter>,
    sse_active: Gauge,
    sse_duration: HistogramFamily<RouteLabels>,
    sqlite_operations: Family<SqliteLabels, Counter>,
    sqlite_duration: HistogramFamily<SqliteLabels>,
    sqlite_wait: HistogramFamily<SqliteLabels>,
    queue_depth: Family<QueueLabels, Gauge>,
    queue_oldest_age: Family<QueueLabels, Gauge<f64, AtomicU64>>,
    upstream_requests: Family<UpstreamLabels, Counter>,
    upstream_duration: HistogramFamily<UpstreamLabels>,
    worker_operations: Family<WorkerLabels, Counter>,
    worker_duration: HistogramFamily<WorkerLabels>,
    worker_active: Family<ExecutorLabels, Gauge>,
    process_rss_bytes: Gauge<f64, AtomicU64>,
    process_cpu_seconds: Gauge<f64, AtomicU64>,
    db_bytes: Gauge<f64, AtomicU64>,
    wal_bytes: Gauge<f64, AtomicU64>,
    enabled: Gauge,
    sampler_stale: Gauge,
    sampler_last_success: Gauge<u64, AtomicU64>,
    sampler_last_success_at: AtomicU64,
    sampler_failures: Counter,
    exposition_limit_hits: Counter,
    series_limit_hits: Counter,
    scrapes: Counter,
    auth_failures: Counter,
    sample_guard: Mutex<()>,
    encode_guard: StdMutex<()>,
    series_keys: StdMutex<HashSet<String>>,
    series_count: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        let mut registry = Registry::with_prefix("octorill");
        let http_requests = Family::default();
        let http_duration = Family::new_with_constructor(http_histogram as fn() -> Histogram);
        let http_in_flight = Gauge::default();
        let http_body_events = Family::default();
        let sse_active = Gauge::default();
        let sse_duration = Family::new_with_constructor(sse_histogram as fn() -> Histogram);
        let sqlite_operations = Family::default();
        let sqlite_duration = Family::new_with_constructor(http_histogram as fn() -> Histogram);
        let sqlite_wait = Family::new_with_constructor(http_histogram as fn() -> Histogram);
        let queue_depth = Family::default();
        let queue_oldest_age = Family::default();
        let upstream_requests = Family::default();
        let upstream_duration = Family::new_with_constructor(http_histogram as fn() -> Histogram);
        let worker_operations = Family::default();
        let worker_duration = Family::new_with_constructor(http_histogram as fn() -> Histogram);
        let worker_active = Family::default();
        let process_rss_bytes = Gauge::default();
        let process_cpu_seconds = Gauge::default();
        let db_bytes = Gauge::default();
        let wal_bytes = Gauge::default();
        process_rss_bytes.set(f64::NAN);
        process_cpu_seconds.set(f64::NAN);
        db_bytes.set(f64::NAN);
        wal_bytes.set(f64::NAN);
        let enabled = Gauge::default();
        let sampler_stale = Gauge::default();
        let sampler_last_success = Gauge::default();
        let sampler_last_success_at = AtomicU64::new(0);
        let sampler_failures = Counter::default();
        let exposition_limit_hits = Counter::default();
        let series_limit_hits = Counter::default();
        let scrapes = Counter::default();
        let auth_failures = Counter::default();

        registry.register("http_requests", "HTTP requests", http_requests.clone());
        registry.register_with_unit(
            "http_duration",
            "HTTP response generation duration",
            prometheus_client::registry::Unit::Seconds,
            http_duration.clone(),
        );
        registry.register(
            "http_in_flight",
            "HTTP requests in flight",
            http_in_flight.clone(),
        );
        registry.register(
            "http_body_events",
            "HTTP response body lifecycle events",
            http_body_events.clone(),
        );
        registry.register("sse_active", "Active SSE responses", sse_active.clone());
        registry.register_with_unit(
            "sse_duration",
            "SSE response duration",
            prometheus_client::registry::Unit::Seconds,
            sse_duration.clone(),
        );
        registry.register(
            "sqlite_operations",
            "SQLite write operations",
            sqlite_operations.clone(),
        );
        registry.register_with_unit(
            "sqlite_duration",
            "SQLite operation duration",
            prometheus_client::registry::Unit::Seconds,
            sqlite_duration.clone(),
        );
        registry.register_with_unit(
            "sqlite_wait",
            "SQLite writer wait duration",
            prometheus_client::registry::Unit::Seconds,
            sqlite_wait.clone(),
        );
        registry.register("queue_depth", "Runnable queue depth", queue_depth.clone());
        registry.register_with_unit(
            "queue_oldest_age",
            "Oldest runnable queue item age",
            prometheus_client::registry::Unit::Seconds,
            queue_oldest_age.clone(),
        );
        registry.register(
            "upstream_requests",
            "Upstream requests",
            upstream_requests.clone(),
        );
        registry.register_with_unit(
            "upstream_duration",
            "Upstream request duration",
            prometheus_client::registry::Unit::Seconds,
            upstream_duration.clone(),
        );
        registry.register(
            "worker_operations",
            "Background worker logical operations and attempts",
            worker_operations.clone(),
        );
        registry.register_with_unit(
            "worker_duration",
            "Background worker operation duration",
            prometheus_client::registry::Unit::Seconds,
            worker_duration.clone(),
        );
        registry.register(
            "worker_active",
            "Active background workers",
            worker_active.clone(),
        );
        registry.register_with_unit(
            "process_rss",
            "Process resident memory",
            prometheus_client::registry::Unit::Bytes,
            process_rss_bytes.clone(),
        );
        registry.register_with_unit(
            "process_cpu",
            "Process CPU time",
            prometheus_client::registry::Unit::Seconds,
            process_cpu_seconds.clone(),
        );
        registry.register_with_unit(
            "database_size",
            "SQLite database size",
            prometheus_client::registry::Unit::Bytes,
            db_bytes.clone(),
        );
        registry.register_with_unit(
            "wal_size",
            "SQLite WAL size",
            prometheus_client::registry::Unit::Bytes,
            wal_bytes.clone(),
        );
        registry.register("enabled", "Performance metrics enabled", enabled.clone());
        registry.register(
            "sampler_stale",
            "State sampler stale flag",
            sampler_stale.clone(),
        );
        registry.register(
            "sampler_last_success",
            "Last successful state sampler timestamp",
            sampler_last_success.clone(),
        );
        registry.register(
            "sampler_failures",
            "State sampler failures",
            sampler_failures.clone(),
        );
        registry.register(
            "exposition_limit_hits",
            "Metrics exposition limit hits",
            exposition_limit_hits.clone(),
        );
        registry.register(
            "series_limit_hits",
            "Dynamic series limit hits",
            series_limit_hits.clone(),
        );
        registry.register("scrapes", "Metrics scrapes", scrapes.clone());
        registry.register(
            "auth_failures",
            "Metrics authentication failures",
            auth_failures.clone(),
        );

        Self {
            registry,
            http_requests,
            http_duration,
            http_in_flight,
            http_body_events,
            sse_active,
            sse_duration,
            sqlite_operations,
            sqlite_duration,
            sqlite_wait,
            queue_depth,
            queue_oldest_age,
            upstream_requests,
            upstream_duration,
            worker_operations,
            worker_duration,
            worker_active,
            process_rss_bytes,
            process_cpu_seconds,
            db_bytes,
            wal_bytes,
            enabled,
            sampler_stale,
            sampler_last_success,
            sampler_last_success_at,
            sampler_failures,
            exposition_limit_hits,
            series_limit_hits,
            scrapes,
            auth_failures,
            sample_guard: Mutex::new(()),
            encode_guard: StdMutex::new(()),
            series_keys: StdMutex::new(HashSet::new()),
            series_count: AtomicU64::new(0),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(i64::from(enabled));
    }

    fn is_enabled(&self) -> bool {
        self.enabled.get() != 0
    }

    fn reserve_series(
        &self,
        family: &str,
        labels: &impl std::fmt::Debug,
        expanded_series: usize,
    ) -> bool {
        let key = format!("{family}:{labels:?}");
        let mut keys = self
            .series_keys
            .lock()
            .expect("metrics series lock poisoned");
        if keys.contains(&key) {
            return true;
        }
        let expanded_series = expanded_series.max(1) as u64;
        let current = self.series_count.load(Ordering::Relaxed);
        if current.saturating_add(expanded_series) > MAX_SERIES.saturating_sub(FIXED_SERIES_BUDGET)
        {
            self.series_limit_hits.inc();
            return false;
        }
        keys.insert(key);
        self.series_count
            .fetch_add(expanded_series, Ordering::Relaxed);
        true
    }

    pub fn begin_http(&self) {
        if !self.is_enabled() {
            return;
        }
        self.http_in_flight.inc();
    }

    pub fn finish_http(&self, method: &str, route: &str, status: u16, elapsed: Duration) {
        if !self.is_enabled() {
            return;
        }
        self.http_in_flight.dec();
        let labels = HttpLabels {
            method: normalize_method(method),
            route: normalize_route(route),
            status_class: status_class(status),
        };
        if !self.reserve_series("http", &labels, 1 + HTTP_HISTOGRAM_SERIES) {
            return;
        }
        self.http_requests.get_or_create(&labels).inc();
        self.http_duration
            .get_or_create(&labels)
            .observe(elapsed.as_secs_f64());
    }

    pub fn begin_sse(&self) {
        if !self.is_enabled() {
            return;
        }
        self.sse_active.inc();
    }

    pub fn record_http_body(&self, route: &str, outcome: &str) {
        if !self.is_enabled() {
            return;
        }
        let labels = BodyLabels {
            route: normalize_route(route),
            outcome: match outcome {
                "completed" | "error" | "discarded" | "cancelled" => outcome.to_owned(),
                _ => "other".to_owned(),
            },
        };
        if self.reserve_series("http_body", &labels, 1) {
            self.http_body_events.get_or_create(&labels).inc();
        }
    }

    pub fn finish_sse(&self, route: &str, elapsed: Duration) {
        if !self.is_enabled() {
            return;
        }
        self.sse_active.dec();
        let labels = RouteLabels {
            route: normalize_route(route),
        };
        if self.reserve_series("sse", &labels, SSE_HISTOGRAM_SERIES) {
            self.sse_duration
                .get_or_create(&labels)
                .observe(elapsed.as_secs_f64());
        }
    }

    pub fn record_sqlite(
        &self,
        phase: &str,
        operation: &str,
        priority: &str,
        outcome: &str,
        elapsed: Duration,
        wait: Duration,
    ) {
        if !self.is_enabled() {
            return;
        }
        let labels = SqliteLabels {
            phase: match phase {
                "attempt" | "try_write" | "begin" | "pool" | "commit" | "cancelled" => {
                    phase.to_owned()
                }
                _ => "other".to_owned(),
            },
            operation: normalize_sqlite_operation(operation),
            priority: match priority {
                "foreground" | "background" | "best_effort" => priority.to_owned(),
                _ => "other".to_owned(),
            },
            outcome: match outcome {
                "success" | "error" | "busy" | "deadline" | "rejected" | "skipped"
                | "cancelled" => outcome.to_owned(),
                _ => "other".to_owned(),
            },
        };
        if !self.reserve_series("sqlite", &labels, 1 + (HTTP_HISTOGRAM_SERIES * 2)) {
            return;
        }
        self.sqlite_operations.get_or_create(&labels).inc();
        self.sqlite_duration
            .get_or_create(&labels)
            .observe(elapsed.as_secs_f64());
        self.sqlite_wait
            .get_or_create(&labels)
            .observe(wait.as_secs_f64());
    }

    pub fn record_upstream(
        &self,
        provider: &str,
        operation: &str,
        outcome: &str,
        elapsed: Duration,
    ) {
        if !self.is_enabled() {
            return;
        }
        let labels = UpstreamLabels {
            provider: match provider {
                "github" | "llm" | "other" => provider.to_owned(),
                _ => "other".to_owned(),
            },
            operation: normalize_upstream_operation(operation),
            outcome: normalize_upstream_outcome(outcome),
        };
        if !self.reserve_series("upstream", &labels, 1 + HTTP_HISTOGRAM_SERIES) {
            return;
        }
        self.upstream_requests.get_or_create(&labels).inc();
        self.upstream_duration
            .get_or_create(&labels)
            .observe(elapsed.as_secs_f64());
    }

    fn normalize_executor(executor: &str) -> String {
        match executor {
            "jobs" | "repo_release" | "content" | "translation" => executor.to_owned(),
            _ => "other".to_owned(),
        }
    }

    pub fn begin_worker(&self, executor: &str, phase: &str) {
        if !self.is_enabled() {
            return;
        }
        if phase != "attempt" {
            return;
        }
        let labels = ExecutorLabels {
            executor: Self::normalize_executor(executor),
        };
        if self.reserve_series("worker_active", &labels, 1) {
            self.worker_active.get_or_create(&labels).inc();
        }
    }

    pub fn record_worker(&self, executor: &str, phase: &str, outcome: &str, elapsed: Duration) {
        if !self.is_enabled() {
            return;
        }
        let executor = Self::normalize_executor(executor);
        let labels = WorkerLabels {
            executor,
            phase: match phase {
                "logical" | "attempt" => phase.to_owned(),
                _ => "other".to_owned(),
            },
            outcome: match outcome {
                "success" | "error" | "cancelled" | "deferred" | "skipped" => outcome.to_owned(),
                _ => "other".to_owned(),
            },
        };
        if !self.reserve_series("worker", &labels, 1 + HTTP_HISTOGRAM_SERIES) {
            return;
        }
        self.worker_operations.get_or_create(&labels).inc();
        self.worker_duration
            .get_or_create(&labels)
            .observe(elapsed.as_secs_f64());
    }

    pub fn finish_worker(&self, executor: &str, phase: &str, outcome: &str, elapsed: Duration) {
        if !self.is_enabled() {
            return;
        }
        let active_labels = ExecutorLabels {
            executor: Self::normalize_executor(executor),
        };
        if phase == "attempt" && self.reserve_series("worker_active", &active_labels, 1) {
            self.worker_active.get_or_create(&active_labels).dec();
        }
        self.record_worker(executor, phase, outcome, elapsed);
    }

    pub fn set_queue(&self, executor: &str, depth: i64, oldest_age_seconds: Option<f64>) {
        if !self.is_enabled() {
            return;
        }
        let labels = QueueLabels {
            executor: match executor {
                "jobs" | "content" | "translation" | "repo_release" | "search_projection" => {
                    executor.to_owned()
                }
                _ => "other".to_owned(),
            },
        };
        if !self.reserve_series("queue", &labels, 2) {
            return;
        }
        self.queue_depth.get_or_create(&labels).set(depth.max(0));
        self.queue_oldest_age
            .get_or_create(&labels)
            .set(oldest_age_seconds.unwrap_or(0.0).max(0.0));
    }

    pub fn record_sampler_failure(&self) {
        if !self.is_enabled() {
            return;
        }
        self.sampler_failures.inc();
        self.refresh_sampler_stale();
    }

    pub fn record_sampler_success(&self) {
        if !self.is_enabled() {
            return;
        }
        let now = now_unix_seconds();
        self.sampler_last_success_at.store(now, Ordering::Relaxed);
        self.sampler_last_success.set(now);
        self.sampler_stale.set(0);
    }

    fn refresh_sampler_stale(&self) {
        if !self.is_enabled() {
            return;
        }
        let last_success = self.sampler_last_success_at.load(Ordering::Relaxed);
        let stale = last_success == 0
            || now_unix_seconds().saturating_sub(last_success) >= STALE_AFTER.as_secs();
        self.sampler_stale.set(i64::from(stale));
    }

    pub fn encode(&self) -> Result<String, MetricsEncodeError> {
        let _guard = self
            .encode_guard
            .lock()
            .expect("metrics encode lock poisoned");
        let mut output = LimitedWriter::default();
        match encode(&mut output, &self.registry) {
            Ok(()) => Ok(output.output),
            Err(_) if output.exceeded => {
                self.exposition_limit_hits.inc();
                Err(MetricsEncodeError::TooLarge)
            }
            Err(_) => Err(MetricsEncodeError::Encoding),
        }
    }

    pub fn scrape(&self) -> Result<String, MetricsEncodeError> {
        self.scrapes.inc();
        self.encode()
    }

    pub fn install_process_snapshot(&self, database_url: &str) {
        if !self.is_enabled() {
            return;
        }
        let (rss, cpu) = process_snapshot();
        self.process_rss_bytes.set(rss);
        self.process_cpu_seconds.set(cpu);
        if let Some(path) = sqlite_path(database_url) {
            self.db_bytes.set(
                fs::metadata(&path)
                    .map(|metadata| metadata.len() as f64)
                    .unwrap_or(f64::NAN),
            );
            let wal = PathBuf::from(format!("{}-wal", path.display()));
            self.wal_bytes.set(
                fs::metadata(wal)
                    .map(|metadata| metadata.len() as f64)
                    .unwrap_or(f64::NAN),
            );
        } else {
            self.db_bytes.set(f64::NAN);
            self.wal_bytes.set(f64::NAN);
        }
    }

    pub fn spawn_sampler(
        self: &Arc<Self>,
        pool: SqlitePool,
        database_url: String,
        sample_interval: Duration,
    ) -> AbortHandle {
        let metrics = Arc::clone(self);
        tokio::spawn(async move {
            let mut sample_ticker = interval(sample_interval);
            sample_ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let stale_deadline = tokio::time::sleep(STALE_AFTER);
            tokio::pin!(stale_deadline);
            loop {
                tokio::select! {
                    _ = &mut stale_deadline => {
                        metrics.refresh_sampler_stale();
                        stale_deadline.as_mut().reset(tokio::time::Instant::now() + STALE_AFTER);
                    }
                    _ = sample_ticker.tick() => {
                        let Ok(_guard) = metrics.sample_guard.try_lock() else {
                            metrics.refresh_sampler_stale();
                            continue;
                        };
                        metrics.install_process_snapshot(&database_url);
                        if let Err(error) = sample_queues(&pool, &metrics).await {
                            tracing::warn!(error = %error, "observability sampler failed");
                            metrics.record_sampler_failure();
                        } else {
                            metrics.record_sampler_success();
                            stale_deadline.as_mut().reset(tokio::time::Instant::now() + STALE_AFTER);
                        }
                    }
                }
            }
        })
        .abort_handle()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetricsEncodeError {
    #[error("metrics encoding failed")]
    Encoding,
    #[error("metrics exposition exceeds the configured size limit")]
    TooLarge,
}

#[derive(Default)]
struct LimitedWriter {
    output: String,
    exceeded: bool,
}

impl fmt::Write for LimitedWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.output.len().saturating_add(value.len()) > MAX_EXPOSITION_BYTES {
            self.exceeded = true;
            return Err(fmt::Error);
        }
        self.output.push_str(value);
        Ok(())
    }
}

#[derive(Clone)]
struct EndpointState {
    metrics: Arc<Metrics>,
    token: Arc<str>,
}

#[derive(Clone)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub bind: SocketAddr,
    pub sample_interval: Duration,
    token: Option<Arc<str>>,
}

impl fmt::Debug for MetricsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MetricsConfig")
            .field("enabled", &self.enabled)
            .field("bind", &self.bind)
            .field("sample_interval", &self.sample_interval)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl MetricsConfig {
    pub fn from_env() -> Result<Self> {
        let enabled = std::env::var("OCTORILL_METRICS_ENABLED")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        let bind = std::env::var("OCTORILL_METRICS_BIND")
            .unwrap_or_else(|_| "127.0.0.1:58091".to_owned())
            .parse()
            .context("invalid OCTORILL_METRICS_BIND (expected ip:port)")?;
        let sample_seconds = std::env::var("OCTORILL_METRICS_SAMPLE_INTERVAL_SECONDS")
            .ok()
            .map(|value| value.parse::<u64>())
            .transpose()
            .context("invalid OCTORILL_METRICS_SAMPLE_INTERVAL_SECONDS")?
            .unwrap_or(5)
            .clamp(1, 60);
        let sample_interval = Duration::from_secs(sample_seconds);
        let token = if enabled {
            let path = std::env::var("OCTORILL_METRICS_TOKEN_FILE")
                .context("OCTORILL_METRICS_TOKEN_FILE is required when metrics are enabled")?;
            let token = fs::read_to_string(&path)
                .with_context(|| format!("failed to read OCTORILL_METRICS_TOKEN_FILE at {path}"))?;
            let token = token.trim();
            if token.is_empty() {
                bail!("OCTORILL_METRICS_TOKEN_FILE must contain a non-empty token");
            }
            Some(Arc::<str>::from(token.to_owned()))
        } else {
            None
        };
        Ok(Self {
            enabled,
            bind,
            sample_interval,
            token,
        })
    }
}

pub async fn spawn_listener(
    config: &MetricsConfig,
    metrics: Arc<Metrics>,
) -> Result<Option<AbortHandle>> {
    if !config.enabled {
        return Ok(None);
    }
    let listener = TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("failed to bind metrics listener at {}", config.bind))?;
    let state = EndpointState {
        metrics,
        token: config.token.clone().expect("enabled metrics token"),
    };
    let router = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(state);
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            tracing::warn!(error = %error, "metrics listener stopped");
        }
    });
    Ok(Some(task.abort_handle()))
}

async fn metrics_handler(State(state): State<EndpointState>, request: Request) -> Response {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|candidate| constant_time_eq(candidate.as_bytes(), state.token.as_bytes()));
    if !authorized {
        state.metrics.auth_failures.inc();
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.metrics.scrape() {
        Ok(body) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static(
                    "application/openmetrics-text; version=1.0.0; charset=utf-8",
                ),
            )],
            body,
        )
            .into_response(),
        Err(MetricsEncodeError::TooLarge) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Err(MetricsEncodeError::Encoding) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

async fn sample_queues(pool: &SqlitePool, metrics: &Metrics) -> Result<()> {
    let deadline = tokio::time::Instant::now() + SAMPLE_TIMEOUT;
    let mut connection = timeout_at(deadline, pool.acquire())
        .await
        .context("queue sample connection timed out")??;
    let jobs = sample_queue(
        &mut connection,
        "job_tasks",
        "status = 'queued' AND (available_at IS NULL OR datetime(available_at) <= datetime('now')) AND (task_type NOT IN ('sync.subscriptions', 'retry.recent_failures') OR NOT EXISTS (SELECT 1 FROM job_tasks running WHERE running.task_type = job_tasks.task_type AND running.status = 'running'))",
        deadline,
    )
    .await?;
    let content = sample_queue(
        &mut connection,
        "content_work_items",
        "status = 'queued' AND (next_retry_at IS NULL OR datetime(next_retry_at) <= datetime('now')) AND (retry_expires_at IS NULL OR datetime(retry_expires_at) > datetime('now')) AND ((priority < 3 AND datetime(created_at) <= datetime('now', '-60 seconds')) OR (priority >= 3 AND EXISTS (SELECT 1 FROM content_attempt_events pending WHERE pending.work_item_id = content_work_items.id AND pending.event_type = 'attempt_queued' AND pending.trigger = 'manual_retry' AND NOT EXISTS (SELECT 1 FROM content_attempt_events started WHERE started.work_item_id = pending.work_item_id AND started.attempt_no = pending.attempt_no AND started.event_type = 'attempt_started')) AND (SELECT COUNT(*) FROM content_batches WHERE status = 'running' AND trigger_reason = 'manual_retry') < MAX(1, COALESCE((SELECT translation_general_worker_concurrency - 1 FROM admin_runtime_settings WHERE id = 1), 1))))",
        deadline,
    )
    .await?;
    let translations = sample_queue(
        &mut connection,
        "translation_work_items",
        "status = 'queued' AND EXISTS (SELECT 1 FROM users u WHERE u.id = translation_work_items.scope_user_id AND u.is_disabled = 0 AND u.paused_at IS NULL)",
        deadline,
    )
    .await?;
    let repo_release = sample_queue(
        &mut connection,
        "repo_release_work_items",
        "status = 'queued' AND NOT EXISTS (SELECT 1 FROM repo_release_sync_state state WHERE state.repo_id = repo_release_work_items.repo_id AND state.backoff_until IS NOT NULL AND julianday(state.backoff_until) > julianday('now'))",
        deadline,
    )
    .await?;
    let search_projection = sample_queue(
        &mut connection,
        "search_projection_outbox",
        "1 = 1",
        deadline,
    )
    .await?;
    metrics.set_queue("jobs", jobs.0, jobs.1);
    metrics.set_queue("content", content.0, content.1);
    metrics.set_queue("translation", translations.0, translations.1);
    metrics.set_queue("repo_release", repo_release.0, repo_release.1);
    metrics.set_queue(
        "search_projection",
        search_projection.0,
        search_projection.1,
    );
    Ok(())
}

async fn sample_queue(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    table: &str,
    predicate: &str,
    deadline: tokio::time::Instant,
) -> Result<(i64, Option<f64>)> {
    let query = format!(
        "SELECT COUNT(*) AS depth, MIN((julianday('now') - julianday(created_at)) * 86400.0) AS oldest_age FROM {table} WHERE {predicate}"
    );
    let row = timeout_at(deadline, sqlx::query(&query).fetch_one(&mut **connection))
        .await
        .context("queue sample timed out")??;
    let depth = row.try_get::<i64, _>("depth")?;
    let age = row.try_get::<Option<f64>, _>("oldest_age")?;
    Ok((depth, age))
}

fn sqlite_path(database_url: &str) -> Option<PathBuf> {
    let path = database_url.strip_prefix("sqlite:")?.split('?').next()?;
    if path.is_empty() || path == ":memory:" {
        None
    } else {
        Some(PathBuf::from(path.trim_start_matches("//")))
    }
}

fn process_snapshot() -> (f64, f64) {
    #[cfg(target_os = "linux")]
    {
        let rss = fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|statm| statm.split_whitespace().nth(1)?.parse::<u64>().ok())
            .map(|pages| pages.saturating_mul(4096) as f64)
            .unwrap_or(f64::NAN);
        let cpu_seconds = fs::read_to_string("/proc/self/stat")
            .ok()
            .and_then(|stat| {
                let fields = stat
                    .rsplit_once(") ")?
                    .1
                    .split_whitespace()
                    .collect::<Vec<_>>();
                fields
                    .get(11)
                    .and_then(|user_ticks| user_ticks.parse::<f64>().ok())
                    .zip(
                        fields
                            .get(12)
                            .and_then(|system_ticks| system_ticks.parse::<f64>().ok()),
                    )
                    .map(|(user_ticks, system_ticks)| user_ticks + system_ticks)
            })
            .map(|ticks| ticks / 100.0)
            .unwrap_or(f64::NAN);
        (rss, cpu_seconds)
    }
    #[cfg(not(target_os = "linux"))]
    {
        (f64::NAN, f64::NAN)
    }
}

static GLOBAL_METRICS: OnceLock<Arc<Metrics>> = OnceLock::new();

pub fn install_global(metrics: Arc<Metrics>) -> Arc<Metrics> {
    GLOBAL_METRICS.get_or_init(|| metrics).clone()
}

pub fn global() -> Option<Arc<Metrics>> {
    GLOBAL_METRICS.get().cloned()
}

pub struct SseGuard {
    metrics: Option<Arc<Metrics>>,
    route: &'static str,
    started_at: Instant,
}

pub struct HttpBodyGuard {
    metrics: Option<Arc<Metrics>>,
    route: String,
    finished: bool,
}

pub struct HttpRequestGuard {
    metrics: Option<Arc<Metrics>>,
    method: String,
    route: String,
    started_at: Instant,
    finished: bool,
}

pub struct WorkerGuard {
    metrics: Option<Arc<Metrics>>,
    executor: &'static str,
    phase: &'static str,
    started_at: Instant,
    finished: bool,
}

pub struct SqliteGuard {
    metrics: Option<Arc<Metrics>>,
    phase: &'static str,
    operation: &'static str,
    priority: &'static str,
    wait: Duration,
    wait_started_at: Option<Instant>,
    started_at: Instant,
    finished: bool,
}

impl SqliteGuard {
    pub fn new(
        phase: &'static str,
        operation: &'static str,
        priority: &'static str,
        wait: Duration,
    ) -> Option<Self> {
        let metrics = global();
        metrics.as_ref()?.is_enabled().then_some(Self {
            metrics,
            phase,
            operation,
            priority,
            wait,
            wait_started_at: None,
            started_at: Instant::now(),
            finished: false,
        })
    }

    pub fn finish(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }
        if let Some(wait_started_at) = self.wait_started_at.take() {
            self.wait = wait_started_at.elapsed();
        }
        self.finished = true;
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.record_sqlite(
                self.phase,
                self.operation,
                self.priority,
                outcome,
                self.started_at.elapsed(),
                self.wait,
            );
        }
    }

    pub fn set_wait(&mut self, wait: Duration) {
        self.wait = wait;
        self.wait_started_at = None;
    }

    pub fn start_wait(&mut self, started_at: Instant) {
        self.wait_started_at = Some(started_at);
    }
}

impl Drop for SqliteGuard {
    fn drop(&mut self) {
        self.finish("cancelled");
    }
}

pub async fn observe_worker<T, E, F>(
    executor: &'static str,
    phase: &'static str,
    future: F,
) -> std::result::Result<T, E>
where
    F: std::future::Future<Output = std::result::Result<T, E>>,
{
    let mut guard = WorkerGuard::new(executor, phase);
    let result = future.await;
    guard.finish(if result.is_ok() { "success" } else { "error" });
    result
}

impl HttpBodyGuard {
    pub fn new(route: &str) -> Self {
        Self::with_metrics(global(), route)
    }

    fn with_metrics(metrics: Option<Arc<Metrics>>, route: &str) -> Self {
        Self {
            metrics,
            route: route.to_owned(),
            finished: false,
        }
    }

    pub fn finish(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.record_http_body(&self.route, outcome);
        }
    }
}

impl HttpRequestGuard {
    pub fn new(method: &str, route: &str) -> Self {
        Self::with_metrics(global(), method, route)
    }

    fn with_metrics(metrics: Option<Arc<Metrics>>, method: &str, route: &str) -> Self {
        if let Some(metrics) = metrics.as_ref() {
            metrics.begin_http();
        }
        Self {
            metrics,
            method: method.to_owned(),
            route: route.to_owned(),
            started_at: Instant::now(),
            finished: false,
        }
    }

    pub fn finish(&mut self, status: u16) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.finish_http(&self.method, &self.route, status, self.started_at.elapsed());
        }
    }
}

impl Drop for HttpRequestGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.finish_http(&self.method, &self.route, 499, self.started_at.elapsed());
            metrics.record_http_body(&self.route, "cancelled");
        }
    }
}

impl Drop for HttpBodyGuard {
    fn drop(&mut self) {
        self.finish("discarded");
    }
}

impl WorkerGuard {
    pub fn new(executor: &'static str, phase: &'static str) -> Self {
        let metrics = global();
        if let Some(metrics) = metrics.as_ref() {
            metrics.begin_worker(executor, phase);
        }
        Self {
            metrics,
            executor,
            phase,
            started_at: Instant::now(),
            finished: false,
        }
    }

    pub fn finish(&mut self, outcome: &'static str) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.finish_worker(
                self.executor,
                self.phase,
                outcome,
                self.started_at.elapsed(),
            );
        }
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.finish("cancelled");
    }
}

impl SseGuard {
    pub fn new(route: &'static str) -> Self {
        let metrics = global();
        if let Some(metrics) = metrics.as_ref() {
            metrics.begin_sse();
        }
        Self {
            metrics,
            route,
            started_at: Instant::now(),
        }
    }
}

impl Drop for SseGuard {
    fn drop(&mut self) {
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.finish_sse(self.route, self.started_at.elapsed());
        }
    }
}

pub fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_normalization_is_bounded() {
        assert_eq!(normalize_route("/api/users/123?x=1"), "/api/*");
        assert_eq!(normalize_route("/api/sync/starred"), "/api/sync/*");
        assert_eq!(normalize_route("/unexpected/123"), "/other");
    }

    #[test]
    fn metrics_encode_openmetrics_text() {
        let metrics = Metrics::new();
        metrics.set_enabled(true);
        metrics.finish_http("GET", "/api/health", 200, Duration::from_millis(5));
        metrics.record_upstream(
            "unknown-provider",
            "secret-operation",
            "secret-outcome",
            Duration::from_millis(2),
        );
        metrics.record_worker("jobs", "attempt", "success", Duration::from_millis(3));
        let body = metrics.encode().expect("encode metrics");
        assert!(body.contains("# EOF"));
        assert!(body.contains("octorill_http_requests_total"));
        assert!(body.contains("provider=\"other\""));
        assert!(body.contains("executor=\"jobs\""));
        assert!(!body.contains("secret-operation"));
    }

    #[test]
    fn series_budget_counts_expanded_samples() {
        let metrics = Metrics::new();
        metrics.set_enabled(true);
        assert!(metrics.reserve_series(
            "test",
            &"first",
            MAX_SERIES as usize - FIXED_SERIES_BUDGET as usize
        ));
        assert!(!metrics.reserve_series("test", &"second", 1));
        assert_eq!(
            metrics.series_count.load(Ordering::Relaxed),
            MAX_SERIES - FIXED_SERIES_BUDGET
        );
    }

    #[test]
    fn worker_active_tracks_attempts_only_when_phases_are_nested() {
        let metrics = Metrics::new();
        metrics.set_enabled(true);
        metrics.begin_worker("jobs", "logical");
        metrics.begin_worker("jobs", "attempt");
        metrics.finish_worker("jobs", "attempt", "success", Duration::from_millis(1));
        metrics.finish_worker("jobs", "logical", "success", Duration::from_millis(1));
        let body = metrics.encode().expect("encode metrics");
        assert!(body.contains("octorill_worker_active{executor=\"jobs\"} 0"));
    }

    #[test]
    fn constant_time_comparison_requires_equal_bytes() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"other"));
        assert!(!constant_time_eq(b"token", b"token-long"));
    }

    #[test]
    fn queue_age_clears_when_a_queue_becomes_empty() {
        let metrics = Metrics::new();
        metrics.set_enabled(true);
        metrics.set_queue("jobs", 2, Some(4.0));
        metrics.set_queue("jobs", 0, None);
        let body = metrics.encode().expect("encode metrics");
        assert!(body.contains("octorill_queue_depth{executor=\"jobs\"} 0"));
        assert!(body.contains("octorill_queue_oldest_age_seconds{executor=\"jobs\"} 0"));
    }

    #[test]
    fn upstream_outcomes_preserve_scheduler_and_forbidden_categories() {
        let metrics = Metrics::new();
        metrics.set_enabled(true);
        metrics.record_upstream("llm", "scheduler_wait", "queued", Duration::ZERO);
        metrics.record_upstream("llm", "scheduler_wait", "admitted", Duration::ZERO);
        metrics.record_upstream("github", "webhook", "forbidden", Duration::ZERO);
        let body = metrics.encode().expect("encode metrics");
        assert!(body.contains("outcome=\"queued\""));
        assert!(body.contains("outcome=\"admitted\""));
        assert!(body.contains("outcome=\"forbidden\""));
    }

    #[test]
    fn http_request_guard_records_cancellation_once() {
        let metrics = Arc::new(Metrics::new());
        metrics.set_enabled(true);
        {
            let _guard =
                HttpRequestGuard::with_metrics(Some(Arc::clone(&metrics)), "GET", "/api/health");
        }
        let body = metrics.encode().expect("encode metrics");
        assert!(body.contains("octorill_http_in_flight 0"));
        assert!(body.contains("status_class=\"4xx\""));
        assert!(body.contains("outcome=\"cancelled\""));
    }

    #[test]
    fn http_body_guard_records_discard_when_dropped_before_poll() {
        let metrics = Arc::new(Metrics::new());
        metrics.set_enabled(true);
        {
            let _guard = HttpBodyGuard::with_metrics(Some(Arc::clone(&metrics)), "/api/health");
        }
        let body = metrics.encode().expect("encode metrics");
        assert!(body.contains("octorill_http_body_events_total"));
        assert!(body.contains("outcome=\"discarded\""));
    }

    #[test]
    fn sampler_stale_refresh_uses_last_success_deadline() {
        let metrics = Metrics::new();
        metrics.set_enabled(true);
        metrics.sampler_last_success_at.store(
            now_unix_seconds().saturating_sub(STALE_AFTER.as_secs()),
            Ordering::Relaxed,
        );
        metrics.refresh_sampler_stale();
        let body = metrics.encode().expect("encode metrics");
        assert!(body.contains("octorill_sampler_stale 1"));
    }
}
