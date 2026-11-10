//! The service's shared state: its configuration and everything the routes reach (the two engines, opencode, the planner, the stores),
//! built from the configuration and the ports. `main` passes the real clients; the flow tests pass doubles. Built inside the runtime,
//! since the orchestrators and the edit-stats writer spawn tasks.

use super::{cors, ws};
use rca_service_core::application::edit_stats::{self, EditStats};
use rca_service_core::application::environment::Environment;
use rca_service_core::application::ports::{Gateway, Ports};
use remote_chat_agent_core::application::ports::{Commands, DocumentText, Ledgers, Outbound, SpeechToText};
use rca_service_core::application::voice_planner::Planner;
use rca_service_core::application::opencode_client::Client;
use crate::ports::{AgentSocketPort, ExecutorSocketPort, McpAddress};
use rca_agent_core::DeviceBearers;
use rca_service_core::application::agent_groups::EngineThreads;
use rca_exec_channel::mcp::ServiceTools;
use rca_exec_channel::Registry;
use remote_chat_agent_core::application::reactor::Orchestrator;
use std::sync::Arc;
use std::time::Duration;

/// What the composition root registers on `/agent/v1`: the executors the groups reach (the same ones `/exec/v2` and `/mcp` use), the device
/// bearers that open the socket, the address of the MCP server a turn's CLI is given, the thread engine's adapter (which the service starts), and the groups.
pub struct AgentApi {
    pub executors: Arc<Registry>,
    pub bearers: Arc<dyn DeviceBearers>,
    pub mcp: Arc<dyn McpAddress>,
    pub sockets: Arc<dyn ExecutorSocketPort>,
    pub agent_socket: Arc<dyn AgentSocketPort>,
    pub threads: Arc<EngineThreads>,
    /// The tools of a thread's MCP server that the service answers itself (`approval_prompt`, `ask_user`).
    pub tools: Arc<dyn ServiceTools>,
    pub groups: rca_agent_core::AgentApi,
    /// The schedules the `schedules` groups answer for, which the clock tick and the webhook route reach too.
    pub schedules: Arc<rca_service_core::application::agent_groups::Schedules>,
}

pub struct Config {
    pub port: u16,
    pub allowed_origins: Vec<String>,
    /// `CHAT_AGENT_API_SECRET`; routes that check it refuse everything when it is not set.
    pub api_secret: Option<String>,
    pub commit: String,
    /// `RCA_DEFAULT_CWD`: the directory sessions are created in, mounted at the same path in this container and opencode's.
    pub default_cwd: String,
    /// `OPENCODE_HARD_TIMEOUT_MS`: how long a chat turn may take before its stream ends with a retryable error.
    pub hard_timeout_ms: u64,
    /// `RCA_STATE_DIR`: where the thread view's event log lives; memory only when unset.
    pub state_dir: Option<String>,
    /// `RCA_EXEC_ROOT`: the directory a turn's CLI starts in, and the root the diff viewer keeps its paths under.
    pub exec_root: String,
    /// How often a WebSocket connection is pinged.
    pub heartbeat: Duration,
    /// How long an edit-latency row may wait for others to share its flush.
    pub edit_stats_flush: Duration,
    /// Requests one caller may make per minute.
    pub requests_per_minute: u32,
    /// Where the state streams' `seq` counts from: above any `seq` an earlier run gave out, so a follower of one is reset.
    pub state_seq_base: u64,
}

impl Config {
    pub fn from_env(env: &dyn Environment) -> Self {
        let s = env.settings();
        Self {
            port: s.port,
            allowed_origins: cors::parse_allowed_origins(s.allowed_origins.as_deref()),
            api_secret: s.api_secret,
            commit: s.git_commit,
            default_cwd: s.default_cwd.unwrap_or_else(|| env.current_dir_or("/")),
            hard_timeout_ms: s.hard_timeout_ms,
            state_dir: s.state_dir,
            exec_root: s.exec_root,
            heartbeat: ws::HEARTBEAT_INTERVAL,
            edit_stats_flush: edit_stats::FLUSH_EVERY,
            requests_per_minute: crate::api::routes::REQUESTS_PER_MINUTE,
            state_seq_base: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros() as u64),
        }
    }
}

pub struct App {
    pub config: Config,
    /// The machine's environment the routes read their settings from.
    pub env: Arc<dyn Environment>,
    pub opencode: Client,
    /// Every program the routes run.
    pub commands: Arc<dyn Commands>,
    /// Every HTTP service besides opencode the routes post to.
    pub outbound: Arc<dyn Outbound>,
    /// The shared pool cooldowns a one-shot run reads and writes.
    pub ledgers: Arc<dyn Ledgers>,
    pub files: Arc<dyn remote_chat_agent_core::application::ports::WorkspaceFiles>,
    pub hosts: Arc<dyn rca_service_core::application::runs::RunHosts>,
    /// Text out of the PDFs, Word documents and pictures the routes and attachments read.
    pub documents: Arc<dyn DocumentText>,
    /// The words in a recording.
    pub speech: Arc<dyn SpeechToText>,
    pub edit_stats: EditStats,
    /// The engine behind the thread view, `/api/control` and `/api/question`: on disk when a state directory is configured.
    pub orch: Arc<Orchestrator>,
    /// opencode as the routes that name a session directly use it.
    pub gateway: Arc<dyn Gateway>,
    /// The voice-edit planner and the opencode sessions it holds between turns.
    pub planner: Planner,
    /// `/exec/v2`: the executor channel open for each workspace.
    pub executors: Arc<Registry>,
    /// The sockets executors connect on, each the channel of its workspace in `executors`.
    pub sockets: Arc<dyn ExecutorSocketPort>,
    /// The groups `/agent/v1` answers with, as the composition root registered them.
    pub agent_groups: rca_agent_core::AgentApi,
    /// The schedules: `main` runs their clock, and `/hooks/v1/schedules` starts their webhook runs.
    pub schedules: Arc<rca_service_core::application::agent_groups::Schedules>,
    /// The device bearers that open `/agent/v1/socket`, and that the service secret mints and revokes.
    pub bearers: Arc<dyn DeviceBearers>,
    /// What answers the MCP tools that wait for a person.
    pub service_tools: Arc<dyn ServiceTools>,
    /// The agent API's thread engine, which `start` brings up and settles the turns a crash left running.
    agent_threads: Arc<EngineThreads>,
    /// Where a CLI of this host reaches a thread's MCP server (the port the listener bound, once it did).
    mcp: Arc<dyn McpAddress>,
    agent_socket: Arc<dyn AgentSocketPort>,
    /// The WebSocket transport's shared state.
    ws: Arc<ws::Context>,
}

impl App {
    /// `agent_api` is what the composition root registered on `/agent/v1`.
    pub fn new(config: Config, ports: Ports, agent_api: AgentApi) -> Self {
        let client = ports.client;
        let orch = ports.orch;
        let planner = Planner::new(client.clone());
        let ws = Arc::new(ws::Context { orch: Arc::clone(&orch), opencode: client.clone(), version: "1.0.0", secret: config.api_secret.clone(), heartbeat: config.heartbeat });
        let edit_stats = EditStats::new(&ports.env.settings(), Arc::clone(&ports.outbound), config.edit_stats_flush, rca_service_core::application::edit_stats::host_name(ports.files.as_ref()));
        Self { config, env: ports.env, opencode: client, commands: ports.commands, outbound: ports.outbound, ledgers: ports.ledgers, files: ports.files, hosts: ports.hosts, documents: ports.documents, speech: ports.speech, edit_stats, orch, gateway: ports.gateway, planner, executors: agent_api.executors, sockets: agent_api.sockets, agent_socket: agent_api.agent_socket, agent_groups: agent_api.groups, schedules: agent_api.schedules, bearers: agent_api.bearers, service_tools: agent_api.tools, agent_threads: agent_api.threads, mcp: agent_api.mcp, ws }
    }

    /// Loads the threads on disk and interrupts the turns a crash left running.
    pub fn start(&self) {
        match self.orch.engine.start() {
            Ok(stale) => self.orch.recover(stale),
            Err(e) => rca_service_core::application::log::error!("engine.start_failed", err = e),
        }
        match self.agent_threads.start() {
            Ok(interrupted) => rca_service_core::application::log::info!("agent_threads.started", interrupted = interrupted),
            Err(e) => rca_service_core::application::log::error!("agent_threads.start_failed", err = e),
        }
    }

    /// Records the port the listener bound (a flow binds port 0).
    pub fn listening_on(&self, port: u16) {
        self.mcp.listening_on(port);
    }

    /// Where a CLI on this host reaches the thread's MCP server.
    pub fn mcp_url(&self, thread: &str) -> String {
        self.mcp.url(thread)
    }

    /// Serves an upgraded agent API socket until it closes: its requests answered by the registered groups, the streams they open pushed on it.
    pub async fn serve_agent_socket(&self, socket: axum::extract::ws::WebSocket) {
        self.agent_socket.serve(&self.agent_groups, rca_agent_core::Build { version: env!("CARGO_PKG_VERSION"), commit: &self.config.commit }, socket).await;
    }

    pub fn ws_context(&self) -> Arc<ws::Context> {
        Arc::clone(&self.ws)
    }
}
