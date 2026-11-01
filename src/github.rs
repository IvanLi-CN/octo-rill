use std::{
    future::Future,
    time::{Duration, Instant},
};

use anyhow::{Context, Result as AnyhowResult, bail};
use reqwest::{
    StatusCode,
    header::{ACCEPT, HeaderMap, USER_AGENT},
};
use serde::Deserialize;
use url::Url;

const API_BASE: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";

/// Compare is still allowed on legacy read paths until the background evidence
/// collector is integrated. Keep the guard deliberately small so a slow GitHub
/// response cannot pin a Feed request indefinitely.
#[allow(dead_code)]
pub const GITHUB_COMPARE_TIMEOUT: Duration = Duration::from_secs(3);
#[allow(dead_code)]
pub const GITHUB_COMPARE_MAX_REQUESTS: usize = 8;
#[allow(dead_code)]
pub const GITHUB_COMPARE_MAX_FILES: usize = 64;
#[allow(dead_code)]
pub const RELEASE_READING_EVIDENCE_UNAVAILABLE: &str = "release_reading_evidence_unavailable";

#[allow(dead_code)]
pub fn is_exact_release_reading_evidence_unavailable(code: &str) -> bool {
    code == RELEASE_READING_EVIDENCE_UNAVAILABLE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CompareFailureKind {
    Success,
    Timeout,
    Authentication,
    RateLimited,
    Forbidden,
    ServiceUnavailable,
    Http4xx,
    Http5xx,
    Transport,
    Decode,
    AttemptBudget,
    FileLimit,
    Fallback,
    Enrichment,
}

#[allow(dead_code)]
impl CompareFailureKind {
    pub const fn metric_outcome(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Timeout => "timeout",
            Self::Authentication => "authentication_failed",
            Self::RateLimited => "rate_limited",
            Self::Forbidden => "forbidden",
            Self::ServiceUnavailable => "service_unavailable",
            Self::Http4xx => "http_4xx",
            Self::Http5xx => "http_5xx",
            Self::Transport => "transport_error",
            Self::Decode => "decode_error",
            Self::AttemptBudget => "attempt_budget_exhausted",
            Self::FileLimit => "too_many_files",
            Self::Fallback => "fallback",
            Self::Enrichment => "enrichment",
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CompareGuardError {
    #[error("github compare deadline exceeded")]
    DeadlineExceeded,
    #[error("github compare attempt budget exhausted")]
    AttemptBudgetExhausted,
    #[error("github compare returned {actual} files; limit is {limit}")]
    TooManyFiles { actual: usize, limit: usize },
}

#[derive(Debug, Clone)]
pub struct CompareBudget {
    deadline: Instant,
    max_requests: usize,
    requests: usize,
}

#[allow(dead_code)]
impl CompareBudget {
    pub fn new(timeout: Duration, max_requests: usize) -> Self {
        Self {
            deadline: Instant::now() + timeout,
            max_requests,
            requests: 0,
        }
    }

    pub fn with_deadline(deadline: Instant, max_requests: usize) -> Self {
        Self {
            deadline,
            max_requests,
            requests: 0,
        }
    }

    pub fn default_for_feed() -> Self {
        Self::new(GITHUB_COMPARE_TIMEOUT, GITHUB_COMPARE_MAX_REQUESTS)
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn attempts(&self) -> usize {
        self.requests
    }

    pub fn remaining_attempts(&self) -> usize {
        self.max_requests.saturating_sub(self.requests)
    }

    pub fn remaining_timeout(&self) -> Result<Duration, CompareGuardError> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(CompareGuardError::DeadlineExceeded)
    }

    pub fn acquire(&mut self) -> Result<Duration, CompareGuardError> {
        if self.requests >= self.max_requests {
            return Err(CompareGuardError::AttemptBudgetExhausted);
        }
        let remaining = self.remaining_timeout()?;
        self.requests += 1;
        Ok(remaining)
    }

    pub async fn run<F, T>(&mut self, future: F) -> Result<T, CompareGuardError>
    where
        F: Future<Output = T>,
    {
        let remaining = self.acquire()?;
        tokio::time::timeout(remaining, future)
            .await
            .map_err(|_| CompareGuardError::DeadlineExceeded)
    }
}

#[allow(dead_code)]
pub fn compare_url(
    base_url: &Url,
    repo_full_name: &str,
    base_ref: &str,
    head_ref: &str,
    separator: &str,
) -> AnyhowResult<Url> {
    if separator != "..." {
        bail!("unsupported github compare separator: {separator}");
    }
    let compare_ref = format!(
        "{}{}{}",
        urlencoding::encode(base_ref),
        separator,
        urlencoding::encode(head_ref)
    );
    base_url
        .join(&format!("repos/{repo_full_name}/compare/{compare_ref}"))
        .context("build github compare URL failed")
}

#[allow(dead_code)]
pub fn compare_file_limit_exceeded(file_count: usize) -> bool {
    file_count > GITHUB_COMPARE_MAX_FILES
}

#[allow(dead_code)]
pub fn validate_compare_file_count(file_count: usize) -> Result<(), CompareGuardError> {
    if compare_file_limit_exceeded(file_count) {
        return Err(CompareGuardError::TooManyFiles {
            actual: file_count,
            limit: GITHUB_COMPARE_MAX_FILES,
        });
    }
    Ok(())
}

#[allow(dead_code)]
pub fn classify_compare_http_status(
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
) -> CompareFailureKind {
    if status == StatusCode::UNAUTHORIZED {
        return CompareFailureKind::Authentication;
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return CompareFailureKind::RateLimited;
    }
    if status == StatusCode::SERVICE_UNAVAILABLE {
        return CompareFailureKind::ServiceUnavailable;
    }
    if status == StatusCode::FORBIDDEN {
        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            .map(str::trim);
        let body = body.to_ascii_lowercase();
        if remaining == Some("0") || body.contains("rate limit") {
            return CompareFailureKind::RateLimited;
        }
        if body.contains("bad credentials") || body.contains("requires authentication") {
            return CompareFailureKind::Authentication;
        }
        return CompareFailureKind::Forbidden;
    }
    if status.is_client_error() {
        CompareFailureKind::Http4xx
    } else if status.is_server_error() {
        CompareFailureKind::Http5xx
    } else {
        CompareFailureKind::Success
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct EmailItem {
    email: String,
    primary: bool,
    verified: bool,
    visibility: Option<String>,
}

pub async fn fetch_user(http: &reqwest::Client, access_token: &str) -> AnyhowResult<GitHubUser> {
    let user = http
        .get(format!("{API_BASE}/user"))
        .bearer_auth(access_token)
        .header(USER_AGENT, "OctoRill")
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .send()
        .await
        .context("github /user request failed")?
        .error_for_status()
        .context("github /user returned error")?
        .json::<GitHubUser>()
        .await
        .context("github /user json decode failed")?;
    Ok(user)
}

pub async fn fetch_primary_email(
    http: &reqwest::Client,
    access_token: &str,
) -> AnyhowResult<Option<String>> {
    let items = http
        .get(format!("{API_BASE}/user/emails"))
        .bearer_auth(access_token)
        .header(USER_AGENT, "OctoRill")
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .send()
        .await
        .context("github /user/emails request failed")?
        .error_for_status()
        .context("github /user/emails returned error")?
        .json::<Vec<EmailItem>>()
        .await
        .context("github /user/emails json decode failed")?;

    let primary = items
        .iter()
        .find(|e| e.primary && e.verified)
        .or_else(|| items.iter().find(|e| e.primary))
        .or_else(|| items.iter().find(|e| e.verified))
        .or_else(|| {
            items
                .iter()
                .find(|e| e.visibility.as_deref() == Some("public"))
        });

    Ok(primary.map(|e| e.email.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Uri, response::IntoResponse, routing::get};
    use reqwest::header::HeaderValue;
    use serde_json::json;

    async fn spawn_compare_fixture() -> (Url, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/compare/{case}",
            get(|uri: Uri| async move {
                let case = uri.path().rsplit('/').next().unwrap_or_default();
                match case {
                    "delay" => {
                        tokio::time::sleep(Duration::from_millis(40)).await;
                        (StatusCode::OK, Body::from(r#"{"files":[]}"#)).into_response()
                    }
                    "503" => (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Body::from(r#"{"message":"unavailable"}"#),
                    )
                        .into_response(),
                    "auth" => (
                        StatusCode::UNAUTHORIZED,
                        Body::from(r#"{"message":"bad credentials"}"#),
                    )
                        .into_response(),
                    "files" => {
                        let files = (0..=GITHUB_COMPARE_MAX_FILES)
                            .map(|index| json!({"filename": format!("src/{index}.rs")}))
                            .collect::<Vec<_>>();
                        (StatusCode::OK, axum::Json(json!({"files": files}))).into_response()
                    }
                    _ => StatusCode::NOT_FOUND.into_response(),
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind compare fixture");
        let address = listener.local_addr().expect("fixture address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve compare fixture");
        });
        (
            Url::parse(&format!("http://{address}/")).expect("fixture URL"),
            task,
        )
    }

    #[tokio::test]
    async fn compare_budget_times_out_delayed_work() {
        let mut budget = CompareBudget::new(Duration::from_millis(10), 2);
        let result = budget
            .run(tokio::time::sleep(Duration::from_millis(50)))
            .await;
        assert_eq!(result, Err(CompareGuardError::DeadlineExceeded));
        assert_eq!(budget.attempts(), 1);
    }

    #[tokio::test]
    async fn deterministic_compare_fixture_covers_delay_status_auth_and_file_limit() {
        let (base, fixture) = spawn_compare_fixture().await;
        let client = reqwest::Client::new();

        let mut delay_budget = CompareBudget::new(Duration::from_millis(10), 1);
        let delayed = delay_budget
            .run(
                client
                    .get(base.join("compare/delay").expect("delay URL"))
                    .send(),
            )
            .await;
        assert!(matches!(delayed, Err(CompareGuardError::DeadlineExceeded)));

        for (case, expected) in [
            ("503", CompareFailureKind::ServiceUnavailable),
            ("auth", CompareFailureKind::Authentication),
        ] {
            let response = client
                .get(base.join(&format!("compare/{case}")).expect("fixture URL"))
                .send()
                .await
                .expect("fixture response");
            let status = response.status();
            let headers = response.headers().clone();
            let body = response.text().await.expect("fixture body");
            let kind = classify_compare_http_status(status, &headers, &body);
            assert_eq!(kind, expected);
        }

        let payload: serde_json::Value = client
            .get(base.join("compare/files").expect("files URL"))
            .send()
            .await
            .expect("fixture response")
            .json()
            .await
            .expect("fixture JSON");
        let file_count = payload
            .get("files")
            .and_then(serde_json::Value::as_array)
            .expect("files array")
            .len();
        assert_eq!(
            validate_compare_file_count(file_count),
            Err(CompareGuardError::TooManyFiles {
                actual: GITHUB_COMPARE_MAX_FILES + 1,
                limit: GITHUB_COMPARE_MAX_FILES,
            })
        );
        fixture.abort();
    }

    #[test]
    fn compare_budget_stops_after_request_limit() {
        let mut budget = CompareBudget::new(Duration::from_secs(1), 2);
        assert!(budget.acquire().is_ok());
        assert!(budget.acquire().is_ok());
        assert_eq!(
            budget.acquire(),
            Err(CompareGuardError::AttemptBudgetExhausted)
        );
        assert_eq!(budget.remaining_attempts(), 0);
    }

    #[test]
    fn compare_url_is_deterministic_and_encodes_refs() {
        let base = Url::parse("http://127.0.0.1:1234/").expect("base URL");
        let url = compare_url(&base, "octo-rill/demo", "release one", "v1.2.3", "...")
            .expect("compare URL");
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:1234/repos/octo-rill/demo/compare/release%20one...v1.2.3"
        );
    }

    #[test]
    fn compare_http_failures_keep_auth_and_503_distinct() {
        let headers = HeaderMap::new();
        assert_eq!(
            classify_compare_http_status(StatusCode::UNAUTHORIZED, &headers, "bad credentials"),
            CompareFailureKind::Authentication
        );
        assert_eq!(
            classify_compare_http_status(StatusCode::SERVICE_UNAVAILABLE, &headers, "retry"),
            CompareFailureKind::ServiceUnavailable
        );
        assert_eq!(
            classify_compare_http_status(StatusCode::FORBIDDEN, &headers, "policy denied"),
            CompareFailureKind::Forbidden
        );
    }

    #[test]
    fn compare_http_rate_limit_and_file_limits_are_bounded() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        assert_eq!(
            classify_compare_http_status(StatusCode::FORBIDDEN, &headers, ""),
            CompareFailureKind::RateLimited
        );
        assert!(!compare_file_limit_exceeded(GITHUB_COMPARE_MAX_FILES));
        assert!(compare_file_limit_exceeded(GITHUB_COMPARE_MAX_FILES + 1));
        assert_eq!(
            validate_compare_file_count(GITHUB_COMPARE_MAX_FILES + 1),
            Err(CompareGuardError::TooManyFiles {
                actual: GITHUB_COMPARE_MAX_FILES + 1,
                limit: GITHUB_COMPARE_MAX_FILES,
            })
        );
    }

    #[test]
    fn only_the_exact_evidence_unavailable_code_is_fail_open_eligible() {
        assert!(is_exact_release_reading_evidence_unavailable(
            RELEASE_READING_EVIDENCE_UNAVAILABLE
        ));
        assert!(!is_exact_release_reading_evidence_unavailable(
            "github_compare_unavailable"
        ));
        assert!(!is_exact_release_reading_evidence_unavailable(
            "release_reading_evidence_unavailable: wrapped"
        ));
    }
}
