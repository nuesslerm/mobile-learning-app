//! The RPC envelope the phone speaks once a socket is authenticated: one JSON object per sealed text frame.
use serde_json::{json, Map, Value};

pub const BAD_REQUEST: &str = "bad_request";
pub const UNAUTHORIZED: &str = "unauthorized";
pub const METHOD_NOT_FOUND: &str = "method_not_found";
pub const INVALID_ARGUMENT: &str = "invalid_argument";
pub const NOT_OFFERED: &str = "not_offered";
pub const INTERNAL: &str = "internal_error";
/// What the host answers for a plain `Error` it throws that no code names: the message is the sentence.
pub const RUNTIME_ERROR: &str = "runtime_error";

/// What a handler refuses with: the error code and message of a failure envelope, and optional data.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

impl Failure {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), data: None }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(INVALID_ARGUMENT, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(INTERNAL, message)
    }
}

/// The name of a JSON value in the words of the schema errors the phone shows (`expected string, received number`).
pub fn received(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `params.<key>` as a string; a missing key (or no payload) reads as `undefined` in the error and any other kind by its name.
pub fn string_field<'a>(params: &'a Value, key: &str) -> Result<Option<&'a str>, Failure> {
    match params.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other)))),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: String,
    pub method: String,
    pub params: Value,
    /// A token a request carries is only ever compared to the one the socket proved; it never authenticates.
    pub device_token: Option<String>,
}

fn meta(runtime_id: &str) -> Value {
    json!({ "runtimeId": runtime_id })
}

pub fn success(id: &str, runtime_id: &str, result: Value) -> Value {
    json!({ "id": id, "ok": true, "result": result, "_meta": meta(runtime_id) })
}

/// One frame of a stream: a success envelope marked `streaming`.
pub fn streaming(id: &str, runtime_id: &str, result: Value) -> Value {
    let mut frame = success(id, runtime_id, result);
    frame["streaming"] = Value::Bool(true);
    frame
}

pub fn failure(id: &str, runtime_id: &str, f: &Failure) -> Value {
    let mut error = Map::new();
    error.insert("code".into(), f.code.clone().into());
    error.insert("message".into(), f.message.clone().into());
    if let Some(data) = &f.data {
        error.insert("data".into(), data.clone());
    }
    json!({ "id": id, "ok": false, "error": Value::Object(error), "_meta": meta(runtime_id) })
}

/// Parses a frame's text. The failure carries the id the answer is addressed to (`unknown` when the frame has none).
/// Boxed: the failure is the rare side of a per-frame call.
pub fn parse(text: &str) -> Result<Request, Box<(String, Failure)>> {
    let value: Value = serde_json::from_str(text).map_err(|_| Box::new(("unknown".to_string(), Failure::new(BAD_REQUEST, "Invalid JSON request"))))?;
    let object = value.as_object();
    let id = object.and_then(|o| o.get("id")).and_then(Value::as_str).filter(|s| !s.is_empty());
    let Some(id) = id else { return Err(Box::new(("unknown".into(), Failure::new(BAD_REQUEST, "Missing request id")))) };
    let method = object.and_then(|o| o.get("method")).and_then(Value::as_str).filter(|s| !s.is_empty());
    let Some(method) = method else { return Err(Box::new((id.to_string(), Failure::new(BAD_REQUEST, "Missing RPC method")))) };
    Ok(Request {
        id: id.to_string(),
        method: method.to_string(),
        params: object.and_then(|o| o.get("params")).cloned().unwrap_or(Value::Null),
        device_token: object.and_then(|o| o.get("deviceToken")).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
    })
}
