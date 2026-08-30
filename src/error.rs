use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    failure_class: Option<&'static str>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            failure_class: None,
        }
    }

    pub fn from_llm_error(err: anyhow::Error) -> Self {
        if let Some(class) = crate::ai::llm_failure_class(&err) {
            return Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "llm_error",
                message: class.safe_message().to_owned(),
                failure_class: Some(class.as_str()),
            };
        }
        Self::internal(err)
    }

    pub fn code(&self) -> &'static str {
        self.code
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
        (
            self.status,
            Json(json!({
                "ok": false,
                "error": {
                    "code": self.code,
                    "message": self.message,
                    "failure_class": self.failure_class,
                }
            })),
        )
            .into_response()
    }
}
