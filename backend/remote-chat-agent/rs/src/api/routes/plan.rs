//! `/api/plan/:id`: a plan's clean URL (no dots, so Telegram's auto-linker keeps it whole) that redirects to the file preview of
//! `.bot-state/plans/<id>.md` under `FORK_DIR`, and `/api/plan/:id/annotate`, which forwards the reader's notes to Telegram
//! (`routes/plan.ts`).

use super::telegram::send_telegram;
use super::{respond, text, Call, ResponseExt};
use rca_service_core::application::environment::Environment;
use rca_service_core::domain::util::{encode_url, escape_html, js_string};
use axum::extract::Path;
use axum::response::Response;
use serde_json::{json, Value};

fn fork_dir(env: &dyn Environment) -> String {
    env.settings().fork_dir.unwrap_or_else(|| env.current_dir_or("/"))
}

fn plain(status: u16, body: impl Into<Vec<u8>>) -> Response {
    text(status, "text/html; charset=utf-8", body)
}

pub async fn get(Path(id): Path<String>, Call { app, req, .. }: Call) -> Response {
    let id = id.as_str();
    let root = app.env.resolve(&fork_dir(app.env.as_ref()));
    let file = app.env.resolve(&format!("{}/.bot-state/plans/{id}.md", root.to_string_lossy()));
    if !file.starts_with(&root) || file == root {
        return plain(403, "Forbidden");
    }
    if !file.exists() {
        return plain(404, format!("Plan {id} not found"));
    }
    let chat = req.query_param("chatId").unwrap_or_default();
    let thread = req.query_param("threadId").unwrap_or_default();
    let location = encode_url(&format!("/api/file/.bot-state/plans/{id}.md?chatId={chat}&threadId={thread}"));
    text(302, "text/plain; charset=utf-8", format!("Found. Redirecting to {location}")).with_header("Location", &location)
}

pub async fn annotate(Path(id): Path<String>, Call { app, body, .. }: Call) -> Response {
    let id = id.as_str();
    let field = |name: &str| body.as_ref().and_then(|b| b.get(name));
    let list: Vec<Value> = field("annotations").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.is_empty() {
        return respond(400, &json!({ "error": "No annotations" }));
    }
    if app.env.settings().telegram_bot_token.is_empty() {
        return respond(500, &json!({ "error": "TELEGRAM_BOT_TOKEN not configured" }));
    }
    let part = |a: &Value, k: &str| a.get(k).map(js_string).unwrap_or_default();
    let formatted: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(i, a)| format!("{}. <blockquote>{}</blockquote>\n💬 {}", i + 1, escape_html(&part(a, "selection").chars().take(150).collect::<String>()), escape_html(&part(a, "comment"))))
        .collect();
    let text = format!("📝 <b>Plan annotations (plan {id})</b>\n\n{}", formatted.join("\n\n"));
    let mut params = json!({ "chat_id": field("chatId").map(js_string).unwrap_or_else(|| "undefined".into()), "text": text, "parse_mode": "HTML" });
    if let Some(thread) = field("threadId").filter(|t| super::truthy(t)) {
        params["message_thread_id"] = json!(js_string(thread));
    }
    match send_telegram(app.outbound.as_ref(), &app.env.settings().telegram_bot_token, &params).await {
        Ok(()) => respond(200, &json!({ "ok": true })),
        Err(message) => respond(502, &json!({ "error": message })),
    }
}
