//! The real `t3-serve` binary: `pair` and `serve` as separate processes sharing a SQLite state directory.
use super::support::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_t3-serve");

struct Server {
    child: Child,
    port: u16,
    errlog: std::path::PathBuf,
}

impl Server {
    fn start(state: &std::path::Path, extra: &[&str]) -> Server {
        let errlog = state.join(format!("serve-{}.err", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut child = Command::new(BIN)
            .args(["t3", "serve", "--port", "0", "--state-dir"])
            .arg(state)
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(&errlog).unwrap())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
        let listening: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"));
        assert_eq!((listening["ok"].clone(), listening["event"].as_str()), (json!(true), Some("listening")));
        Server { port: listening["port"].as_u64().unwrap() as u16, child, errlog }
    }

    fn addr(&self) -> std::net::SocketAddr {
        std::net::SocketAddr::from(([127, 0, 0, 1], self.port))
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.errlog).unwrap()
    }

    fn stop(mut self) -> std::path::PathBuf {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
        self.errlog.clone()
    }
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).stdin(Stdio::null()).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

fn pair(state: &std::path::Path, extra: &[&str]) -> Value {
    let mut args = vec!["t3", "pair", "--state-dir", state.to_str().unwrap()];
    args.extend(extra);
    let (code, stdout, stderr) = run(&args);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout.trim().lines().count(), 1, "{stdout}");
    serde_json::from_str(&stdout).unwrap()
}

fn socket_for(server: &Server, token: &str) -> Rpc {
    let t = ticket(server.addr(), token);
    open_socket(server.addr(), &format!("orchestrationProtocol=2&wsTicket={}", t.body["ticket"].as_str().unwrap())).expect("socket")
}

#[test]
fn pair_prints_one_json_object_with_a_url_a_12_character_code_and_the_default_scopes() {
    // Given a state directory and a base URL
    let dir = tempfile::tempdir().unwrap();
    // When a code is minted
    let out = pair(dir.path(), &["--base-url", "http://rca.tailnet:3774/"]);
    // Then the object carries the URL <base>/pair#token=<code>, the ttl, the scopes and the environment id
    let code = out["code"].as_str().unwrap();
    assert_eq!(code.len(), 12);
    assert!(code.chars().all(|c| "23456789ABCDEFGHJKLMNPQRSTUVWXYZ".contains(c)));
    assert_eq!(out["url"], format!("http://rca.tailnet:3774/pair#token={code}"));
    assert_eq!((out["ok"].clone(), out["ttlSeconds"].as_i64()), (json!(true), Some(300)));
    assert_eq!(out["scopes"], json!(["orchestration:read", "orchestration:operate", "terminal:read", "terminal:operate", "filesystem:read", "filesystem:write"]));
    // And the same state directory gives the same environment id again, and a different code
    let second = pair(dir.path(), &[]);
    assert_eq!(second["environmentId"], out["environmentId"]);
    assert_ne!(second["code"], out["code"]);
}

#[test]
fn pair_refuses_an_unknown_scope_with_the_error_and_the_next_step_and_a_bad_ttl_as_usage() {
    let dir = tempfile::tempdir().unwrap();
    // Given a scope that is not one of the 17
    let (code, stdout, _) = run(&["t3", "pair", "--state-dir", dir.path().to_str().unwrap(), "--scope", "orchestration:read,root:everything"]);
    // Then it is ok:false with exit 1, naming the scope and the next step
    let out: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!((code, out["ok"].clone()), (1, json!(false)));
    assert!(out["error"].as_str().unwrap().contains("root:everything") && out["next"].is_string(), "{out}");
    // And a ttl of zero is a malformed line (exit 2) with nothing on stdout
    let (code, stdout, stderr) = run(&["t3", "pair", "--state-dir", dir.path().to_str().unwrap(), "--ttl", "0"]);
    assert_eq!((code, stdout.as_str()), (2, ""));
    assert!(stderr.contains("--ttl"), "{stderr}");
}

#[test]
fn a_code_minted_while_serve_runs_pairs_a_client_once_and_a_wrong_or_expired_code_never_does() {
    // Given a running server and a code minted by a separate `wf t3 pair` process on the same state directory
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start(dir.path(), &[]);
    let minted = pair(dir.path(), &["--label", "phone"]);
    let code = minted["code"].as_str().unwrap();
    let short = pair(dir.path(), &["--ttl", "1"]);

    // When the code is exchanged, the same code again, a wrong code, and the one-second code after it ran out
    let first = exchange(server.addr(), code);
    let reused = exchange(server.addr(), code);
    let wrong = exchange(server.addr(), "ZZZZZZZZZZZZ");
    std::thread::sleep(std::time::Duration::from_millis(1300));
    let expired = exchange(server.addr(), short["code"].as_str().unwrap());

    // Then the first pairs, the others are 401, and the server's own log shows each attempt arrived with its reason
    assert_eq!((first.status, reused.status, wrong.status, expired.status), (200, 401, 401, 401));
    let log = server.log();
    for needle in ["oauth.exchange.ok", "reason=used", "reason=unknown", "reason=expired", "label=phone"] {
        assert!(log.contains(needle), "{needle} not in {log}");
    }
    // And no secret is in that log
    let token = first.body["access_token"].as_str().unwrap();
    for secret in [code, token, short["code"].as_str().unwrap(), "ZZZZZZZZZZZZ"] {
        assert!(!log.contains(secret), "{secret} leaked in {log}");
    }
    server.stop();
}

#[test]
fn what_a_client_created_and_its_token_survive_a_restart_with_the_same_environment_and_sequence() {
    // Given a paired client that created a project and a thread
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start(dir.path(), &[]);
    let descriptor = http(server.addr(), "GET", "/.well-known/t3/environment", &[], "").body;
    let token = bearer_of(server.addr(), pair(dir.path(), &[])["code"].as_str().unwrap());
    let mut rpc = socket_for(&server, &token);
    seed(&mut rpc);
    drop(rpc);
    server.stop();

    // When the server restarts on the same directory and the same bearer reconnects
    let again = Server::start(dir.path(), &[]);
    let after = http(again.addr(), "GET", "/.well-known/t3/environment", &[], "").body;
    let mut rpc = socket_for(&again, &token);
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    let snapshot = rpc.chunk(&shell)[0]["snapshot"].clone();
    // And repeats the original thread.create command id
    let repeat = rpc.call("orchestration.dispatchCommand", thread_create("c-t1", "t1", "p1"));
    let next = rpc.call("orchestration.dispatchCommand", thread_create("c-t2", "t2", "p1"));

    // Then the environment is the same, the shell has the project and thread, the repeat is idempotent and the sequence continues at 3
    assert_eq!(after["environmentId"], descriptor["environmentId"]);
    assert_eq!((snapshot["snapshotSequence"].as_i64(), snapshot["projects"][0]["id"].as_str(), snapshot["threads"][0]["id"].as_str()), (Some(2), Some("p1"), Some("t1")));
    assert_eq!((success(&repeat)["sequence"].as_i64(), success(&next)["sequence"].as_i64()), (Some(2), Some(3)));
    again.stop();
}

#[test]
fn without_an_rca_url_a_message_fails_its_run_saying_no_agent_api_is_configured_and_nothing_hangs() {
    // Given a server started with no --rca-url, and a thread on a project
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start(dir.path(), &[]);
    let token = bearer_of(server.addr(), pair(dir.path(), &[])["code"].as_str().unwrap());
    let mut rpc = socket_for(&server, &token);
    seed(&mut rpc);
    // When a message is dispatched
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "commandId": "c-m", "threadId": "t1", "message": {"messageId": "m1", "text": "go"}})));
    let p = settled(&mut rpc, "t1");
    // Then the run failed with the reason, an error row says the same, and the thread takes another message
    assert_eq!((p["runs"][0]["status"].as_str(), p["runs"][0]["error"].as_str()), (Some("failed"), Some("no agent API is configured (start serve with --rca-url)")));
    assert!(p["turnItems"].as_array().unwrap().iter().any(|i| i["type"] == "error"));
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "commandId": "c-n", "threadId": "t1", "message": {"messageId": "m2", "text": "again"}})));
    server.stop();
}

#[test]
fn the_catalog_file_names_the_models_the_config_lists() {
    // Given a catalog file with two models
    let dir = tempfile::tempdir().unwrap();
    let catalog = dir.path().join("models.json");
    std::fs::write(&catalog, r#"[{"slug":"ladder:fast","name":"Fast"},{"slug":"ladder:deep"}]"#).unwrap();
    let server = Server::start(dir.path(), &["--catalog", catalog.to_str().unwrap(), "--label", "vm rca"]);
    let token = bearer_of(server.addr(), pair(dir.path(), &[])["code"].as_str().unwrap());
    // When the config is read
    let mut rpc = socket_for(&server, &token);
    let config = success(&rpc.call("server.getConfig", json!({}))).clone();
    // Then the provider lists both models and the descriptor carries the label
    let slugs: Vec<&str> = config["providers"][0]["models"].as_array().unwrap().iter().map(|m| m["slug"].as_str().unwrap()).collect();
    assert_eq!(slugs, ["ladder:fast", "ladder:deep"]);
    assert_eq!(config["environment"]["label"], "vm rca");
    server.stop();
}

#[test]
fn serve_on_a_port_in_use_fails_with_ok_false_and_the_next_step() {
    // Given a server holding a port
    let dir = tempfile::tempdir().unwrap();
    let first = Server::start(dir.path(), &[]);
    // When a second one is told to listen there (on its own state directory)
    let other = tempfile::tempdir().unwrap();
    let (code, stdout, _) = run(&["t3", "serve", "--port", &first.port.to_string(), "--state-dir", other.path().to_str().unwrap()]);
    // Then it is ok:false with exit 1
    let out: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!((code, out["ok"].clone()), (1, json!(false)));
    assert!(out["next"].is_string());
    first.stop();
}

#[test]
fn the_help_documents_the_output_schema_and_holds_no_code_that_was_minted() {
    // Given a minted code
    let dir = tempfile::tempdir().unwrap();
    let code = pair(dir.path(), &[])["code"].as_str().unwrap().to_string();
    // When every help text is read
    for args in [vec!["t3", "--help"], vec!["t3", "serve", "--help"], vec!["t3", "pair", "--help"]] {
        let (exit, stdout, _) = run(&args);
        // Then it prints with exit 0, documents its output, and shows no secret
        assert_eq!(exit, 0, "{args:?}");
        assert!(stdout.starts_with("Usage: wf t3"), "{stdout}");
        assert!(!stdout.contains(&code));
    }
    let (_, pair_help, _) = run(&["t3", "pair", "--help"]);
    assert!(pair_help.contains("Output: one JSON object") && pair_help.contains("--scope"));
}

#[test]
fn the_state_directory_holds_no_code_token_or_ticket_only_their_hashes() {
    // Given a paired client with a ticket, while the server runs
    let dir = tempfile::tempdir().unwrap();
    let server = Server::start(dir.path(), &[]);
    let code = pair(dir.path(), &[])["code"].as_str().unwrap().to_string();
    let token = bearer_of(server.addr(), &code);
    let ticket = ticket(server.addr(), &token).body["ticket"].as_str().unwrap().to_string();

    // When every file of the state directory is read, the write-ahead log included
    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(dir.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("state.sqlite")) {
        bytes.extend(std::fs::read(entry.path()).unwrap());
    }

    // Then the store is there and none of the three secrets is in it
    assert!(bytes.windows(6).any(|w| w == b"grants"), "the store was not written");
    for secret in [&code, &token, &ticket] {
        assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()), "{secret} is stored in the clear");
    }
    server.stop();
}

/// The `service.status` exchange the contract crate recorded: the request a mapper sends and the response a service gives.
fn recorded_status() -> (Value, Value) {
    let fixtures = agent_api_service::testing::fixtures();
    let status = fixtures.iter().find(|f| f.method == agent_api_service::STATUS.name()).expect("a recorded service.status");
    let exchange = status.exchanges.iter().find(|e| e.succeeds()).expect("a recorded status that succeeds");
    (exchange.request.clone(), exchange.response["result"].clone())
}

type Seen = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// A stand-in rca agent API: answers every request with `status` under the request's id, after checking the bearer; records each bearer and
/// each request it saw.
#[allow(clippy::result_large_err)] // tungstenite fixes the callback's error type
fn fake_agent_api(status: Value) -> (u16, Seen, Seen) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (bearers, requests): (Seen, Seen) = Default::default();
    let (record, asked) = (std::sync::Arc::clone(&bearers), std::sync::Arc::clone(&requests));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (record, asked) = (std::sync::Arc::clone(&record), std::sync::Arc::clone(&asked));
            let status = status.clone();
            std::thread::spawn(move || {
                let callback = |req: &tungstenite::handshake::server::Request, res| {
                    record.lock().unwrap().push(req.headers().get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string());
                    Ok(res)
                };
                let Ok(mut socket) = tungstenite::accept_hdr(stream, callback) else { return };
                while let Ok(tungstenite::Message::Text(text)) = socket.read() {
                    asked.lock().unwrap().push(text.to_string());
                    let request: Value = serde_json::from_str(&text).unwrap();
                    let _ = socket.send(tungstenite::Message::Text(json!({ "id": request["id"], "result": status }).to_string().into()));
                }
            });
        }
    });
    (port, bearers, requests)
}

#[test]
fn serve_against_rca_checks_its_agent_api_and_refuses_a_protocol_or_group_it_cannot_use() {
    // Given the status the contract recorded, and rcas that differ from it by a protocol, by each group this mapper needs, and by the build the status names
    let dir = tempfile::tempdir().unwrap();
    let (recorded_request, recorded) = recorded_status();
    let other_protocol = recorded["protocol"].as_u64().unwrap() + 1;
    let without = |group: &str| {
        let mut status = recorded.clone();
        status["groups"] = json!(recorded["groups"].as_array().unwrap().iter().filter(|g| g.as_str() != Some(group)).collect::<Vec<_>>());
        status
    };
    let mut other = recorded.clone();
    other["protocol"] = json!(other_protocol);
    let mut unreadable = recorded.clone();
    unreadable.as_object_mut().unwrap().remove("version");
    let (service, threads, files) = (agent_api_service::GROUP.as_str(), agent_api_agent::GROUP.as_str(), agent_api_workspace::FILES.as_str());
    let refused = [
        (other, format!("protocol {other_protocol}")),
        (without(service), format!("group(s) {service}")),
        (without(threads), format!("group(s) {threads}")),
        (without(files), format!("group(s) {files}")),
        (unreadable, "cannot read".to_string()),
    ];
    for (status, says) in refused {
        let (port, bearers, _) = fake_agent_api(status);
        // When t3 serve starts against it
        let out = Command::new(BIN)
            .args(["t3", "serve", "--port", "0", "--state-dir"])
            .arg(dir.path())
            .args(["--rca-url", &format!("http://127.0.0.1:{port}"), "--rca-workspace", "w1"])
            .env("T3_RCA_BEARER", "flow-rca-bearer")
            .env_remove("CHAT_AGENT_API_SECRET")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        // Then it exits 1 with ok:false naming what does not fit, and it asked with its device bearer
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line: Value = serde_json::from_str(stdout.lines().next().unwrap_or("null")).unwrap_or(Value::Null);
        assert_eq!((out.status.code(), line["ok"].clone()), (Some(1), json!(false)), "{stdout}");
        assert!(stdout.contains(says.as_str()), "{stdout}");
        assert_eq!(bearers.lock().unwrap().as_slice(), ["Bearer flow-rca-bearer"]);
    }
    // Given an rca that answers with the recorded status, When t3 serve starts against it, Then it listens, having sent the recorded request
    let (port, _, requests) = fake_agent_api(recorded);
    std::env::set_var("T3_RCA_BEARER", "flow-rca-bearer");
    let server = Server::start(dir.path(), &["--rca-url", &format!("http://127.0.0.1:{port}"), "--rca-workspace", "w1"]);
    std::env::remove_var("T3_RCA_BEARER");
    let sent: Vec<Value> = requests.lock().unwrap().iter().map(|r| serde_json::from_str(r).unwrap()).collect();
    assert_eq!(sent, [recorded_request]);
    server.stop();
}

#[test]
fn serve_against_rca_without_a_device_bearer_or_a_workspace_refuses_to_start_and_never_asks_rca() {
    // Given an rca that would answer the recorded status
    let dir = tempfile::tempdir().unwrap();
    let (_, recorded) = recorded_status();
    let serve = |bearer: Option<&str>, workspace: Option<&str>| {
        let (port, bearers, requests) = fake_agent_api(recorded.clone());
        let mut command = Command::new(BIN);
        command.args(["t3", "serve", "--port", "0", "--state-dir"]).arg(dir.path()).args(["--rca-url", &format!("http://127.0.0.1:{port}")]).stdin(Stdio::null());
        // The service secret is in the environment, as it is for an operator: serve must not use it
        command.env("CHAT_AGENT_API_SECRET", "flow-rca-secret").env_remove("T3_RCA_BEARER").env_remove("T3_RCA_WORKSPACE");
        if let Some(bearer) = bearer {
            command.env("T3_RCA_BEARER", bearer);
        }
        if let Some(workspace) = workspace {
            command.args(["--rca-workspace", workspace]);
        }
        let out = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let line: Value = serde_json::from_str(stdout.lines().next().unwrap_or("null")).unwrap_or(Value::Null);
        let sent = (bearers.lock().unwrap().len(), requests.lock().unwrap().len());
        (out.status.code(), line["ok"].clone(), stdout, sent)
    };
    // When t3 serve starts with no bearer, and with a bearer and no workspace
    let no_bearer = serve(None, Some("w1"));
    let no_workspace = serve(Some("flow-rca-bearer"), None);
    // Then each exits 1 saying which setting is missing and how to get it, and rca was sent neither the secret nor a request
    assert_eq!((no_bearer.0, no_bearer.1.clone()), (Some(1), json!(false)), "{}", no_bearer.2);
    assert!(no_bearer.2.contains("T3_RCA_BEARER") && no_bearer.2.contains("wf offload bearers mint"), "{}", no_bearer.2);
    assert_eq!((no_workspace.0, no_workspace.1.clone()), (Some(1), json!(false)), "{}", no_workspace.2);
    assert!(no_workspace.2.contains("--rca-workspace"), "{}", no_workspace.2);
    assert_eq!((no_bearer.3, no_workspace.3), ((0, 0), (0, 0)));
}
