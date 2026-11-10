//! Per-call MCP permission tier for the PreToolUse hook (`mcp-guard`): servers declared for
//! Claude are trusted, every other server is read-only by tool-name shape and fails closed.
use crate::contract::{read_mcps, trusted_mcp_servers, FIRST_PARTY_TRUSTED_SERVERS};
use crate::Res;
use regex::Regex;
use serde_json::{json, Value};

pub enum Decision {
    Passthrough,
    Allow,
    Deny(String),
}

const UUID: &str = r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";

pub fn classify(tool_name: &str, trusted: &[String]) -> Decision {
    let tool_re = Regex::new(&format!(
        r"^mcp__({UUID}|[A-Za-z0-9]+(?:_[A-Za-z0-9]+)*)__(.+)$"
    ))
    .unwrap();
    let Some(caps) = tool_re.captures(tool_name) else {
        return Decision::Passthrough;
    };
    let (server, tool) = (&caps[1], &caps[2]);
    if trusted.iter().any(|t| t == server) || FIRST_PARTY_TRUSTED_SERVERS.contains(&server) {
        return Decision::Passthrough;
    }
    if Regex::new(&format!("^{UUID}$")).unwrap().is_match(server) {
        return Decision::Allow;
    }
    let read_shaped = Regex::new(
        r"(?i)(^|[_-])(get|list|search|read|fetch|find|query|view|watch|poll|explore|preview|extract|download|who[_-]?am[_-]?i)([_-]|$)",
    )
    .unwrap();
    if read_shaped.is_match(tool) {
        return Decision::Allow;
    }
    Decision::Deny(format!(
        "mcp__{server} is not a repo-declared trusted server (see .agents/config/mcps.json) \
         and \"{tool}\" doesn't look read-only. Only get/list/search/read/fetch/-style calls are \
         auto-allowed on untrusted MCP servers — ask the user before running a write/mutating \
         action here."
    ))
}

/// Nothing on stdout for passthrough, so Claude Code's normal permission flow decides.
pub fn format_decision(d: &Decision) -> Option<String> {
    let (decision, reason) = match d {
        Decision::Passthrough => return None,
        Decision::Allow => ("allow", None),
        Decision::Deny(r) => ("deny", Some(r)),
    };
    let mut inner = json!({ "hookEventName": "PreToolUse", "permissionDecision": decision });
    if let Some(reason) = reason {
        inner["permissionDecisionReason"] = Value::String(reason.clone());
    }
    Some(json!({ "hookSpecificOutput": inner }).to_string())
}

pub fn run_guard(root: &str, stdin: &str) -> Res<Option<String>> {
    let input: Value = serde_json::from_str(stdin).map_err(|e| e.to_string())?;
    let tool = input.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let trusted = trusted_mcp_servers(&read_mcps(root)?);
    Ok(format_decision(&classify(tool, &trusted)))
}
