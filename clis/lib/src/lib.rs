//! `wf lib`: versioned shared libraries. A release goes through a `Publisher` port (our bucket as a static Cargo sparse registry,
//! or any registry with Cargo's publish API); `status`, `prune` and `retarget` read the lockfiles and `libraries.json`, so a new
//! registry kind is one adapter. `domain` is pure, `ports` is what it needs, `adapters` implement them, `wiring` is the one place
//! that picks them (plan `docs/plans/versioned-libraries/plan.md`).
pub mod adapters;
pub mod domain;
mod help_table;
pub mod ports;
pub mod usecases;
mod verbs;
mod wiring;

#[cfg(feature = "testing")]
pub mod testing;

pub use help_table::HELP;

/// What the thin binary mounts.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("lib", verbs::run)];

pub use verbs::run;
