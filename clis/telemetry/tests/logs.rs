//! The one-shot log sender against a local collector: the OTLP/HTTP JSON structure an ingest accepts.
#![cfg(feature = "oneshot")]

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use telemetry::oneshot::{send, Record};
use telemetry::{Destination, Source};

/// A collector answering one request with `status`; yields `(path, authorization, content-type, body)`.
fn collector(status: u16) -> (String, mpsc::Receiver<(String, String, String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
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
        let _ = write!(stream, "HTTP/1.1 {status} X\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}");
        tx.send((path, header("authorization"), header("content-type"), serde_json::from_slice(&body).unwrap_or(Value::Null))).unwrap();
    });
    (host, rx)
}

fn source() -> Source {
    Source { service: "svc-test".into(), version: "1.2.3", commit: Some("abc1234".into()), host: Some("vm-main".into()) }
}

fn attr<'a>(attributes: &'a Value, key: &str) -> &'a Value {
    &attributes.as_array().unwrap().iter().find(|a| a["key"] == key).unwrap_or_else(|| panic!("no attribute {key}"))["value"]
}

#[test]
fn given_two_records_when_sent_then_one_authorized_json_request_has_the_otlp_log_structure() {
    // Given a collector and a destination pointing at it
    let (host, rx) = collector(200);
    let destination = Destination::new(format!("http://{host}"), "tok-123");

    // When a warn record with typed attributes and an info record are sent together
    let records = [Record::new("warn", "disk.low").attr("free_mb", 12).attr("ratio", 0.5).attr("ok", false).attr("mount", "/data"), Record::new("info", "started")];
    let result = send(&destination, &source(), &records);

    // Then the call succeeded and the request is a JSON POST to the logs path with the bearer token
    assert_eq!(result, Ok(()));
    let (path, auth, content_type, body) = rx.recv().unwrap();
    assert_eq!((path.as_str(), auth.as_str(), content_type.as_str()), ("/v1/logs", "Bearer tok-123", "application/json"));

    // and the resource carries the service and source, the scope the service name, and both records their severity, body and typed attributes
    let resource = &body["resourceLogs"][0]["resource"]["attributes"];
    assert_eq!(attr(resource, "service.name")["stringValue"], "svc-test");
    assert_eq!((attr(resource, "service.version")["stringValue"].as_str(), attr(resource, "host.name")["stringValue"].as_str()), (Some("1.2.3"), Some("vm-main")));
    let scope = &body["resourceLogs"][0]["scopeLogs"][0];
    assert_eq!(scope["scope"]["name"], "svc-test");
    let logs = scope["logRecords"].as_array().unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!((logs[0]["severityNumber"].as_u64(), logs[0]["severityText"].as_str(), logs[0]["body"]["stringValue"].as_str()), (Some(13), Some("WARN"), Some("disk.low")));
    assert_eq!((logs[1]["severityNumber"].as_u64(), logs[1]["severityText"].as_str()), (Some(9), Some("INFO")));
    assert!(logs[0]["timeUnixNano"].as_str().unwrap().parse::<u64>().unwrap() > 1_700_000_000_000_000_000);
    let attrs = &logs[0]["attributes"];
    assert_eq!(attr(attrs, "free_mb")["intValue"], "12");
    assert_eq!(attr(attrs, "ratio")["doubleValue"], 0.5);
    assert_eq!(attr(attrs, "ok")["boolValue"], false);
    assert_eq!(attr(attrs, "mount")["stringValue"], "/data");
    assert!(attr(attrs, "code.filepath")["stringValue"].as_str().unwrap().ends_with("logs.rs"));
}

#[test]
fn given_an_ingest_that_refuses_when_a_record_is_sent_then_the_error_names_the_http_status() {
    let (host, rx) = collector(401);
    let destination = Destination::new(format!("http://{host}"), "wrong");

    let result = send(&destination, &source(), &[Record::new("info", "started")]);

    assert_eq!(result, Err("HTTP 401".to_string()));
    assert!(rx.recv().is_ok());
}

#[test]
fn given_a_secret_in_a_message_attribute_when_sent_then_the_ingest_never_sees_it() {
    let (host, rx) = collector(200);
    let destination = Destination::new(format!("http://{host}"), "t");

    send(&destination, &source(), &[Record::new("info", "login").attr("password", "hunter2-hunter2").attr("mail", "me@example.com")]).unwrap();

    let (_, _, _, body) = rx.recv().unwrap();
    let text = body.to_string();
    assert!(!text.contains("hunter2-hunter2") && !text.contains("me@example.com"), "{text}");
}
