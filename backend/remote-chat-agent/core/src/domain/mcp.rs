//! What opencode's `/mcp` calls may answer: a map of server name to status, and a boolean for connect and disconnect.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum McpStatus {
    Connected,
    Disabled,
    Failed { error: String },
    NeedsAuth,
    NeedsClientRegistration { error: String },
}

pub fn parse_mcp_map(reply: &Value) -> Result<BTreeMap<String, McpStatus>, String> {
    serde_json::from_value(reply.clone()).map_err(|e| e.to_string())
}

pub fn parse_toggle(reply: &Value) -> Result<bool, String> {
    reply.as_bool().ok_or_else(|| format!("expected a boolean, got {reply}"))
}
