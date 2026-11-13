//! The remote-chat-agent service: the HTTP and WebSocket routes the flow tests in `tests/flow.rs` pin, over the orchestration
//! engine and the opencode gateway. Standard crates carry the server, the clients and the telemetry.
//!
//! This crate holds the adapters and the front door only. The use cases, the rules and the ports they call out through are
//! `rca-service-core` (`rca_service_core::{domain, application}`); `remote-chat-agent-core` has the thread engine. Layers (enforced by
//! `pnpm run arch:services`): `infrastructure` implements the ports and `api` serves them; `main.rs` and `compose` fill `Ports` with the real adapters and
//! `api::app::App::new` wires the rest from them; the flow tests pass doubles instead.

pub mod api;
pub mod compose;
pub mod infrastructure;
pub mod ports;

use api::app::{AgentApi, Config};
use compose::thread_engine;
use infrastructure::agent_socket::AgentSockets;
use infrastructure::mcp_address::LoopbackMcp;
use infrastructure::ws_channel::ExecutorSockets;
use rca_agent_core::{DeviceBearers, Group};
use rca_cli_driver::application::ports::Spawner;
use rca_exec_channel::Registry;
use rca_group_accounts::{AccountsGroup, GitAiGroup, ModelApi, ProviderDirs, ProviderLogin, Sessions};
use rca_group_agent::{AgentLifecycleGroup, AgentQueueGroup, AgentRequestsGroup, AgentThreadsGroup, Runner};
use rca_service_core::application::agent_groups::{ActivityGroup, EngineThreads, FilesEditGroup, FilesGroup, ServiceGroup, StateGroup, StateHub, StateStore, StateWatchGroup, SystemClock, TerminalGroup, GitGroup, HostedAuthGroup, HostedCloneGroup, HostedPrGroup, ExecutorCredentials, WorkspaceGroup, WorktreesGroup, ScheduleClock, ScheduleGroup, ScheduleIds, ScheduleStore, ScheduleWatchGroup, Schedules, ThreadsTrigger, AttachmentClock, AttachmentIds, Attachments, AttachmentsGroup, BlobStore, Catalog, CatalogGroup, StagesAttachments, PreviewGroup, PreviewSessionGroup, PreviewWatchGroup, Previews};
use std::sync::Arc;

/// What the `accounts` and `git.ai` groups stand on: the providers' directories, the start of a CLI's login and the model the service asks.
pub struct Accounts {
    pub dirs: Arc<dyn ProviderDirs>,
    pub login: Arc<dyn ProviderLogin>,
    pub models: Arc<dyn ModelApi>,
}

impl Accounts {
    /// The real adapters: directories and logins under the settings' providers root (what `Processes::at` starts the CLIs with), the model over `outbound`.
    pub fn real(settings: &rca_service_core::application::settings::Settings, outbound: Arc<dyn remote_chat_agent_core::application::ports::Outbound>) -> Accounts {
        let root = settings.providers_root.as_str();
        Accounts {
            dirs: Arc::new(infrastructure::provider_dirs::ProviderFiles::at(root)),
            login: Arc::new(infrastructure::provider_login::CliLogin::at(root)),
            models: Arc::new(infrastructure::model_api::HttpModelApi::from_settings(settings, outbound)),
        }
    }
}

/// What the `schedules` groups stand on: where schedules are kept, the clock their due times are read from, and the source of their ids and tokens.
pub struct ScheduleParts {
    pub store: Arc<dyn ScheduleStore>,
    pub clock: Arc<dyn ScheduleClock>,
    pub ids: Arc<dyn ScheduleIds>,
}

impl ScheduleParts {
    /// The real adapters: the schedules in `file`, the system clock, random ids.
    pub fn real(file: std::path::PathBuf) -> ScheduleParts {
        ScheduleParts { store: Arc::new(infrastructure::schedules::ScheduleFile::at(file)), clock: Arc::new(infrastructure::schedules::SystemTime), ids: Arc::new(infrastructure::schedules::RandomIds) }
    }
}

/// What the `attachments` group stands on: where the bytes are kept, the clock their time is read from, and the source of their ids.
pub struct AttachmentParts {
    pub store: Arc<dyn BlobStore>,
    pub clock: Arc<dyn AttachmentClock>,
    pub ids: Arc<dyn AttachmentIds>,
}

impl AttachmentParts {
    /// The real adapters: a directory per attachment under `dir`, the system clock, random ids.
    pub fn real(dir: std::path::PathBuf) -> AttachmentParts {
        AttachmentParts { store: Arc::new(infrastructure::blobs::BlobDir::at(dir)), clock: Arc::new(infrastructure::blobs::SystemClock), ids: Arc::new(infrastructure::blobs::RandomIds) }
    }
}

/// The agent API `/agent/v1` answers: the one list of groups, built here for `main` and for the flow tests alike so a registration cannot differ
/// between them. the `service.activity` sub-group keeps client leases; `state` and `state.watch` share one hub, so a watcher sees every write; `service.status` lists each group once it serves all of its methods; `workspace` answers what it serves but stays unlisted
/// until the rest of its owner set (`workspace.activate`, `repo.hooks`) is served. `accounts` (provider logins, usage) and `git.ai` (commit messages) stand on `accounts`: directories of the providers, their login commands and a model API. `agent.threads` and its three sub-groups, `agent.requests`, `agent.queue` and `agent.lifecycle`, share one
/// runner of the CLIs `spawner` starts, in the executor root, against a thread engine of its own; the runner also answers the MCP tools that wait for a person.
pub fn agent_api(config: &Config, spawner: Arc<dyn Spawner>, state: Arc<dyn StateStore>, bearers: Arc<dyn DeviceBearers>, accounts: Accounts, schedule_parts: ScheduleParts, attachment_parts: AttachmentParts) -> AgentApi {
    let executors: Arc<Registry> = Arc::new(match &config.state_dir {
        Some(dir) => Registry::with_ledger(Arc::new(infrastructure::workspace_ledger::WorkspaceFile::at(std::path::Path::new(dir).join("agent-api").join("workspaces.json")))),
        None => Registry::default(),
    });
    let threads = Arc::new(EngineThreads::new(thread_engine(config)));
    let mcp = Arc::new(LoopbackMcp::new(config.port));
    let runner = Runner::new(threads.clone(), executors.clone(), spawner, mcp.clone(), std::path::PathBuf::from(&config.exec_root));
    let hub = Arc::new(StateHub::from_base(state, config.state_seq_base));
    let attachments = Attachments::new(attachment_parts.store, attachment_parts.clock, attachment_parts.ids, executors.clone());
    let agent_threads = Arc::new(AgentThreadsGroup::with_runner(runner.clone()).with_staging(Arc::new(StagesAttachments::new(attachments.clone()))));
    let schedules = Schedules::new(schedule_parts.store, schedule_parts.clock, schedule_parts.ids, Arc::new(ThreadsTrigger::new(agent_threads.clone(), runner.engine().clone())), config.state_seq_base);
    if let Err(failure) = schedules.recover() {
        rca_service_core::application::log::error!("schedules.recover_failed", code = failure.code, message = failure.message);
    }
    let previews = Previews::new(Arc::new(infrastructure::previews::SystemClock), Arc::new(infrastructure::previews::RandomIds), config.state_seq_base);
    let groups: Vec<Arc<dyn Group>> = vec![
        Arc::new(ServiceGroup),
        Arc::new(ActivityGroup::new(Arc::new(SystemClock))),
        Arc::new(StateGroup::new(hub.clone())),
        Arc::new(StateWatchGroup::new(hub)),
        Arc::new(FilesGroup::new(executors.clone())),
        Arc::new(FilesEditGroup::new(executors.clone())),
        Arc::new(WorkspaceGroup::new(executors.clone())),
        Arc::new(WorktreesGroup::new(executors.clone())),
        Arc::new(TerminalGroup::new(executors.clone(), Arc::new(SystemClock))),
        Arc::new(GitGroup::new(executors.clone())),
        Arc::new(HostedPrGroup::new(executors.clone())),
        Arc::new(HostedCloneGroup::new(executors.clone())),
        Arc::new(HostedAuthGroup::new(executors.clone(), Arc::new(ExecutorCredentials::new(executors.clone())))),
        Arc::new(GitAiGroup::new(executors.clone(), accounts.models.clone())),
        Arc::new(AccountsGroup::new(Arc::new(Sessions::new(accounts.dirs, accounts.login)), accounts.models)),
        agent_threads,
        Arc::new(AgentRequestsGroup::new(runner.clone())),
        Arc::new(AgentQueueGroup::new(runner.clone())),
        Arc::new(AgentLifecycleGroup::new(runner.clone())),
        Arc::new(ScheduleGroup::new(schedules.clone())),
        Arc::new(ScheduleWatchGroup::new(schedules.clone())),
        Arc::new(AttachmentsGroup::new(attachments)),
        Arc::new(CatalogGroup::new(Catalog::new(executors.clone(), Arc::new(infrastructure::catalog::SystemClock)))),
        Arc::new(PreviewGroup::new(executors.clone())),
        Arc::new(PreviewSessionGroup::new(previews.clone())),
        Arc::new(PreviewWatchGroup::new(previews)),
    ];
    AgentApi { sockets: Arc::new(ExecutorSockets::new(executors.clone())), agent_socket: Arc::new(AgentSockets), executors, bearers, mcp, threads, tools: runner, groups: rca_agent_core::AgentApi::new(groups).expect("the groups registered here do not clash"), schedules }
}
