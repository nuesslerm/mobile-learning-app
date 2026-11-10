//! The service's use cases and rules, free of transports and adapters: `domain` (the agent CLIs and models, request validation, the
//! voice-edit grammar, small text helpers) and `application` (one-shot runs, voice edits, attachments, edit-latency rows, the agent API
//! groups, and the ports they reach the machine through: the environment, the files, programs, HTTP services). Nothing here opens a file,
//! a socket or a process; `rca-service` implements the ports (`infrastructure`) and serves the routes (`api`).
//!
//! Layers (enforced by `pnpm run arch:services`): `domain` <- `application`.

pub mod application;
pub mod domain;

#[cfg(feature = "testing")]
pub mod testing;
