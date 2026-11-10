use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use leetcode_backend_core::application::AppError;
use serde_json::json;

/// What a handler can fail with, as the response the callers of this service expect: `{ "error": ... }` with the status below.
pub enum ApiError {
    App(AppError),
    Unauthorized,
    NotFound,
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

fn reply(status: StatusCode, body: serde_json::Value) -> Response {
    (status, Json(body)).into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized => reply(StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" })),
            Self::NotFound => reply(StatusCode::NOT_FOUND, json!({ "error": "not_found" })),
            Self::App(AppError::NoSession) => {
                reply(StatusCode::UNAUTHORIZED, json!({ "error": "no_session", "message": "Run a login-capture session first" }))
            }
            Self::App(AppError::InvalidCookie(message)) => reply(StatusCode::BAD_REQUEST, json!({ "error": message })),
            Self::App(AppError::Store(message)) => reply(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": message })),
            Self::App(AppError::Upstream(message)) => reply(StatusCode::BAD_GATEWAY, json!({ "error": message })),
        }
    }
}
