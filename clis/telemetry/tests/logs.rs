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

/// An HTTPS ingest under a throwaway CA: serves one request with a 200; returns its port and the CA's PEM file.
fn https_ingest() -> (u16, tempfile::TempDir, std::thread::JoinHandle<()>) {
    use rcgen::{BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose};
    use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.distinguished_name.push(DnType::CommonName, "telemetry-test-ca");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let leaf_key = KeyPair::generate().unwrap();
    let mut leaf = CertificateParams::new(vec!["127.0.0.1".to_string()]).unwrap();
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let leaf_cert = leaf.signed_by(&leaf_key, &Issuer::new(ca, ca_key)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ca.pem"), ca_cert.pem()).unwrap();
    let certs = CertificateDer::pem_slice_iter(leaf_cert.pem().as_bytes()).collect::<Result<Vec<_>, _>>().unwrap();
    let key = PrivateKeyDer::from_pem_slice(leaf_key.serialize_pem().as_bytes()).unwrap();
    let config = std::sync::Arc::new(
        rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider())).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(certs, key).unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let worker = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut tls = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), stream);
        let mut raw = Vec::new();
        let mut chunk = [0u8; 4096];
        while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
            match tls.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => raw.extend_from_slice(&chunk[..n]),
            }
        }
        let _ = write!(tls, "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}");
        let _ = tls.flush();
    });
    (port, dir, worker)
}

#[test]
fn given_an_https_ingest_under_a_custom_ca_in_ssl_cert_file_when_a_record_is_sent_then_it_is_trusted() {
    // Given an HTTPS ingest whose certificate comes from a throwaway CA, and that CA named by SSL_CERT_FILE
    let (port, dir, worker) = https_ingest();
    std::env::set_var("SSL_CERT_FILE", dir.path().join("ca.pem"));
    let destination = Destination::new(format!("https://127.0.0.1:{port}"), "tok");

    // When a record is sent
    let result = send(&destination, &source(), &[Record::new("info", "started")]);

    // Then the TLS handshake trusted that root and the ingest received the record
    assert_eq!(result, Ok(()));
    worker.join().unwrap();
}
