use super::error::ApiError;
use super::AppState;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use secrecy::ExposeSecret;
use subtle::ConstantTimeEq;

/// Everything under `/api/leetcode` needs `Authorization: Bearer <seed>`.
pub async fn require_bearer(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let expected = format!("Bearer {}", state.bearer.expose_secret());
    let provided = request.headers().get(AUTHORIZATION).map(|value| value.as_bytes()).unwrap_or_default();
    if bool::from(expected.as_bytes().ct_eq(provided)) {
        next.run(request).await
    } else {
        ApiError::Unauthorized.into_response()
    }
}

/// Proof of `X-Admin-Secret`; list it before the body extractor so a caller without it is refused before its body is read.
pub struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let provided = parts.headers.get("x-admin-secret").map(|value| value.as_bytes()).unwrap_or_default();
        if !provided.is_empty() && bool::from(provided.ct_eq(state.admin.expose_secret().as_bytes())) {
            Ok(Self)
        } else {
            Err(ApiError::Unauthorized)
        }
    }
}
