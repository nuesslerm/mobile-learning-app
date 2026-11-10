//! The Nx cache's contract (owner, 18 Oct): a task runs again exactly when one of its inputs changed and is replayed exactly when none did,
//! proved against the real Nx on a copy of this repository (`flow/nx_cache.rs`) and on a fixture of cargo workspaces (`flow/nx_crates.rs`),
//! and the build cache shim's own contract with Nx (`flow/cache_contract.rs`). It is its own test binary, the `cargo-contract` task of this crate, because it takes minutes (real Nx
//! runs over a hundred projects, dozens of times): `wf test plan` runs it when a file that can move the contract changed, and the
//! pipeline's other flow tests (`flow.rs`) run on every change to the crate.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/cache_contract.rs"]
mod cache_contract;
#[path = "flow/nx_cache.rs"]
mod nx_cache;
#[path = "flow/nx_crates.rs"]
mod nx_crates;
