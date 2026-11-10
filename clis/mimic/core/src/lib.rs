//! The pure core of `wf mimic` (docs/plans/mimic/plan.md): characterization and differential testing of a candidate against a pinned
//! reference. A reference is recorded through a `Driver` into normalised cassettes with machine-checked provenance; replay feeds the
//! cassette's inputs to the candidate and diffs under declared rules and an allowlist. No I/O here: the `Store`, `Driver`, `Clock` and
//! `Rng` ports are owned by this crate, their adapters live in the thin `mimic` crate and their doubles in `testing`.
pub mod allowlist;
pub mod cassette;
pub mod corpus;
pub mod crawl;
pub mod differ;
pub mod fixtures;
pub mod gaps;
pub mod import;
pub mod json;
pub mod ports;
pub mod presets;
pub mod record;
pub mod rng;
pub mod rules;
pub mod stabilize;
pub mod t3_golden;
pub mod usecase;
pub mod wire;

#[cfg(feature = "testing")]
pub mod testing;

#[cfg(test)]
mod unit_tests;
