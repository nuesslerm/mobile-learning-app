//! `aiVault.listSessions` and `aiVault.prepareSessionResume`: the CLI session files in the executor's own home (`sessions.list`, read by `session_scan`),
//! as the host's scan result; `totalTokens` is 0 and `previewMessages` the first and last prompt (`docs/gaps/orca-ai-vault-session-fields.md`).
use crate::agent::ask;
use crate::ids::mint_upload;
use crate::session::now_ms;
use agent_api_catalog::SESSIONS_LIST;
use orca_core::wire::{string_field, Failure};
use orca_core::AgentApi;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const LIMIT_MAX: u64 = 2000;
const LOCAL_HOST: &str = "local";

/// The agents the host's history knows (`AI_VAULT_AGENTS`); this host holds sessions of the first and fourth only.
const AGENTS: &[&str] = &[
    "claude", "codebuddy", "qoder", "codex", "hermes", "pi", "omp", "prime-agent", "cursor", "gemini", "antigravity", "rovo", "copilot", "opencode", "opencode2", "zcode", "grok", "openclaw", "devin", "droid", "cline", "kimi", "muse", "jcode",
    "kiro",
];

fn iso(ms: u64) -> String {
    let (days, rest) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (day, month) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
}

fn quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    let mut out = String::new();
    let mut literal = String::new();
    for c in value.chars() {
        match c {
            '\'' | '\\' => {
                if !literal.is_empty() {
                    out.push_str(&format!("'{literal}'"));
                    literal.clear();
                }
                out.push_str(if c == '\'' { "\"'\"" } else { "\"\\\\\"" });
            }
            _ => literal.push(c),
        }
    }
    if !literal.is_empty() {
        out.push_str(&format!("'{literal}'"));
    }
    out
}

/// The line the phone's terminal runs to resume a session, as the host builds it for a POSIX shell with no account home.
fn resume_command(agent: &str, session: &str, cwd: Option<&str>) -> String {
    let resume = if agent == "codex" { format!("codex resume {}", quote(session)) } else { format!("claude --resume {}", quote(session)) };
    match cwd {
        Some(dir) => format!("cd {} && {resume}", quote(dir)),
        None => resume,
    }
}

fn object(params: &Value) -> Result<&Map<String, Value>, Failure> {
    match params {
        Value::Object(map) => Ok(map),
        other => Err(Failure::invalid(format!("Invalid input: expected object, received {}", orca_core::wire::received(other)))),
    }
}

fn flag(params: &Value, key: &str) -> Result<bool, Failure> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", orca_core::wire::received(other)))),
    }
}

struct Request {
    limit: Option<u64>,
    after: Option<String>,
    force: bool,
    scope: Vec<String>,
    scope_only: bool,
    host: String,
}

fn read(params: &Value) -> Result<Request, Failure> {
    let empty = json!({});
    let params = if params.is_null() { &empty } else { params };
    object(params)?;
    let unlimited = flag(params, "unlimited")?;
    let limit = params.get("limit").and_then(Value::as_f64).filter(|l| l.is_finite() && *l >= 0.0).map(|l| l as u64);
    if !unlimited && limit.is_some_and(|l| l > LIMIT_MAX) {
        return Err(Failure::invalid("Limit exceeds maximum"));
    }
    let after = match params.get("afterFingerprint") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) => Some(s.clone()),
        Some(Value::String(_)) => return Err(Failure::invalid("Invalid string: must match pattern /^[0-9a-f]{64}$/")),
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected string, received {}", orca_core::wire::received(other)))),
    };
    let scope = match params.get("scopePaths") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(|p| p.as_str().filter(|s| !s.is_empty()).map(str::to_string).ok_or_else(|| Failure::invalid("Invalid input: expected string, received non-empty string"))).collect::<Result<_, _>>()?,
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected array, received {}", orca_core::wire::received(other)))),
    };
    let host = string_field(params, "executionHostId")?.filter(|h| !h.is_empty()).unwrap_or(LOCAL_HOST).to_string();
    Ok(Request { limit: if unlimited { Some(0) } else { limit.filter(|l| *l > 0) }, after, force: flag(params, "force")?, scope, scope_only: flag(params, "scopeOnly")?, host })
}

fn inside(cwd: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    cwd == root || cwd.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

fn session(s: &Value, host: &str) -> Value {
    let (agent, id) = (s["agent"].as_str().unwrap_or_default(), s["session_id"].as_str().unwrap_or_default());
    let cwd = s["cwd"].as_str();
    let mut preview = Vec::new();
    for key in ["first_prompt", "last_prompt"] {
        if let Some(text) = s[key].as_str().filter(|t| preview.last().is_none_or(|p: &Value| p["text"] != *t)) {
            preview.push(json!({"role": "user", "text": text, "timestamp": null}));
        }
    }
    json!({
        "id": format!("{agent}:{id}"),
        "executionHostId": host,
        "agent": agent,
        "sessionId": id,
        "title": s["title"],
        "cwd": cwd,
        "branch": s["branch"],
        "model": s["model"],
        "filePath": s["path"],
        "codexHome": null,
        "createdAt": s["created_at"],
        "updatedAt": s["updated_at"],
        "modifiedAt": iso(s["modified_ms"].as_u64().unwrap_or(0)),
        "messageCount": s["message_count"],
        "totalTokens": 0,
        "previewMessages": preview,
        "firstUserPrompt": s["first_prompt"],
        "lastUserPrompt": s["last_prompt"],
        "queuedMessageCount": 0,
        "subagentTranscriptCount": 0,
        "resumeCommand": resume_command(agent, id, cwd),
        "subagent": null,
    })
}

pub(crate) fn list_sessions(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let req = read(params)?;
    let mut ask_params = json!({});
    if let Some(limit) = req.limit {
        ask_params["limit"] = json!(if limit == 0 { 500 } else { limit.min(500) });
    }
    let found = ask(agent, SESSIONS_LIST.name(), ask_params)?;
    let mut sessions: Vec<Value> = found["sessions"].as_array().cloned().unwrap_or_default();
    if req.scope_only && !req.scope.is_empty() {
        sessions.retain(|s| s["cwd"].as_str().is_some_and(|cwd| req.scope.iter().any(|root| inside(cwd, root))));
    }
    let sessions: Vec<Value> = sessions.iter().map(|s| session(s, &req.host)).collect();
    let result = json!({"sessions": sessions, "issues": [], "scannedAt": iso(now_ms())});
    let Some(after) = req.after else {
        let mut result = result;
        result["scanId"] = json!(mint_upload());
        return Ok(result);
    };
    let fingerprint: String = Sha256::digest(json!({"sessions": result["sessions"], "issues": []}).to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    if !req.force && after == fingerprint {
        return Ok(json!({"sessions": [], "issues": [], "scannedAt": result["scannedAt"], "fingerprint": fingerprint, "unchanged": true}));
    }
    let mut result = result;
    result["scanId"] = json!(mint_upload());
    result["fingerprint"] = json!(fingerprint);
    Ok(result)
}

pub(crate) fn prepare_resume(params: &Value) -> Result<Value, Failure> {
    let map = match params {
        Value::Object(map) if map.contains_key("agent") || map.contains_key("filePath") || map.contains_key("codexHome") => map,
        _ => return Err(Failure::invalid("Invalid input")),
    };
    match map.get("agent").and_then(Value::as_str) {
        Some(agent) if AGENTS.contains(&agent) => {}
        _ => return Err(Failure::invalid("Invalid option: expected one of the agents of the history")),
    }
    if map.get("filePath").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Err(Failure::invalid("Invalid input: expected string, received undefined"));
    }
    if !matches!(map.get("codexHome"), Some(Value::String(s)) if !s.is_empty()) && !matches!(map.get("codexHome"), Some(Value::Null)) {
        return Err(Failure::invalid("Invalid input: expected string, received undefined"));
    }
    for key in ["sessionId", "executionHostId"] {
        string_field(params, key)?;
    }
    flag(params, "fork")?;
    Ok(json!({"useRealCodexHome": false}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_session_when_its_resume_line_is_built_then_it_is_the_hosts_posix_line_with_quoted_arguments() {
        // Given a claude session in a directory with a quote in its name, a codex session with none, and a claude session without a directory
        let (claude, codex, bare) = (resume_command("claude", "s1", Some("/w/it's")), resume_command("codex", "c-1", Some("/w")), resume_command("claude", "s2", None));
        // Then each is the host's `cd <dir> && <cli> <resume form>`
        assert_eq!(claude, r#"cd '/w/it'"'"'s' && claude --resume 's1'"#);
        assert_eq!((codex.as_str(), bare.as_str()), ("cd '/w' && codex resume 'c-1'", "claude --resume 's2'"));
    }

    #[test]
    fn given_milliseconds_when_written_as_iso_then_the_date_is_the_proleptic_gregorian_one() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_793_494_861_123), "2026-11-01T01:01:01.123Z");
    }
}
