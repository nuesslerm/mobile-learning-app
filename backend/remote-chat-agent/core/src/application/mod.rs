//! The use cases over the domain: the engine (decide, append, project, notify, replay), the reactor that turns engine events into
//! agent calls, and the ingestion loop that turns an agent's event stream into commands. They reach the outside only through `ports`
//! and `store`.

pub mod engine;
pub mod ingestion;
pub mod ports;
pub mod reactor;
pub mod store;
