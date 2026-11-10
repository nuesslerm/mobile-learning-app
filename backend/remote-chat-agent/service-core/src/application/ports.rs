//! The service's bundle of ports: `main` fills `Ports` with the real clients from `infrastructure`; the flow tests fill it with in-process
//! doubles. The traits live in `remote-chat-agent-core`, and `environment` beside this file.

use super::environment::Environment;
use super::opencode_client::Client;
use async_trait::async_trait;
use remote_chat_agent_core::application::reactor::Orchestrator;
use serde_json::Value;
use remote_chat_agent_core::application::ports::{Commands, Ledgers, Outbound, WorkspaceFiles};
use std::sync::Arc;

#[derive(Clone)]
pub struct Ports {
    pub spawner: Arc<dyn rca_cli_driver::application::ports::Spawner>,
    /// opencode as every use case asks it: the port, the clock and the environment it reads.
    pub client: Client,
    /// The engine behind the thread view, `/api/control` and `/api/question`.
    pub orch: Arc<Orchestrator>,
    /// opencode as the routes that name a session directly use it.
    pub gateway: Arc<dyn Gateway>,
    pub commands: Arc<dyn Commands>,
    pub outbound: Arc<dyn Outbound>,
    pub ledgers: Arc<dyn Ledgers>,
    pub files: Arc<dyn WorkspaceFiles>,
    /// The host of a one-shot run's attempts.
    pub hosts: Arc<dyn super::runs::RunHosts>,
    pub env: Arc<dyn Environment>,
    /// Text out of the PDFs, Word documents and pictures the routes and attachments read.
    pub documents: Arc<dyn remote_chat_agent_core::application::ports::DocumentText>,
    /// The words in a recording.
    pub speech: Arc<dyn remote_chat_agent_core::application::ports::SpeechToText>,
}

/// What the routes that name an opencode session directly ask of it.
#[async_trait]
pub trait Gateway: Send + Sync {
    /// The question the user answered, to opencode by request id and directory (the route has no thread to take them from).
    async fn question_reply(&self, request_id: &str, directory: &str, answers: &Value) -> Result<(), String>;
    async fn session_abort(&self, session_id: &str, directory: &str) -> Result<(), String>;
    /// A prompt that is queued behind the running turn (`deferred`) or injected into it (`immediate`).
    async fn session_prompt(&self, session_id: &str, directory: &str, text: &str, delivery: &str) -> Result<(), String>;
}
