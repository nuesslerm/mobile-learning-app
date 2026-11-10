//! `POST /api/question/:requestId/reply`: the answer to a question or an approval, from the bot to the agent (`routes/question.ts`).
//! The thread view's engine is asked first: a thread in the same directory with that request open is answered through it. A
//! request it does not hold goes to opencode as the answers matrix. 204 on success.

use super::{empty, respond, Call};
use crate::api::app::App;
use remote_chat_agent_core::domain::types::*;
use rca_service_core::domain::util::{js_string, uuid_v4};
use axum::extract::Path;
use axum::response::Response;
use serde_json::{json, Map, Value};

struct Open {
    approval: bool,
    headers: Vec<String>,
}

/// The last activity that mentions the request decides whether it is open, and what kind it is.
fn open_request(thread: &Thread, request_id: &str) -> Option<Open> {
    let mut state = None;
    for activity in &thread.activities {
        if activity.payload.get("requestId").and_then(Value::as_str) != Some(request_id) {
            continue;
        }
        match activity.kind.as_str() {
            "approval.requested" => state = Some(Open { approval: true, headers: Vec::new() }),
            "user-input.requested" => {
                let headers = activity.payload.get("questions").and_then(Value::as_array).map(|qs| qs.iter().map(|q| q.get("header").map(js_string).unwrap_or_default()).collect()).unwrap_or_default();
                state = Some(Open { approval: false, headers });
            }
            "approval.resolved" | "user-input.resolved" => state = None,
            _ => {}
        }
    }
    state
}

fn find(app: &App, directory: &str, request_id: &str) -> Result<Option<(String, Open)>, String> {
    for summary in app.orch.engine.list_threads() {
        if summary.cwd != directory {
            continue;
        }
        let Some(thread) = app.orch.engine.get_thread(&summary.id)? else { continue };
        if let Some(open) = open_request(&thread, request_id) {
            return Ok(Some((summary.id, open)));
        }
    }
    Ok(None)
}

/// A keyboard label that is not a decision token still unblocks the request by accepting.
fn decision(answers: &[Value]) -> &'static str {
    match answers.first().and_then(|a| a.get(0)).and_then(Value::as_str) {
        Some("acceptForSession") => "acceptForSession",
        Some("decline") => "decline",
        Some("cancel") => "cancel",
        _ => "accept",
    }
}

fn bad_gateway(message: String) -> Response {
    respond(502, &json!({ "error": message }))
}

pub async fn reply(Path(request_id): Path<String>, Call { app, body, .. }: Call) -> Response {
    let request_id = request_id.as_str();
    let directory = body.as_ref().and_then(|b| b.get("directory")).filter(|v| super::truthy(v)).map(js_string);
    let Some(directory) = directory else { return respond(400, &json!({ "error": "Missing directory" })) };
    let Some(answers) = body.as_ref().and_then(|b| b.get("answers")).and_then(Value::as_array) else {
        return respond(400, &json!({ "error": "answers must be an array" }));
    };

    match find(&app, &directory, request_id) {
        Err(e) => return bad_gateway(e),
        Ok(Some((thread_id, open))) => {
            let created_at = rca_service_core::domain::util::iso_now();
            let body = if open.approval {
                Body::ApprovalRespond { request_id: request_id.into(), decision: decision(answers).into(), created_at }
            } else {
                let mut by_header = Map::new();
                for (i, header) in open.headers.iter().enumerate() {
                    by_header.insert(header.clone(), answers.get(i).cloned().unwrap_or_else(|| json!([])));
                }
                Body::UserInputRespond { request_id: request_id.into(), answers: by_header, created_at }
            };
            let tag = if open.approval { "question-approval" } else { "question-user-input" };
            return match app.orch.dispatch_awaiting(Command { command_id: format!("{tag}:{}", uuid_v4()), thread_id, body }, "client").await {
                Ok(_) => empty(204),
                Err(e) => bad_gateway(e),
            };
        }
        Ok(None) => {}
    }
    match app.gateway.question_reply(request_id, &directory, &Value::Array(answers.clone())).await {
        Ok(()) => empty(204),
        Err(e) => bad_gateway(e),
    }
}
