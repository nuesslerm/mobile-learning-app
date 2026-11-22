//! The scripted `opencode serve` as the executor's bridge starts it (loopback, a password in the environment): it refuses a caller without the credential, serves the
//! sessions and the event stream of the subset the adapter uses, plays an unscripted prompt as a turn that says nothing and ends, and keeps its sessions across starts.
use exec_testkit::{Script, ScriptedCli, Turn};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const PASSWORD: &str = "a-password-of-this-test";

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(cli: &ScriptedCli) -> Server {
    let mut child = Command::new(cli.dir().join("opencode")).args(["serve", "--hostname", "127.0.0.1", "--port", "0"]).env_clear().env("OPENCODE_SERVER_PASSWORD", PASSWORD).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let port = line.trim().rsplit(':').next().unwrap().parse().unwrap_or_else(|_| panic!("no port in {line:?}"));
    Server { child, port }
}

fn basic(user: &str, password: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let text = format!("{user}:{password}");
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { TABLE[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    format!("Basic {out}")
}

/// One request and its status and body.
fn call(server: &Server, method: &str, path: &str, auth: Option<&str>, body: Option<Value>) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let auth = auth.map(|a| format!("Authorization: {a}\r\n")).unwrap_or_default();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Connection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    let status = text.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, text.split("\r\n\r\n").nth(1).unwrap_or_default().to_string())
}

#[test]
fn given_the_scripted_opencode_server_when_called_then_it_wants_the_credential_serves_a_session_and_plays_an_unscripted_prompt_as_a_turn_that_ends() {
    // Given a scripted opencode with no script, started with a password
    let cli = ScriptedCli::install();
    let server = start(&cli);
    let auth = basic("rca", PASSWORD);
    // When it is called without the credential, with a wrong one, and with the right one
    let none = call(&server, "GET", "/global/health", None, None).0;
    let wrong = call(&server, "GET", "/global/health", Some(&basic("rca", "nope")), None).0;
    let (health, body) = call(&server, "GET", "/global/health", Some(&auth), None);
    // Then only the right one is served
    assert_eq!((none, wrong, health), (401, 401, 200));
    assert!(body.contains("healthy"));
    // When a session is made, an unknown one asked for, and a prompt sent while the event stream is open
    let stream = {
        let mut s = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(s, "GET /global/event HTTP/1.1\r\nHost: x\r\nAuthorization: {auth}\r\n\r\n").unwrap();
        s
    };
    let (made, created) = call(&server, "POST", "/session", Some(&auth), Some(json!({"permission": []})));
    let id = serde_json::from_str::<Value>(&created).unwrap()["id"].as_str().unwrap().to_string();
    let unknown = call(&server, "GET", "/session/ses_nope", Some(&auth), None).0;
    let prompt = call(&server, "POST", &format!("/session/{id}/prompt_async"), Some(&auth), Some(json!({"parts": [{"type": "text", "text": "hi"}]}))).0;
    // Then the session exists, the unknown one is 404, the prompt is taken with 204, and the stream says the session is idle, having said nothing of its own
    assert_eq!((made, unknown, prompt), (200, 404, 204));
    let mut seen = String::new();
    let mut lines = BufReader::new(stream);
    while !seen.contains("session.idle") {
        let mut line = String::new();
        assert!(lines.read_line(&mut line).unwrap() > 0, "the stream ended: {seen}");
        seen.push_str(&line);
    }
    assert!(seen.contains("server.connected") && seen.contains("session.status") && !seen.contains("message.part.delta"), "{seen}");
    // And the requests it logged never hold the credential, and a start that follows knows the session
    let log = std::fs::read_to_string(cli.dir().join("opencode.http.jsonl")).unwrap();
    assert!(!log.contains(PASSWORD) && log.contains("\"authorized\":false"));
    drop(server);
    cli.script("opencode", &Script::of(vec![Turn::default()]));
    let again = start(&cli);
    assert_eq!(call(&again, "GET", &format!("/session/{id}"), Some(&auth), None).0, 200);
}
