//! The wire of T3 Code at protocol 2 (protocol.md of docs/plans/t3-parity): pure data, no I/O. Shapes marked "recorded later" in
//! the plan are our reading of the source until the phase-0 goldens settle them; they live here and nowhere else.
pub mod environment;
pub mod history;
pub mod rows;
pub mod rpc;
pub mod scopes;
pub mod time;
pub mod wire;

pub const ORCHESTRATION_PROTOCOL_VERSION: u32 = 2;
pub const ORCHESTRATION_PROTOCOL_PARAM: &str = "orchestrationProtocol";
