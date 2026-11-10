//! The preview family through the real adapter: `fake-devserver` (exec-testkit) is the dev server, so every start runs a real process on a real
//! port with the environment, directory and process group the executor chooses. Nothing is doubled in the executor.
use exec_preview::{Listed, PreviewInfo, State, MAX_RUNNING, PREVIEW_LIST, PREVIEW_START, PREVIEW_STOP};
use exec_proto::{Code, ExecResult, Stream};
use exec_testkit::devserver::{ask, command, free_port};
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::execute;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

fn call(ex: &LocalExecutor, tool: &str, args: Value) -> ExecResult<Value> {
    execute(ex, "id-1", tool, &args, &mut |_: Stream, _: &str| {}, &mut |_| {})
}

fn start(ex: &LocalExecutor, flags: &str, port: u16, extra: Value) -> ExecResult<PreviewInfo> {
    let mut args = json!({"command": command(flags), "port": port, "ready_timeout_ms": 10_000});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        args[k] = v;
    }
    call(ex, PREVIEW_START, args).map(|v| serde_json::from_value(v).unwrap())
}

fn listed(ex: &LocalExecutor) -> Vec<PreviewInfo> {
    serde_json::from_value::<Listed>(call(ex, PREVIEW_LIST, json!({})).unwrap()).unwrap().previews
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only asks whether the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn root() -> (tempfile::TempDir, LocalExecutor) {
    let dir = tempfile::tempdir().unwrap();
    let ex = LocalExecutor::new(dir.path()).unwrap();
    (dir, ex)
}

#[test]
fn given_the_recorded_exchanges_of_the_family_when_replayed_on_a_real_directory_then_each_response_is_what_was_recorded() {
    // Given the family's recorded exchanges and an executor on an empty directory
    let (_dir, ex) = root();
    for fixture in exec_preview::testing::fixtures() {
        for exchange in &fixture.exchanges {
            // When the recorded request runs
            let (tool, args) = (exchange.request["tool"].as_str().unwrap(), &exchange.request["args"]);
            let result = call(&ex, tool, args.clone());
            // Then the answer is the recorded one, result or error code and message
            match (&exchange.response["ok"].as_bool().unwrap(), result) {
                (true, Ok(got)) => assert_eq!(got, exchange.response["result"], "{}", exchange.name),
                (false, Err(e)) => assert_eq!((json!(e.code.as_str()), json!(e.message)), (exchange.response["error"]["code"].clone(), exchange.response["error"]["message"].clone()), "{}", exchange.name),
                (_, other) => panic!("{}: {other:?}", exchange.name),
            }
        }
    }
}

#[test]
fn given_a_dev_server_when_started_then_it_listens_where_the_request_said_in_the_directory_asked_for_and_the_list_shows_it_until_stopped() {
    // Given a workspace with a subdirectory and a free port
    let (dir, ex) = root();
    std::fs::create_dir(dir.path().join("web")).unwrap();
    let port = free_port();
    // When the server is started there with a name
    let info = start(&ex, "", port, json!({"cwd": "web", "name": "web"})).unwrap();
    // Then it answers on the port, ran in that directory with PORT set, and the list shows it running
    let seen = ask(port);
    assert_eq!((info.port, info.url.as_str(), info.name.as_str(), info.cwd.as_str(), info.state), (port, format!("http://127.0.0.1:{port}/").as_str(), "web", "web", State::Running));
    assert_eq!(Path::new(seen["cwd"].as_str().unwrap()).canonicalize().unwrap(), dir.path().canonicalize().unwrap().join("web"));
    assert_eq!(seen["port_env"], json!(port.to_string()));
    assert_eq!(listed(&ex).iter().map(|p| (p.id.clone(), p.state)).collect::<Vec<_>>(), [(info.id.clone(), State::Running)]);
    // And stopping it ends the process and the list reports it exited; a second stop finds nothing
    assert_eq!(call(&ex, PREVIEW_STOP, json!({"id": info.id})).unwrap(), json!({"stopped": true}));
    wait_until("the server's process to end", || !alive(info.pid as i32));
    assert_eq!(listed(&ex)[0].state, State::Exited);
    assert_eq!(call(&ex, PREVIEW_STOP, json!({"id": info.id})).unwrap(), json!({"stopped": false}));
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn given_secrets_in_the_executors_environment_when_a_server_starts_then_it_sees_the_path_and_what_the_request_named_and_nothing_else() {
    // Given an executor process that holds a token, a proxy and a credentials path
    for (name, value) in [("GH_TOKEN", "ghp_canary"), ("ANTHROPIC_API_KEY", "sk-canary"), ("HTTPS_PROXY", "http://proxy.invalid"), ("AWS_SECRET_ACCESS_KEY", "aws-canary")] {
        std::env::set_var(name, value);
    }
    let (_dir, ex) = root();
    let port = free_port();
    // When a server starts with one variable named by the request
    let info = start(&ex, "", port, json!({"env": {"FEATURE_FLAG": "on"}})).unwrap();
    // Then it saw PATH, the flag, PORT and HOST, and none of the executor's secrets
    let names: Vec<String> = ask(port)["env"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    for expected in ["PATH", "FEATURE_FLAG", "PORT", "HOST"] {
        assert!(names.iter().any(|n| n == expected), "{expected} is missing from {names:?}");
    }
    for secret in ["GH_TOKEN", "ANTHROPIC_API_KEY", "HTTPS_PROXY", "AWS_SECRET_ACCESS_KEY"] {
        assert!(!names.iter().any(|n| n == secret), "{secret} leaked into {names:?}");
    }
    call(&ex, PREVIEW_STOP, json!({"id": info.id})).unwrap();
}

#[test]
fn given_a_server_that_starts_a_child_when_it_is_stopped_then_the_whole_process_group_ends() {
    // Given a server that also started a `sleep` in its group
    let (_dir, ex) = root();
    let port = free_port();
    let info = start(&ex, "--child", port, json!({})).unwrap();
    wait_until("the server's output", || listed(&ex)[0].tail.contains("child "));
    let child: i32 = listed(&ex)[0].tail.lines().find_map(|l| l.strip_prefix("child ")).unwrap().trim().parse().unwrap();
    assert!(alive(child));
    // When it is stopped
    call(&ex, PREVIEW_STOP, json!({"id": info.id})).unwrap();
    // Then the child is gone too
    wait_until("the child to end", || {
        // SAFETY: reaps nothing of ours; a zombie child of the server is reparented, so existence is the check.
        !alive(child) || std::fs::read_to_string(format!("/proc/{child}/stat")).map_or(true, |s| s.contains(") Z "))
    });
}

#[test]
fn given_starts_that_cannot_succeed_when_requested_then_each_is_refused_with_why_and_leaves_nothing_running() {
    let (_dir, ex) = root();
    // Given a server that exits before it listens, one that never listens, and a port another process holds
    let early = start(&ex, "--exit-now 7", free_port(), json!({})).unwrap_err();
    let never = start(&ex, "--no-listen", free_port(), json!({"ready_timeout_ms": 300})).unwrap_err();
    let held = std::net::TcpListener::bind(("127.0.0.1", free_port())).unwrap();
    let taken = start(&ex, "", held.local_addr().unwrap().port(), json!({})).unwrap_err();
    let outside = start(&ex, "", free_port(), json!({"cwd": "../elsewhere"})).unwrap_err();
    let missing = start(&ex, "", free_port(), json!({"cwd": "nothing"})).unwrap_err();
    // Then each says what happened, and no server is left in the list
    assert!(early.code == Code::Io && early.message.contains("code 7") && early.message.contains("refusing to start"), "{early:?}");
    assert!(never.code == Code::Io && never.message.contains("not listening") && never.message.contains("300 ms"), "{never:?}");
    assert!(taken.code == Code::AlreadyExists && taken.message.contains("already in use"), "{taken:?}");
    assert_eq!(outside.code, Code::PathOutsideRoot);
    assert_eq!(missing.code, Code::NotFound);
    assert!(listed(&ex).is_empty());
}

#[test]
fn given_a_server_that_exits_by_itself_when_listed_then_it_is_exited_with_its_code_and_the_tail_of_its_output() {
    // Given a server that exits with code 3 after listening for a moment
    let (_dir, ex) = root();
    let info = start(&ex, "--exit-after-ms 200", free_port(), json!({})).unwrap();
    // When it is listed after it ended
    wait_until("the server to exit", || listed(&ex)[0].state == State::Exited);
    let after = &listed(&ex)[0];
    // Then the list says exited with code 3 and keeps what it printed, and its port is free again
    assert_eq!((after.id.as_str(), after.exit_code), (info.id.as_str(), Some(3)));
    assert!(after.tail.contains(&format!("listening {}", info.port)), "{after:?}");
    assert!(after.exited_ms.is_some_and(|t| t >= after.started_ms));
}

#[test]
fn given_as_many_running_servers_as_allowed_when_one_more_starts_then_it_is_refused_until_one_is_stopped() {
    // Given the most servers the executor runs
    let (_dir, ex) = root();
    let started: Vec<PreviewInfo> = (0..MAX_RUNNING).map(|_| start(&ex, "", free_port(), json!({})).unwrap()).collect();
    // When another starts
    let refused = start(&ex, "", free_port(), json!({})).unwrap_err();
    // Then it is refused, and after one is stopped a new one starts
    assert!(refused.message.contains(&format!("{MAX_RUNNING} preview servers")), "{refused:?}");
    call(&ex, PREVIEW_STOP, json!({"id": started[0].id})).unwrap();
    let again = start(&ex, "", free_port(), json!({})).unwrap();
    for p in started.iter().skip(1).chain([&again]) {
        call(&ex, PREVIEW_STOP, json!({"id": p.id})).unwrap();
    }
}

#[test]
fn given_a_running_server_when_the_executor_is_dropped_then_the_server_goes_with_it() {
    // Given a running server
    let (dir, ex) = root();
    let info = start(&ex, "", free_port(), json!({})).unwrap();
    assert!(alive(info.pid as i32));
    // When the executor is dropped
    drop(ex);
    // Then the server is gone
    wait_until("the server to end", || std::fs::read_to_string(format!("/proc/{}/stat", info.pid)).map_or(true, |s| s.contains(") Z ")));
    drop(dir);
}
