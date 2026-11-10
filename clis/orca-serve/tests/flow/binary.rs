//! The thin binary `orca-serve` against a fake agent API: pairing, startup checks and a phone's handshake and request, end to end over sockets.
use crate::support::*;
use orca_crypto::pairing::PairingOffer;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tungstenite::handshake::server::{ErrorResponse, Request as HttpRequest, Response as HttpResponse};
use tungstenite::Message;

const BEARER: &str = "rca-device-bearer";

/// An agent API that answers `service.status` and `state.get` and keeps what it was asked and by whom.
struct FakeRca {
    url: String,
    methods: Arc<Mutex<Vec<String>>>,
    bearers: Arc<Mutex<Vec<Option<String>>>>,
    stop: Arc<AtomicBool>,
    port: u16,
}

impl FakeRca {
    fn start(status: Value) -> FakeRca {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (methods, bearers, stop) = (Arc::new(Mutex::new(vec![])), Arc::new(Mutex::new(vec![])), Arc::new(AtomicBool::new(false)));
        let (m, b, s) = (methods.clone(), bearers.clone(), stop.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if s.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                let (m, b, status) = (m.clone(), b.clone(), status.clone());
                std::thread::spawn(move || play(stream, &m, &b, &status));
            }
        });
        FakeRca { url: format!("ws://127.0.0.1:{port}"), methods, bearers, stop, port }
    }
}

impl Drop for FakeRca {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

#[allow(clippy::result_large_err)]
fn play(stream: TcpStream, methods: &Mutex<Vec<String>>, bearers: &Mutex<Vec<Option<String>>>, status: &Value) {
    let accepted = tungstenite::accept_hdr(stream, |req: &HttpRequest, resp: HttpResponse| -> Result<HttpResponse, ErrorResponse> {
        let auth = req.headers().get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
        let good = auth.as_deref() == Some(&format!("Bearer {BEARER}"));
        bearers.lock().unwrap().push(auth);
        if good {
            Ok(resp)
        } else {
            Err(tungstenite::http::Response::builder().status(401).body(Some("unauthorized".to_string())).unwrap())
        }
    });
    let Ok(mut ws) = accepted else { return };
    while let Ok(Message::Text(text)) = ws.read() {
        let request: Value = serde_json::from_str(text.as_str()).unwrap();
        let method = request["method"].as_str().unwrap().to_string();
        let result = match method.as_str() {
            "service.status" => status.clone(),
            "state.get" => json!({"value": null}),
            other => panic!("the server asked rca for {other}"),
        };
        methods.lock().unwrap().push(method);
        ws.send(Message::text(json!({"id": request["id"], "result": result}).to_string())).unwrap();
    }
}

struct Server {
    child: Child,
    addr: SocketAddr,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn orca_serve() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_orca-serve"));
    c.env_remove("ORCA_RCA_BEARER").env_remove("ORCA_RCA_URL").env_remove("ORCA_STATE_DIR");
    c
}

fn output_json(c: &mut Command) -> (Value, i32) {
    let out = c.output().expect("the binary runs");
    let line = String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().to_string();
    (serde_json::from_str(&line).unwrap_or_else(|_| panic!("stdout is not JSON: {line:?} stderr: {}", String::from_utf8_lossy(&out.stderr))), out.status.code().unwrap_or(-1))
}

fn serve(dir: &std::path::Path, rca: &FakeRca) -> Server {
    let mut child = orca_serve().args(["serve", "--port", "0", "--state-dir"]).arg(dir).args(["--rca-url", &rca.url, "--label", "flow host"]).env("ORCA_RCA_BEARER", BEARER).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let listening: Value = serde_json::from_str(&line).unwrap_or_else(|_| panic!("serve printed no listening line: {line:?}"));
    assert_eq!(listening["event"], "listening");
    let port = listening["port"].as_u64().unwrap();
    Server { child, addr: format!("127.0.0.1:{port}").parse().unwrap() }
}

fn pair(dir: &std::path::Path) -> PairingOffer {
    let (out, code) = output_json(orca_serve().args(["pair", "--endpoint", "ws://phone.example:6768", "--state-dir"]).arg(dir));
    assert_eq!((code, out["ok"].clone()), (0, json!(true)), "{out}");
    PairingOffer::parse(out["url"].as_str().unwrap()).expect("a link the phone can read")
}

#[test]
fn given_a_pairing_link_when_the_server_runs_then_the_phone_connects_authenticates_and_status_comes_from_the_service() {
    // Given a state directory paired once and a server in front of a fake rca
    let dir = tempfile::tempdir().unwrap();
    let rca = FakeRca::start(recorded_status_result());
    let offer = pair(dir.path());
    let server = serve(dir.path(), &rca);
    // When the phone, holding only what the link carries, connects and asks for status
    let mut phone = PhoneClient::open_v2(server.addr, offer.host_public().expect("the link carries the host key"), &offer.device_token);
    let status = phone.call("status.get", json!({}));
    // Then the answer is the host's, rca was asked with its bearer, and /health answers without authentication
    assert_eq!(status["result"]["machineName"], "flow host");
    assert_eq!(status["result"]["pairedDeviceId"], offer.paired_device_id.clone().unwrap().as_str());
    assert!(status["result"].get("degradations").is_none(), "{status}");
    assert!(rca.methods.lock().unwrap().iter().filter(|m| *m == "service.status").count() >= 2, "startup check and status.get both asked");
    assert!(rca.bearers.lock().unwrap().iter().all(|b| b.as_deref() == Some(&format!("Bearer {BEARER}"))), "every call to rca carried the bearer");
    let mut health = TcpStream::connect(server.addr).unwrap();
    health.write_all(b"GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let mut raw = String::new();
    health.read_to_string(&mut raw).unwrap();
    assert!(raw.starts_with("HTTP/1.1 200") && raw.contains("\"service\":\"orca-serve\""), "{raw}");
}

#[test]
fn given_a_server_when_a_wrong_token_connects_then_it_is_unauthorized_and_a_device_paired_later_is_known_without_a_restart() {
    // Given a running server with no device paired yet
    let dir = tempfile::tempdir().unwrap();
    let rca = FakeRca::start(recorded_status_result());
    let server = serve(dir.path(), &rca);
    let offer = pair(dir.path());
    let host_public = offer.host_public().unwrap();
    // When a phone presents a token nobody holds
    let mut stranger = PhoneClient::connect(server.addr, host_public);
    stranger.hello_v2().expect("e2ee_ready");
    let answer = stranger.authenticate("0123456789abcdef").expect("a sealed answer");
    // Then it gets unauthorized, while the device paired after the server started is accepted
    assert_eq!(answer["error"]["code"], "unauthorized");
    assert_eq!(stranger.wait_closed().code, 4001);
    let mut phone = PhoneClient::open_v2(server.addr, host_public, &offer.device_token);
    assert_eq!(phone.call("status.get", json!({"probe": true}))["ok"], true);
}

#[test]
fn given_the_state_directory_when_paired_and_served_then_secrets_are_private_and_the_host_key_is_stable_across_restarts() {
    use std::os::unix::fs::PermissionsExt;
    // Given a paired state directory and a server started on it twice
    let dir = tempfile::tempdir().unwrap();
    let rca = FakeRca::start(recorded_status_result());
    let first = pair(dir.path());
    drop(serve(dir.path(), &rca));
    let second = pair(dir.path());
    // Then the files holding secrets are readable by the owner only, the host key did not change, and the devices are distinct
    for file in ["host-key.json", "devices.json"] {
        let mode = std::fs::metadata(dir.path().join(file)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{file}");
    }
    assert_eq!(first.host_public(), second.host_public());
    assert_ne!(first.device_token, second.device_token);
}

/// Every file under `dir` with its bytes.
fn snapshot(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(path.strip_prefix(root).unwrap().display().to_string(), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn given_a_restored_state_directory_when_the_server_starts_serves_and_stops_then_every_file_in_it_is_byte_identical() {
    // Given the state a backup restored: a paired device, the host key, and a runtime id and an extra file of the restored host's own
    let dir = tempfile::tempdir().unwrap();
    let rca = FakeRca::start(recorded_status_result());
    let offer = pair(dir.path());
    std::fs::write(dir.path().join("runtime-id"), "restored-runtime-id\n").unwrap();
    std::fs::write(dir.path().join("notes-from-the-old-host.txt"), "kept\n").unwrap();
    let before = snapshot(dir.path());

    // When the server starts on it, a phone paired before the backup connects and asks, and the server is stopped
    let server = serve(dir.path(), &rca);
    let mut phone = PhoneClient::open_v2(server.addr, offer.host_public().unwrap(), &offer.device_token);
    let status = phone.call("status.get", json!({}));
    drop(phone);
    drop(server);

    // Then the restored phone is still known (same host key, same device) and no file was created over, rewritten or removed
    assert_eq!(status["result"]["pairedDeviceId"], offer.paired_device_id.clone().unwrap().as_str());
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn given_a_service_that_cannot_be_worked_with_when_serve_starts_then_it_exits_1_and_says_what_to_fix() {
    let dir = tempfile::tempdir().unwrap();
    // Given no bearer for rca
    let rca = FakeRca::start(recorded_status_result());
    let (out, code) = output_json(orca_serve().args(["serve", "--port", "0", "--rca-url", &rca.url, "--state-dir"]).arg(dir.path()));
    assert_eq!((code, out["ok"].clone()), (1, json!(false)));
    assert!(out["error"].as_str().unwrap().contains("ORCA_RCA_BEARER"), "{out}");
    // Given a service that speaks another protocol, and one that lacks the state group
    let mut other = recorded_status_result();
    other["protocol"] = json!(other["protocol"].as_u64().unwrap() + 1);
    let mut no_state = recorded_status_result();
    no_state["groups"] = json!(["service"]);
    for (status, expected) in [(other, "protocol"), (no_state, "group(s) state")] {
        let rca = FakeRca::start(status);
        let (out, code) = output_json(orca_serve().args(["serve", "--port", "0", "--rca-url", &rca.url, "--state-dir"]).arg(dir.path()).env("ORCA_RCA_BEARER", BEARER));
        // Then each refuses to start
        assert_eq!((code, out["ok"].clone()), (1, json!(false)));
        assert!(out["error"].as_str().unwrap().contains(expected), "{out}");
    }
    // Given a bearer rca does not know, serve refuses too and the refusal reached rca
    let (out, code) = output_json(orca_serve().args(["serve", "--port", "0", "--rca-url", &rca.url, "--state-dir"]).arg(dir.path()).env("ORCA_RCA_BEARER", "wrong"));
    assert_eq!((code, out["ok"].clone()), (1, json!(false)));
    assert!(rca.bearers.lock().unwrap().iter().any(|b| b.as_deref() == Some("Bearer wrong")), "rca saw the wrong bearer");
}

#[test]
fn given_the_api_verb_when_run_then_it_prints_the_method_table_without_rca() {
    // Given no rca and no state directory
    // When the table is asked for
    let (table, code) = output_json(orca_serve().arg("api"));
    // Then it lists the served methods and the not-offered rows, each once
    assert_eq!(code, 0);
    let rows = table["methods"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["method"] == "status.get" && r["state"] == "served"));
    assert!(rows.iter().any(|r| r["method"] == "pairing.provisionRelay" && r["state"] == "not-offered"));
}
