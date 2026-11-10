//! `/agent/v1/bearers`: the device bearers of the agent API socket, minted, listed and revoked by an operator who holds the service secret. A device
//! bearer opens none of these: each answers 401 to anything but the secret. The token is in the answer to the mint and nowhere else.

use super::{respond, Call};
use crate::api::auth::{admin_authorised, unauthorized};
use axum::extract::Path;
use axum::response::Response;
use rca_agent_core::Bearer;
use serde_json::{json, Value};

fn shown(bearer: &Bearer) -> Value {
    json!({ "id": bearer.id, "label": bearer.label, "createdAt": bearer.created_at })
}

fn unavailable(why: &str) -> Response {
    respond(500, &json!({ "error": why, "code": "BEARERS_UNAVAILABLE" }))
}

/// `POST /agent/v1/bearers` `{label}`: a new bearer, and its token (shown once).
pub async fn mint(Call { app, req, body }: Call) -> Response {
    if !admin_authorised(&app, &req) {
        return unauthorized();
    }
    let label = body.as_ref().and_then(|b| b["label"].as_str()).map(str::trim).filter(|l| !l.is_empty());
    let Some(label) = label else { return respond(400, &json!({ "error": "label is required", "code": "INVALID_REQUEST" })) };
    match app.bearers.mint(label) {
        Ok(minted) => {
            tracing::info!(event = "device_bearer.minted", bearer = %minted.bearer.id, label = %minted.bearer.label);
            let mut answer = shown(&minted.bearer);
            answer["token"] = json!(minted.token);
            respond(201, &answer)
        }
        Err(why) => unavailable(&why),
    }
}

/// `GET /agent/v1/bearers`: every bearer that was not revoked, never a token.
pub async fn list(Call { app, req, .. }: Call) -> Response {
    if !admin_authorised(&app, &req) {
        return unauthorized();
    }
    match app.bearers.list() {
        Ok(bearers) => respond(200, &json!({ "bearers": bearers.iter().map(shown).collect::<Vec<_>>() })),
        Err(why) => unavailable(&why),
    }
}

/// `DELETE /agent/v1/bearers/{id}`: the bearer's token opens the socket no longer, from the next connection on.
pub async fn revoke(Path(id): Path<String>, Call { app, req, .. }: Call) -> Response {
    if !admin_authorised(&app, &req) {
        return unauthorized();
    }
    match app.bearers.revoke(&id) {
        Ok(true) => {
            tracing::info!(event = "device_bearer.revoked", bearer = %id);
            respond(200, &json!({ "revoked": id }))
        }
        Ok(false) => respond(404, &json!({ "error": format!("no bearer has the id {id}"), "code": "NOT_FOUND" })),
        Err(why) => unavailable(&why),
    }
}
