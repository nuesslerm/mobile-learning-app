//! The egress policy of a workspace host, as data: the destinations its processes may reach once the gateway is in front of the model providers.
//! Nothing enforces it yet (the lockdown waits for the real-binary proof of Claude Code); the tests pin what it allows.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub host: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressPolicy {
    pub allow: Vec<Destination>,
}

impl EgressPolicy {
    /// A workspace host that may reach the service (its gateway, its tool server and its channel) and nothing else.
    pub fn workspace_host(service: Destination) -> EgressPolicy {
        EgressPolicy { allow: vec![service] }
    }

    /// Whether a connection to `host` on `port` is allowed: the host matches exactly, and a destination with a port names that port only.
    pub fn allows(&self, host: &str, port: u16) -> bool {
        self.allow.iter().any(|d| d.host.eq_ignore_ascii_case(host) && d.port.is_none_or(|p| p == port))
    }
}
