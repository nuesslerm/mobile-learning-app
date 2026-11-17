//! The OTLP/HTTP JSON wire format of logs and gauges, and the one POST that carries it. Hand-written so a CLI that logs a few
//! lines links a small blocking client instead of the OpenTelemetry SDK and an async HTTP stack.

use crate::{Destination, Source};
use serde_json::{json, Map, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TIMEOUT: Duration = Duration::from_secs(10);

/// An OTLP attribute list from key/value pairs; a value keeps its JSON type (bool, int, double), anything else is its text.
pub fn attributes<K: AsRef<str>>(pairs: impl IntoIterator<Item = (K, Value)>) -> Value {
    Value::Array(pairs.into_iter().map(|(key, value)| json!({ "key": key.as_ref(), "value": any_value(&value) })).collect())
}

pub fn any_value(value: &Value) -> Value {
    match value {
        Value::Bool(b) => json!({ "boolValue": b }),
        Value::Number(n) => match n.as_i64() {
            Some(i) => json!({ "intValue": i.to_string() }),
            None => json!({ "doubleValue": n.as_f64().unwrap_or(0.0) }),
        },
        Value::String(s) => json!({ "stringValue": s }),
        other => json!({ "stringValue": other.to_string() }),
    }
}

pub fn nanos(at: SystemTime) -> String {
    at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos()).to_string()
}

/// The resource of `source`: its service name and the attributes inferred around it.
pub fn resource(source: &Source) -> Value {
    let mut pairs: Vec<(String, Value)> = vec![("service.name".into(), source.service.clone().into())];
    pairs.extend(source.attributes().into_iter().map(|(k, v)| (k.to_string(), Value::from(v))));
    json!({ "attributes": attributes(pairs) })
}

/// The OTLP severity number and text of a level; anything but debug, warn and error is info.
pub fn severity(level: &str) -> (u8, &'static str) {
    match level {
        "debug" => (5, "DEBUG"),
        "warn" => (13, "WARN"),
        "error" => (17, "ERROR"),
        _ => (9, "INFO"),
    }
}

pub fn logs_body(resource: Value, scope: &str, records: Vec<Value>) -> Value {
    json!({ "resourceLogs": [{ "resource": resource, "scopeLogs": [{ "scope": { "name": scope }, "logRecords": records }] }] })
}

#[cfg(feature = "service")]
pub fn spans_body(resource: Value, scope: &str, spans: Vec<Value>) -> Value {
    json!({ "resourceSpans": [{ "resource": resource, "scopeSpans": [{ "scope": { "name": scope }, "spans": spans }] }] })
}

pub fn log_record(at: SystemTime, level: &str, body: &str, attributes: Value) -> Value {
    let (number, text) = severity(level);
    let mut record = Map::new();
    record.insert("timeUnixNano".into(), nanos(at).into());
    record.insert("observedTimeUnixNano".into(), nanos(at).into());
    record.insert("severityNumber".into(), number.into());
    record.insert("severityText".into(), text.into());
    record.insert("body".into(), json!({ "stringValue": body }));
    record.insert("attributes".into(), attributes);
    Value::Object(record)
}

#[cfg(feature = "metrics")]
/// Gauges grouped by metric name, one data point per `(attributes, value)`.
pub fn gauges_body(resource: Value, scope: &str, at: SystemTime, gauges: Vec<(String, Value, f64)>) -> Value {
    let mut by_name: Vec<(String, Vec<Value>)> = Vec::new();
    for (name, attributes, value) in gauges {
        let point = json!({ "attributes": attributes, "timeUnixNano": nanos(at), "asDouble": value });
        match by_name.iter_mut().find(|(n, _)| *n == name) {
            Some((_, points)) => points.push(point),
            None => by_name.push((name, vec![point])),
        }
    }
    let metrics: Vec<Value> = by_name.into_iter().map(|(name, points)| json!({ "name": name, "gauge": { "dataPoints": points } })).collect();
    json!({ "resourceMetrics": [{ "resource": resource, "scopeMetrics": [{ "scope": { "name": scope }, "metrics": metrics }] }] })
}

/// POSTs `body` as JSON to `path` of the ingest with its bearer token: `Ok` only on a 2xx, otherwise `HTTP <status>` or the
/// transport error.
pub fn post(destination: &Destination, path: &str, body: &Value) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).http_status_as_error(false).build().into();
    let response = agent
        .post(destination.endpoint(path))
        .header("Authorization", format!("Bearer {}", destination.token()))
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("HTTP {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_typed_values_when_encoded_then_each_uses_its_otlp_variant() {
        let encoded = attributes([("s", json!("x")), ("i", json!(7)), ("f", json!(1.5)), ("b", json!(true)), ("o", json!({"a": 1}))]);

        assert_eq!(encoded[0]["value"], json!({ "stringValue": "x" }));
        assert_eq!(encoded[1]["value"], json!({ "intValue": "7" }));
        assert_eq!(encoded[2]["value"], json!({ "doubleValue": 1.5 }));
        assert_eq!(encoded[3]["value"], json!({ "boolValue": true }));
        assert_eq!(encoded[4]["value"], json!({ "stringValue": "{\"a\":1}" }));
    }

    #[test]
    fn given_levels_when_mapped_then_severity_numbers_follow_the_otlp_spec() {
        assert_eq!(["debug", "info", "warn", "error", "other"].map(|l| severity(l).0), [5, 9, 13, 17, 9]);
    }
}
