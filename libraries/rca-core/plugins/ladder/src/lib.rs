//! The model ladder as an optional plugin of rca-core: failure classification and rung selection, the pool cooldown
//! ledger and the run loop that rotates rungs. Pure logic with no I/O; the caller supplies the stores through `RunHost`.
//! Behaviour is pinned by `tests/flow/golden/rca-ladder.json`, whose outputs the TypeScript implementation produced.
pub mod file_store;
pub mod ladder;
pub mod ords_store;
pub mod pool_ledger;
pub mod run;
pub mod store;
