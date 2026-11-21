//! Upstream GitHub endpoint selection shared by the PostgreSQL runtime.
//!
//! Production keeps the public API. Validation suites point the same seam at a
//! local stub so no test performs real GitHub requests.
const BASE_ENV: &str = "OCTORILL_GITHUB_API_BASE";
const DEFAULT_BASE: &str = "https://api.github.com";

/// REST base URL without a trailing slash.
pub(crate) fn rest_base() -> String {
    std::env::var(BASE_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_owned())
}

/// GraphQL endpoint derived from the configured REST base.
pub(crate) fn graphql_url() -> String {
    format!("{}/graphql", rest_base())
}
