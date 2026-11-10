//! Against the deployed service (`ORCA_SERVE_BASE_URL`, default `http://orca-serve:6768` on the tailnet): its `/health` names the commit the pipeline
//! expects, and a phone paired for this check (`ORCA_VERIFY_PAIR_URL`) handshakes, authenticates and asks the read-only `status.get`, while a wrong
//! token is refused. Nothing is written to the service.
use orca_crypto::pairing::PairingOffer;
use orca_serve::testing::PhoneClient;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

fn authority() -> String {
    std::env::var("ORCA_SERVE_BASE_URL").unwrap_or_else(|_| "http://orca-serve:6768".to_string()).trim_start_matches("http://").trim_end_matches('/').to_string()
}

/// From the environment, else from the vault through `wf secret get`.
fn setting(key: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            let out = std::process::Command::new("wf").args(["secret", "get", key]).output().ok()?;
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| panic!("{key} is unset and `wf secret get {key}` gave nothing; pair a device on the deployed service (docker exec orca-serve orca-serve pair --endpoint ws://orca-serve:6768) and store its link under {key}"))
}

fn resolve(authority: &str) -> SocketAddr {
    authority.to_socket_addrs().unwrap_or_else(|e| panic!("{authority} does not resolve (the runner must be on the tailnet): {e}")).next().expect("an address")
}

fn health() -> Value {
    let mut stream = TcpStream::connect_timeout(&resolve(&authority()), Duration::from_secs(10)).expect("the deployed service listens");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(format!("GET /health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", authority()).as_bytes()).unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "/health answered: {answer}");
    serde_json::from_str(answer.split("\r\n\r\n").nth(1).expect("a body")).expect("a JSON body")
}

fn expected_commit() -> Option<String> {
    let all: Value = serde_json::from_str(&std::env::var("WF_EXPECTED_REVISIONS").ok().filter(|v| !v.is_empty())?).ok()?;
    all.as_object()?.get("orca-serve")?.as_str().map(str::to_string)
}

#[test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
fn given_the_deployed_service_when_health_is_asked_then_it_is_orca_serve_on_the_pushed_commit() {
    // When health is asked
    let health = health();
    // Then it is ok, names itself and runs the commit the pipeline expects
    assert_eq!((health["ok"].as_bool(), health["service"].as_str()), (Some(true), Some("orca-serve")), "{health}");
    let commit = health["commit"].as_str().unwrap_or_default();
    assert!(!commit.is_empty() && commit != "unknown", "/health names no commit: {health}");
    if let Some(expected) = expected_commit() {
        assert!(expected.starts_with(commit) || commit.starts_with(&expected), "built from {commit}, expected {expected}: {health}");
    }
}

#[test]
#[ignore = "against the deployed service: run by test:verify:deployed, and only with WF_DEPLOYED=1"]
fn given_a_device_paired_for_the_check_when_it_connects_then_status_get_answers_and_a_wrong_token_is_unauthorized() {
    if std::env::var("WF_DEPLOYED").as_deref() != Ok("1") {
        eprintln!("skipped: the paired phone's check runs only with WF_DEPLOYED=1");
        return;
    }
    // Given the pairing link of a device paired with the deployed service
    let offer = PairingOffer::parse(&setting("ORCA_VERIFY_PAIR_URL")).expect("ORCA_VERIFY_PAIR_URL is an orca://pair link of a direct host");
    let host_public = offer.host_public().expect("the link carries the host's public key");
    let addr = resolve(offer.endpoint.trim_start_matches("ws://").trim_end_matches('/'));
    // When that phone handshakes, authenticates and asks status.get
    let mut phone = PhoneClient::open_v2(addr, host_public, &offer.device_token);
    let status = phone.call("status.get", json!({}));
    // Then the host answers with its identity and the runtime id its link was made for
    assert_eq!(status["ok"], true, "{status}");
    assert!(status["result"]["machineName"].is_string() && status["_meta"]["runtimeId"].is_string(), "{status}");
    // And a phone with another token gets the sealed unauthorized answer and a closed socket
    let mut stranger = PhoneClient::connect(addr, host_public);
    stranger.hello_v2().expect("e2ee_ready");
    let refused = stranger.authenticate("0123456789abcdef").expect("a sealed answer");
    assert_eq!(refused["error"]["code"], "unauthorized", "{refused}");
    assert_eq!(stranger.wait_closed().code, 4001);
}
