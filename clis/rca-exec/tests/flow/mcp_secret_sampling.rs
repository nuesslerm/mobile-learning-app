//! The secret file, the process sample and the MCP request on real files and processes. The scripted MCP servers are authored doubles in `sh`,
//! not recordings of a real MCP server; they prove the transport (spawn, handshake, one request, stop), not any server's own tool results.
use exec_proto::Code;
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::execute;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn call(ex: &LocalExecutor, tool: &str, args: Value) -> Result<Value, exec_proto::ExecError> {
    execute(ex, "c1", tool, &args, &mut |_, _| {}, &mut |_| {})
}

fn dir() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = std::fs::canonicalize(d.path()).unwrap();
    (d, p)
}

fn executor(root: &Path, home: &Path) -> LocalExecutor {
    LocalExecutor::new(root).unwrap().with_skill_home(Some(home))
}

/// A workspace whose `.mcp.json` declares `app` as `sh server.sh` with one declared variable, and whose server runs `after_init` once it read the request.
fn workspace(root: &Path, between: &str, reply: &str) {
    std::fs::write(root.join(".mcp.json"), json!({"mcpServers": {"app": {"command": "sh", "args": ["server.sh"], "env": {"APP_KEY": "declared-value"}}}}).to_string()).unwrap();
    let init = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"double","version":"0"}}}"#;
    std::fs::write(root.join("server.sh"), format!("read init\nprintf '%s\\n' '{init}'\nread initialized\nread request\n{between}\nprintf '%s\\n' {reply}\n")).unwrap();
}

#[test]
fn given_a_secret_when_it_is_put_then_the_file_is_private_and_exact_and_no_result_or_error_carries_the_value() {
    // Given an executor on an empty workspace
    let ((_r, root), (_h, home)) = (dir(), dir());
    let ex = executor(&root, &home);
    // When a secret is put at a path below a directory that does not exist yet
    let put = call(&ex, "secret_file_put", json!({"path": ".rca/secrets/key", "value": "s3cr3t-value"})).unwrap();
    // Then the answer is the path and the size, the file holds exactly the value and only its owner can read it
    assert_eq!(put, json!({"path": ".rca/secrets/key", "bytes": 12}));
    let file = root.join(".rca/secrets/key");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "s3cr3t-value");
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    // And a second put at the path is refused, keeps the first value and does not repeat the new one in the refusal
    let again = call(&ex, "secret_file_put", json!({"path": ".rca/secrets/key", "value": "other-value"})).unwrap_err();
    assert_eq!(again.code, Code::AlreadyExists);
    assert!(!again.message.contains("other-value") && !format!("{again:?}").contains("other-value"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "s3cr3t-value");
}

#[test]
fn given_a_link_at_the_path_when_a_secret_is_put_then_it_is_refused_and_the_link_target_is_untouched() {
    // Given a link in the workspace that points at a file outside it
    let ((_r, root), (_h, home), (_o, outside)) = (dir(), dir(), dir());
    std::fs::write(outside.join("target"), "keep").unwrap();
    std::os::unix::fs::symlink(outside.join("target"), root.join("key")).unwrap();
    // When a secret is put at the link
    let refused = call(&executor(&root, &home), "secret_file_put", json!({"path": "key", "value": "s3cr3t"})).unwrap_err();
    // Then it is refused as outside the root and the target holds what it held
    assert_eq!(refused.code, Code::PathOutsideRoot);
    assert_eq!(std::fs::read_to_string(outside.join("target")).unwrap(), "keep");
}

#[test]
fn given_a_child_process_when_the_executor_is_sampled_then_the_child_is_listed_below_it_and_a_process_that_is_not_below_it_is_not() {
    // Given an executor and a child it runs
    let ((_r, root), (_h, home)) = (dir(), dir());
    let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    // When it is sampled
    let sample = call(&executor(&root, &home), "proc_sample", json!({})).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    // Then the executor's own process and the child are listed, the child under its parent, and init is not
    let pids: Vec<u64> = sample["processes"].as_array().unwrap().iter().map(|p| p["pid"].as_u64().unwrap()).collect();
    let me = u64::from(std::process::id());
    assert!(pids.contains(&me) && pids.contains(&u64::from(child.id())), "{pids:?}");
    assert!(!pids.contains(&1), "{pids:?}");
    let listed = sample["processes"].as_array().unwrap().iter().find(|p| p["pid"] == json!(child.id())).unwrap();
    assert_eq!((listed["ppid"].clone(), listed["name"].clone()), (json!(me), json!("sleep")));
    // And the totals cover the list
    assert_eq!(sample["count"].as_u64().unwrap(), pids.len() as u64);
    assert_eq!(sample["total_rss_kib"].as_u64().unwrap(), sample["processes"].as_array().unwrap().iter().map(|p| p["rss_kib"].as_u64().unwrap()).sum::<u64>());
}

#[test]
fn given_a_server_that_prints_its_environment_when_asked_then_it_sees_its_declared_variables_and_not_the_executors() {
    // Given a server that answers with the names of the variables it was started with
    let ((_r, root), (_h, home)) = (dir(), dir());
    workspace(&root, r#"names=$(env | cut -d= -f1 | tr '\n' ' ')"#, r#""{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"names\":\"$names\"}}""#);
    // When a request is sent
    let answer = call(&executor(&root, &home), "mcp_request", json!({"server": "app", "method": "tools/list"})).unwrap();
    // Then the variables are the declared one and the few every program needs, and none of the executor's own (cargo sets CARGO_MANIFEST_DIR for this test)
    let names = answer["result"]["names"].as_str().unwrap();
    assert!(names.split_whitespace().any(|n| n == "APP_KEY") && names.split_whitespace().any(|n| n == "PATH"), "{names}");
    assert!(!names.contains("CARGO_MANIFEST_DIR"), "{names}");
    // And the declared value is nowhere in the answer
    assert!(!answer.to_string().contains("declared-value"));
}

#[test]
fn given_a_server_that_never_answers_when_asked_then_the_request_fails_in_time_and_the_server_is_gone() {
    // Given a server that writes its pid and then waits
    let ((_r, root), (_h, home)) = (dir(), dir());
    workspace(&root, &format!("echo $$ > {}/pid; sleep 30", root.display()), "''");
    let started = std::time::Instant::now();
    // When a request with a short limit is sent
    let failed = call(&executor(&root, &home), "mcp_request", json!({"server": "app", "method": "tools/list", "timeout_ms": 700})).unwrap_err();
    // Then it fails with a message naming the server, well before the server would have answered
    assert_eq!(failed.code, Code::Io);
    assert!(failed.message.contains("did not answer in time") && failed.message.contains("app"), "{}", failed.message);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    // And the server's process is gone
    let pid = std::fs::read_to_string(root.join("pid")).unwrap().trim().to_string();
    let gone = (0..50).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        !Path::new("/proc").join(&pid).exists() || std::fs::read_to_string(Path::new("/proc").join(&pid).join("stat")).is_ok_and(|s| s.contains(") Z "))
    });
    assert!(gone, "the server {pid} still runs");
}

#[test]
fn given_a_program_that_is_not_installed_when_its_server_is_asked_then_the_refusal_names_the_server_and_no_command_line() {
    // Given a config whose server runs a program that does not exist, with a declared key
    let ((_r, root), (_h, home)) = (dir(), dir());
    std::fs::write(root.join(".mcp.json"), json!({"mcpServers": {"app": {"command": "no-such-mcp-server", "args": ["--key", "declared-value"]}}}).to_string()).unwrap();
    // When a request is sent
    let refused = call(&executor(&root, &home), "mcp_request", json!({"server": "app", "method": "tools/list"})).unwrap_err();
    // Then it is NOT_FOUND by the server's name and repeats none of the arguments
    assert_eq!(refused.code, Code::NotFound);
    assert!(refused.message.contains("\"app\"") && !refused.message.contains("declared-value"), "{}", refused.message);
}

#[test]
fn given_a_server_that_answers_with_an_error_quoting_its_environment_when_asked_then_the_refusal_is_fixed_text_without_the_quote() {
    // Given a server whose JSON-RPC error text repeats its declared key
    let ((_r, root), (_h, home)) = (dir(), dir());
    workspace(&root, "", r#""{\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":-1,\"message\":\"bad key $APP_KEY\"}}""#);
    // When a request is sent
    let refused = call(&executor(&root, &home), "mcp_request", json!({"server": "app", "method": "tools/list"})).unwrap_err();
    // Then it is refused without the server's text
    assert_eq!(refused.code, Code::Io);
    assert!(!refused.message.contains("declared-value") && !format!("{refused:?}").contains("declared-value"), "{}", refused.message);
}

#[test]
fn given_a_request_with_params_when_it_is_sent_then_the_server_receives_the_method_and_the_params() {
    // Given a server that answers with the request line it read
    let ((_r, root), (_h, home)) = (dir(), dir());
    workspace(&root, "", r#""{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"seen\":\"$request\"}}""#.replace(r#"$request"#, r#"'"$(printf %s "$request" | tr '"' "'")"'"#).as_str());
    // When a tools/call with params is sent
    let answer = call(&executor(&root, &home), "mcp_request", json!({"server": "app", "method": "tools/call", "params": {"name": "echo"}})).unwrap();
    // Then the line the server read names the method and holds the params
    let seen = answer["result"]["seen"].as_str().unwrap();
    assert!(seen.contains("tools/call") && seen.contains("echo"), "{seen}");
}

#[test]
fn given_params_over_the_limit_or_no_config_when_a_request_is_sent_then_it_is_refused_before_any_server_starts() {
    // Given a workspace without .mcp.json
    let ((_r, root), (_h, home)) = (dir(), dir());
    let ex = executor(&root, &home);
    // When a request is sent, and one with params over the limit
    let none = call(&ex, "mcp_request", json!({"server": "app", "method": "tools/list"})).unwrap_err();
    let huge = call(&ex, "mcp_request", json!({"server": "app", "method": "tools/call", "params": {"x": "a".repeat(exec_proc::MCP_PARAMS_MAX)}})).unwrap_err();
    // Then the first is NOT_FOUND and the second INVALID_ARGS
    assert_eq!((none.code, huge.code), (Code::NotFound, Code::InvalidArgs));
}
