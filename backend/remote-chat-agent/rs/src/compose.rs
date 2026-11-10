//! The composition root's share of the engines: the thread engines and the opencode backends over the real clock, ids and event-log files.
//! `main` calls it with the real adapters and the flow harness with its own ports, so `api::app::App::new` builds no adapter.

use crate::api::app::Config;
use remote_chat_agent_host::FileStore;
use crate::infrastructure::system::{RandomIds, SystemClock};
use rca_service_core::application::environment::Environment;
use rca_service_core::application::opencode_client::Client;
use remote_chat_agent_opencode::OpencodeBackend;
use remote_chat_agent_core::application::engine::Engine;
use remote_chat_agent_core::application::ingestion::Options as TurnOptions;
use remote_chat_agent_core::application::ports::Backend;
use remote_chat_agent_core::application::reactor::Orchestrator;
use remote_chat_agent_core::application::store::{MemoryStore, Store};
use std::sync::Arc;
use std::time::Duration;

/// The engine of the agent API's threads: its own log, on disk beside the thread view's when a state directory is configured, so the thread view's
/// clients never see a thread or an event of the agent API (their event types are a closed set).
pub fn thread_engine(config: &Config) -> Arc<Engine> {
    let store: Box<dyn Store> = match &config.state_dir {
        Some(dir) => Box::new(FileStore::new(std::path::Path::new(dir).join("agent-api"))),
        None => Box::new(MemoryStore::default()),
    };
    Arc::new(Engine::new(store, config.default_cwd.clone(), Arc::new(SystemClock), Arc::new(RandomIds)))
}

/// The opencode `Client` over `transport`, on the real clock.
pub fn opencode_client(transport: Arc<dyn remote_chat_agent_core::application::ports::Opencode>, env: Arc<dyn Environment>) -> Client {
    Client::new(transport, Arc::new(SystemClock), env)
}

/// opencode behind the thread view's orchestrator (its log is on disk when a state directory is configured), and the gateway the routes that
/// name a session directly use. Built inside the runtime: the orchestrator spawns tasks.
pub fn orchestration(config: &Config, client: &Client, env: &dyn Environment) -> (Arc<Orchestrator>, Arc<OpencodeBackend>) {
    let settings = env.settings();
    let store: Box<dyn Store> = match &config.state_dir {
        Some(dir) => Box::new(FileStore::new(dir)),
        None => Box::new(MemoryStore::default()),
    };
    let engine = Arc::new(Engine::new(store, config.default_cwd.clone(), Arc::new(SystemClock), Arc::new(RandomIds)));
    let by_thread = Arc::clone(&engine);
    let process_cwd = env.current_dir_or("/");
    let backend: Arc<dyn Backend> = Arc::new(OpencodeBackend::new(client.clone(), Box::new(move |thread| by_thread.cwd_of(thread).unwrap_or_else(|| process_cwd.clone())), &settings));
    let orch = Orchestrator::new(engine, vec![backend], TurnOptions { tool_data: true, turn_deadline: Some(Duration::from_millis(config.hard_timeout_ms)) });
    (orch, Arc::new(OpencodeBackend::new(client.clone(), Box::new(|_| String::new()), &settings)))
}
