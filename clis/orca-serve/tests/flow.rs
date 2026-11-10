//! Orca's transport, authentication and first methods through the real `orca-serve`: the transport over loopback with rca doubled, the thin
//! binary against a fake agent API, and a phone-side client built from the primitives the recorded vectors pin. Given/When/Then in each test.
#[path = "flow/binary.rs"]
mod binary;
#[path = "flow/devices.rs"]
mod devices;
#[path = "flow/status.rs"]
mod status;
#[path = "flow/support.rs"]
mod support;
#[path = "flow/table.rs"]
mod table;
#[path = "flow/transport.rs"]
mod transport;
#[path = "flow/web.rs"]
mod web;
