use std::fmt;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use axum::{
    extract::MatchedPath, extract::Request, http::StatusCode, middleware::Next, response::Response,
};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tracing::Span;

const DEFAULT_HTTP_SLOW_MS: usize = 1_000;
const DEFAULT_UPSTREAM_SLOW_MS: usize = 2_000;
const DEFAULT_SQLITE_WRITE_SLOW_MS: usize = 250;

static LOGGING_THRESHOLDS: OnceLock<RwLock<LoggingThresholds>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct LoggingThresholds {
    pub http_slow_ms: usize,
    pub upstream_slow_ms: usize,
    pub sqlite_write_slow_ms: usize,
}

impl Default for LoggingThresholds {
    fn default() -> Self {
        Self {
            http_slow_ms: DEFAULT_HTTP_SLOW_MS,
            upstream_slow_ms: DEFAULT_UPSTREAM_SLOW_MS,
            sqlite_write_slow_ms: DEFAULT_SQLITE_WRITE_SLOW_MS,
        }
    }
}

pub fn set_logging_thresholds(thresholds: LoggingThresholds) {
    let cell = LOGGING_THRESHOLDS.get_or_init(|| RwLock::new(LoggingThresholds::default()));
    *cell.write().expect("logging thresholds lock poisoned") = thresholds;
}

pub fn logging_thresholds() -> LoggingThresholds {
    LOGGING_THRESHOLDS
        .get_or_init(|| RwLock::new(LoggingThresholds::default()))
        .read()
        .expect("logging thresholds lock poisoned")
        .clone()
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_current_span(true)
        .with_span_list(false)
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();
}

pub fn request_id_layers() -> (SetRequestIdLayer<MakeRequestUuid>, PropagateRequestIdLayer) {
    (
        SetRequestIdLayer::x_request_id(MakeRequestUuid),
        PropagateRequestIdLayer::x_request_id(),
    )
}

pub fn request_route(request: &Request) -> String {
    request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_else(|| request.uri().path().to_owned())
}

pub fn request_id_from_headers(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_owned()
}

pub fn request_trace_span(request: &Request) -> Span {
    let request_id = request_id_from_headers(request.headers());
    let route = request_route(request);
    tracing::info_span!(
        "http.request",
        request_id = %request_id,
        method = %request.method(),
        route = %route,
    )
}

pub async fn access_log_middleware(request: Request, next: Next) -> Response {
    let request_id = request_id_from_headers(request.headers());
    let method = request.method().to_string();
    let route = request_route(&request);
    let thresholds = logging_thresholds();
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    let latency = started.elapsed();
    let status = response.status();
    if is_slow_or_error(status, latency, thresholds.http_slow_ms) {
        let latency_ms = latency.as_millis();
        if status.is_client_error() || status.is_server_error() {
            tracing::warn!(
                event = "http.access",
                request_id = %request_id,
                method = %method,
                route = %route,
                status = status.as_u16(),
                latency_ms,
                "http request finished with error"
            );
        } else {
            tracing::info!(
                event = "http.access",
                request_id = %request_id,
                method = %method,
                route = %route,
                status = status.as_u16(),
                latency_ms,
                threshold_ms = thresholds.http_slow_ms,
                "http request finished slowly"
            );
        }
    }
    response
}

pub fn error_chain_summary(err: &dyn std::error::Error) -> String {
    let mut parts = Vec::new();
    parts.push(err.to_string());
    let mut current = err.source();
    while let Some(source) = current {
        parts.push(source.to_string());
        current = source.source();
    }
    parts.join(": ")
}

pub fn is_slow_http_latency(latency: Duration, threshold_ms: usize) -> bool {
    latency.as_millis() >= threshold_ms as u128
}

pub fn is_slow_or_error(status: StatusCode, latency: Duration, threshold_ms: usize) -> bool {
    status.is_client_error()
        || status.is_server_error()
        || is_slow_http_latency(latency, threshold_ms)
}

impl fmt::Display for LoggingThresholds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "http={}ms upstream={}ms sqlite_write={}ms",
            self.http_slow_ms, self.upstream_slow_ms, self.sqlite_write_slow_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_success_request_is_not_logged() {
        assert!(!is_slow_or_error(
            StatusCode::OK,
            Duration::from_millis(200),
            1_000,
        ));
    }

    #[test]
    fn slow_success_request_is_logged() {
        assert!(is_slow_or_error(
            StatusCode::OK,
            Duration::from_millis(1_200),
            1_000,
        ));
    }

    #[test]
    fn client_error_request_is_logged_even_when_fast() {
        assert!(is_slow_or_error(
            StatusCode::BAD_REQUEST,
            Duration::from_millis(10),
            1_000,
        ));
    }

    #[test]
    fn error_chain_summary_includes_sources() {
        let err = anyhow::anyhow!("outer").context("middle");
        let summary = error_chain_summary(err.as_ref());
        assert!(summary.contains("middle"));
        assert!(summary.contains("outer"));
    }
}
