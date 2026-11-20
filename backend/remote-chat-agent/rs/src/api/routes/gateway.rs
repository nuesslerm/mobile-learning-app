//! `/gateway/{provider}/...`: the model gateway the agent CLIs at the workspace host call with the token of their turn. The token is the only
//! credential taken (as `x-api-key` or as a bearer): neither the service secret nor a device bearer opens it, and the token opens nothing else.
//! The answer streams as the provider sends it; a provider that fails ends the response as a failure, never as a complete one.
use super::{respond, Req};
use crate::api::app::App;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::response::Response;
use futures_util::StreamExt;
use rca_group_gateway::{Inbound, Reply};
use serde_json::json;
use std::sync::Arc;

fn refusal(status: u16, code: &str, message: &str) -> Response {
    respond(status, &json!({ "type": "error", "error": { "type": "gateway_error", "code": code, "message": message } }))
}

pub async fn forward(State(app): State<Arc<App>>, Path((provider, rest)): Path<(String, String)>, req: Req, body: axum::body::Bytes) -> Response {
    let token = req.header("x-api-key").or_else(|| rca_agent_core::bearer_token(req.header("authorization")));
    let headers = req.headers.iter().filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string()))).collect();
    let inbound = Inbound { method: req.method.to_string(), path: format!("/{rest}"), query: Some(req.query.clone()).filter(|q| !q.is_empty()), headers, body: body.to_vec() };
    match app.model_gateway.handle(&provider, token, inbound).await {
        Reply::Refused(why) => {
            tracing::warn!(event = "gateway.refused", provider = %provider, code = why.code, status = why.status);
            refusal(why.status, why.code, &why.message)
        }
        Reply::Answer(answer) => {
            let provider = provider.clone();
            let mut response = Response::new(Body::from_stream(answer.body.map(move |chunk| chunk.map_err(|why| {
                tracing::warn!(event = "gateway.stream_failed", provider = %provider, reason = %why);
                std::io::Error::other(why)
            }))));
            *response.status_mut() = axum::http::StatusCode::from_u16(answer.status).unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
            for (name, value) in answer.headers {
                if let (Ok(name), Ok(value)) = (axum::http::HeaderName::from_bytes(name.as_bytes()), axum::http::HeaderValue::from_str(&value)) {
                    response.headers_mut().append(name, value);
                }
            }
            response
        }
    }
}
