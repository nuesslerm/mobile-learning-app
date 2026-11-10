//! Session control: stop, queue and steer a running turn (`routes/control.ts`). `sessionId` is the thread id the chat stream
//! reported. A thread the thread view's engine owns is driven through it; any other, a chat thread or an opencode session this
//! service never ran, goes straight to opencode, by the opencode session id the thread maps to when there is one.

use super::{respond, truthy, Call};
use crate::api::app::App;
use remote_chat_agent_core::domain::types::*;
use rca_service_core::domain::util::{js_string, uuid_v4};
use axum::response::Response;
use serde_json::{json, Value};

fn field<'a>(body: &'a Option<Value>, name: &str) -> Option<&'a Value> {
    body.as_ref().and_then(|b| b.get(name)).filter(|v| truthy(v))
}

/// opencode's own id for a thread the service ran, else the id as given (a session this service never saw).
fn opencode_session(app: &App, id: &str) -> String {
    app.orch.backend("opencode").and_then(|b| b.session_for(id)).unwrap_or_else(|| id.to_string())
}

fn failure(message: String) -> Response {
    respond(500, &json!({ "error": message }))
}

async fn interrupt(app: &App, thread_id: &str, tag: &str) -> Result<Receipt, String> {
    let command = Command { command_id: format!("{tag}:{}", uuid_v4()), thread_id: thread_id.into(), body: Body::TurnInterrupt { turn_id: None, created_at: rca_service_core::domain::util::iso_now() } };
    app.orch.dispatch_awaiting(command, "client").await
}

/// A queued or steering message lands on the thread the engine has and continues its runtime and interaction profile.
fn start_turn(app: &App, thread_id: &str, message: &str, tag: &str) -> Result<Receipt, String> {
    let thread = app.orch.engine.get_thread(thread_id).ok().flatten();
    let command = Command {
        command_id: format!("{tag}:{}", uuid_v4()),
        thread_id: thread_id.into(),
        body: Body::TurnStart {
            message_id: format!("msg_{}", uuid_v4()),
            text: message.into(),
            attachments: None,
            model_selection: thread.as_ref().map(|t| t.model_selection.clone()),
            runtime_mode: thread.as_ref().map_or_else(|| "full-access".into(), |t| t.runtime_mode.clone()),
            interaction_mode: thread.as_ref().map_or_else(|| "default".into(), |t| t.interaction_mode.clone()),
            source_proposed_plan: None,
            bootstrap: None,
        },
    };
    app.orch.engine.dispatch(command, "client")
}

pub async fn stop(Call { app, body, .. }: Call) -> Response {
    let (Some(session), Some(directory)) = (field(&body, "sessionId"), field(&body, "directory")) else {
        return respond(400, &json!({ "error": "Missing sessionId or directory" }));
    };
    let (id, directory) = (js_string(session), js_string(directory));
    let receipt = match interrupt(&app, &id, "control-stop").await {
        Ok(receipt) => receipt,
        Err(e) => return failure(e),
    };
    if !receipt.accepted {
        if let Err(e) = app.gateway.session_abort(&opencode_session(&app, &id), &directory).await {
            return failure(e);
        }
    }
    respond(200, &json!({ "ok": true, "action": "stop", "sessionId": session }))
}

pub async fn queue(Call { app, body, .. }: Call) -> Response {
    deliver(&app, body, "queue", "deferred").await
}

pub async fn steer(Call { app, body, .. }: Call) -> Response {
    deliver(&app, body, "steer", "immediate").await
}

async fn deliver(app: &App, body: Option<Value>, action: &str, delivery: &str) -> Response {
    let (Some(session), Some(directory), Some(message)) = (field(&body, "sessionId"), field(&body, "directory"), field(&body, "message")) else {
        return respond(400, &json!({ "error": "Missing sessionId, directory, or message" }));
    };
    let (id, directory, text) = (js_string(session), js_string(directory), js_string(message));
    if action == "steer" {
        // Best effort: a refused interrupt (nothing running) still lets the steering turn start.
        if let Err(e) = interrupt(app, &id, "control-steer-interrupt").await {
            return failure(e);
        }
    }
    let receipt = match start_turn(app, &id, &text, &format!("control-{action}")) {
        Ok(receipt) => receipt,
        Err(e) => return failure(e),
    };
    if !receipt.accepted {
        if let Err(e) = app.gateway.session_prompt(&opencode_session(app, &id), &directory, &text, delivery).await {
            return failure(e);
        }
    }
    respond(200, &json!({ "ok": true, "action": action, "sessionId": session, "message": message }))
}
