use orca_core::ServiceStatus;
use serde_json::Value;
use std::path::PathBuf;

pub use orca_serve::testing::{Harness, PhoneClient, RUNTIME_ID};
pub use serde_json::json;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `service.status` rca's contract crate recorded: the result a mapper reads at startup.
pub fn recorded_status_result() -> Value {
    let path = repo_root().join("libraries/agent-api/agent-api-service/tests/fixtures/service.status.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("the recorded service.status")).unwrap();
    fixture["exchanges"][0]["response"]["result"].clone()
}

pub fn recorded_status() -> ServiceStatus {
    let r = recorded_status_result();
    ServiceStatus {
        protocol: r["protocol"].as_u64().unwrap() as u32,
        version: r["version"].as_str().unwrap().into(),
        commit: r["commit"].as_str().unwrap().into(),
        groups: r["groups"].as_array().unwrap().iter().map(|g| g.as_str().unwrap().to_string()).collect(),
    }
}

pub fn harness() -> Harness {
    Harness::start(recorded_status())
}

/// A phone that is paired with the harness and past the v2 handshake.
pub fn paired(h: &Harness) -> PhoneClient {
    PhoneClient::open_v2(h.addr, h.host_public, &h.device.token)
}
