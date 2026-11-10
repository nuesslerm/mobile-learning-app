//! The phone's retries: a mutation sent again with the same client operation id answers what it answered, so a retry after an ambiguous
//! send never starts a second turn. The ledger of a session lives in rca's state under `session-op.<session>` (operation id to fingerprint and answer).
use crate::agent::{state_get, state_set};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Map, Value};

/// A session keeps the answers of its newest operations only; the host's own ledger expires too.
const KEEP: usize = 64;

pub(crate) struct Envelope {
    pub session: String,
    pub op: String,
    pub fingerprint: String,
}

impl Envelope {
    pub fn read(params: &Value) -> Result<Envelope, Failure> {
        let e = &params["envelope"];
        let text = |k: &str| e[k].as_str().filter(|s| !s.is_empty()).map(str::to_string).ok_or_else(|| Failure::invalid(format!("Invalid envelope: missing {k}")));
        Ok(Envelope { session: text("sessionId")?, op: text("clientOperationId")?, fingerprint: text("payloadFingerprint")? })
    }
}

fn key(session: &str) -> String {
    format!("session-op.{session}")
}

pub(crate) fn refused(code: &str, message: &str) -> Value {
    json!({"ok": false, "refusal": {"code": code, "message": message}})
}

/// The recorded answer of an operation id (marked replayed), a conflict when the same id came with another fingerprint, or none.
pub(crate) fn lookup(agent: &dyn AgentApi, env: &Envelope) -> Result<Option<Value>, Failure> {
    let Some(Value::Object(ledger)) = state_get(agent, &key(&env.session))? else { return Ok(None) };
    let Some(entry) = ledger.get(&env.op) else { return Ok(None) };
    if entry["fingerprint"] != env.fingerprint.as_str() {
        return Ok(Some(refused("agent_session_operation_conflict", "the operation id was used for another request")));
    }
    let mut answer = entry["answer"].clone();
    answer["replayed"] = true.into();
    Ok(Some(answer))
}

pub(crate) fn record(agent: &dyn AgentApi, env: &Envelope, answer: &Value) -> Result<(), Failure> {
    let mut ledger: Map<String, Value> = match state_get(agent, &key(&env.session))? {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    ledger.insert(env.op.clone(), json!({"fingerprint": env.fingerprint, "answer": answer}));
    while ledger.len() > KEEP {
        let Some(oldest) = ledger.keys().next().cloned() else { break };
        ledger.remove(&oldest);
    }
    state_set(agent, &key(&env.session), Value::Object(ledger))
}

/// A refusal with the facts the phone reads beside the code (`details`, and `reason` on the refusal itself as the host's legacy field).
pub(crate) fn refused_with(code: &str, message: &str, details: Value) -> Value {
    let mut r = json!({"code": code, "message": message, "details": details});
    if let Some(reason) = details.get("reason") {
        r["reason"] = reason.clone();
    }
    json!({"ok": false, "refusal": r})
}
