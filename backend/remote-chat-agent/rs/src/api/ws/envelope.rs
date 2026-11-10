//! What a client sends over the socket (`ClientEnvelopeSchema`), read by serde: the `kind` first, then the fields that kind needs.
//! `params` must be present on a request or a subscription, even as `null`. The first thing wrong is the error, in serde's words.

use rca_service_core::domain::schema::{parse, NonEmpty};
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Debug, PartialEq)]
pub enum Envelope {
    Ping(Value),
    Request { id: String, method: String, params: Value },
    Subscribe { id: String, method: String, params: Value },
    Unsubscribe { id: String },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Request,
    Subscribe,
    Unsubscribe,
    Ping,
}

#[derive(Deserialize)]
struct Tagged {
    kind: Kind,
}

#[derive(Deserialize)]
struct PingIn {
    ts: i64,
}

#[derive(Deserialize)]
struct CallIn {
    id: NonEmpty,
    method: String,
    params: Value,
}

#[derive(Deserialize)]
struct UnsubscribeIn {
    id: NonEmpty,
}

/// Reads a parsed message. The error is the first thing wrong with it.
pub fn parse_envelope(value: &Value) -> Result<Envelope, String> {
    parse::<Map<String, Value>>(value)?;
    Ok(match parse::<Tagged>(value)?.kind {
        Kind::Ping => Envelope::Ping(json!(parse::<PingIn>(value)?.ts)),
        Kind::Unsubscribe => Envelope::Unsubscribe { id: parse::<UnsubscribeIn>(value)?.id.into() },
        kind @ (Kind::Request | Kind::Subscribe) => {
            let call = parse::<CallIn>(value)?;
            let (id, method, params) = (call.id.into(), call.method, call.params);
            if matches!(kind, Kind::Request) {
                Envelope::Request { id, method, params }
            } else {
                Envelope::Subscribe { id, method, params }
            }
        }
    })
}

/// `{ kind: 'response', id, ok: false, error }`.
pub fn error_response(id: &str, code: &str, message: &str) -> Value {
    json!({ "kind": "response", "id": id, "ok": false, "error": { "code": code, "message": message } })
}

pub fn ok_response(id: &str, result: Value) -> Value {
    json!({ "kind": "response", "id": id, "ok": true, "result": result })
}
