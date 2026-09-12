use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use serde_json::json;

#[derive(Debug)]
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

    pub fn internal(err: impl std::fmt::Display) -> Self {
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
