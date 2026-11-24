//! The provider-account actions on real processes: an agent CLI's self-update, one request to a scripted `codex app-server`, and the Claude
//! account's usage against a local server. The scripted app-server and CLIs are authored doubles, not recordings of the real binaries.
use exec_proto::{Code, ExecError};
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::execute;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn call(ex: &LocalExecutor, tool: &str, args: Value) -> Result<Value, ExecError> {
    execute(ex, "c1", tool, &args, &mut |_, _| {}, &mut |_| {})
}

fn dir() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = std::fs::canonicalize(d.path()).unwrap();
    (d, p)
}

fn program(bin: &Path, name: &str, body: &str) {
    let path = bin.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A Codex whose app-server answers the handshake, the two account reads and the credit spend, and logs each request it got.
fn codex(bin: &Path, log: &Path) {
    program(
        bin,
        "codex",
        &format!(
            r#"[ "$1" = "app-server" ] || exit 2
while IFS= read -r line; do
  printf '%s\n' "$line" >> {log}
  case "$line" in
    *'"method":"initialize"'*) printf '%s\n' 'not json' '{{"id":1,"result":{{"userAgent":"codex-double"}}}}' ;;
    *'"method":"account/read"'*) printf '%s\n' '{{"id":2,"result":{{"account":{{"type":"chatgpt","email":"a@example.test","planType":"plus"}}}}}}' ;;
    *'"method":"account/rateLimits/read"'*) printf '%s\n' '{{"id":2,"result":{{"rateLimits":{{"primary":{{"usedPercent":40,"windowDurationMins":300,"resetsAt":1900000000}},"secondary":{{"usedPercent":10,"windowDurationMins":10080,"resetsAt":1900500000}}}},"rateLimitResetCredits":{{"availableCount":2}}}}}}' ;;
    *'"method":"account/rateLimitResetCredit/consume"'*)
      case "$line" in
        *spent-already*) printf '%s\n' '{{"id":2,"error":{{"code":-32000,"message":"no credit left"}}}}' ;;
        *) printf '%s\n' '{{"id":2,"result":{{"outcome":"reset"}}}}' ;;
      esac ;;
  esac
done"#,
            log = log.display()
        ),
    );
}

fn executor(root: &Path, home: &Path, bin: &Path) -> LocalExecutor {
    LocalExecutor::new(root).unwrap().with_skill_home(Some(home)).with_cli_path(bin.to_str().unwrap())
}

#[test]
fn given_a_codex_with_an_app_server_when_one_request_is_sent_then_the_answer_comes_back_after_the_handshake_and_the_server_is_stopped() {
    // Given an executor with a scripted Codex
    let ((_r, root), (_h, home), (_b, bin), (_l, logs)) = (dir(), dir(), dir(), dir());
    let log = logs.join("requests.log");
    codex(&bin, &log);
    let ex = executor(&root, &home, &bin);
    // When the limits are read, then the account, then a credit is spent
    let limits = call(&ex, "codex_app_server_request", json!({"method": "account/rateLimits/read"})).unwrap();
    let account = call(&ex, "codex_app_server_request", json!({"method": "account/read", "params": {}})).unwrap();
    let spent = call(&ex, "codex_app_server_request", json!({"method": "account/rateLimitResetCredit/consume", "params": {"idempotencyKey": "k-1"}})).unwrap();
    // Then each answer is the server's result, as it sent it
    assert_eq!(limits["result"]["rateLimits"]["primary"]["usedPercent"], 40);
    assert_eq!(limits["result"]["rateLimitResetCredits"]["availableCount"], 2);
    assert_eq!(account["result"]["account"]["planType"], "plus");
    assert_eq!(spent["result"], json!({"outcome": "reset"}));
    // And the server was told its handshake first, in order, and was sent the params it was given
    let seen = std::fs::read_to_string(&log).unwrap();
    let methods: Vec<&str> = seen.lines().map(|l| l.split("\"method\":\"").nth(1).unwrap().split('"').next().unwrap()).collect();
    assert_eq!(methods, ["initialize", "initialized", "account/rateLimits/read", "initialize", "initialized", "account/read", "initialize", "initialized", "account/rateLimitResetCredit/consume"]);
    assert!(seen.contains("\"idempotencyKey\":\"k-1\""), "{seen}");
}

#[test]
fn given_an_app_server_that_refuses_hangs_or_is_missing_when_asked_then_the_refusal_is_its_own_words_and_a_hang_ends_at_the_timeout() {
    // Given a scripted Codex, and a second one whose app-server never answers
    let ((_r, root), (_h, home), (_b, bin), (_l, logs)) = (dir(), dir(), dir(), dir());
    codex(&bin, &logs.join("requests.log"));
    let ex = executor(&root, &home, &bin);
    // When a spend the server refuses is sent
    let refused = call(&ex, "codex_app_server_request", json!({"method": "account/rateLimitResetCredit/consume", "params": {"key": "spent-already"}})).unwrap_err();
    // Then the refusal carries the server's message
    assert_eq!(refused.code, Code::Io);
    assert!(refused.message.contains("no credit left"), "{refused:?}");
    // And a method outside the account methods, params that are no object and no Codex at all are refused before anything runs
    assert_eq!(call(&ex, "codex_app_server_request", json!({"method": "thread/start"})).unwrap_err().code, Code::InvalidArgs);
    assert_eq!(call(&ex, "codex_app_server_request", json!({"method": "account/read", "params": 3})).unwrap_err().code, Code::InvalidArgs);
    let (_b2, empty) = dir();
    assert_eq!(call(&executor(&root, &home, &empty), "codex_app_server_request", json!({"method": "account/read"})).unwrap_err().code, Code::NotFound);
    // And a server that reads its input and never answers ends at the timeout, with no process left behind
    let (_b3, silent) = dir();
    program(&silent, "codex", "echo $$ > \"$HOME/pid\"\ncat > /dev/null");
    let started = std::time::Instant::now();
    let hung = call(&executor(&root, &home, &silent), "codex_app_server_request", json!({"method": "account/read", "timeout_ms": 300})).unwrap_err();
    assert!(hung.message.contains("did not answer in time"), "{hung:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let pid: i32 = std::fs::read_to_string(home.join("pid")).unwrap().trim().parse().unwrap();
    super::wait_for("the silent app-server to be gone", || super::gone(pid));
}

#[test]
fn given_agent_clis_with_and_without_an_update_command_when_updated_then_the_output_and_the_new_version_come_back_and_a_failure_is_not_ok() {
    // Given a claude that updates and reports a new version, an opencode whose upgrade fails, and a codex
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    program(&bin, "claude", "case \"$1\" in update) echo 'Updating from 2.1.0'; echo 'Updated to 2.2.0'; echo 2.2.0 > \"$(dirname \"$0\")/version\" ;; --version) cat \"$(dirname \"$0\")/version\" 2>/dev/null || echo 2.1.0 ;; esac");
    program(&bin, "opencode", "case \"$1\" in upgrade) echo 'no network' >&2; exit 3 ;; --version) echo 0.4.0 ;; esac");
    program(&bin, "codex", "echo codex-cli 0.9.1");
    let ex = executor(&root, &home, &bin);
    // When each is updated
    let claude = call(&ex, "cli_update", json!({"program": "claude"})).unwrap();
    let opencode = call(&ex, "cli_update", json!({"program": "opencode"})).unwrap();
    let codex = call(&ex, "cli_update", json!({"program": "codex"})).unwrap_err();
    // Then claude says it ended well with its words and the version it reports now
    assert_eq!((claude["ok"].clone(), claude["exit_code"].clone(), claude["version"].clone()), (json!(true), json!(0), json!("2.2.0")));
    assert!(claude["output"].as_str().unwrap().contains("Updated to 2.2.0"), "{claude}");
    // And a failed upgrade is not ok and keeps its exit code and what it printed
    assert_eq!((opencode["ok"].clone(), opencode["exit_code"].clone(), opencode["version"].clone()), (json!(false), json!(3), json!("0.4.0")));
    assert!(opencode["output"].as_str().unwrap().contains("no network"), "{opencode}");
    // And codex, which has no update command, is refused
    assert_eq!(codex.code, Code::InvalidArgs);
}

/// Serves `answers` one after another on a local port: each request's text goes to the returned channel, the answer is the status and body.
fn usage_server(answers: Vec<(u16, &'static str)>) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/oauth/usage", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for (status, body) in answers {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            tx.send(String::from_utf8_lossy(&buf[..n]).into_owned()).unwrap();
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    (url, rx)
}

#[test]
fn given_a_claude_login_in_the_executors_home_when_usage_is_read_then_the_status_and_body_come_back_and_the_token_goes_only_to_the_service() {
    // Given a home with the Claude credential file and a local service that answers the usage twice, once refusing
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/.credentials.json"), r#"{"claudeAiOauth":{"accessToken":"tok-secret-1"}}"#).unwrap();
    let (url, seen) = usage_server(vec![(200, r#"{"five_hour":{"utilization":12.5,"resets_at":"2030-01-01T00:00:00Z"},"seven_day":{"utilization":3}}"#), (401, r#"{"error":"expired"}"#)]);
    let ex = executor(&root, &home, &bin).with_claude_usage_url(&url);
    // When the usage is read, and read again after the service refuses
    let ok = call(&ex, "claude_account_request", json!({"request": "usage"})).unwrap();
    let refused = call(&ex, "claude_account_request", json!({"request": "usage"})).unwrap();
    // Then the first is its status and body, and the refusal is the service's status with its body
    assert_eq!((ok["status"].clone(), ok["body"]["five_hour"]["utilization"].clone()), (json!(200), json!(12.5)));
    assert_eq!((refused["status"].clone(), refused["body"]["error"].clone()), (json!(401), json!("expired")));
    // And the service was sent the bearer token and the beta header, and no answer carries the token
    let request = seen.recv().unwrap();
    assert!(request.contains("Authorization: Bearer tok-secret-1") || request.contains("authorization: Bearer tok-secret-1"), "{request}");
    assert!(request.to_lowercase().contains("anthropic-beta: oauth-2025-04-20"), "{request}");
    assert!(!ok.to_string().contains("tok-secret") && !refused.to_string().contains("tok-secret"));
}

#[test]
fn given_no_claude_login_or_an_unknown_request_or_an_unreachable_service_when_usage_is_read_then_each_is_refused_without_leaking_the_token() {
    // Given an empty home
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    let ex = executor(&root, &home, &bin);
    // When usage is read with no login, an unknown request is made, and a login points at a port nothing listens on
    let signed_out = call(&ex, "claude_account_request", json!({"request": "usage"})).unwrap_err();
    let unknown = call(&ex, "claude_account_request", json!({"request": "token"})).unwrap_err();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/.credentials.json"), r#"{"claudeAiOauth":{"accessToken":"tok-secret-2"}}"#).unwrap();
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/usage", closed.local_addr().unwrap());
    drop(closed);
    let down = call(&executor(&root, &home, &bin).with_claude_usage_url(&url), "claude_account_request", json!({"request": "usage"})).unwrap_err();
    // Then each has its own code and no message holds the token
    assert_eq!((signed_out.code, unknown.code, down.code), (Code::NotFound, Code::InvalidArgs, Code::Io));
    assert!(!down.message.contains("tok-secret"), "{down:?}");
}
