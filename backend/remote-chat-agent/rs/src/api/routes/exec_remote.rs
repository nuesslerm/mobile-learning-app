//! `/exec/v2` and `/mcp`: the executor channel (`docs/plans/rca-client/protocol.md`). `rca-exec` opens `GET /exec/v2/executors?workspace=[&executor=]`
//! and performs every action there: the executor is `executor`, else the workspace's own id, and `workspace` is its root workspace. A turn of `agent.threads` (`turn.start` on `/agent/v1`) starts the CLI with its own file and shell tools off and
//! `POST /mcp/{thread}`, served on loopback only, as its tool server, which forwards each `tools/call` over the thread's workspace executor.
//! The socket's lifecycle is `infrastructure::ws_channel`'s; these handlers authenticate, validate and hand over.

use super::{respond, Req};
use crate::api::app::App;
use crate::api::auth::{channel_authorised, unauthorized};
use axum::body::Bytes;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, Path, State};
use axum::response::{IntoResponse, Response};
use rca_exec_channel::mcp;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

fn failure(status: u16, code: &str, message: &str) -> Response {
    respond(status, &json!({ "error": message, "code": code }))
}

/// `GET /exec/v2/executors?workspace=[&executor=]`: the upgrade, then the channel until the socket closes. A workspace another executor owns is 409.
pub async fn channel(State(app): State<Arc<App>>, req: Req, upgrade: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>) -> Response {
    if !channel_authorised(&app, &req) {
        return unauthorized();
    }
    let Some(workspace) = req.query_param("workspace").filter(|t| !t.is_empty()) else { return failure(400, "INVALID_REQUEST", "workspace is required") };
    let executor = req.query_param("executor").filter(|t| !t.is_empty()).unwrap_or_else(|| workspace.clone());
    match upgrade {
        Ok(upgrade) => match app.sockets.accept(upgrade, executor, workspace) {
            Ok(response) => response,
            Err(rca_exec_channel::WorkspaceError::Taken { workspace, owner }) => failure(409, "WORKSPACE_TAKEN", &format!("workspace {workspace} belongs to executor {owner}")),
            Err(rca_exec_channel::WorkspaceError::UnknownExecutor(id)) => failure(409, "UNKNOWN_EXECUTOR", &format!("executor {id} is not attached")),
        },
        Err(rejection) => rejection.into_response(),
    }
}

/// `POST /mcp/{thread}`: one JSON-RPC message of the thread's tool server. Only a loopback peer is let in: the CLI runs in this container.
pub async fn mcp_server(State(app): State<Arc<App>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Path(thread): Path<String>, body: Bytes) -> Response {
    if !peer.ip().to_canonical().is_loopback() {
        tracing::warn!(event = "mcp.refused.non_loopback", peer = %peer);
        return failure(403, "MCP_LOOPBACK_ONLY", "the MCP server answers loopback callers only");
    }
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        return respond(400, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}));
    };
    match mcp::handle(&thread, app.executors.for_thread(&thread), Some(app.service_tools.as_ref()), &message).await {
        Some(reply) => respond(200, &reply),
        None => super::empty(202),
    }
}
