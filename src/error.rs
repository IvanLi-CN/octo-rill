use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    failure_class: Option<&'static str>,
    details: Option<Value>,
    retry_after_seconds: Option<u64>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            failure_class: None,
            details: None,
            retry_after_seconds: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn with_retry_after(mut self, seconds: u64) -> Self {
        self.retry_after_seconds = Some(seconds);
        self
    }

    pub fn from_llm_error(err: anyhow::Error) -> Self {
        if let Some(class) = crate::ai::llm_failure_class(&err) {
            return Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "llm_error",
                message: class.safe_message().to_owned(),
                failure_class: Some(class.as_str()),
                details: None,
                retry_after_seconds: None,
            };
        }
        Self::internal(err)
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    #[cfg(test)]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn failure_class(&self) -> Option<&'static str> {
        self.failure_class
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn internal(err: impl std::fmt::Display + std::fmt::Debug) -> Self {
        let error_chain = format!("{err:?}");
        let error_chain_lower = error_chain.to_ascii_lowercase();
        let display_lower = err.to_string().to_ascii_lowercase();
        let is_write_deadline = display_lower
            .starts_with("retryable sqlite write deadline exceeded")
            || error_chain_lower.contains("retryable sqlite write deadline exceeded")
            || ((error_chain_lower.contains("code: \"9\"")
                || error_chain_lower.contains("code: 9"))
                && error_chain_lower.contains("interrupted"));
        let is_write_busy = error_chain_lower.contains("database is locked")
            || error_chain_lower.contains("database table is locked")
            || error_chain_lower.contains("sqlite_busy")
            || error_chain_lower.contains("sqlite busy");
        let is_session_conflict = error_chain_lower.contains("retryable sqlite session conflict");
        if is_write_deadline || is_write_busy || is_session_conflict {
            return Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "sqlite_write_retryable",
                "SQLite write capacity is busy; retry the request.",
            )
            .with_retry_after(1);
        }

        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            err.to_string(),
        )
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({
            "ok": false,
            "error": {
                "code": self.code,
                "message": self.message,
                "failure_class": self.failure_class,
            },
        });
        if let Some(details) = self.details.and_then(|value| value.as_object().cloned())
            && let Some(body_object) = body.as_object_mut()
        {
            for (key, value) in details {
                body_object.insert(key, value);
            }
        }
        let mut response = (self.status, Json(body)).into_response();
        if let Some(seconds) = self.retry_after_seconds
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_write_deadline_maps_to_retryable_service_unavailable() {
        let error = crate::sqlite_write::SqliteWriteDeadlineError {
            lane: "test",
            priority: "foreground",
            phase: "writer_queue",
            deadline_ms: 900,
        };
        let response = ApiError::internal(error).into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "1");
    }

    #[test]
    fn ordinary_internal_error_remains_internal_server_error() {
        let response = ApiError::internal(anyhow::anyhow!("database unavailable")).into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn sqlite_interruption_maps_to_retryable_service_unavailable() {
        let response = ApiError::internal(anyhow::anyhow!(
            "error returned from database: (code: 9) interrupted"
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn sqlite_busy_maps_to_retryable_service_unavailable() {
        let response = ApiError::internal(anyhow::anyhow!(
            "failed to insert job task: error returned from database: (code: 5) database is locked"
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "1");
    }

    #[test]
    fn sqlite_session_conflict_maps_to_retryable_service_unavailable() {
        let response = ApiError::internal(anyhow::anyhow!(
            "retryable sqlite session conflict: request baseline unavailable"
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "1");
    }
}
