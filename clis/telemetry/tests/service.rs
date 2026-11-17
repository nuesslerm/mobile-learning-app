//! The service subscriber against a local collector: the OTLP/HTTP JSON an ingest accepts for spans and log records.
#![cfg(feature = "service")]

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use telemetry::{build, Destination, Source};

type Received = Arc<Mutex<Vec<(String, String, Value)>>>;

/// A collector that answers every request with 200 and keeps `(path, authorization, body)`.
fn collector() -> (String, Received) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let received: Received = Arc::default();
    let kept = received.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut raw = Vec::new();
            let mut chunk = [0u8; 4096];
            let end = loop {
                let n = stream.read(&mut chunk).unwrap();
                raw.extend_from_slice(&chunk[..n]);
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break end;
                }
            };
            let head = String::from_utf8_lossy(&raw[..end]).to_string();
            let header = |name: &str| head.lines().find(|l| l.to_ascii_lowercase().starts_with(&format!("{name}:"))).map(|l| l.split_once(':').unwrap().1.trim().to_string()).unwrap_or_default();
            let length: usize = header("content-length").parse().unwrap_or(0);
            let mut body = raw[end + 4..].to_vec();
            while body.len() < length {
                let n = stream.read(&mut chunk).unwrap();
                body.extend_from_slice(&chunk[..n]);
            }
            let path = head.split_whitespace().nth(1).unwrap().to_string();
            let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}");
            kept.lock().unwrap().push((path, header("authorization"), serde_json::from_slice(&body).unwrap_or(Value::Null)));
        }
    });
    (host, received)
}

fn source() -> Source {
    Source { service: "svc-test".into(), version: "1.2.3", commit: Some("abc1234".into()), host: Some("vm-main".into()) }
}

fn attr<'a>(attributes: &'a Value, key: &str) -> &'a Value {
    &attributes.as_array().unwrap().iter().find(|a| a["key"] == key).unwrap_or_else(|| panic!("no attribute {key}"))["value"]
}

fn bodies(received: &Received, path: &str) -> Vec<Value> {
    received.lock().unwrap().iter().filter(|(p, _, _)| p == path).map(|(_, _, body)| body.clone()).collect()
}

fn request_under(traceparent: Option<&str>) {
    let span = tracing::info_span!("request", otel.name = "GET /health", http.status_code = tracing::field::Empty, otel.status_code = tracing::field::Empty, traceparent = traceparent);
    let _entered = span.enter();
    tracing::info!(cookie = "abc", note = "carries super-secret-value inside", "test.event");
    span.record("http.status_code", 503u64);
    span.record("otel.status_code", "ERROR");
}

#[test]
fn given_a_request_span_with_a_traceparent_when_it_closes_then_the_ingest_gets_its_span_and_log_in_the_otlp_json_shape() {
    // Given a subscriber exporting to a collector
    let (host, received) = collector();
    let (dispatch, telemetry) = build(source(), vec!["super-secret-value".into()], Some(Destination::new(format!("http://{host}"), "tok-123")));

    // When a sampled request with a traceparent logs a line and closes
    tracing::dispatcher::with_default(&dispatch, || request_under(Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")));
    telemetry.shutdown();

    // Then both requests carry the bearer token
    assert!(received.lock().unwrap().iter().all(|(_, auth, _)| auth == "Bearer tok-123"));

    // and the span is a server span of the caller's trace, a child of the caller's span, named and tagged by the request
    let trace = &bodies(&received, "/v1/traces")[0]["resourceSpans"][0];
    assert_eq!(attr(&trace["resource"]["attributes"], "service.name")["stringValue"], "svc-test");
    assert_eq!(attr(&trace["resource"]["attributes"], "host.name")["stringValue"], "vm-main");
    assert_eq!(trace["scopeSpans"][0]["scope"]["name"], "svc-test");
    let span = &trace["scopeSpans"][0]["spans"][0];
    assert_eq!((span["traceId"].as_str(), span["parentSpanId"].as_str(), span["name"].as_str(), span["kind"].as_u64()), (Some("4bf92f3577b34da6a3ce929d0e0e4736"), Some("00f067aa0ba902b7"), Some("GET /health"), Some(2)));
    assert_eq!(span["spanId"].as_str().unwrap().len(), 16);
    assert_eq!(span["status"]["code"], 2);
    assert_eq!(attr(&span["attributes"], "http.status_code")["intValue"], "503");
    assert!(span["attributes"].to_string().find("traceparent").is_none() && span["attributes"].to_string().find("otel.").is_none());
    assert!(span["endTimeUnixNano"].as_str().unwrap().parse::<u64>().unwrap() >= span["startTimeUnixNano"].as_str().unwrap().parse::<u64>().unwrap());

    // and the log record carries that trace and span, severity, body and call site, with the credential replaced
    let record = &bodies(&received, "/v1/logs")[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
    assert_eq!((record["traceId"].as_str(), record["spanId"].as_str(), record["severityText"].as_str(), record["body"]["stringValue"].as_str()), (Some("4bf92f3577b34da6a3ce929d0e0e4736"), span["spanId"].as_str(), Some("INFO"), Some("test.event")));
    assert!(attr(&record["attributes"], "code.filepath")["stringValue"].as_str().unwrap().ends_with("service.rs"));
    let text = record.to_string();
    assert!(text.contains("[REDACTED]") && !text.contains("super-secret-value") && !text.contains("\"abc\""), "{text}");
}

#[test]
fn given_a_parent_that_is_not_sampled_when_a_request_closes_then_no_span_is_exported_but_its_log_keeps_the_trace() {
    let (host, received) = collector();
    let (dispatch, telemetry) = build(source(), Vec::new(), Some(Destination::new(format!("http://{host}"), "t")));

    tracing::dispatcher::with_default(&dispatch, || request_under(Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00")));
    telemetry.shutdown();

    assert!(bodies(&received, "/v1/traces").is_empty());
    assert_eq!(bodies(&received, "/v1/logs")[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
}

#[test]
fn given_no_traceparent_when_a_nested_span_closes_then_it_is_a_child_in_a_new_trace() {
    let (host, received) = collector();
    let (dispatch, telemetry) = build(source(), Vec::new(), Some(Destination::new(format!("http://{host}"), "t")));

    tracing::dispatcher::with_default(&dispatch, || {
        let outer = tracing::info_span!("outer");
        let _outer = outer.enter();
        let _inner = tracing::info_span!("inner").entered();
    });
    telemetry.shutdown();

    let spans = bodies(&received, "/v1/traces")[0]["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap().clone();
    let find = |name: &str| spans.iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("no span {name}"));
    let (outer, inner) = (find("outer"), find("inner"));
    assert_eq!(outer["traceId"].as_str().unwrap().len(), 32);
    assert!(outer.get("parentSpanId").is_none());
    assert_eq!((inner["traceId"].as_str(), inner["parentSpanId"].as_str()), (outer["traceId"].as_str(), outer["spanId"].as_str()));
}

#[test]
fn given_no_ingest_when_a_request_is_traced_then_nothing_is_exported() {
    let (dispatch, telemetry) = build(source(), Vec::new(), None);

    tracing::dispatcher::with_default(&dispatch, || request_under(None));

    assert!(!telemetry.is_exporting());
}
