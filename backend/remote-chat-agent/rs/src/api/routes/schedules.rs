//! `POST /hooks/v1/schedules/{id}/{token}`: the inbound webhook of a schedule. It is outside the service secret and the device bearers: the
//! schedule's own token, which only a digest of is kept, is the credential. A request that does not carry it is 401 whichever way it is wrong (an
//! unknown schedule, a schedule that is no webhook, a bad token), and every request, let in or not, is an event on the `schedules` stream.
use super::{respond, Req};
use crate::api::app::App;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::response::Response;
use rca_service_core::application::agent_groups::{WebhookOutcome, WebhookRequest};
use serde_json::json;
use std::sync::Arc;

/// The most of a request's body a prompt can use.
pub const BODY_LIMIT: usize = 256 * 1024;

/// Headers that are the sender's credentials for something else: they never reach a prompt.
const WITHHELD: [&str; 4] = ["authorization", "cookie", "x-bot-secret", "proxy-authorization"];

pub async fn webhook(State(app): State<Arc<App>>, Path((id, token)): Path<(String, String)>, req: Req, body: Bytes) -> Response {
    let headers = req.headers.iter().filter(|(name, _)| !WITHHELD.contains(&name.as_str())).filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string()))).collect();
    let request = WebhookRequest { body: String::from_utf8_lossy(&body).into_owned(), query: req.query, headers };
    match app.schedules.webhook(&id, &token, &request).await {
        WebhookOutcome::Accepted(run) => respond(202, &json!({ "run": run })),
        WebhookOutcome::Rejected => respond(401, &json!({ "error": "unauthorized" })),
        WebhookOutcome::Disabled => respond(409, &json!({ "error": "schedule_disabled" })),
        WebhookOutcome::Failed(refusal) => respond(if refusal.code == "TURN_RUNNING" { 409 } else { 502 }, &json!({ "error": refusal.code, "message": refusal.message })),
    }
}
