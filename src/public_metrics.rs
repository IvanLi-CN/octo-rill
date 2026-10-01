use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};

use anyhow::{Context, Result, bail};
use axum::{
    Router,
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::Response,
    routing::get,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use tokio::{
    sync::{Mutex, RwLock},
    task::AbortHandle,
    time::{self, MissedTickBehavior},
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::warn;

use crate::{admin_runtime, sqlite_write::SqliteWriteCoordinator};

pub const PUBLIC_ROUTE: &str = "/api/public/metrics/v1/octo-rill";

const CACHE_TTL: StdDuration = StdDuration::from_secs(5 * 60);
const REFRESH_TIMEOUT: StdDuration = StdDuration::from_secs(3);
const REFRESH_FAILURE_BACKOFF: StdDuration = StdDuration::from_secs(30);
const REFRESH_INTERVAL: StdDuration = StdDuration::from_secs(5 * 60);
const SAMPLE_WINDOW_HOURS: i64 = 12;
const SAMPLE_RETENTION_HOURS: i64 = 24;
const RATE_LIMIT_WINDOW: StdDuration = StdDuration::from_secs(60);
const RATE_LIMIT_REQUESTS_PER_WINDOW: u16 = 120;
const RATE_LIMIT_MAX_CLIENTS: usize = 4096;
const CACHE_CONTROL: &str = "public, max-age=60, stale-while-revalidate=300";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Stat<T> {
    value: T,
    trend: Vec<T>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicSnapshot {
    deduplicated_repositories: Stat<i64>,
    pressure: Stat<f64>,
    freshness: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CachedSnapshot {
    #[cfg(test)]
    payload: PublicSnapshot,
    body: Vec<u8>,
    etag: HeaderValue,
    refreshed_at: Instant,
}

#[derive(Debug)]
struct CurrentAggregate {
    deduplicated_repositories: i64,
    pressure: f64,
    freshness: Vec<u8>,
}

#[derive(Debug)]
struct RateBucket {
    window_started_at: Instant,
    last_seen_at: Instant,
    request_count: u16,
}

#[derive(Debug, Default)]
struct RateLimiter {
    clients: Mutex<HashMap<IpAddr, RateBucket>>,
}

#[derive(Debug, Default)]
struct RefreshState {
    retry_not_before: Option<Instant>,
}

pub struct PublicMetricsService {
    pool: SqlitePool,
    sqlite_writer: SqliteWriteCoordinator,
    cached: RwLock<Option<Arc<CachedSnapshot>>>,
    refresh_state: Mutex<RefreshState>,
    rate_limiter: RateLimiter,
}

impl PublicMetricsService {
    pub fn new(pool: SqlitePool, sqlite_writer: SqliteWriteCoordinator) -> Self {
        Self {
            pool,
            sqlite_writer,
            cached: RwLock::new(None),
            refresh_state: Mutex::new(RefreshState::default()),
            rate_limiter: RateLimiter::default(),
        }
    }

    async fn cached_snapshot(&self) -> Option<Arc<CachedSnapshot>> {
        self.cached.read().await.clone()
    }

    async fn snapshot(&self) -> Option<Arc<CachedSnapshot>> {
        if let Some(snapshot) = self.cached_snapshot().await
            && snapshot.refreshed_at.elapsed() < CACHE_TTL
        {
            return Some(snapshot);
        }

        match time::timeout(REFRESH_TIMEOUT, self.refresh_if_due()).await {
            Ok(Ok(Some(snapshot))) => Some(snapshot),
            Ok(Ok(None)) => self.cached_snapshot().await,
            Ok(Err(error)) => {
                warn!(?error, "failed to refresh public public metrics snapshot");
                self.cached_snapshot().await
            }
            Err(_) => {
                warn!("timed out refreshing public public metrics snapshot");
                self.cached_snapshot().await
            }
        }
    }

    async fn refresh_if_due(&self) -> Result<Option<Arc<CachedSnapshot>>> {
        let mut refresh_state = self.refresh_state.lock().await;

        if let Some(snapshot) = self.cached_snapshot().await
            && snapshot.refreshed_at.elapsed() < CACHE_TTL
        {
            return Ok(Some(snapshot));
        }

        if refresh_state
            .retry_not_before
            .is_some_and(|retry_at| retry_at > Instant::now())
        {
            return Ok(self.cached_snapshot().await);
        }

        refresh_state.retry_not_before = Some(Instant::now() + REFRESH_FAILURE_BACKOFF);
        let current = load_current_aggregate(&self.pool, Utc::now()).await?;
        let trend = self.store_hourly_sample(&current, Utc::now()).await?;
        let payload = PublicSnapshot {
            deduplicated_repositories: Stat {
                value: current.deduplicated_repositories,
                trend: trend.iter().map(|sample| sample.0).collect(),
            },
            pressure: Stat {
                value: current.pressure,
                trend: trend.iter().map(|sample| sample.1).collect(),
            },
            freshness: current.freshness,
        };
        let body =
            serde_json::to_vec(&payload).context("serialize public public metrics snapshot")?;
        let digest = Sha256::digest(&body);
        let etag: HeaderValue = format!("\"{:x}\"", digest)
            .parse()
            .context("build public public metrics ETag")?;
        let snapshot = Arc::new(CachedSnapshot {
            #[cfg(test)]
            payload,
            body,
            etag,
            refreshed_at: Instant::now(),
        });
        *self.cached.write().await = Some(snapshot.clone());
        refresh_state.retry_not_before = None;
        Ok(Some(snapshot))
    }

    async fn store_hourly_sample(
        &self,
        aggregate: &CurrentAggregate,
        sampled_at: DateTime<Utc>,
    ) -> Result<Vec<(i64, f64)>> {
        let sampled_hour = hour_bucket(sampled_at);
        let oldest_hour =
            hour_bucket(sampled_at - chrono::Duration::hours(SAMPLE_WINDOW_HOURS - 1));
        let retention_cutoff =
            hour_bucket(sampled_at - chrono::Duration::hours(SAMPLE_RETENTION_HOURS - 1));
        let observed_at = sampled_at.to_rfc3339_opts(SecondsFormat::Secs, true);
        let repository_count = aggregate.deduplicated_repositories;
        let pressure = aggregate.pressure;
        let pool = self.sqlite_writer.write_pool_or(&self.pool).clone();

        self.sqlite_writer
            .write("public_metrics_hourly_snapshot", move |_| {
                let pool = pool.clone();
                let sampled_hour = sampled_hour.clone();
                let oldest_hour = oldest_hour.clone();
                let retention_cutoff = retention_cutoff.clone();
                let observed_at = observed_at.clone();
                async move {
                    let mut tx = pool
                        .begin()
                        .await
                        .context("begin public metrics sample write")?;
                    sqlx::query(
                        r#"
                        INSERT INTO public_metrics_hourly_snapshots (
                          sampled_hour,
                          observed_at,
                          deduplicated_repositories,
                          pressure
                        ) VALUES (?, ?, ?, ?)
                        ON CONFLICT(sampled_hour) DO UPDATE SET
                          observed_at = excluded.observed_at,
                          deduplicated_repositories = excluded.deduplicated_repositories,
                          pressure = excluded.pressure
                        WHERE excluded.observed_at >= public_metrics_hourly_snapshots.observed_at
                        "#,
                    )
                    .bind(&sampled_hour)
                    .bind(&observed_at)
                    .bind(repository_count)
                    .bind(pressure)
                    .execute(&mut *tx)
                    .await
                    .context("store public metrics hourly sample")?;
                    sqlx::query(
                        "DELETE FROM public_metrics_hourly_snapshots WHERE sampled_hour < ?",
                    )
                    .bind(retention_cutoff)
                    .execute(&mut *tx)
                    .await
                    .context("prune public metrics hourly samples")?;
                    let rows = sqlx::query(
                        r#"
                        SELECT deduplicated_repositories, pressure
                        FROM public_metrics_hourly_snapshots
                        WHERE sampled_hour >= ? AND sampled_hour <= ?
                        ORDER BY sampled_hour DESC
                        LIMIT 12
                        "#,
                    )
                    .bind(oldest_hour)
                    .bind(sampled_hour)
                    .fetch_all(&mut *tx)
                    .await
                    .context("load recent public metrics hourly samples")?;
                    tx.commit()
                        .await
                        .context("commit public metrics hourly sample")?;
                    let mut samples = rows
                        .into_iter()
                        .map(|row| {
                            Ok((
                                row.try_get("deduplicated_repositories")?,
                                row.try_get("pressure")?,
                            ))
                        })
                        .collect::<Result<Vec<(i64, f64)>>>()?;
                    samples.reverse();
                    Ok(samples)
                }
            })
            .await
    }

    async fn allow_request(&self, ip: IpAddr) -> Option<u64> {
        self.rate_limiter.check(ip).await
    }
}

pub fn router(service: Arc<PublicMetricsService>, allowed_origins: Vec<HeaderValue>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods([Method::GET])
        .allow_headers([header::IF_NONE_MATCH])
        .expose_headers([header::ETAG, header::CACHE_CONTROL, header::RETRY_AFTER]);

    Router::new()
        .route(
            PUBLIC_ROUTE,
            get(get_public_snapshot).head(|| async { StatusCode::METHOD_NOT_ALLOWED }),
        )
        .with_state(service)
        .layer(cors)
}

#[axum::debug_handler]
async fn get_public_snapshot(
    State(service): State<Arc<PublicMetricsService>>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let client_ip = address.ip();
    if let Some(retry_after) = service.allow_request(client_ip).await {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&retry_after.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("60")),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }

    let Some(snapshot) = service.snapshot().await else {
        let mut response =
            Response::new(Body::from(r#"{"error":"metrics_unavailable"}"#.to_owned()));
        *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        return response;
    };

    let not_modified = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| if_none_match(value, &snapshot.etag));
    let mut response = Response::new(if not_modified {
        Body::empty()
    } else {
        Body::from(snapshot.body.clone())
    });
    *response.status_mut() = if not_modified {
        StatusCode::NOT_MODIFIED
    } else {
        StatusCode::OK
    };
    response
        .headers_mut()
        .insert(header::ETAG, snapshot.etag.clone());
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_CONTROL),
    );
    if !not_modified {
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    response
}

fn if_none_match(value: &str, etag: &HeaderValue) -> bool {
    let Ok(etag) = etag.to_str() else {
        return false;
    };
    value.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    })
}

fn hour_bucket(time: DateTime<Utc>) -> String {
    let hour_start = time.timestamp().div_euclid(3600) * 3600;
    DateTime::from_timestamp(hour_start, 0)
        .expect("UTC hour start is a valid timestamp")
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

async fn load_current_aggregate(pool: &SqlitePool, now: DateTime<Utc>) -> Result<CurrentAggregate> {
    let budget = admin_runtime::load_repo_refresh_system_budget_per_window(pool)
        .await
        .context("load repo refresh system budget for public metrics")?;
    if budget <= 0 {
        bail!("repo refresh system budget must be positive");
    }

    let rows = sqlx::query(
        r#"
        SELECT urgency_score, actual_last_success_at
        FROM repo_refresh_governance_snapshots
        ORDER BY priority_rank ASC, repo_id ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .context("load ordered repo refresh aggregate rows")?;

    let deduplicated_repositories = i64::try_from(rows.len())
        .context("deduplicated repository count exceeds supported range")?;
    let mut pressure_numerator = 0.0_f64;
    let mut freshness = Vec::with_capacity(rows.len());
    for row in rows {
        let urgency_score: f64 = row
            .try_get("urgency_score")
            .context("read repo refresh urgency score")?;
        if !urgency_score.is_finite() {
            bail!("repo refresh urgency score is not finite");
        }
        pressure_numerator += (urgency_score.min(4.0) - 1.0).max(0.0);
        let last_success_at: Option<String> = row
            .try_get("actual_last_success_at")
            .context("read repo refresh success timestamp")?;
        freshness.push(freshness_code(last_success_at.as_deref(), now)?);
    }

    let pressure = pressure_numerator / budget as f64;
    if !pressure.is_finite() || pressure < 0.0 {
        bail!("computed repo refresh pressure is invalid");
    }

    Ok(CurrentAggregate {
        deduplicated_repositories,
        pressure,
        freshness,
    })
}

fn freshness_code(last_success_at: Option<&str>, now: DateTime<Utc>) -> Result<u8> {
    let Some(last_success_at) = last_success_at else {
        return Ok(4);
    };
    let timestamp = DateTime::parse_from_rfc3339(last_success_at)
        .context("invalid repo refresh success timestamp")?
        .with_timezone(&Utc);
    let age = now.signed_duration_since(timestamp);
    Ok(if age <= chrono::Duration::hours(4) {
        0
    } else if age <= chrono::Duration::hours(12) {
        1
    } else if age <= chrono::Duration::hours(24) {
        2
    } else {
        3
    })
}

impl RateLimiter {
    async fn check(&self, ip: IpAddr) -> Option<u64> {
        let now = Instant::now();
        let mut clients = self.clients.lock().await;
        clients.retain(|_, bucket| now.duration_since(bucket.last_seen_at) < RATE_LIMIT_WINDOW * 2);
        if !clients.contains_key(&ip)
            && clients.len() >= RATE_LIMIT_MAX_CLIENTS
            && let Some(oldest_ip) = clients
                .iter()
                .min_by_key(|(_, bucket)| bucket.last_seen_at)
                .map(|(ip, _)| *ip)
        {
            clients.remove(&oldest_ip);
        }
        let bucket = clients.entry(ip).or_insert(RateBucket {
            window_started_at: now,
            last_seen_at: now,
            request_count: 0,
        });
        if now.duration_since(bucket.window_started_at) >= RATE_LIMIT_WINDOW {
            bucket.window_started_at = now;
            bucket.request_count = 0;
        }
        bucket.last_seen_at = now;
        if bucket.request_count >= RATE_LIMIT_REQUESTS_PER_WINDOW {
            let retry_after = RATE_LIMIT_WINDOW
                .saturating_sub(now.duration_since(bucket.window_started_at))
                .as_secs()
                .max(1);
            return Some(retry_after);
        }
        bucket.request_count += 1;
        None
    }
}

pub fn spawn_refresh_worker(service: Arc<PublicMetricsService>) -> AbortHandle {
    tokio::spawn(async move {
        let mut interval = time::interval(REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let _ = service.snapshot().await;
        }
    })
    .abort_handle()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, extract::connect_info::MockConnectInfo, http::Request};
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::ServiceExt;

    async fn test_service() -> (Arc<PublicMetricsService>, SqlitePool) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        sqlx::query(
            "CREATE TABLE admin_runtime_settings (id INTEGER PRIMARY KEY, repo_refresh_system_budget_per_window INTEGER NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("create runtime settings");
        sqlx::query("INSERT INTO admin_runtime_settings (id, repo_refresh_system_budget_per_window) VALUES (1, 2)")
            .execute(&pool)
            .await
            .expect("seed runtime settings");
        sqlx::query(
            r#"
            CREATE TABLE repo_refresh_governance_snapshots (
              repo_id INTEGER PRIMARY KEY,
              priority_rank INTEGER NOT NULL,
              urgency_score REAL NOT NULL,
              actual_last_success_at TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create governance snapshots");
        sqlx::query(
            r#"
            CREATE TABLE public_metrics_hourly_snapshots (
              sampled_hour TEXT PRIMARY KEY,
              observed_at TEXT NOT NULL,
              deduplicated_repositories INTEGER NOT NULL CHECK (deduplicated_repositories >= 0),
              pressure REAL NOT NULL CHECK (pressure >= 0)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create hourly snapshots");
        (
            Arc::new(PublicMetricsService::new(
                pool.clone(),
                SqliteWriteCoordinator::new(),
            )),
            pool,
        )
    }

    #[tokio::test]
    async fn migrated_hourly_snapshot_table_supports_public_metrics_refresh() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        crate::database_migrations::run(&pool)
            .await
            .expect("apply database migrations");
        sqlx::query(
            r#"
            INSERT INTO repo_refresh_governance_snapshots (
              repo_id, repo_full_name, watcher_user_count, watcher_repo_total_sum,
              priority_rank, target_window, target_interval_minutes, updated_at
            ) VALUES (1, 'owner/repo', 1, 1, 1, 1, 60, '2026-01-01T00:00:00Z')
            "#,
        )
        .execute(&pool)
        .await
        .expect("seed migrated governance snapshot");

        let service = PublicMetricsService::new(pool.clone(), SqliteWriteCoordinator::new());
        let snapshot = service
            .snapshot()
            .await
            .expect("refresh against migrated schema");
        assert_eq!(snapshot.payload.deduplicated_repositories.value, 1);
        assert_eq!(snapshot.payload.deduplicated_repositories.trend, [1]);
        let hourly_sample_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM public_metrics_hourly_snapshots")
                .fetch_one(&pool)
                .await
                .expect("count migrated hourly snapshots");
        assert_eq!(hourly_sample_count, 1);
    }

    async fn insert_repo(
        pool: &SqlitePool,
        repo_id: i64,
        priority_rank: i64,
        urgency_score: f64,
        actual_last_success_at: Option<String>,
    ) {
        sqlx::query(
            "INSERT INTO repo_refresh_governance_snapshots (repo_id, priority_rank, urgency_score, actual_last_success_at) VALUES (?, ?, ?, ?)",
        )
        .bind(repo_id)
        .bind(priority_rank)
        .bind(urgency_score)
        .bind(actual_last_success_at)
        .execute(pool)
        .await
        .expect("insert repo snapshot");
    }

    fn freshness_at(now: DateTime<Utc>, hours_ago: i64) -> String {
        (now - chrono::Duration::hours(hours_ago)).to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    #[tokio::test]
    async fn payload_contains_only_approved_fields_and_preserves_freshness_order() {
        let (service, pool) = test_service().await;
        let now = Utc::now();
        insert_repo(&pool, 10, 5, 1.0, Some(freshness_at(now, 2))).await;
        insert_repo(&pool, 11, 1, 2.0, Some(freshness_at(now, 5))).await;
        insert_repo(&pool, 12, 4, 3.0, Some(freshness_at(now, 13))).await;
        insert_repo(&pool, 13, 2, 4.0, Some(freshness_at(now, 25))).await;
        insert_repo(&pool, 14, 3, 5.0, None).await;

        let snapshot = service.snapshot().await.expect("snapshot available");
        let json = serde_json::to_value(&snapshot.payload).expect("serialize payload");
        let fields = json
            .as_object()
            .expect("payload object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(
            fields,
            ["deduplicatedRepositories", "freshness", "pressure"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
        assert_eq!(snapshot.payload.deduplicated_repositories.value, 5);
        assert_eq!(snapshot.payload.deduplicated_repositories.trend, [5]);
        assert_eq!(snapshot.payload.pressure.value, 4.5);
        assert_eq!(snapshot.payload.pressure.trend, [4.5]);
        assert_eq!(snapshot.payload.freshness, [1, 3, 4, 2, 0]);
        assert_eq!(
            snapshot.payload.freshness.len() as i64,
            snapshot.payload.deduplicated_repositories.value
        );
        let freshness_json = json["freshness"].as_array().expect("JSON numeric array");
        assert!(
            freshness_json
                .iter()
                .all(|value| value.as_u64().is_some_and(|v| v <= 4))
        );
    }

    #[tokio::test]
    async fn recent_trends_are_chronological_and_return_available_points() {
        let (service, pool) = test_service().await;
        let now = Utc::now();
        for hour in 1..SAMPLE_WINDOW_HOURS {
            let sampled_hour =
                hour_bucket(now - chrono::Duration::hours(SAMPLE_WINDOW_HOURS - hour));
            sqlx::query(
                "INSERT INTO public_metrics_hourly_snapshots (sampled_hour, observed_at, deduplicated_repositories, pressure) VALUES (?, ?, ?, ?)",
            )
            .bind(sampled_hour)
            .bind(now.to_rfc3339_opts(SecondsFormat::Secs, true))
            .bind(hour)
            .bind(hour as f64 / 10.0)
            .execute(&pool)
            .await
            .expect("seed hourly sample");
        }
        for repo_id in 1..=12 {
            insert_repo(&pool, repo_id, repo_id, 1.2, None).await;
        }

        let snapshot = service.snapshot().await.expect("snapshot available");
        assert_eq!(snapshot.payload.deduplicated_repositories.trend.len(), 12);
        assert_eq!(snapshot.payload.pressure.trend.len(), 12);
        assert_eq!(
            snapshot.payload.deduplicated_repositories.trend,
            (1..=12).collect::<Vec<_>>()
        );
        for (actual, expected) in snapshot
            .payload
            .pressure
            .trend
            .iter()
            .zip((1..=12).map(|hour| hour as f64 / 10.0))
        {
            assert!((actual - expected).abs() < 1e-12);
        }

        let (cold_service, cold_pool) = test_service().await;
        insert_repo(&cold_pool, 1, 1, 1.0, None).await;
        let cold_snapshot = cold_service
            .snapshot()
            .await
            .expect("partial snapshot available");
        assert_eq!(cold_snapshot.payload.deduplicated_repositories.value, 1);
        assert_eq!(cold_snapshot.payload.deduplicated_repositories.trend, [1]);
    }

    #[tokio::test]
    async fn hourly_snapshot_retention_keeps_at_most_24_hour_buckets() {
        let (service, pool) = test_service().await;
        let hour_timestamp = Utc::now().timestamp().div_euclid(3600) * 3600;
        let sampled_at = DateTime::from_timestamp(hour_timestamp, 0).expect("valid hour");
        for hours_ago in (0..=SAMPLE_RETENTION_HOURS).rev() {
            let observed_at = sampled_at - chrono::Duration::hours(hours_ago);
            sqlx::query(
                "INSERT INTO public_metrics_hourly_snapshots (sampled_hour, observed_at, deduplicated_repositories, pressure) VALUES (?, ?, 1, 0.5)",
            )
            .bind(hour_bucket(observed_at))
            .bind(observed_at.to_rfc3339_opts(SecondsFormat::Secs, true))
            .execute(&pool)
            .await
            .expect("seed hourly retention boundary");
        }

        service
            .store_hourly_sample(
                &CurrentAggregate {
                    deduplicated_repositories: 1,
                    pressure: 0.5,
                    freshness: vec![4],
                },
                sampled_at,
            )
            .await
            .expect("store current hour sample");

        let sample_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM public_metrics_hourly_snapshots")
                .fetch_one(&pool)
                .await
                .expect("count retained samples");
        let oldest_sample = sqlx::query_scalar::<_, Option<String>>(
            "SELECT MIN(sampled_hour) FROM public_metrics_hourly_snapshots",
        )
        .fetch_one(&pool)
        .await
        .expect("read oldest retained sample");

        assert_eq!(sample_count, SAMPLE_RETENTION_HOURS);
        assert_eq!(
            oldest_sample,
            Some(hour_bucket(
                sampled_at - chrono::Duration::hours(SAMPLE_RETENTION_HOURS - 1)
            ))
        );
    }

    #[tokio::test]
    async fn cached_snapshot_is_reused_and_refresh_failure_keeps_last_good_data() {
        let (service, pool) = test_service().await;
        insert_repo(&pool, 1, 1, 1.0, None).await;
        let first = service.snapshot().await.expect("first snapshot available");
        insert_repo(&pool, 2, 2, 1.0, None).await;

        let cached = service.snapshot().await.expect("cached snapshot available");
        assert_eq!(cached.payload.deduplicated_repositories.value, 1);

        sqlx::query("DROP TABLE repo_refresh_governance_snapshots")
            .execute(&pool)
            .await
            .expect("remove aggregate source");
        {
            let mut cache = service.cached.write().await;
            Arc::make_mut(cache.as_mut().expect("cache seeded")).refreshed_at =
                Instant::now() - CACHE_TTL - StdDuration::from_secs(1);
        }
        let stale = service
            .snapshot()
            .await
            .expect("last-known-good snapshot available");
        assert_eq!(stale.payload.deduplicated_repositories.value, 1);
        assert_eq!(stale.etag, first.etag);
    }

    #[tokio::test]
    async fn concurrent_cold_start_refreshes_write_one_hourly_sample() {
        let (service, pool) = test_service().await;
        insert_repo(&pool, 1, 1, 1.0, None).await;
        sqlx::query(
            r#"
            CREATE TABLE public_metrics_test_writes (count INTEGER NOT NULL);
            INSERT INTO public_metrics_test_writes (count) VALUES (0);
            CREATE TRIGGER count_public_metrics_inserts
            AFTER INSERT ON public_metrics_hourly_snapshots
            BEGIN
              UPDATE public_metrics_test_writes SET count = count + 1;
            END;
            CREATE TRIGGER count_public_metrics_updates
            AFTER UPDATE ON public_metrics_hourly_snapshots
            BEGIN
              UPDATE public_metrics_test_writes SET count = count + 1;
            END;
            "#,
        )
        .execute(&pool)
        .await
        .expect("install refresh counter");

        const REQUESTS: usize = 16;
        let barrier = Arc::new(tokio::sync::Barrier::new(REQUESTS));
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..REQUESTS {
            let service = service.clone();
            let barrier = barrier.clone();
            tasks.spawn(async move {
                barrier.wait().await;
                service.snapshot().await.expect("snapshot available")
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.expect("refresh task completes");
        }

        let refresh_writes =
            sqlx::query_scalar::<_, i64>("SELECT count FROM public_metrics_test_writes")
                .fetch_one(&pool)
                .await
                .expect("read refresh count");
        assert_eq!(refresh_writes, 1);
    }

    #[tokio::test]
    async fn route_supports_allowlisted_cors_and_etag_without_credentials() {
        let (service, pool) = test_service().await;
        insert_repo(&pool, 1, 1, 1.0, None).await;
        let origins = ["https://ivanli.cc", "http://127.0.0.1:12620"]
            .into_iter()
            .map(|origin| origin.parse::<HeaderValue>().expect("valid origin"))
            .collect();
        let app = router(service, origins)
            .layer(MockConnectInfo(SocketAddr::from(([192, 0, 2, 10], 8080))));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(PUBLIC_ROUTE)
                    .header(header::ORIGIN, "https://ivanli.cc")
                    .body(Body::empty())
                    .expect("build get request"),
            )
            .await
            .expect("route response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok()),
            Some("https://ivanli.cc")
        );
        assert!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .is_none()
        );
        assert!(response.headers().get(header::SET_COOKIE).is_none());
        let etag = response
            .headers()
            .get(header::ETAG)
            .expect("ETag header")
            .clone();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("JSON payload");
        assert_eq!(
            json["deduplicatedRepositories"]["trend"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        let not_modified = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(PUBLIC_ROUTE)
                    .header(header::ORIGIN, "http://127.0.0.1:12620")
                    .header(header::IF_NONE_MATCH, etag)
                    .body(Body::empty())
                    .expect("build conditional request"),
            )
            .await
            .expect("conditional response");
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            not_modified
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok()),
            Some("http://127.0.0.1:12620")
        );

        let preflight = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri(PUBLIC_ROUTE)
                    .header(header::ORIGIN, "https://ivanli.cc")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "if-none-match")
                    .body(Body::empty())
                    .expect("build preflight request"),
            )
            .await
            .expect("preflight response");
        assert!(preflight.status().is_success());
        assert_eq!(
            preflight
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                .and_then(|value| value.to_str().ok()),
            Some("GET")
        );

        let denied = app
            .oneshot(
                Request::builder()
                    .uri(PUBLIC_ROUTE)
                    .header(header::ORIGIN, "https://unlisted.example")
                    .body(Body::empty())
                    .expect("build unlisted request"),
            )
            .await
            .expect("unlisted origin response");
        assert!(
            denied
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );
    }

    #[tokio::test]
    async fn cold_start_failure_returns_retryable_json_error() {
        let (service, pool) = test_service().await;
        sqlx::query("DROP TABLE repo_refresh_governance_snapshots")
            .execute(&pool)
            .await
            .expect("remove aggregate source");
        let app = router(service, Vec::new())
            .layer(MockConnectInfo(SocketAddr::from(([192, 0, 2, 12], 8080))));

        let response = app
            .oneshot(
                Request::builder()
                    .uri(PUBLIC_ROUTE)
                    .body(Body::empty())
                    .expect("build cold-start request"),
            )
            .await
            .expect("cold-start response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(header::RETRY_AFTER),
            Some(&HeaderValue::from_static("5"))
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json; charset=utf-8"))
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read error response body");
        assert_eq!(body.as_ref(), br#"{"error":"metrics_unavailable"}"#);
    }

    #[tokio::test]
    async fn route_is_get_only_and_rate_limited() {
        let (service, pool) = test_service().await;
        insert_repo(&pool, 1, 1, 1.0, None).await;
        let app = router(service.clone(), Vec::new())
            .layer(MockConnectInfo(SocketAddr::from(([192, 0, 2, 11], 8080))));
        let head = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri(PUBLIC_ROUTE)
                    .body(Body::empty())
                    .expect("build head request"),
            )
            .await
            .expect("head response");
        assert_eq!(head.status(), StatusCode::METHOD_NOT_ALLOWED);
        let post = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(PUBLIC_ROUTE)
                    .body(Body::empty())
                    .expect("build post request"),
            )
            .await
            .expect("post response");
        assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);

        let ip = IpAddr::from([192, 0, 2, 11]);
        for _ in 0..RATE_LIMIT_REQUESTS_PER_WINDOW {
            assert_eq!(service.allow_request(ip).await, None);
        }
        assert!(service.allow_request(ip).await.is_some());
    }

    #[tokio::test]
    async fn freshness_timestamp_must_be_valid() {
        assert_eq!(freshness_code(None, Utc::now()).expect("missing status"), 4);
        assert!(freshness_code(Some("not-a-date"), Utc::now()).is_err());
    }
}
