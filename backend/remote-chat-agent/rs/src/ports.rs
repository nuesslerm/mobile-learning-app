//! The transport ports: what `api` calls to take a socket over. `infrastructure` implements them; the composition root registers the adapters.

use async_trait::async_trait;
use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::response::Response;
use rca_agent_core::{AgentApi, Build};
use rca_exec_channel::WorkspaceError;
use rca_group_agent::McpEndpoint;

/// Where a CLI of this host reaches a thread's MCP server, and the port the listener bound.
pub trait McpAddress: McpEndpoint {
    fn listening_on(&self, port: u16);
}

/// The sockets executors connect on, each the channel of its workspace.
pub trait ExecutorSocketPort: Send + Sync {
    fn accept(&self, upgrade: WebSocketUpgrade, executor: String, workspace: String) -> Result<Response, WorkspaceError>;
}

/// The agent API socket: requests answered by the registered groups, the streams they open pushed on it.
#[async_trait]
pub trait AgentSocketPort: Send + Sync {
    async fn serve(&self, api: &AgentApi, build: Build<'_>, socket: WebSocket);
}
