//! `golden/rca-core.json`: cases whose outputs the TypeScript implementation produced before it was removed.
use serde_json::Value;

pub fn golden() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/flow/golden/rca-core.json")).expect("golden file");
    serde_json::from_str(&text).expect("golden is JSON")
}

pub fn s(v: &Value) -> &str {
    v.as_str().expect("string")
}

pub fn opt(v: &Value) -> Option<&str> {
    v.as_str()
}

/// Every number as f64: serde_json keeps 1 and 1.0 apart, JavaScript does not.
pub fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => serde_json::json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        other => other.clone(),
    }
}

pub fn iso_or_null(ms: Option<i64>) -> Value {
    ms.map_or(Value::Null, |m| Value::String(rca_core::time::iso(m)))
}
