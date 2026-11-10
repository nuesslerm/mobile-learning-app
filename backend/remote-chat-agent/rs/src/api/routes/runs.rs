//! `POST /runs`: one shot of the agent in a caller-named directory, or in a staging directory holding the `files` of the request. The same `X-Bot-Secret` gate as `/api/worktree`, failing
//! closed when no secret is configured, since a run writes into the working directory it is given.

use super::{respond, Call};
use crate::api::auth::{authorised_strict, unauthorized};
use rca_service_core::domain::schema::{parse, Cli, NonEmpty};
use rca_service_core::application::runs;
use rca_service_core::application::log;
use axum::response::Response;
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// A file of a run that names no cwd.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachedFile {
    path: NonEmpty,
    /// Text; a binary file (a screenshot) comes as `contentBase64` instead.
    content: Option<String>,
    #[serde(rename = "contentBase64")]
    content_base64: Option<String>,
}

fn staged(file: AttachedFile) -> Result<runs::AttachedFile, String> {
    use base64::Engine;
    let path: String = file.path.into();
    let content = match (file.content, file.content_base64) {
        (Some(text), None) => text.into_bytes(),
        (None, Some(encoded)) => base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| format!("file {path:?}: contentBase64 is not base64: {e}"))?,
        _ => return Err(format!("file {path:?} takes exactly one of content and contentBase64")),
    };
    Ok(runs::AttachedFile { path, content })
}

/// `RunRequestSchema`.
#[derive(Deserialize)]
struct RunRequest {
    prompt: NonEmpty,
    cwd: Option<NonEmpty>,
    files: Option<Vec<AttachedFile>>,
    shape: Option<NonEmpty>,
    models: Option<Vec<NonEmpty>>,
    cli: Option<Cli>,
    caller: Option<NonEmpty>,
    session: Option<NonEmpty>,
    agent: Option<NonEmpty>,
    variant: Option<NonEmpty>,
    title: Option<String>,
    auto: Option<bool>,
    /// Staged paths the prompt carries as file parts, as `opencode run -f`.
    attach: Option<Vec<NonEmpty>>,
    command: Option<NonEmpty>,
}

fn read(body: Option<&Value>) -> Result<RunRequest, String> {
    let body = body.unwrap_or(&Value::Null);
    parse::<Map<String, Value>>(body)?;
    parse(body)
}

pub async fn post(Call { app, req, body }: Call) -> Response {
    if !authorised_strict(&app, &req) {
        return unauthorized();
    }
    let asked = match read(body.as_ref()) {
        Ok(asked) => asked,
        Err(message) => return respond(400, &json!({ "error": message, "code": "INVALID_REQUEST" })),
    };
    let files = match asked.files.unwrap_or_default().into_iter().map(staged).collect::<Result<Vec<_>, _>>() {
        Ok(files) => files,
        Err(message) => return respond(400, &json!({ "error": message, "code": "INVALID_REQUEST" })),
    };
    let request = runs::Request {
        prompt: asked.prompt.into(),
        cwd: asked.cwd.map(String::from),
        files,
        shape: asked.shape.map(String::from),
        models: asked.models.unwrap_or_default().into_iter().map(String::from).collect(),
        cli: asked.cli.map(|cli| cli.as_str().to_string()),
        caller: asked.caller.map(String::from),
        session: asked.session.map(String::from),
        agent: asked.agent.map(String::from),
        variant: asked.variant.map(String::from),
        title: asked.title.filter(|t| !t.is_empty()),
        auto: asked.auto.unwrap_or(false),
        attach: asked.attach.unwrap_or_default().into_iter().map(String::from).collect(),
        command: asked.command.map(String::from),
    };
    let (client, commands, ledgers, fs, hosts) = (app.opencode.clone(), app.commands.clone(), app.ledgers.clone(), app.files.clone(), app.hosts.clone());
    let cwd = request.cwd.clone().unwrap_or_default();
    // The run is a blocking walk through `rca-core`'s `run_once`; it gets a thread of its own and drives the async calls it makes.
    let handle = tokio::runtime::Handle::current();
    match tokio::task::spawn_blocking(move || runs::run(&handle, &client, commands.as_ref(), ledgers.as_ref(), &fs, hosts.as_ref(), &request)).await {
        Ok(Ok(result)) => respond(200, &result),
        Ok(Err(runs::Refused::Invalid(why))) => respond(400, &json!({ "error": why, "code": "INVALID_REQUEST" })),
        Ok(Err(runs::Refused::Ledger(why))) => {
            log::error!("runs.ledger_unavailable", cwd = cwd, why = why);
            respond(503, &json!({ "error": why, "code": "LEDGER_UNAVAILABLE" }))
        }
        Err(_) => {
            log::error!("runs.failed", cwd = cwd);
            respond(500, &json!({ "error": "Internal error", "code": "INTERNAL_ERROR" }))
        }
    }
}

/// `DELETE /runs/sessions?caller=<name>`: drops every session that caller made (opencode's and the kept directories) and answers how many.
pub async fn purge(Call { app, req, .. }: Call) -> Response {
    if !authorised_strict(&app, &req) {
        return unauthorized();
    }
    let Some(caller) = req.query_param("caller").filter(|c| !c.is_empty()) else {
        return respond(400, &json!({ "error": "caller is required", "code": "INVALID_REQUEST" }));
    };
    let (client, fs, handle) = (app.opencode.clone(), app.files.clone(), tokio::runtime::Handle::current());
    match tokio::task::spawn_blocking(move || runs::purge(&handle, &client, fs.as_ref(), &caller)).await {
        Ok(dropped) => respond(200, &json!({ "dropped": dropped })),
        Err(_) => respond(500, &json!({ "error": "Internal error", "code": "INTERNAL_ERROR" })),
    }
}

/// `GET /runs/log[?limit=n]`: the run log as text, for `wf offload ask --stats` and `--savings`.
pub async fn log(Call { app, req, .. }: Call) -> Response {
    if !authorised_strict(&app, &req) {
        return unauthorized();
    }
    let limit = req.query_param("limit").and_then(|n| n.parse::<usize>().ok());
    respond(200, &json!({ "log": runs::run_log(app.files.as_ref(), &app.env.settings(), limit) }))
}

/// `POST /runs/cooldowns/clear` with `{ "pool": ... }`: warms a pool again in the shared ledger.
pub async fn clear_cooldown(Call { app, req, body }: Call) -> Response {
    if !authorised_strict(&app, &req) {
        return unauthorized();
    }
    let Some(pool) = body.as_ref().and_then(|b| b["pool"].as_str()).filter(|p| !p.is_empty()).map(str::to_string) else {
        return respond(400, &json!({ "error": "pool is required", "code": "INVALID_REQUEST" }));
    };
    let handle = tokio::runtime::Handle::current();
    let ledgers = app.ledgers.clone();
    let asked = pool.clone();
    match tokio::task::spawn_blocking(move || runs::clear_cooldown(&handle, ledgers.as_ref(), &asked)).await {
        Ok(Ok(was_cold)) => respond(200, &json!({ "pool": pool, "wasCold": was_cold })),
        Ok(Err(why)) => respond(503, &json!({ "error": why, "code": "LEDGER_UNAVAILABLE" })),
        Err(_) => respond(500, &json!({ "error": "Internal error", "code": "INTERNAL_ERROR" })),
    }
}
