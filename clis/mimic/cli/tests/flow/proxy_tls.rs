//! `wf mimic proxy record --allow-host`: the explicit proxy. A client asks for an HTTPS host by name; an approved host is
//! intercepted under a disposable CA and recorded, anything else passes through unread, and what was not covered is said.
use crate::proxy_support::*;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use wf_testkit::MockHttps;

const BEARER: &str = "CANARY-TLS-BEARER-7c1d0e55a9";
const ISSUED: &str = "CANARY-TLS-ISSUED-TOKEN-41b2";

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// Asks the proxy for a tunnel to `target` and returns the open connection, or the status the proxy refused it with.
fn tunnel(proxy: SocketAddr, target: &str) -> Result<TcpStream, u16> {
    let mut stream = TcpStream::connect(proxy).expect("the proxy is listening");
    write!(stream, "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").expect("the request goes out");
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
        head.push(byte[0]);
    }
    let status = String::from_utf8_lossy(&head).split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    if status == 200 { Ok(stream) } else { Err(status) }
}

/// One HTTPS request through the proxy by a client that trusts only the CA in `trusted`: the answer, or why TLS failed.
fn https(proxy: SocketAddr, port: u16, trusted: &Path, request: &str) -> Result<String, String> {
    let stream = tunnel(proxy, &format!("127.0.0.1:{port}")).map_err(|status| format!("the proxy answered {status}"))?;
    let mut roots = rustls::RootCertStore::empty();
    for certificate in CertificateDer::pem_file_iter(trusted).map_err(|e| e.to_string())? {
        roots.add(certificate.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().map_err(|e| e.to_string())?.with_root_certificates(roots).with_no_client_auth();
    let name = ServerName::try_from("127.0.0.1").map_err(|e| e.to_string())?;
    let mut tls = rustls::StreamOwned::new(rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?, stream);
    tls.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let mut answer = Vec::new();
    match tls.read_to_end(&mut answer) {
        Ok(_) => {}
        Err(_) if !answer.is_empty() => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(String::from_utf8_lossy(&answer).into_owned())
}

fn ingest() -> String {
    let body = r#"{"event":"fixture"}"#;
    format!("POST /v1/ingest HTTP/1.1\r\nhost: 127.0.0.1\r\nauthorization: Bearer {BEARER}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len())
}

/// A plain server no test approved: it says back what it is told.
fn echo() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let addr = listener.local_addr().expect("its address");
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut chunk = [0u8; 64];
            if let Ok(n) = stream.read(&mut chunk) {
                let _ = stream.write_all(&chunk[..n]);
            }
        }
    });
    addr
}

#[test]
fn given_an_approved_https_host_when_a_client_uses_the_explicit_proxy_then_it_is_recorded_under_the_test_ca_and_the_rest_is_named_as_gaps() {
    // Given an HTTPS fixture server, another server nobody approved, and a recording that may read the fixture host only
    let dir = tempfile::tempdir().unwrap();
    let upstream = MockHttps::start();
    upstream.reply(&json!({"accepted": true, "access_token": ISSUED}).to_string());
    let other = echo();
    let (set, ca) = (dir.path().join("set"), dir.path().join("mimic-ca.pem"));
    let host = format!("127.0.0.1:{}", upstream.port);
    let mut record = Tool::start(dir.path(), &["record", "--set", &text(&set), "--allow-host", &host, "--upstream-ca", &text(&upstream.ca), "--ca-out", &text(&ca)], &[]);

    // When a client that trusts the test CA posts to the fixture host,
    let answer = https(record.addr, upstream.port, &ca, &ingest()).expect("the request is answered");
    // and a client that trusts only the server's own CA tries the same,
    let distrusting = https(record.addr, upstream.port, &upstream.ca, &ingest());
    // and a client tunnels to the server nobody approved
    let mut through = tunnel(record.addr, &other.to_string()).expect("a tunnel");
    through.write_all(b"unread").unwrap();
    let mut said = [0u8; 6];
    through.read_exact(&mut said).unwrap();
    drop(through);
    let ended = record.stop();

    // Then the first client got the server's answer, and the server got its real credential
    assert!(answer.starts_with("HTTP/1.1 200") && answer.contains(ISSUED), "{answer}");
    assert_eq!(upstream.requests().iter().filter(|r| r.headers.get("authorization").map(String::as_str) == Some(&format!("Bearer {BEARER}"))).count(), 1);
    // and the second was not served, and the tunnel carried bytes the proxy did not read
    assert!(distrusting.is_err(), "{distrusting:?}");
    assert_eq!(&said, b"unread");
    // and the set holds the one exchange, by its full address, with fixture values for the credentials
    assert_eq!((ended.code, &ended.json["ok"], &ended.json["http"], &ended.json["interactions"]), (0, &json!(true), &json!(1), &json!(1)), "{}", ended.stdout);
    let cassette = std::fs::read_to_string(set.join("interactions.jsonl")).unwrap();
    assert!(cassette.contains(&format!("https://{host}/v1/ingest")) && cassette.contains("Bearer fx-authorization-1") && cassette.contains("fx-access-token-1"), "{cassette}");
    // and what was not covered is said: the client that rejected the CA, the host that passed through
    let gaps = &ended.json["gaps"];
    assert_eq!((&gaps["rejected_ca"][&host], &gaps["tunnelled"][&other.to_string()], &gaps["never_seen"]), (&json!(1), &json!(1), &json!([])), "{gaps}");
    // and no artifact holds a credential or a private key: the CA file is its certificate alone
    for (file, content) in files(dir.path()).into_iter().chain([("stdout".into(), ended.stdout.clone()), ("stderr".into(), ended.stderr.clone())]) {
        assert!(!content.contains("CANARY") && !content.contains("PRIVATE KEY"), "{} holds a credential or a key", file.display());
    }
    assert!(std::fs::read_to_string(&ca).unwrap().starts_with("-----BEGIN CERTIFICATE-----"));
}

#[test]
fn given_a_server_whose_certificate_does_not_validate_when_a_client_asks_for_it_then_nothing_is_sent_to_it_and_the_gap_is_named() {
    // Given an HTTPS server under a CA the recording was not told to trust
    let dir = tempfile::tempdir().unwrap();
    let upstream = MockHttps::start();
    let (set, ca) = (dir.path().join("set"), dir.path().join("mimic-ca.pem"));
    let host = format!("127.0.0.1:{}", upstream.port);
    let mut record = Tool::start(dir.path(), &["record", "--set", &text(&set), "--allow-host", &host, "--ca-out", &text(&ca)], &[]);

    // When a client posts to it through the proxy
    let answer = https(record.addr, upstream.port, &ca, &ingest()).expect("the proxy answers");
    let ended = record.stop();

    // Then the client is told the server could not be reached, and the server never saw the request
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
    assert!(upstream.requests().is_empty());
    assert_eq!(&ended.json["gaps"]["untrusted_upstream"][&host], &json!(1), "{}", ended.stdout);
}
