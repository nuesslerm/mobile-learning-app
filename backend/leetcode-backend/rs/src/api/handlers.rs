use super::auth::Admin;
use super::error::ApiError;
use super::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use leetcode_backend_core::application::requests::{FormatRequest, RunRequest, SessionRequest, SubmitRequest, TestRequest};
use serde_json::{json, Value};

type Reply = Result<Json<Value>, ApiError>;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "status": "ok", "commit": &*state.commit }))
}

pub async fn user_status(State(state): State<AppState>) -> Reply {
    Ok(Json(state.service.user_status().await?))
}

pub async fn login_session(State(state): State<AppState>) -> Json<Value> {
    let until = state.service.login_capture().active_until;
    Json(json!({ "active": until.is_some(), "expiresAt": until }))
}

pub async fn start_login(_admin: Admin, State(state): State<AppState>) -> Response {
    match state.service.start_login_capture() {
        (true, expires_at) => Json(json!({ "started": true, "expiresAt": expires_at })).into_response(),
        (false, expires_at) => (StatusCode::CONFLICT, Json(json!({ "error": "capture_already_active", "expiresAt": expires_at }))).into_response(),
    }
}

pub async fn test(State(state): State<AppState>, Json(request): Json<TestRequest>) -> Reply {
    Ok(Json(state.service.test(&request).await?))
}

pub async fn submit(State(state): State<AppState>, Json(request): Json<SubmitRequest>) -> Reply {
    Ok(Json(state.service.submit(&request).await?))
}

pub async fn poll(State(state): State<AppState>, Path(id): Path<String>) -> Reply {
    Ok(Json(state.service.poll(&id).await?))
}

pub async fn run(State(state): State<AppState>, Json(request): Json<RunRequest>) -> Reply {
    Ok(Json(state.service.run(&request).await?))
}

pub async fn format(State(state): State<AppState>, Json(request): Json<FormatRequest>) -> Reply {
    Ok(Json(state.service.format(&request).await?))
}

pub async fn set_session(_admin: Admin, State(state): State<AppState>, Json(request): Json<SessionRequest>) -> Reply {
    state.service.set_session(request.cookie.as_str()).await?;
    Ok(Json(json!({ "ok": true })))
}

/// A path under `/api/leetcode` that no route has: refused for a caller without the admin secret before it is told 404, so the
/// admin surface is not probed.
pub async fn admin_then_not_found(_admin: Admin) -> ApiError {
    ApiError::NotFound
}
