//! The flow tests of `rca-exec` (`docs/plans/rca-client/protocol.md`): the local adapter on a real temp directory with real processes, and
//! the channel client against an in-test WebSocket server that plays rca. Nothing here doubles the executor.
#[path = "flow/catalog.rs"]
mod catalog;
#[path = "flow/contracts.rs"]
mod contracts;
#[path = "flow/git.rs"]
mod git;
#[path = "flow/hosted.rs"]
mod hosted;
#[path = "flow/preview.rs"]
mod preview;
#[path = "flow/pty.rs"]
mod pty;
#[path = "flow/workspace.rs"]
mod workspace;

use serde_json::{json, Value};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use exec_fs::{EditFile, EntryKind, ListDir, ReadFile, Search, SearchKind, WriteFile};
use exec_proc::{RunCommand, RunResult};
use exec_proto::{Code, ExecResult, Stream};
use rca_exec::domain::{execute, ToolExecutor};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::Message;
use rca_exec::adapters::exec_channel::ExecChannel;
use rca_exec::adapters::local_executor::{LocalExecutor, OUTPUT_CAP};

fn executor(dir: &Path) -> LocalExecutor {
    LocalExecutor::new(dir).unwrap()
}

fn run(ex: &LocalExecutor, id: &str, command: &str, timeout_ms: u64) -> (RunResult, Vec<(Stream, String)>) {
    let mut frames = Vec::new();
    let r = ex.run_command(id, &RunCommand { command: command.into(), cwd: None, timeout_ms, env: vec![] }, &mut |s, d| frames.push((s, d.to_string()))).unwrap();
    (r, frames)
}

fn write(ex: &LocalExecutor, path: &str, content: &str) -> ExecResult<usize> {
    ex.write_file(&WriteFile { path: path.into(), content: content.into(), create_dirs: false })
}

fn gone(pid: i32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
}

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn write_read_edit_list_and_search_do_the_thing_on_disk() {
    // Given an executor rooted at a real directory
    let dir = tempfile::tempdir().unwrap();
    let ex = executor(dir.path());
    // When a file is written (with its parent created), read in a window, edited and listed
    assert_eq!(ex.write_file(&WriteFile { path: "src/a.txt".into(), content: "one\ntwo\nthree\nfour\n".into(), create_dirs: true }).unwrap(), 19);
    let window = ex.read_file(&ReadFile { path: "src/a.txt".into(), offset: 1, limit: Some(2) }).unwrap();
    assert_eq!(ex.edit_file(&EditFile { path: "src/a.txt".into(), old: "three".into(), new: "3".into(), replace_all: false }).unwrap(), 1);
    // Then the bytes on disk are the edited ones and the reads and listings report them
    assert_eq!(std::fs::read_to_string(dir.path().join("src/a.txt")).unwrap(), "one\ntwo\n3\nfour\n");
    assert_eq!((window.content.as_str(), window.total_lines), ("two\nthree\n", 4));
    let top = ex.list_dir(&ListDir { path: ".".into(), depth: 1 }).unwrap();
    assert_eq!(top.iter().map(|e| (e.name.as_str(), e.kind)).collect::<Vec<_>>(), [("src", EntryKind::Dir)]);
    let deep = ex.list_dir(&ListDir { path: ".".into(), depth: 2 }).unwrap();
    assert_eq!(deep.iter().map(|e| (e.name.as_str(), e.size)).collect::<Vec<_>>(), [("src", deep[0].size), ("src/a.txt", 15)]);
    // And both searches find it, content with its line
    ex.write_file(&WriteFile { path: "b.rs".into(), content: "fn main() {}\n".into(), create_dirs: false }).unwrap();
    let content = ex.search(&Search { pattern: "^f.u.$".into(), path: ".".into(), glob: None, kind: SearchKind::Content }).unwrap();
    assert_eq!(content.matches.iter().map(|m| (m.path.as_str(), m.line)).collect::<Vec<_>>(), [("src/a.txt", Some(4))]);
    let files = ex.search(&Search { pattern: "**/*.txt".into(), path: ".".into(), glob: None, kind: SearchKind::Files }).unwrap();
    assert_eq!(files.matches.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), ["src/a.txt"]);
    let globbed = ex.search(&Search { pattern: "main".into(), path: ".".into(), glob: Some("*.rs".into()), kind: SearchKind::Content }).unwrap();
    assert_eq!(globbed.matches.len(), 1);
    // And a missing file is NOT_FOUND
    assert_eq!(ex.read_file(&ReadFile { path: "nope".into(), offset: 0, limit: None }).unwrap_err().code, Code::NotFound);
}

#[test]
fn an_edit_that_matches_twice_is_ambiguous_until_replace_all_and_a_missing_old_is_not_found() {
    // Given a file where "x" occurs twice
    let dir = tempfile::tempdir().unwrap();
    let ex = executor(dir.path());
    write(&ex, "f", "x and x").unwrap();
    let edit = |old: &str, replace_all| ex.edit_file(&EditFile { path: "f".into(), old: old.into(), new: "y".into(), replace_all });
    // When the edit is made without replace_all, Then it is refused and the file is untouched
    assert_eq!(edit("x", false).unwrap_err().code, Code::EditAmbiguous);
    assert_eq!(std::fs::read_to_string(dir.path().join("f")).unwrap(), "x and x");
    // And an absent `old` is a different code
    assert_eq!(edit("zzz", false).unwrap_err().code, Code::EditNotFound);
    // And with replace_all both are replaced
    assert_eq!(edit("x", true).unwrap(), 2);
    assert_eq!(std::fs::read_to_string(dir.path().join("f")).unwrap(), "y and y");
}

#[test]
fn confinement_refuses_dotdot_and_a_symlink_out_while_a_sibling_inside_the_root_works() {
    // Given a root with a sibling directory next to it holding a secret, and a symlink in the root pointing at that sibling
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("root");
    let outside = parent.path().join("outside");
    std::fs::create_dir_all(root.join("inside")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret"), "s3cret").unwrap();
    symlink(&outside, root.join("escape")).unwrap();
    symlink(outside.join("not-yet"), root.join("dangling")).unwrap();
    symlink(root.join("inside"), root.join("shortcut")).unwrap();
    let ex = executor(&root);
    let read = |p: &str| ex.read_file(&ReadFile { path: p.into(), offset: 0, limit: None });
    // When a path climbs out with `..`, or goes through the symlink, or writes through a dangling one
    // Then each is PATH_OUTSIDE_ROOT, and nothing was created outside
    assert_eq!(read("../outside/secret").unwrap_err().code, Code::PathOutsideRoot);
    assert_eq!(read("inside/../../outside/secret").unwrap_err().code, Code::PathOutsideRoot);
    assert_eq!(read("escape/secret").unwrap_err().code, Code::PathOutsideRoot);
    assert_eq!(read(outside.join("secret").to_str().unwrap()).unwrap_err().code, Code::PathOutsideRoot);
    assert_eq!(write(&ex, "dangling", "x").unwrap_err().code, Code::PathOutsideRoot);
    assert_eq!(write(&ex, "escape/new", "x").unwrap_err().code, Code::PathOutsideRoot);
    assert_eq!(ex.list_dir(&ListDir { path: "escape".into(), depth: 1 }).unwrap_err().code, Code::PathOutsideRoot);
    assert!(!outside.join("new").exists() && !outside.join("not-yet").exists());
    // And a command's cwd is confined the same way
    let cwd_out = ex.run_command("c", &RunCommand { command: "true".into(), cwd: Some("escape".into()), timeout_ms: 1000, env: vec![] }, &mut |_, _| {});
    assert_eq!(cwd_out.unwrap_err().code, Code::PathOutsideRoot);
    // But a sibling path inside the root, directly or through an in-root symlink, works
    write(&ex, "inside/ok", "fine").unwrap();
    assert_eq!(read("inside/ok").unwrap().content, "fine");
    assert_eq!(read("shortcut/ok").unwrap().content, "fine");
    assert_eq!(read(root.join("inside/ok").to_str().unwrap()).unwrap().content, "fine");
}

#[test]
fn run_command_streams_stdout_and_stderr_as_they_arrive_and_reports_the_exit_code() {
    // Given a command that prints, waits, then prints on both streams and exits 3
    let dir = tempfile::tempdir().unwrap();
    let ex = executor(dir.path());
    let started = Instant::now();
    let mut arrivals: Vec<(Duration, Stream, String)> = Vec::new();
    let r = ex.run_command("r", &RunCommand { command: "echo first; sleep 0.4; echo second >&2; exit 3".into(), cwd: None, timeout_ms: 10_000, env: vec![("GREETING".into(), "hi".into())] }, &mut |s, d| arrivals.push((started.elapsed(), s, d.to_string()))).unwrap();
    // Then the first chunk reached the sink while the command was still sleeping, the stderr one later, on its own stream
    assert_eq!((r.exit_code, r.timed_out, r.cancelled, r.truncated), (3, false, false, false));
    assert_eq!((arrivals[0].1, arrivals[0].2.as_str()), (Stream::Stdout, "first\n"));
    assert_eq!((arrivals[1].1, arrivals[1].2.as_str()), (Stream::Stderr, "second\n"));
    assert!(arrivals[0].0 < Duration::from_millis(300) && arrivals[1].0 >= Duration::from_millis(400), "{arrivals:?}");
    // And the extra environment and the working directory reach the process
    let (_, frames) = run(&ex, "e", "pwd; echo $0", 5000);
    assert!(frames[0].1.starts_with(dir.path().canonicalize().unwrap().to_str().unwrap()));
    let mut out = String::new();
    ex.run_command("v", &RunCommand { command: "echo $GREETING".into(), cwd: None, timeout_ms: 5000, env: vec![("GREETING".into(), "hi".into())] }, &mut |_, d| out.push_str(d)).unwrap();
    assert_eq!(out, "hi\n");
}

#[test]
fn run_command_past_its_timeout_is_killed_with_its_children() {
    // Given a command that starts a background child and then blocks
    let dir = tempfile::tempdir().unwrap();
    let ex = executor(dir.path());
    let started = Instant::now();
    // When it runs with a 300 ms timeout
    let (r, _) = run(&ex, "t", "sleep 60 & echo $! > child.pid; wait", 300);
    // Then it returns promptly as timed out and the child is gone as well
    assert!(r.timed_out && !r.cancelled, "{r:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let pid: i32 = std::fs::read_to_string(dir.path().join("child.pid")).unwrap().trim().parse().unwrap();
    wait_for("the timed-out child to die", || gone(pid));
}

#[test]
fn cancel_kills_the_running_commands_process_group_and_a_second_cancel_finds_nothing() {
    // Given a command running on another thread that started a background child
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    let runner = {
        let ex = ex.clone();
        std::thread::spawn(move || run(&ex, "job-1", "sleep 60 & echo $! > child.pid; echo up; wait", 120_000))
    };
    wait_for("the command to start", || dir.path().join("child.pid").exists() && std::fs::read_to_string(dir.path().join("child.pid")).is_ok_and(|s| s.ends_with('\n')));
    let pid: i32 = std::fs::read_to_string(dir.path().join("child.pid")).unwrap().trim().parse().unwrap();
    assert!(!gone(pid));
    // When it is cancelled by id
    assert!(ex.cancel("job-1"));
    // Then the run ends cancelled, not timed out, and neither it nor its child is left
    let (r, frames) = runner.join().unwrap();
    assert!(r.cancelled && !r.timed_out && r.exit_code == 128 + 9, "{r:?}");
    assert_eq!(frames[0].1, "up\n");
    wait_for("the child to die", || gone(pid));
    assert!(!ex.cancel("job-1"));
}

#[test]
fn run_command_output_over_the_cap_is_cut_and_marked_truncated_without_blocking_the_process() {
    // Given a command that writes more than the cap
    let dir = tempfile::tempdir().unwrap();
    let ex = executor(dir.path());
    // When it runs
    let (r, frames) = run(&ex, "big", &format!("head -c {} /dev/zero | tr '\\0' a", OUTPUT_CAP + 500_000), 20_000);
    // Then it finished normally, the sink got exactly the cap and the result says truncated
    assert_eq!((r.exit_code, r.truncated, r.timed_out), (0, true, false));
    assert_eq!(frames.iter().map(|f| f.1.len()).sum::<usize>(), OUTPUT_CAP);
}

#[test]
fn execute_maps_wire_requests_onto_the_executor_and_refuses_bad_ones_with_codes() {
    // Given an executor
    let dir = tempfile::tempdir().unwrap();
    let ex = executor(dir.path());
    let mut sink = |_: Stream, _: &str| {};
    // When requests arrive as {tool, args}
    let w = execute(&ex, "1", "write_file", &json!({"path": "n/x.txt", "content": "hi", "create_dirs": true}), &mut sink, &mut |_| {}).unwrap();
    let bad = execute(&ex, "2", "write_file", &json!({"path": 5}), &mut sink, &mut |_| {}).unwrap_err();
    let unknown = execute(&ex, "3", "format_disk", &json!({}), &mut sink, &mut |_| {}).unwrap_err();
    // Then the effect is on disk and the errors carry their codes
    assert_eq!(w, json!({"bytes": 2}));
    assert_eq!(std::fs::read_to_string(dir.path().join("n/x.txt")).unwrap(), "hi");
    assert_eq!((bad.code.as_str(), unknown.code.as_str()), ("INVALID_ARGS", "UNKNOWN_TOOL"));
}

fn send(ws: &mut tungstenite::WebSocket<TcpStream>, v: Value) {
    ws.send(Message::text(v.to_string())).unwrap();
}

fn recv(ws: &mut tungstenite::WebSocket<TcpStream>) -> Value {
    loop {
        if let Message::Text(t) = ws.read().unwrap() {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

/// Reads frames until the result for `id`; returns the stream data per stream name and the result frame. Subscribe frames are skipped.
fn until_result(ws: &mut tungstenite::WebSocket<TcpStream>, id: &str) -> (Vec<(String, String)>, Value) {
    let (streams, _, result) = until_result_events(ws, id);
    (streams, result)
}

/// Like `until_result`, and also the `event` of each subscribe frame, whose `seq` must count 1, 2, 3 ...
fn until_result_events(ws: &mut tungstenite::WebSocket<TcpStream>, id: &str) -> (Vec<(String, String)>, Vec<Value>, Value) {
    let (mut streams, mut events) = (Vec::new(), Vec::new());
    loop {
        let f = recv(ws);
        assert_eq!(f["id"], id, "frame for another request: {f}");
        if f.get("stream").is_some() {
            streams.push((f["stream"].as_str().unwrap().to_string(), f["data"].as_str().unwrap().to_string()));
        } else if f.get("seq").is_some() {
            assert_eq!(f["seq"], events.len() + 1, "subscribe frames are out of order: {f}");
            events.push(f["event"].clone());
        } else {
            return (streams, events, f);
        }
    }
}

type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

#[allow(clippy::result_large_err)]
fn accept(listener: &TcpListener, seen: &Seen) -> tungstenite::WebSocket<TcpStream> {
    let (stream, _) = listener.accept().unwrap();
    let seen = seen.clone();
    tungstenite::accept_hdr(stream, move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let auth = req.headers().get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
        seen.lock().unwrap().push((req.uri().to_string(), auth));
        Ok(resp)
    })
    .unwrap()
}

#[test]
fn the_channel_client_executes_what_rca_sends_and_answers_with_ids_streams_and_results() {
    // Given an executor directory and a WebSocket server that plays rca
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let rca = {
        let (seen, dir) = (seen.clone(), dir.path().to_path_buf());
        std::thread::spawn(move || {
            let mut ws = accept(&listener, &seen);
            // When rca asks for a file to be written, then a command that reads it and streams on both streams
            send(&mut ws, json!({"id": "w1", "tool": "write_file", "args": {"path": "hello.txt", "content": "from rca\n"}}));
            let (_, wrote) = until_result(&mut ws, "w1");
            send(&mut ws, json!({"id": "r1", "tool": "run_command", "args": {"command": "cat hello.txt; echo oops >&2"}}));
            let (streams, ran) = until_result(&mut ws, "r1");
            // And one that escapes the root, and a long one it cancels
            send(&mut ws, json!({"id": "x1", "tool": "read_file", "args": {"path": "../etc/passwd"}}));
            let (_, escaped) = until_result(&mut ws, "x1");
            send(&mut ws, json!({"id": "long", "tool": "run_command", "args": {"command": "sleep 60 & echo $! > long.pid; wait"}}));
            wait_for("the long command", || std::fs::read_to_string(dir.join("long.pid")).is_ok_and(|s| s.ends_with('\n')));
            send(&mut ws, json!({"id": "c1", "tool": "cancel", "args": {"id": "long"}}));
            let mut by_id = std::collections::HashMap::new();
            while by_id.len() < 2 {
                let f = recv(&mut ws);
                by_id.insert(f["id"].as_str().unwrap().to_string(), f);
            }
            // Then rca closes cleanly and the client acknowledges
            ws.close(Some(tungstenite::protocol::CloseFrame { code: 1000.into(), reason: "".into() })).unwrap();
            loop {
                match ws.read() {
                    Err(tungstenite::Error::ConnectionClosed) => break,
                    Err(e) => panic!("{e}"),
                    Ok(_) => {}
                }
            }
            (wrote, streams, ran, escaped, by_id)
        })
    };
    let executor: Arc<dyn ToolExecutor> = Arc::new(executor(dir.path()));
    let channel = ExecChannel::open(&url, "workspace 1", "tok", executor).unwrap();
    let (wrote, streams, ran, escaped, by_id) = rca.join().unwrap();
    // Then the request carried the workspace and the bearer
    assert_eq!(seen.lock().unwrap().as_slice(), [("/exec/v2/executors?workspace=workspace%201".to_string(), Some("Bearer tok".to_string()))]);
    // And the write landed in the executor's directory under the request's id
    assert_eq!(std::fs::read_to_string(dir.path().join("hello.txt")).unwrap(), "from rca\n");
    assert_eq!(wrote, json!({"id": "w1", "ok": true, "result": {"bytes": 9}}));
    // And the command's output came back as stream frames with its id, then its result
    let mut streams = streams;
    streams.sort();
    assert_eq!(streams, [("stderr".to_string(), "oops\n".to_string()), ("stdout".to_string(), "from rca\n".to_string())]);
    assert_eq!((ran["ok"].clone(), ran["result"]["exit_code"].clone()), (json!(true), json!(0)));
    // And the escape is an error frame with the distinguishing code, while the in-root write beside it worked
    assert_eq!((escaped["ok"].clone(), escaped["error"]["code"].clone()), (json!(false), json!("PATH_OUTSIDE_ROOT")));
    // And the cancel was answered and the cancelled command's process is gone
    assert_eq!(by_id["c1"]["result"]["cancelled"], true);
    assert_eq!((by_id["long"]["ok"].clone(), by_id["long"]["result"]["cancelled"].clone()), (json!(true), json!(true)));
    let pid: i32 = std::fs::read_to_string(dir.path().join("long.pid")).unwrap().trim().parse().unwrap();
    wait_for("the cancelled child", || gone(pid));
    // And a clean close from rca ends the client with its close code and no reconnect
    assert_eq!(channel.wait().unwrap(), Some(1000));
}

#[test]
fn closing_the_channel_mid_command_stops_the_command() {
    // Given a channel on which rca started a long command
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let rca = {
        let seen = seen.clone();
        std::thread::spawn(move || {
            let mut ws = accept(&listener, &seen);
            send(&mut ws, json!({"id": "long", "tool": "run_command", "args": {"command": "sleep 60 & echo $! > long.pid; wait"}}));
            ws
        })
    };
    let executor: Arc<dyn ToolExecutor> = Arc::new(executor(dir.path()));
    let channel = ExecChannel::open(&url, "t", "tok", executor).unwrap();
    let mut ws = rca.join().unwrap();
    wait_for("the command", || std::fs::read_to_string(dir.path().join("long.pid")).is_ok_and(|s| s.ends_with('\n')));
    let pid: i32 = std::fs::read_to_string(dir.path().join("long.pid")).unwrap().trim().parse().unwrap();
    // When rca drops the channel (the 4001 close of a replaced channel)
    ws.close(Some(tungstenite::protocol::CloseFrame { code: 4001.into(), reason: "replaced".into() })).unwrap();
    // Then the client ends with that code and the command's process is gone
    assert_eq!(channel.wait().unwrap(), Some(4001));
    wait_for("the orphaned command to die", || gone(pid));
}

const BIN: &str = env!("CARGO_BIN_EXE_rca-exec");

/// Runs `rca-exec serve` as its own process against `url`, with the bearer in `RCA_TEST_BEARER` unless `bearer` is None.
fn serve_process(url: &str, root: &Path, bearer: Option<&str>) -> std::process::Child {
    let mut command = std::process::Command::new(BIN);
    command.args(["serve", "--rca-url", url, "--workspace", "w 1", "--root", root.to_str().unwrap(), "--secret-env", "RCA_TEST_BEARER"]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    command.env_remove("RCA_TEST_BEARER");
    if let Some(bearer) = bearer {
        command.env("RCA_TEST_BEARER", bearer);
    }
    command.spawn().unwrap()
}

fn json_lines(child: std::process::Child) -> (i32, Vec<Value>) {
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}"))).collect())
}

#[test]
fn the_serve_verb_attaches_to_rca_for_its_workspace_executes_a_request_and_ends_with_the_close_code() {
    // Given an in-test rca and a served directory
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    // When `rca-exec serve` is started and rca asks it to write a file, then closes the channel cleanly
    let child = serve_process(&url, dir.path(), Some("tok"));
    let mut ws = accept(&listener, &seen);
    send(&mut ws, json!({"id": "w1", "tool": "write_file", "args": {"path": "made.txt", "content": "by rca"}}));
    let (_, wrote) = until_result(&mut ws, "w1");
    ws.close(Some(tungstenite::protocol::CloseFrame { code: 1000.into(), reason: "".into() })).unwrap();
    while ws.read().is_ok() {}
    let (code, lines) = json_lines(child);
    // Then the channel was opened for the workspace with the bearer, the file is in the directory, and the process printed attached then closed
    assert_eq!(seen.lock().unwrap().as_slice(), [("/exec/v2/executors?workspace=w%201".to_string(), Some("Bearer tok".to_string()))]);
    assert_eq!((wrote["ok"].clone(), std::fs::read_to_string(dir.path().join("made.txt")).unwrap()), (json!(true), "by rca".to_string()));
    assert_eq!(code, 0);
    assert_eq!(lines, [json!({"ok": true, "event": "attached", "workspace": "w 1", "executor": "w 1", "root": dir.path().to_str().unwrap(), "rcaUrl": url}), json!({"ok": true, "event": "closed", "closeCode": 1000})]);
}

#[test]
fn the_serve_verb_reports_a_replaced_channel_as_a_failure_with_its_close_code() {
    // Given an executor attached to an in-test rca
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let child = serve_process(&url, dir.path(), Some("tok"));
    let mut ws = accept(&listener, &Seen::default());
    // When rca replaces it with another executor of the same workspace (close 4001)
    ws.close(Some(tungstenite::protocol::CloseFrame { code: 4001.into(), reason: "replaced".into() })).unwrap();
    while ws.read().is_ok() {}
    let (code, lines) = json_lines(child);
    // Then the process exits 1 with an error line that carries the code and the next step
    assert_eq!(code, 1);
    let last = lines.last().unwrap();
    assert_eq!((last["ok"].clone(), last["closeCode"].clone()), (json!(false), json!(4001)));
    assert!(last["error"].as_str().unwrap().contains("replaced") && last["next"].as_str().is_some_and(|n| !n.is_empty()), "{last}");
}

#[test]
fn the_serve_verb_without_its_bearer_or_a_reachable_rca_fails_with_json_and_does_not_prompt() {
    // Given no bearer in the named variable, and then a bearer but nothing listening
    let dir = tempfile::tempdir().unwrap();
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    // When the verb runs
    let (no_bearer_code, no_bearer) = json_lines(serve_process(&url, dir.path(), None));
    let (refused_code, refused) = json_lines(serve_process(&url, dir.path(), Some("tok")));
    // Then each exits 1 with one {ok: false, error, next} line naming the problem
    assert_eq!((no_bearer_code, refused_code), (1, 1));
    assert!(no_bearer.len() == 1 && no_bearer[0]["ok"] == false && no_bearer[0]["error"].as_str().unwrap().contains("RCA_TEST_BEARER"), "{no_bearer:?}");
    assert!(refused.len() == 1 && refused[0]["ok"] == false && refused[0]["error"].as_str().unwrap().contains("channel connect"), "{refused:?}");
}

#[test]
fn git_diff_shows_the_working_trees_change_for_a_path_and_refuses_a_base_that_would_be_an_option() {
    // Given a git checkout as the root, with a committed file then changed
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(dir.path()).output().unwrap().status.success(), "{args:?}");
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "two\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "first"]);
    std::fs::write(dir.path().join("a.txt"), "one\nmore\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "changed\n").unwrap();
    let ex = executor(dir.path());
    let mut no_stream = |_: Stream, _: &str| {};
    // When the diff of a.txt is asked, Then it carries a.txt's added line and not b.txt
    let diff = execute(&ex, "d1", "git_diff", &serde_json::json!({"path": "a.txt"}), &mut no_stream, &mut |_| {}).unwrap();
    let text = diff["diff"].as_str().unwrap();
    assert!(text.contains("+more") && !text.contains("b.txt"), "{text}");
    // When the whole tree is diffed against HEAD, Then both files are in it
    let all = execute(&ex, "d2", "git_diff", &serde_json::json!({"base": "HEAD"}), &mut no_stream, &mut |_| {}).unwrap();
    assert!(all["diff"].as_str().unwrap().contains("b.txt"));
    // When the base looks like an option or the path leaves the root, Then each is refused before git runs
    let option = execute(&ex, "d3", "git_diff", &serde_json::json!({"base": "--output=/tmp/x"}), &mut no_stream, &mut |_| {}).unwrap_err();
    assert_eq!(option.code.as_str(), "INVALID_ARGS");
    let outside = execute(&ex, "d4", "git_diff", &serde_json::json!({"path": "../etc/passwd"}), &mut no_stream, &mut |_| {}).unwrap_err();
    assert_eq!(outside.code.as_str(), "PATH_OUTSIDE_ROOT");
}
