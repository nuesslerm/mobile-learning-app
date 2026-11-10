//! The rules and vocabulary of the service, free of I/O: the agent CLIs and models, request validation, the voice-edit grammar,
//! markdown conversion and small text helpers. Nothing here names a framework, a client or the filesystem.

pub mod clis;
pub mod models;
pub mod schema;
pub mod util;
pub mod voice_dsl;
