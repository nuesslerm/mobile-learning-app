//! One cache over a blob store, shared by every kind of cache entry (Nx task results, the pnpm store, later kinds).
//! `domain` is pure; `ports` is what it needs from outside; `adapters` implement the ports; `Cache` is the use case that ties a
//! budget, a breaker and a generation to a store, so a store that fails or runs out makes a run cold, never red.

pub mod adapters;
pub mod admin;
pub mod app_build;
mod cache;
pub mod domain;
pub mod nx_server;
pub mod pnpm;
pub mod ports;
pub mod sccache_server;
pub mod storage_guard;
pub mod storage_policy;

pub use cache::{Cache, Put, Skip, Writes};
pub use nx_server::{NxServer, Served};
pub use sccache_server::SccacheServer;
