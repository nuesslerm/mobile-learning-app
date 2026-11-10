//! The agent CLIs this service knows (`config/agentClis.ts`) and which of them can be used here.

pub const ALL: [(&str, &str); 3] = [
    ("opencode", "opencode serve, reached over HTTP. No heavy local dependency."),
    ("claude", "claude CLI, driven in-process via the Anthropic Agent SDK."),
    ("codex", "codex CLI, driven in-process via its app-server subprocess mode."),
];

pub fn is_cli(id: &str) -> bool {
    ALL.iter().any(|(known, _)| *known == id)
}

/// Whether a CLI can be used with this build. The service drives opencode, which it reaches over HTTP; the claude and codex backends
/// are not ported, and the production image is built without them (`AGENT_CLIS=opencode` prunes both), so the Node service it
/// replaces serves neither there either.
pub fn is_available(id: &str) -> bool {
    id == "opencode"
}

pub fn available() -> Vec<&'static str> {
    ALL.iter().map(|(id, _)| *id).filter(|id| is_available(id)).collect()
}

/// The CLI `wanted` (`AGENT_CLI`) names, `opencode` when unset or not one of the three.
pub fn selected(wanted: Option<&str>) -> &'static str {
    let wanted = wanted.unwrap_or_default().trim();
    ALL.iter().map(|(id, _)| *id).find(|id| *id == wanted).unwrap_or("opencode")
}
