//! The ports the core owns. `AgentApi` is rca's agent API in Orca's words: each mapping crate asks only for what it maps.
use serde_json::Value;

/// What `service.status` says of the service this server fronts.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceStatus {
    pub protocol: u32,
    pub version: String,
    pub commit: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentError {
    /// The service did not answer, or answered something unreadable.
    Failed(String),
    /// The service answered with a refusal that keeps its code.
    Refused { code: String, message: String },
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(m) => write!(f, "{m}"),
            Self::Refused { code, message } => write!(f, "{code}: {message}"),
        }
    }
}

pub trait AgentApi: Send + Sync {
    fn status(&self) -> Result<ServiceStatus, AgentError>;
    /// `state.get`: the value kept for the device scope and key, or none.
    fn state_get(&self, scope: &str, key: &str) -> Result<Option<Value>, AgentError>;
    /// Any other agent method, answered with its result. A mapping crate names the methods it uses (its `USES`), and a flow test holds the
    /// list to the methods it really sends.
    fn request(&self, method: &'static str, params: Value) -> Result<Value, AgentError>;
}

/// What this host says of itself: set once at the composition root.
#[derive(Debug, Clone, PartialEq)]
pub struct HostInfo {
    /// A Node.js platform name (`linux`, `darwin`, `win32`): the phone reads it as the host's own.
    pub platform: String,
    pub machine_name: String,
    pub app_version: String,
}

/// Whether a device is still paired, asked before every request: a revoked phone is refused at once, before its socket is closed.
pub trait Principals: Send + Sync {
    fn still_paired(&self, device_id: &str, token: &str) -> bool;
}
