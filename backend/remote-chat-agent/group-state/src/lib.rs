//! The `state` and `state.watch` groups of the agent API (`libraries/agent-api/agent-api-state`) over the `StateStore` port this crate owns.
//! Both groups share one `StateHub`: every write goes through it, so the change stream of a scope sees exactly the writes the store kept.
mod group;
mod hub;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use group::{StateGroup, StateWatchGroup};
pub use hub::{StateHub, StateStore, KEEP};

#[cfg(test)]
mod tests;
