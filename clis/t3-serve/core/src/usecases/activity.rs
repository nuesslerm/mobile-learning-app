//! `server.reportClientActivity`: a client says whether a person is looking at it, and the service that does the optional work keeps that
//! as a lease (`service.clientActivity` of the agent API). Nothing is kept here: the lease is the service's.
use super::queries::QueryFailure;
use super::service::Service;
use crate::ports::{ClientActivity, Op};
use serde_json::Value;
use t3_contracts::time::parse_iso;
use t3_contracts::wire;

const KINDS: [&str; 4] = ["web", "desktop-renderer", "mobile", "unknown"];
const APP_STATES: [&str; 4] = ["active", "inactive", "background", "unknown"];
const TRI: [&str; 3] = ["true", "false", "unknown"];

fn defect(path: &str, expected: &str) -> QueryFailure {
    QueryFailure::Defect(format!("Expected {expected} at [\"{path}\"]"))
}

fn flag(payload: &Value, key: &str) -> Result<bool, QueryFailure> {
    payload.get(key).and_then(Value::as_bool).ok_or_else(|| defect(key, "a boolean"))
}

fn one_of(payload: &Value, key: &str, allowed: &[&str]) -> Result<Option<String>, QueryFailure> {
    match payload.get(key) {
        None => Ok(None),
        Some(v) => v.as_str().filter(|s| allowed.contains(s)).map(|s| Some(s.to_string())).ok_or_else(|| defect(key, &allowed.join(" | "))),
    }
}

/// A scope as the string the service keeps: `thread:<id>`, `vcs-status:<cwd>`, `provider-status:<instance>`, or the bare type.
fn scope(value: &Value, at: usize) -> Result<String, QueryFailure> {
    let path = format!("scopes\"][{at}");
    let kind = value.get("type").and_then(Value::as_str).ok_or_else(|| defect(&path, "a scope with a type"))?;
    let detail = |key: &str| value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
    match kind {
        "server-config" | "diagnostics" => Ok(kind.to_string()),
        "provider-status" => Ok(detail("instanceId").map_or_else(|| kind.to_string(), |i| format!("{kind}:{i}"))),
        "vcs-status" | "git-refs" => detail("cwd").map(|c| format!("{kind}:{c}")).ok_or_else(|| defect(&path, "a scope with a cwd")),
        "thread" => detail("threadId").map(|t| format!("thread:{t}")).ok_or_else(|| defect(&path, "a scope with a threadId")),
        _ => Err(defect(&path, "server-config | provider-status | vcs-status | git-refs | diagnostics | thread")),
    }
}

impl Service {
    /// What the payload of the RPC says, in the agent API's words.
    pub fn client_activity(payload: &Value) -> Result<ClientActivity, QueryFailure> {
        let client = payload.get("clientId").and_then(Value::as_str).map(str::trim).filter(|c| !c.is_empty() && c.chars().count() <= 128).ok_or_else(|| defect("clientId", "a non-empty string of at most 128 characters"))?;
        let kind = one_of(payload, "clientKind", &KINDS)?.ok_or_else(|| defect("clientKind", &KINDS.join(" | ")))?;
        let observed = payload.get("observedAt").and_then(Value::as_str).and_then(parse_iso).ok_or_else(|| defect("observedAt", "a UTC date-time"))?;
        let scopes = payload.get("scopes").and_then(Value::as_array).ok_or_else(|| defect("scopes", "an array"))?;
        Ok(ClientActivity {
            client: client.to_string(),
            kind,
            visible: flag(payload, "visible")?,
            focused: flag(payload, "focused")?,
            recently_interacted: flag(payload, "recentlyInteracted")?,
            app_state: one_of(payload, "appState", &APP_STATES)?,
            low_power_mode: one_of(payload, "lowPowerMode", &TRI)?,
            scopes: scopes.iter().enumerate().map(|(at, s)| scope(s, at)).collect::<Result<_, _>>()?,
            ttl_ms: match payload.get("ttlMs") {
                None => None,
                Some(v) => Some(v.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64).ok_or_else(|| defect("ttlMs", "a number"))?),
            },
            observed_at_ms: observed.max(0) as u64,
        })
    }

    /// `server.reportClientActivity`: forwards the report to the agent API, which keeps the lease.
    pub fn report_client_activity(&self, payload: &Value) -> Result<(), QueryFailure> {
        let report = Self::client_activity(payload)?;
        self.agent().run(&Op::ClientActivity(report)).map(drop).map_err(|e| QueryFailure::Error(wire::host_error(&e.message())))
    }
}
