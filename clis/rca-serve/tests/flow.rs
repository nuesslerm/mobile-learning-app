use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

struct Gateway(Child, String);
impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct State(PathBuf);
impl Drop for State {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn cli() -> Command {
    if let Some(bin) = std::env::var_os("WF_BIN") {
        let mut cmd = Command::new(bin);
        cmd.arg("rca-serve");
        cmd
    } else {
        Command::new(env!("CARGO_BIN_EXE_rca-serve"))
    }
}
fn start(dir: &Path) -> Gateway {
    let mut child = cli()
        .args(["serve", "--state-dir"])
        .arg(dir)
        .env("RCA_MOBILE_UPSTREAM", "ws://127.0.0.1:1")
        .env("RCA_MOBILE_BEARER", "test")
        .env("RCA_MOBILE_BIND", "127.0.0.1:0")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let ready: Value = serde_json::from_str(&line).unwrap();
    Gateway(child, ready["address"].as_str().unwrap().into())
}
fn request(server: &Gateway, method: &str, token: &str, body: Value) -> (u16, Value) {
    call(&server.1, method, "/v1/pair", token, body)
}
fn call(address: &str, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = body.to_string();
    write!(socket,"{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    let (headers, json) = response.split_once("\r\n\r\n").unwrap();
    (
        headers.split_whitespace().nth(1).unwrap().parse().unwrap(),
        serde_json::from_str(json).unwrap(),
    )
}

#[test]
fn given_one_invite_when_a_device_pairs_then_it_survives_restart_until_revoked() {
    let state = State(std::env::temp_dir().join(Uuid::new_v4().to_string()));
    let output = cli()
        .args(["pair", "--state-dir"])
        .arg(&state.0)
        .output()
        .unwrap();
    assert!(output.status.success());
    let invite: Value = serde_json::from_slice(&output.stdout).unwrap();
    let gateway = start(&state.0);
    let payload = json!({"code":invite["code"]});
    let (status, paired) = request(&gateway, "POST", "", payload.clone());
    assert_eq!(status, 200);
    assert_eq!(request(&gateway, "POST", "", payload).0, 401);
    drop(gateway);
    let gateway = start(&state.0);
    let token = paired["token"].as_str().unwrap();
    assert_eq!(request(&gateway, "DELETE", token, Value::Null).0, 200);
    assert_eq!(request(&gateway, "DELETE", token, Value::Null).0, 401);
}

#[test]
#[ignore = "requires an authenticated mobile gateway, RCA and an attached executor"]
fn given_a_real_executor_when_a_client_loses_its_receipt_then_its_turn_resumes_once() {
    let address = std::env::var("RCA_TEST_MOBILE_ADDRESS").unwrap();
    let token = std::env::var("RCA_TEST_DEVICE_TOKEN").unwrap();
    let workspace = std::env::var("RCA_TEST_WORKSPACE").unwrap();
    let thread = Uuid::new_v4().to_string();
    let turn = Uuid::new_v4().to_string();
    let (status, _) = call(
        &address,
        "POST",
        &format!("/v1/workspaces/{workspace}/threads"),
        &token,
        json!({"title":"Mobile reconnect integration","thread":thread,"driver":"codex"}),
    );
    assert_eq!(status, 200);
    let url = format!("/v1/threads/{thread}/turns/{turn}");
    let prompt =
        json!({"prompt":"Reply with the word connected. Do not run tools or change files."});
    assert_eq!(call(&address, "POST", &url, &token, prompt.clone()).0, 200);
    let history = format!("/v1/threads/{thread}/history");
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    let events = loop {
        let (_, page) = call(&address, "GET", &history, &token, Value::Null);
        let events = page["events"].as_array().unwrap();
        if events.iter().any(|e| e["type"] == "turn.finished") {
            break events.clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "executor did not finish the turn"
        );
        std::thread::sleep(Duration::from_millis(350));
    };
    assert!(events
        .iter()
        .any(|e| e["role"] == "assistant" && e["type"] == "message.delta"));
    let (status, recovered) = call(&address, "POST", &url, &token, prompt);
    assert_eq!(status, 200);
    assert_eq!(recovered["turn"], turn);
    let (_, page) = call(&address, "GET", &history, &token, Value::Null);
    assert_eq!(page["events"].as_array().unwrap(), &events);
    let cursor = events[0]["seq"].as_u64().unwrap();
    let uri = format!("ws://{address}/v1/threads/{thread}/events?after={cursor}");
    let tcp = TcpStream::connect(&address).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let (mut socket, _) = tungstenite::client(uri.as_str(), tcp).unwrap();
    socket
        .send(tungstenite::Message::Text(
            json!({"token":token}).to_string().into(),
        ))
        .unwrap();
    let frame: Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
    assert_eq!(frame["event"], events[1]);
}
