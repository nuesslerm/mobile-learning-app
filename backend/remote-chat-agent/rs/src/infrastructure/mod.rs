//! The adapters of the ports `remote-chat-agent-core` defines and the clients the service's features call: the event log's
//! and the processes' adapters live in `remote-chat-agent-host`; here are the outbound HTTP client, the vault and the executor's WebSocket.

pub mod agent_socket;
pub mod agent_state;
pub mod device_bearers;
pub mod env;
pub mod mcp_address;
pub mod model_api;
pub mod ords;
pub mod outbound;
pub mod blobs;
pub mod catalog;
pub mod previews;
pub mod schedules;
pub mod provider_dirs;
pub mod provider_login;
pub mod secrets;
pub mod system;
pub mod workspace_ledger;
pub mod ws_channel;
