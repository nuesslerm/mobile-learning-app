//! The request methods of the socket (`ws/methods.ts`): each checks its params against its schema (the issues are the error's message),
//! then asks the orchestration engine, or opencode for its models.

use super::Context;
use remote_chat_agent_core::domain::types::Body;
use rca_service_core::domain::clis;
use crate::api::wire::parse_command;
use rca_service_core::domain::schema::{parse, Cli, NonEmpty};
use rca_service_core::domain::util::encode_uri_component;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

pub struct WsError {
    pub code: &'static str,
    pub message: String,
}

fn error(code: &'static str, message: impl Into<String>) -> WsError {
    WsError { code, message: message.into() }
}

/// The params as the type their method takes; the error is serde's message.
fn params_of<T: DeserializeOwned>(params: &Value) -> Result<T, WsError> {
    parse(params).map_err(|message| error("invalid_params", message))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListThreadsParams {
    #[allow(dead_code)]
    include_archived: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadParams {
    thread_id: NonEmpty,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnDiffParams {
    thread_id: NonEmpty,
    turn_id: NonEmpty,
}

#[derive(Deserialize)]
struct ModelsParams {
    cli: Cli,
    directory: NonEmpty,
}

/// `{ providerID, modelID, name }` for each model opencode lists.
fn mapped(list: &Value) -> Vec<Value> {
    list.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|m| json!({ "providerID": m.get("providerID").cloned().unwrap_or(Value::Null), "modelID": m.get("id").cloned().unwrap_or(Value::Null), "name": m.get("name").cloned().unwrap_or(Value::Null) }))
        .collect()
}

async fn opencode_models(ctx: &Context, directory: &str) -> Result<Vec<Value>, String> {
    let path = format!("/api/model?directory={}", encode_uri_component(directory));
    ctx.opencode.fetch_json("GET", &path, None).await.map(|list| mapped(&list)).map_err(|e| e.to_string())
}

/// The CLIs a client may use: those this build serves and has a backend for.
fn served(ctx: &Context) -> Vec<&'static str> {
    let backends = ctx.orch.clis();
    clis::available().into_iter().filter(|cli| backends.iter().any(|b| b == cli)).collect()
}

async fn probe(ctx: &Context) -> Result<Value, WsError> {
    let directory = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|_| "/".into());
    let available = served(ctx);
    let mut providers = Vec::new();
    for cli in &available {
        let models = match *cli {
            "opencode" => opencode_models(ctx, &directory).await.map_err(|m| error("probe_failed", m))?,
            other => return Err(error("probe_failed", format!("the {other} CLI is not served by this build"))),
        };
        let display = clis::ALL.iter().find(|(id, _)| id == cli).map_or("", |(_, d)| *d);
        providers.push(json!({ "cli": cli, "displayName": display, "available": true, "models": models }));
    }
    let selected = ctx.opencode.env().settings().agent_cli.map(|v| v.trim().to_string()).filter(|v| Cli::ALL.contains(&v.as_str()));
    Ok(json!({ "availableClis": available, "selectedCli": selected, "version": ctx.version, "providers": providers }))
}

pub async fn handle(ctx: &Context, method: &str, params: &Value) -> Result<Value, WsError> {
    let engine = &ctx.orch.engine;
    match method {
        "orchestration.dispatchCommand" => {
            let command = parse_command(params).map_err(|message| error("invalid_params", message))?;
            // Nothing sends a picture over the socket to opencode yet, so a turn that carries one is refused, not run on its words alone.
            if matches!(&command.body, Body::TurnStart { attachments: Some(files), .. } if !files.is_empty()) {
                return Err(error("attachments_unsupported", "thread.turn.start does not support message.attachments"));
            }
            let receipt = engine.dispatch(command, "client").map_err(|e| error("dispatch_failed", e))?;
            Ok(receipt.to_json())
        }
        "orchestration.listThreads" => {
            let empty = json!({});
            let params = if params.is_null() { &empty } else { params };
            params_of::<ListThreadsParams>(params)?;
            Ok(json!({ "threads": engine.list_threads() }))
        }
        "orchestration.getThread" => {
            let thread_id = String::from(params_of::<ThreadParams>(params)?.thread_id);
            match engine.get_thread(&thread_id) {
                Ok(Some(thread)) => Ok(serde_json::to_value(thread).unwrap_or(Value::Null)),
                Ok(None) => Err(error("thread_not_found", "Thread not found")),
                Err(e) => Err(error("thread_read_failed", e)),
            }
        }
        "orchestration.getTurnDiff" => {
            let TurnDiffParams { thread_id, turn_id } = params_of(params)?;
            let (thread_id, turn_id) = (String::from(thread_id), String::from(turn_id));
            let diff = engine.get_turn_diff(&thread_id, &turn_id).map_err(|e| error("thread_read_failed", e))?;
            Ok(json!({ "unifiedDiff": diff.unwrap_or_default() }))
        }
        "server.probe" => probe(ctx).await,
        "models.list" => {
            let ModelsParams { cli, directory } = params_of(params)?;
            let cli = cli.as_str();
            if cli != "opencode" || ctx.orch.backend(cli).is_none() {
                return Err(error("models_list_failed", format!("the {cli} CLI is not served by this build")));
            }
            let models = opencode_models(ctx, &String::from(directory)).await.map_err(|m| error("models_list_failed", m))?;
            Ok(json!({ "models": models }))
        }
        _ => Err(error("unknown_method", "Unknown method")),
    }
}

/// Whether a method waits on the network, so the connection must not wait for it before reading the next message.
pub fn is_slow(method: &str) -> bool {
    matches!(method, "server.probe" | "models.list")
}
