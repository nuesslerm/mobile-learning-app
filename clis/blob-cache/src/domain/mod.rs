//! The pure core: kinds, keys, generations, budgets, the breaker and the write guard. It touches no file, socket, process,
//! clock or environment (`tests/flow.rs` greps this directory for those).

mod breaker;
mod budget;
mod guard;
mod kind;
pub mod pnpm;
mod stats;

pub use breaker::{Breaker, Degraded};
pub use budget::{Budget, Meter};
pub use guard::{write_allowed, Pins, Refusal, WriteFacts};
pub use kind::{BadName, Generation, Key, Kind};
pub use stats::{Failure, Seeded, Stats, Timing};
