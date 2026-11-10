//! Records sent one request at a time, blocking: for a CLI that logs a few events and exits. A record carries its call site
//! (`Record::new` is `#[track_caller]`), every request carries its `Source`, and everything is scrubbed like every other sink.

use crate::redact::{key_is_sensitive, Redactor, REDACTED};
use crate::destination::why;
use crate::{Destination, Site, Source};
use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::{InstrumentationScope, Key, KeyValue};
use opentelemetry_otlp::{LogExporter, Protocol, WithExportConfig as _, WithHttpConfig as _};
use opentelemetry_sdk::logs::{LogBatch, LogExporter as _, SdkLoggerProvider};
use opentelemetry_sdk::Resource;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TIMEOUT: Duration = Duration::from_secs(10);

pub struct Record {
    /// `debug`, `info`, `warn` or `error`; anything else is sent as info.
    pub level: &'static str,
    pub message: String,
    pub attributes: Vec<(String, Value)>,
    pub site: Option<Site>,
    pub at: SystemTime,
}

impl Record {
    /// A record made now, at the caller's file and line.
    #[track_caller]
    pub fn new(level: &'static str, message: impl Into<String>) -> Self {
        Self::at(Site::caller(), level, message)
    }

    /// A record made at `site` (the `site!` macro: file, line and function).
    pub fn at(site: Site, level: &'static str, message: impl Into<String>) -> Self {
        Self { level, message: message.into(), attributes: Vec::new(), site: Some(site), at: SystemTime::now() }
    }

    pub fn attr(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.push((key.into(), value.into()));
        self
    }

    /// The console line, scrubbed: `{"level","ts","msg",...attributes,"filename","line_number","function"}`, the keys the
    /// service subscriber's console lines use (`function` only for a record made by `record!`).
    pub fn line(&self, redactor: &Redactor) -> String {
        let ts = self.at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let mut line = Map::new();
        line.insert("level".into(), self.level.into());
        line.insert("ts".into(), ts.into());
        line.insert("msg".into(), redactor.scrub(&self.message).into());
        for (key, value) in &self.attributes {
            line.insert(key.clone(), scrubbed(redactor, key, value));
        }
        if let Some(site) = self.site {
            line.insert("filename".into(), site.file.into());
            line.insert("line_number".into(), site.line.into());
            if let Some(function) = site.function {
                line.insert("function".into(), function.into());
            }
        }
        Value::Object(line).to_string()
    }
}

fn scrubbed(redactor: &Redactor, key: &str, value: &Value) -> Value {
    match value {
        Value::String(s) => redactor.field(key, s).into_owned().into(),
        _ if key_is_sensitive(key) => REDACTED.into(),
        Value::Array(_) | Value::Object(_) => redactor.scrub(&value.to_string()).into_owned().into(),
        other => other.clone(),
    }
}

fn any_value(value: Value) -> AnyValue {
    match value {
        Value::Bool(b) => AnyValue::Boolean(b),
        Value::Number(n) => n.as_i64().map_or_else(|| AnyValue::Double(n.as_f64().unwrap_or(0.0)), AnyValue::Int),
        Value::String(s) => AnyValue::from(s),
        other => AnyValue::from(other.to_string()),
    }
}

/// The level's severity and its text; anything but debug, warn and error is info.
fn severity(level: &str) -> (Severity, &'static str) {
    match level {
        "debug" => (Severity::Debug, "DEBUG"),
        "warn" => (Severity::Warn, "WARN"),
        "error" => (Severity::Error, "ERROR"),
        _ => (Severity::Info, "INFO"),
    }
}

/// Sends `record` to the ingest the environment names; false when there is none or the export failed.
pub fn emit(source: &Source, record: &Record) -> bool {
    Destination::from_env().is_some_and(|destination| send(&destination, source, std::slice::from_ref(record)).is_ok())
}

/// Sends `records` in one request: `Ok` only on a 2xx, otherwise `HTTP <status>` or the transport error. The exporter's
/// blocking client may not be created or dropped on a Tokio worker, so it runs on a thread of its own.
pub fn send(destination: &Destination, source: &Source, records: &[Record]) -> Result<(), String> {
    std::thread::scope(|scope| scope.spawn(|| export(destination, source, records)).join()).unwrap_or_else(|_| Err("the exporter panicked".to_string()))
}

fn export(destination: &Destination, source: &Source, records: &[Record]) -> Result<(), String> {
    let mut exporter = LogExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpJson)
        .with_endpoint(destination.endpoint("/v1/logs"))
        .with_timeout(TIMEOUT)
        .with_headers(HashMap::from([("Authorization".to_string(), format!("Bearer {}", destination.token()))]))
        .build()
        .map_err(|e| e.to_string())?;
    let resource = Resource::builder_empty()
        .with_service_name(source.service.clone())
        .with_attributes(source.attributes().into_iter().map(|(k, v)| KeyValue::new(k, v)))
        .build();
    exporter.set_resource(&resource);
    let scope = InstrumentationScope::builder(source.service.clone()).build();
    let logger = SdkLoggerProvider::builder().build().logger_with_scope(scope.clone());
    let redactor = Redactor::from_env(Vec::new());
    let logs: Vec<_> = records
        .iter()
        .map(|record| {
            let (number, text) = severity(record.level);
            let mut log = logger.create_log_record();
            log.set_timestamp(record.at);
            log.set_observed_timestamp(record.at);
            log.set_severity_number(number);
            log.set_severity_text(text);
            log.set_body(AnyValue::from(redactor.scrub(&record.message).into_owned()));
            let code = record.site.iter().flat_map(Site::attributes).map(|(k, v)| (k.to_string(), v));
            let attributes = record.attributes.iter().map(|(k, v)| (k.clone(), scrubbed(&redactor, k, v))).chain(code);
            log.add_attributes(attributes.map(|(k, v)| (Key::from(k), any_value(v))));
            log
        })
        .collect();
    let batch: Vec<_> = logs.iter().map(|log| (log, &scope)).collect();
    futures_executor::block_on(exporter.export(LogBatch::new(&batch))).map_err(|e| why(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_record_when_printed_then_the_line_is_scrubbed_and_names_its_call_site() {
        let redactor = Redactor::from_values(["hunter2-hunter2".to_string()]);
        let made_at = line!() + 1;
        let record = Record::new("warn", "login hunter2-hunter2").attr("password", "x").attr("count", 3).attr("note", "me@example.com");
        let line: Value = serde_json::from_str(&record.line(&redactor)).unwrap();

        assert_eq!((line["level"].as_str(), line["msg"].as_str()), (Some("warn"), Some("login [REDACTED]")));
        assert_eq!((line["password"].as_str(), line["count"].as_i64(), line["note"].as_str()), (Some("[REDACTED]"), Some(3), Some("[REDACTED]")));
        assert_eq!((line["filename"].as_str(), line["line_number"].as_u64()), (Some(file!()), Some(u64::from(made_at))));
        assert!(line["ts"].as_u64().unwrap() > 1_700_000_000_000);
    }
}
