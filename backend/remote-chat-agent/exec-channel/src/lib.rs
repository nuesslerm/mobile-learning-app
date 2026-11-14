//! The service's side of the executor channel (`docs/plans/rca-client/protocol.md`), without its transport: `remote` (a channel that asks the
//! workspace's executor to perform an action), `mcp` (the tool server a model's CLI is given, which forwards each call over that channel),
//! `registry` (the open channels by executor, the workspaces each owns, and the `Executors` port the agent API's groups reach them through). The transport (a
//! WebSocket) is the service's adapter of `FrameSink`.

pub mod mcp;
pub mod placed;
pub mod registry;
pub mod remote;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use placed::Placed;
pub use registry::{Executors, Place, Registry, Route, RouteError, WorkspaceError, WorkspaceLedger};

/// Every action of the executor contract, whichever family crate defines it.
pub fn contract_actions() -> impl Iterator<Item = &'static exec_proto::Action> {
    [exec_fs::ACTIONS, exec_proc::ACTIONS, exec_git::ACTIONS, exec_pty::ACTIONS, exec_hosted::ACTIONS, exec_preview::ACTIONS].into_iter().flatten()
}
