//! The OTLP metrics adapter against a local collector: what it sends, how often, and what it does when the collector refuses.
#![cfg(all(feature = "metrics", feature = "testing"))]

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use telemetry::testing::InMemoryMetricsSink;
use telemetry::{Destination, MetricsSink, OtlpMetricsSink, Reading};

struct Request {
    path: String,
    authorization: String,
    body: Value,
}

/// A collector answering every request with `status`, recording what it was sent.
fn collector(status: u16) -> (String, Arc<Mutex<Vec<Request>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut raw = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head, length) = loop {
                let n = stream.read(&mut chunk).unwrap();
                raw.extend_from_slice(&chunk[..n]);
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..end]).to_string();
                    let length = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    raw.drain(..end + 4);
                    break (head, length);
                }
            };
            while raw.len() < length {
                let n = stream.read(&mut chunk).unwrap();
                raw.extend_from_slice(&chunk[..n]);
            }
            let header = |name: &str| head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix(&format!("{name}:")).map(|_| l.split_once(':').unwrap().1.trim().to_string())).unwrap_or_default();
            log.lock().unwrap().push(Request { path: head.split_whitespace().nth(1).unwrap().to_string(), authorization: header("authorization"), body: serde_json::from_slice(&raw).unwrap_or(Value::Null) });
            let body = "{}";
            let _ = write!(stream, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        }
    });
    (host, seen)
}

fn reading(metric: &str, scope: &str, name: &str, value: f64) -> Reading {
    Reading { metric: metric.into(), scope: scope.into(), name: name.into(), value, at: SystemTime::now() }
}

/// `(metric, label scope, label name, value)` of every data point in a request body.
fn points(body: &Value) -> Vec<(String, String, String, f64)> {
    let mut out = Vec::new();
    for resource in body["resourceMetrics"].as_array().into_iter().flatten() {
        for scope in resource["scopeMetrics"].as_array().into_iter().flatten() {
            for metric in scope["metrics"].as_array().into_iter().flatten() {
                for point in metric["gauge"]["dataPoints"].as_array().into_iter().flatten() {
                    let label = |key: &str| point["attributes"].as_array().unwrap().iter().find(|a| a["key"] == key).map(|a| a["value"]["stringValue"].as_str().unwrap().to_string()).unwrap();
                    let value = point["asDouble"].as_f64().or_else(|| point["asInt"].as_str().and_then(|s| s.parse().ok())).unwrap();
                    out.push((metric["name"].as_str().unwrap().to_string(), label("scope"), label("name"), value));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    out
}

#[test]
fn given_a_batch_of_readings_when_recorded_then_one_authorized_request_carries_every_gauge_with_its_labels() {
    // Given a collector and a sink pointed at it
    let (host, seen) = collector(200);
    let sink = OtlpMetricsSink::new(Destination::new(format!("http://{host}"), "tok-123"), "reporting-service");

    // When three readings of two metrics are recorded in one call
    let result = sink.record(&[reading("container.health", "container", "agent-shell-dind", 0.0), reading("container.health", "container", "web", 1.0), reading("host.docker.list_ms", "host", "docker", 42.5)]);

    // Then the call succeeded and exactly one request arrived, with the token, on the metrics path, carrying all three gauges
    assert_eq!(result, Ok(()));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "requests: {}", seen.len());
    assert_eq!((seen[0].path.as_str(), seen[0].authorization.as_str()), ("/v1/metrics", "Bearer tok-123"));
    let expected = [("container.health", "container", "agent-shell-dind", 0.0), ("container.health", "container", "web", 1.0), ("host.docker.list_ms", "host", "docker", 42.5)];
    assert_eq!(points(&seen[0].body), expected.map(|(m, s, n, v)| (m.to_string(), s.to_string(), n.to_string(), v)).to_vec());
}

#[test]
fn given_a_collector_that_refuses_when_a_batch_is_recorded_then_the_caller_gets_an_error_and_the_request_did_arrive() {
    // Given a collector answering 401
    let (host, seen) = collector(401);
    let sink = OtlpMetricsSink::new(Destination::new(format!("http://{host}"), "wrong"), "reporting-service");

    // When a batch is recorded
    let result = sink.record(&[reading("container.health", "container", "web", 1.0)]);

    // Then the caller gets an error, and the collector did see the request (the refusal is the collector's, not a transport failure)
    assert!(result.is_err(), "{result:?}");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn given_an_empty_batch_when_recorded_then_nothing_is_sent() {
    let (host, seen) = collector(200);
    let sink = OtlpMetricsSink::new(Destination::new(format!("http://{host}"), "t"), "reporting-service");

    assert_eq!(sink.record(&[]), Ok(()));

    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn given_the_in_memory_double_when_it_is_failing_then_it_counts_the_attempt_and_keeps_nothing() {
    let sink = InMemoryMetricsSink::failing();

    let result = sink.record(&[reading("container.health", "container", "web", 1.0)]);

    assert!(result.is_err());
    assert_eq!((sink.attempts(), sink.batches().len()), (1, 0));
}
