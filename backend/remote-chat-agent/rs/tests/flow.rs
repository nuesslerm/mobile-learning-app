//! The service's flow tests: each one a caller's journey through the router `main` serves, over in-process doubles of opencode, the
//! programs the service runs and the HTTP services it posts to. One test binary for the whole service.

#[path = "flow/doubles.rs"]
mod doubles;
#[path = "flow/harness.rs"]
mod harness;
#[path = "flow/mapper.rs"]
mod mapper;

#[path = "flow/log_lines.rs"]
mod log_lines;

#[path = "flow/agent_accounts.rs"]
mod agent_accounts;
#[path = "flow/agent_api.rs"]
mod agent_api;
#[path = "flow/agent_git.rs"]
mod agent_git;
#[path = "flow/agent_hosted.rs"]
mod agent_hosted;
#[path = "flow/agent_lifecycle.rs"]
mod agent_lifecycle;
#[path = "flow/agent_preview.rs"]
mod agent_preview;
#[path = "flow/agent_queue.rs"]
mod agent_queue;
#[path = "flow/agent_requests.rs"]
mod agent_requests;
#[path = "flow/agent_schedules.rs"]
mod agent_schedules;
#[path = "flow/agent_attachments.rs"]
mod agent_attachments;
#[path = "flow/agent_catalog.rs"]
mod agent_catalog;
#[path = "flow/agent_state.rs"]
mod agent_state;
#[path = "flow/agent_terminal.rs"]
mod agent_terminal;
#[path = "flow/agent_threads.rs"]
mod agent_threads;
#[path = "flow/agent_workspace.rs"]
mod agent_workspace;
#[path = "flow/agent_workspaces.rs"]
mod agent_workspaces;
#[path = "flow/contracts.rs"]
mod contracts;
#[path = "flow/control.rs"]
mod control;
#[path = "flow/exec_remote.rs"]
mod exec_remote;
#[path = "flow/documents.rs"]
mod documents;
#[path = "flow/runs.rs"]
mod runs;
#[path = "flow/source_control.rs"]
mod source_control;
#[path = "flow/front_door.rs"]
mod front_door;
#[path = "flow/inventory.rs"]
mod inventory;
#[path = "flow/settings.rs"]
mod settings;
#[path = "flow/viewers.rs"]
mod viewers;
#[path = "flow/voice.rs"]
mod voice;
#[path = "flow/tamil.rs"]
mod tamil;
#[path = "flow/websocket.rs"]
mod websocket;
