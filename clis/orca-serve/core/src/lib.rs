//! The pure core of `orca-serve`: no I/O. The phone's wire (`references/orca/src/main/runtime/rpc`), the `Area` trait each `orca-map-*`
//! crate implements, the table the composition root assembles from them, and the dispatcher of one authenticated connection.
pub mod area;
pub mod connection;
pub mod ports;
pub mod table;
pub mod wire;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use area::{Area, Ctx, Kind, Method, NotOffered, Subscription, Subscriptions};
pub use connection::{Connection, Host};
pub use ports::{AgentApi, AgentError, EventStream, HostInfo, Principals, ServiceStatus, StreamEvent};
pub use wire::Failure;

static KEPT_STATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Held across each read-modify-write of a value kept under the scope `orca` that the map crates make (settings, UI, quick commands, layout, session options, voice, repo and worktree meta), so two connections take turns instead of the later write dropping the earlier one.
/// One process-wide lock: a slow state call stalls every connection's writes, which is accepted for a single-user server.
pub fn kept_state_turn() -> std::sync::MutexGuard<'static, ()> {
    KEPT_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
