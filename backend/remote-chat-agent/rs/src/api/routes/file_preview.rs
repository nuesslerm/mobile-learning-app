//! `/api/file/*path`: an annotatable HTML preview of a file under `FORK_DIR` (markdown rendered, anything else as a code block),
//! and `/api/file/*path/annotate`, which forwards the reader's annotations to Telegram (`routes/filePreview.ts`).

use super::telegram::send_telegram;
use super::{respond, text, truthy, Call};
use remote_chat_agent_render::md::md_to_html;
use rca_service_core::application::environment::Environment;
use rca_service_core::domain::util::{escape_html, js_string, script_json};
use axum::extract::Path as UrlPath;
use axum::response::Response;
use remote_chat_agent_core::application::ports::WorkspaceFiles;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const PAGE: &str = include_str!("../../../assets/file_page.html");

fn fork_dir(env: &dyn Environment) -> String {
    env.settings().fork_dir.unwrap_or_else(|| env.current_dir_or("/"))
}

/// The path under `FORK_DIR`, or None when it climbs out of it; symlinks that lead outside count as outside.
fn resolve_safe(env: &dyn Environment, files: &dyn WorkspaceFiles, rel: &str) -> Option<PathBuf> {
    let root = env.resolve(&fork_dir(env));
    let abs = env.resolve(&format!("{}/{rel}", root.to_string_lossy()));
    if !abs.starts_with(&root) {
        return None;
    }
    match (files.canonical(&abs), files.canonical(&root)) {
        (Some(real), Some(real_root)) if !real.starts_with(&real_root) => None,
        _ => Some(abs),
    }
}

fn html(status: u16, body: impl Into<Vec<u8>>) -> Response {
    text(status, "text/html; charset=utf-8", body)
}

pub async fn get(UrlPath(rel): UrlPath<String>, Call { app, req, .. }: Call) -> Response {
    let rel = rel.as_str();
    let Some(abs) = resolve_safe(app.env.as_ref(), app.files.as_ref(), rel) else { return html(403, "Forbidden") };
    if !app.files.exists(&abs) {
        return html(404, format!("File not found: {rel}"));
    }
    let Some(bytes) = app.files.read(&abs) else { return html(500, "Could not read file") };
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let ext = Path::new(&abs).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let body = if ext == "md" { md_to_html(&content) } else { format!("<pre><code class=\"language-{}\">{}</code></pre>", escape_html(&ext), escape_html(&content)) };
    let chat = req.query_param("chatId").unwrap_or_default();
    let thread = req.query_param("threadId").unwrap_or_default();
    let page = PAGE
        .replace("{{PATH_JSON}}", &script_json(&json!(rel)))
        .replace("{{CHAT_JSON}}", &script_json(&json!(chat)))
        .replace("{{THREAD_JSON}}", &script_json(&json!(thread)))
        .replace("{{DISABLED}}", if chat.is_empty() { "disabled" } else { "" })
        .replace("{{SEND_LABEL}}", if chat.is_empty() { "(no chat context)" } else { "📤 Send to Telegram" })
        .replace("{{REL}}", &escape_html(rel))
        .replace("{{BODY}}", &body);
    html(200, page)
}

/// `rel` is everything after `/api/file/`; only `<path>/annotate` is a route.
pub async fn annotate(UrlPath(rel): UrlPath<String>, Call { app, body, .. }: Call) -> Response {
    let rel = rel.as_str();
    let Some(file_path) = rel.strip_suffix("/annotate") else { return respond(404, &json!({ "error": "Not found" })) };
    let list: Vec<Value> = body.as_ref().and_then(|b| b.get("annotations")).and_then(Value::as_array).cloned().unwrap_or_default();
    if list.is_empty() {
        return respond(400, &json!({ "error": "No annotations" }));
    }
    if app.env.settings().telegram_bot_token.is_empty() {
        return respond(500, &json!({ "error": "TELEGRAM_BOT_TOKEN not configured — cannot forward to Telegram" }));
    }
    let field = |a: &Value, k: &str| a.get(k).map(js_string).unwrap_or_default();
    let formatted: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(i, a)| format!("{}. <blockquote>{}</blockquote>\n💬 {}", i + 1, escape_html(&field(a, "selection").chars().take(150).collect::<String>()), escape_html(&field(a, "comment"))))
        .collect();
    let text = format!("📝 <b>Annotations on {}</b>\n\n{}", escape_html(file_path), formatted.join("\n\n"));
    let chat = body.as_ref().and_then(|b| b.get("chatId")).map(js_string).unwrap_or_else(|| "undefined".into());
    let mut params = json!({ "chat_id": chat, "text": text, "parse_mode": "HTML" });
    if let Some(thread) = body.as_ref().and_then(|b| b.get("threadId")).filter(|v| truthy(v)) {
        params["message_thread_id"] = json!(js_string(thread));
    }
    match send_telegram(app.outbound.as_ref(), &app.env.settings().telegram_bot_token, &params).await {
        Ok(()) => respond(200, &json!({ "ok": true })),
        Err(message) => respond(502, &json!({ "error": message })),
    }
}
