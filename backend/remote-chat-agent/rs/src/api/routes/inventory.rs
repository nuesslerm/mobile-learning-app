//! Which agent CLIs and models the service offers (`/api/agent-clis`, `/api/models/preferences`, `/api/models/recent`) and the
//! opencode proxies behind the pickers (`/api/mcp`, `/api/skills`, `/api/commands`, `/api/models`), from `server.ts`.

use super::{respond, Call, Req};
use crate::api::app::App;
use rca_service_core::application::environment::Environment;
use rca_service_core::domain::clis;
use remote_chat_agent_core::domain::mcp::{parse_mcp_map, parse_toggle};
use rca_service_core::application::opencode_client::FetchError;
use rca_service_core::domain::util::encode_uri_component;
use axum::extract::{Path, State};
use axum::response::Response;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;

fn clis_listed(env: &dyn Environment) -> Value {
    let selected = clis::selected(env.settings().agent_cli.as_deref());
    let data: Vec<Value> = clis::ALL.iter().map(|(id, description)| json!({ "id": id, "description": description, "available": clis::is_available(id), "selected": *id == selected })).collect();
    json!({ "data": data })
}

pub async fn agent_clis(State(app): State<Arc<App>>) -> Response {
    respond(200, &clis_listed(app.env.as_ref()))
}


pub async fn preferences(State(app): State<Arc<App>>) -> Response {
    let settings = app.env.settings();
    respond(
        200,
        &json!({
            "text": settings.opencode_text_model,
            "vision": settings.opencode_vision_model,
            "complex": settings.opencode_complex_model,
        }),
    )
}

/// The one `directory` query value, the way Express reads `req.query.directory`: a repeated or bracketed key is an array or an
/// object there, which the route refuses like a missing one.
fn directory(req: &Req) -> Option<String> {
    let mut found = None;
    for (key, value) in req.query_pairs() {
        if key == "directory" {
            if found.is_some() {
                return None;
            }
            found = Some(value.into_owned());
        } else if key.starts_with("directory[") {
            return None;
        }
    }
    found.filter(|d| !d.is_empty())
}

fn missing_directory() -> Response {
    respond(400, &json!({ "error": "Missing required query parameter: directory" }))
}

fn bad_gateway(error: &FetchError) -> Response {
    respond(502, &json!({ "error": error.to_string() }))
}

async fn listed(app: &App, path: &str, directory: &str) -> Result<Value, FetchError> {
    app.opencode.fetch_json("GET", &format!("{path}?directory={}", encode_uri_component(directory)), None).await
}

pub async fn mcp_list(Call { app, req, .. }: Call) -> Response {
    let Some(dir) = directory(&req) else { return missing_directory() };
    match listed(&app, "/mcp", &dir).await {
        Ok(reply) => match parse_mcp_map(&reply) {
            Ok(servers) => respond(200, &json!(servers)),
            Err(e) => unexpected_mcp_reply("list", &e),
        },
        Err(e) => bad_gateway(&e),
    }
}

fn unexpected_mcp_reply(call: &str, reason: &str) -> Response {
    rca_service_core::application::log::warn!("mcp.reply.unexpected", call = call, reason = reason);
    respond(502, &json!({ "error": "unexpected_mcp_reply" }))
}

/// `connect` or `disconnect`. The name is one path segment and is encoded again on the way to opencode, so a `%2F` in it cannot
/// address another opencode route.
pub async fn mcp_connect(Path(name): Path<String>, Call { app, .. }: Call) -> Response {
    mcp_toggle(&app, &name, "connect").await
}

pub async fn mcp_disconnect(Path(name): Path<String>, Call { app, .. }: Call) -> Response {
    mcp_toggle(&app, &name, "disconnect").await
}

async fn mcp_toggle(app: &App, name: &str, action: &str) -> Response {
    match app.opencode.fetch_json("POST", &format!("/mcp/{}/{action}", encode_uri_component(name)), None).await {
        Ok(reply) => match parse_toggle(&reply) {
            Ok(ok) => respond(200, &json!({ "ok": ok })),
            Err(e) => unexpected_mcp_reply(action, &e),
        },
        Err(e) => bad_gateway(&e),
    }
}

fn text<'a>(command: &'a Value, field: &str) -> &'a str {
    command.get(field).and_then(Value::as_str).unwrap_or_default()
}

async fn commands_of(app: &App, req: &Req) -> Result<Vec<Value>, Response> {
    let dir = directory(req).ok_or_else(missing_directory)?;
    match listed(app, "/command", &dir).await {
        Ok(Value::Array(all)) => Ok(all),
        Ok(_) => Err(respond(502, &json!({ "error": "TypeError: all.filter is not a function" }))),
        Err(e) => Err(bad_gateway(&e)),
    }
}

pub async fn skills(Call { app, req, .. }: Call) -> Response {
    match commands_of(&app, &req).await {
        Ok(all) => {
            let data: Vec<Value> = all
                .iter()
                .filter(|c| text(c, "source") == "skill")
                .map(|c| json!({ "name": c.get("name").cloned().unwrap_or(Value::Null), "description": text(c, "description"), "template": text(c, "template") }))
                .collect();
            respond(200, &json!({ "data": data }))
        }
        Err(response) => response,
    }
}

pub async fn commands(Call { app, req, .. }: Call) -> Response {
    match commands_of(&app, &req).await {
        Ok(all) => {
            let data: Vec<Value> = all
                .iter()
                .filter(|c| text(c, "source") == "command" && !text(c, "template").is_empty() && !text(c, "description").is_empty())
                .map(|c| json!({ "name": c.get("name").cloned().unwrap_or(Value::Null), "description": text(c, "description") }))
                .collect();
            respond(200, &json!({ "data": data }))
        }
        Err(response) => response,
    }
}

pub async fn models(Call { app, req, .. }: Call) -> Response {
    let Some(dir) = directory(&req) else { return missing_directory() };
    match listed(&app, "/api/model", &dir).await {
        Ok(data) => respond(200, &data),
        Err(e) => bad_gateway(&e),
    }
}

const RECENT_SQL: &str = "SELECT
          json_extract(model, '$.providerID') as providerID,
          json_extract(model, '$.id') as modelID,
          count(*) as cnt
        FROM session
        WHERE model IS NOT NULL AND model != ''
          AND json_extract(model, '$.id') IS NOT NULL
        GROUP BY providerID, modelID
        ORDER BY cnt DESC
        LIMIT 10";

fn recent_failure(message: String) -> Response {
    respond(502, &json!({ "error": message, "data": [] }))
}

/// The ten most used models of opencode's own session history, named from the live model list when it answers.
pub async fn models_recent(Call { app, .. }: Call) -> Response {
    let path = app.env.settings().opencode_db_path.unwrap_or_else(|| format!("{}/.local/share/opencode/opencode.db", app.env.home_dir().to_string_lossy()));
    let raw = match app.commands.run("sqlite3", &[&path, RECENT_SQL], None, Some(Duration::from_secs(3))).await {
        Ok(out) => out.stdout.trim().to_string(),
        Err(e) => return recent_failure(format!("Error: {}", e.message)),
    };
    if raw.is_empty() {
        return respond(200, &json!({ "data": [] }));
    }
    let rows: Vec<(String, String, Value)> = raw
        .split('\n')
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() < 3 || parts[0].is_empty() || parts[1].is_empty() {
                return None;
            }
            Some((parts[0].to_string(), parts[1].to_string(), parts[2].trim().parse::<i64>().map_or(Value::Null, |n| json!(n))))
        })
        .collect();

    let cwd = app.env.settings().opencode_cwd.unwrap_or_else(|| app.env.current_dir_or(""));
    let mut names: Map<String, Value> = Map::new();
    if let Ok(models) = listed(&app, "/api/model", &cwd).await {
        for m in models.get("data").and_then(Value::as_array).into_iter().flatten() {
            if let (Some(id), Some(name)) = (m.get("id").and_then(Value::as_str), m.get("name")) {
                names.insert(id.to_string(), name.clone());
            }
        }
    }
    let data: Vec<Value> = rows
        .iter()
        .map(|(provider, id, count)| json!({ "id": id, "providerID": provider, "name": names.get(id).cloned().unwrap_or_else(|| json!(id)), "count": count }))
        .collect();
    respond(200, &json!({ "data": data }))
}
