//! `orca-serve`: the Orca-compatible server. This crate is the composition root of the `orca-*` crates: `transport` is the WebSocket adapter
//! (handshake and authentication through `orca-crypto`, then the core's connection), `adapters` are the system implementations of the ports,
//! `wiring` builds the graph, `verbs` is the command line. Plan `docs/plans/rca-parity/plan.md` section 3.
pub mod adapters;
mod help_table;
mod http;
pub mod transport;
mod verbs;
pub mod wiring;

#[cfg(feature = "testing")]
pub mod testing;

pub use help_table::HELP;

/// What the thin binary mounts.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("serve", verbs::serve), ("pair", verbs::pair), ("api", verbs::api)];
