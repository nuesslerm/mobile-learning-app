//! `session_open` and `session_send` on real processes: a scripted `codex app-server` that answers each line it is sent, kept open between
//! the lines, stopped by a cancel, and ended when it exits even if something it left behind still holds its output. The scripted CLIs are authored
//! doubles; the recorded exchanges of both actions are replayed in `contracts.rs`.
use exec_proto::{Code, ExecError, Stream};
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::ToolExecutor;
use exec_proc::{SessionOpen, SessionSend};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

fn dir() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = std::fs::canonicalize(d.path()).unwrap();
    (d, p)
}

fn codex(bin: &Path, body: &str) {
    let path = bin.join("codex");
    std::fs::write(&path, format!("#!/bin/sh\n[ \"$1\" = app-server ] || exit 2\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn executor(root: &Path, home: &Path, bin: &Path) -> Arc<LocalExecutor> {
    Arc::new(LocalExecutor::new(root).unwrap().with_skill_home(Some(home)).with_cli_path(bin.to_str().unwrap()))
}

fn open(session: &str) -> SessionOpen {
    SessionOpen { session: session.into(), program: "codex".into(), args: Vec::new(), cwd: None }
}

fn send(ex: &LocalExecutor, session: &str, line: &str) -> Result<usize, ExecError> {
    ex.session_send(&SessionSend { session: session.into(), line: line.into() })
}

/// What a running session told its caller: its chunks of output and the `started` event.
enum Heard {
    Started,
    Out(String),
    Ended(Result<exec_proc::SessionEnded, ExecError>),
}

/// Opens `session` on a thread of its own, as the channel does for a long action.
fn start(ex: &Arc<LocalExecutor>, id: &str, session: &str) -> mpsc::Receiver<Heard> {
    start_with(ex, id, open(session))
}

fn start_with(ex: &Arc<LocalExecutor>, id: &str, args: SessionOpen) -> mpsc::Receiver<Heard> {
    let (tx, rx) = mpsc::channel();
    let (ex, id) = (Arc::clone(ex), id.to_string());
    std::thread::spawn(move || {
        let (out, started) = (tx.clone(), tx.clone());
        let ended = ex.session_open(&id, &args, &mut |s, d| {
            if s == Stream::Stdout {
                let _ = out.send(Heard::Out(d.to_string()));
            }
        }, &mut |event: Value| {
            if event["kind"] == "session.started" {
                let _ = started.send(Heard::Started);
            }
        });
        let _ = tx.send(Heard::Ended(ended));
    });
    rx
}

fn next(rx: &mpsc::Receiver<Heard>) -> Heard {
    rx.recv_timeout(Duration::from_secs(10)).expect("the session said something")
}

fn gone(pid: i32) -> bool {
    #[cfg(not(target_os = "linux"))]
    {
        // SAFETY: signal zero only checks whether this PID exists.
        let status = unsafe { libc::kill(pid, 0) };
        status == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
    #[cfg(target_os = "linux")]
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
}

#[test]
fn given_a_claude_shell_task_when_it_completes_then_the_session_relays_only_its_owned_output() {
    use serde_json::json;
    use std::os::unix::fs::symlink;

    // Given a Claude session with task output in its spool and a second task pointing outside it
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    let session_id = "3f1c0e52-8d3b-4c71-9a55-0a1b2c3d4e5f";
    let base = std::env::var_os("CLAUDE_CODE_TMPDIR").map(PathBuf::from).unwrap_or_else(|| {
        if cfg!(target_os = "macos") { PathBuf::from("/tmp") } else { std::env::temp_dir() }
    });
    let uid = unsafe { libc::geteuid() };
    let spool = base.join(format!("claude-{uid}"));
    std::fs::create_dir_all(&spool).unwrap();
    let case = tempfile::tempdir_in(spool).unwrap();
    let task_dir = std::fs::canonicalize(case.path()).unwrap().join(session_id).join("tasks");
    std::fs::create_dir_all(&task_dir).unwrap();
    let allowed = task_dir.join("owned.output");
    let outside = case.path().join("outside.output");
    std::fs::write(&allowed, "detached stdout marker\n").unwrap();
    std::fs::write(&outside, "unrelated file").unwrap();
    let large = task_dir.join("large.output");
    std::fs::write(&large, "x".repeat((32 << 10) + 1)).unwrap();
    symlink(&outside, task_dir.join("linked.output")).unwrap();
    std::fs::hard_link(&outside, task_dir.join("hardlinked.output")).unwrap();
    let cli = bin.join("claude");
    let mut lines = Vec::new();
    for (task, file, kind) in [
        ("owned", allowed, "local_bash"),
        ("large", large, "local_bash"),
        ("outside", outside, "local_bash"),
        ("linked", task_dir.join("linked.output"), "local_bash"),
        ("hardlinked", task_dir.join("hardlinked.output"), "local_bash"),
        ("transcript", {
            let file = task_dir.join("transcript.output");
            std::fs::write(&file, "unrelated agent transcript").unwrap();
            file
        }, "local_agent"),
    ] {
        lines.push(json!({"type":"system","subtype":"task_started","session_id":session_id,"task_id":task,"run_id":"provider-run","is_backgrounded":true,"task_type":kind}));
        lines.push(json!({"type":"system","subtype":"task_notification","session_id":session_id,"task_id":task,"run_id":"provider-run","status":"completed","summary":"Task finished","output_file":file}));
    }
    let frames = lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    std::fs::write(&cli, format!("#!/bin/sh\n/bin/cat <<'RCA_FRAMES'\n{frames}\nRCA_FRAMES\n")).unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();

    // When the real executor streams the provider's task notifications
    let ex = executor(&root, &home, &bin);
    let args = SessionOpen { session: "claude-case".into(), program: "claude".into(), args: vec!["--session-id".into(), session_id.into()], cwd: None };
    let mut stdout = String::new();
    let ended = ex.session_open("claude-case", &args, &mut |stream, text| {
        if stream == Stream::Stdout {
            stdout.push_str(text);
        }
    }, &mut |_| {}).unwrap();

    // Then only the session's regular shell output was attached and all notifications arrived
    assert_eq!(ended.exit_code, 0);
    let notifications: Vec<Value> = stdout.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).filter(|v| v["subtype"] == "task_notification").collect();
    assert_eq!(notifications.len(), 6);
    assert_eq!(notifications[0]["rca_task_stdout"], json!({"text":"detached stdout marker\n","truncated":false}));
    assert_eq!(notifications[1]["rca_task_stdout"], json!({"text":"x".repeat(32 << 10),"truncated":true}));
    assert!(notifications[2..].iter().all(|frame| frame.get("rca_task_stdout").is_none()));
}

#[test]
fn given_a_session_cli_that_answers_each_line_when_lines_are_sent_then_each_answer_streams_back_in_order_and_a_cancel_ends_it_with_its_group() {
    // Given an executor whose codex answers every line it reads and records its own pid
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    let pidfile = root.join("pid");
    codex(&bin, &format!("echo $$ > {}\nwhile IFS= read -r line; do printf 'echo:%s\\n' \"$line\"; done", pidfile.display()));
    let ex = executor(&root, &home, &bin);
    // When the session is opened and two lines are sent once it has started
    let heard = start(&ex, "a1", "s-1");
    assert!(matches!(next(&heard), Heard::Started));
    assert_eq!(send(&ex, "s-1", "one").unwrap(), 4);
    assert_eq!(send(&ex, "s-1", "two").unwrap(), 4);
    let mut text = String::new();
    while text.matches('\n').count() < 2 {
        let Heard::Out(chunk) = next(&heard) else { panic!("the session ended early") };
        text.push_str(&chunk);
    }
    // Then both answers came back in order from one process that is still running
    assert_eq!(text, "echo:one\necho:two\n");
    let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    assert!(!gone(pid));
    // And a cancel of the open action ends it as cancelled and the process is gone, and its id can be used again
    assert!(ex.cancel("a1"));
    let Heard::Ended(ended) = next(&heard) else { panic!("no end") };
    assert!(ended.unwrap().cancelled);
    assert!(gone(pid));
    assert_eq!(send(&ex, "s-1", "late").unwrap_err().code, Code::NotFound);
    let again = start(&ex, "a2", "s-1");
    assert!(matches!(next(&again), Heard::Started));
    assert!(ex.cancel("a2"));
}

#[test]
fn given_a_session_that_is_open_when_the_same_id_is_opened_again_then_it_is_refused_and_the_first_is_untouched() {
    // Given an open session
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    codex(&bin, "while IFS= read -r line; do printf 'echo:%s\\n' \"$line\"; done");
    let ex = executor(&root, &home, &bin);
    let first = start(&ex, "a1", "s-1");
    assert!(matches!(next(&first), Heard::Started));
    // When the same id is opened again
    let second = start(&ex, "a2", "s-1");
    // Then the second is INVALID_ARGS without starting, and the first still answers
    let Heard::Ended(refused) = next(&second) else { panic!("the second did not end") };
    assert_eq!(refused.unwrap_err().code, Code::InvalidArgs);
    send(&ex, "s-1", "still").unwrap();
    assert!(matches!(next(&first), Heard::Out(text) if text == "echo:still\n"));
    assert!(ex.cancel("a1"));
}

#[test]
fn given_a_session_cli_that_exits_leaving_a_process_that_holds_its_output_when_it_runs_then_the_open_ends_with_the_cli_and_nothing_is_left_behind() {
    // Given a codex that prints a line, starts a long sleep that inherits its output, and exits 0
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    let pidfile = root.join("child.pid");
    codex(&bin, &format!("echo ready\nsleep 30 &\necho $! > {}\nexit 0", pidfile.display()));
    let ex = executor(&root, &home, &bin);
    let started = Instant::now();
    // When the session is opened
    let heard = start(&ex, "a1", "s-1");
    let mut said = String::new();
    let ended = loop {
        match next(&heard) {
            Heard::Started => {}
            Heard::Out(text) => said.push_str(&text),
            Heard::Ended(done) => break done,
        }
    };
    // Then it ends in seconds with its own exit status and the line it printed, not when the sleep does, and the sleep is gone
    assert_eq!((said.as_str(), ended.unwrap().exit_code), ("ready\n", 0));
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !gone(pid) {
        assert!(Instant::now() < until, "the background process is still running");
        std::thread::sleep(Duration::from_millis(20));
    }
    // And a line for the ended session is refused
    assert_eq!(send(&ex, "s-1", "late").unwrap_err().code, Code::NotFound);
}

#[test]
fn given_a_claude_session_when_it_is_opened_with_a_conversation_then_the_cli_is_started_with_its_stream_json_arguments_and_the_callers_flags_and_a_line_reaches_it() {
    // Given an executor whose claude prints the arguments it was started with, then echoes each line it reads
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    let path = bin.join("claude");
    std::fs::write(&path, "#!/bin/sh\nprintf 'argv:%s\\n' \"$*\"\nwhile IFS= read -r line; do printf 'echo:%s\\n' \"$line\"; done\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ex = executor(&root, &home, &bin);
    let id = "6b0b2f0e-6d0e-4a0a-9a52-8d54b4b9a4de";
    let (tx, rx) = mpsc::channel();
    let (runner, args) = (Arc::clone(&ex), SessionOpen { session: "s-1".into(), program: "claude".into(), args: vec!["--resume".into(), id.into(), "--permission-mode".into(), "acceptEdits".into()], cwd: None });
    std::thread::spawn(move || {
        let (out, started) = (tx.clone(), tx.clone());
        let ended = runner.session_open("a1", &args, &mut |s, d| {
            if s == Stream::Stdout {
                let _ = out.send(Heard::Out(d.to_string()));
            }
        }, &mut |event: Value| {
            if event["kind"] == "session.started" {
                let _ = started.send(Heard::Started);
            }
        });
        let _ = tx.send(Heard::Ended(ended));
    });
    assert!(matches!(next(&rx), Heard::Started));
    // When a line is sent
    send(&ex, "s-1", r#"{"type":"user"}"#).unwrap();
    let mut text = String::new();
    while text.matches('\n').count() < 2 {
        let Heard::Out(chunk) = next(&rx) else { panic!("the session ended early") };
        text.push_str(&chunk);
    }
    // Then the CLI started with its own arguments first and the caller's flags after, and answered the line on the same process
    let argv = text.lines().next().unwrap();
    assert!(argv.starts_with("argv:-p --input-format stream-json --output-format stream-json --verbose") && argv.ends_with(&format!("--permission-prompt-tool stdio --resume {id} --permission-mode acceptEdits")), "{argv}");
    assert_eq!(text.lines().nth(1), Some(r#"echo:{"type":"user"}"#));
    assert!(ex.cancel("a1"));
}

#[test]
fn given_a_claude_session_that_exits_leaving_a_process_that_holds_its_output_when_it_runs_then_the_open_ends_with_the_cli_in_seconds_and_nothing_is_left_behind() {
    // Given a claude that prints a line, starts a long sleep that inherits its output, and exits 0
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    let pidfile = root.join("child.pid");
    let path = bin.join("claude");
    std::fs::write(&path, format!("#!/bin/sh\necho ready\nsleep 30 &\necho $! > {}\nexit 0\n", pidfile.display())).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ex = executor(&root, &home, &bin);
    let started = Instant::now();
    // When the session is opened
    let heard = start_with(&ex, "a1", SessionOpen { session: "s-1".into(), program: "claude".into(), args: vec!["--session-id".into(), "6b0b2f0e-6d0e-4a0a-9a52-8d54b4b9a4de".into()], cwd: None });
    let mut said = String::new();
    let ended = loop {
        match next(&heard) {
            Heard::Started => {}
            Heard::Out(text) => said.push_str(&text),
            Heard::Ended(done) => break done,
        }
    };
    // Then it ends in seconds with its own exit status and the line it printed, not when the sleep does, and the sleep is gone
    assert_eq!((said.as_str(), ended.unwrap().exit_code), ("ready\n", 0));
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !gone(pid) {
        assert!(Instant::now() < until, "the background process is still running");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What an opencode session said, as the lines of its output, read until `want` of them match or it ends.
fn lines_until(heard: &mpsc::Receiver<Heard>, text: &mut String, done: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    loop {
        let lines: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        if done(&lines) {
            return lines;
        }
        // the real server takes a while to start in a fresh home
        match heard.recv_timeout(Duration::from_secs(90)).unwrap_or_else(|_| panic!("the session said nothing more: {}", &text[text.len().saturating_sub(3000)..])) {
            Heard::Out(chunk) => text.push_str(&chunk),
            Heard::Started => {}
            Heard::Ended(_) => panic!("the session ended early: {text}"),
        }
    }
}

fn request(ex: &LocalExecutor, id: u64, method: &str, path: &str, body: Value) {
    send(ex, "o-1", &serde_json::json!({"id": id, "method": method, "path": path, "body": body}).to_string()).unwrap();
}

fn answer(lines: &[Value], id: u64) -> Option<&Value> {
    lines.iter().find(|l| l["id"] == id)
}

#[test]
fn given_an_opencode_session_when_requests_are_sent_then_the_executor_makes_them_with_its_own_credential_and_the_answers_and_events_stream_back_without_it() {
    // Given an executor whose opencode is the scripted server, a script that plays one turn saying hello, and a session opened for it
    let ((_r, root), (_h, home)) = (dir(), dir());
    let cli = exec_testkit::ScriptedCli::install();
    cli.script("opencode", &exec_testkit::Script::of(vec![exec_testkit::Turn::default().then_line(r#"{"type":"text","part":{"text":"hello"}}"#)]));
    let ex = Arc::new(LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home)).with_cli_path(cli.dir().to_str().unwrap()));
    let heard = start_with(&ex, "a1", SessionOpen { session: "o-1".into(), program: "opencode".into(), args: Vec::new(), cwd: None });
    let mut text = String::new();
    // When the server is ready, a session is created and a prompt is sent
    lines_until(&heard, &mut text, |l| l.iter().any(|v| v["ready"] == true));
    request(&ex, 1, "POST", &format!("/session?directory={}", root.display()), serde_json::json!({"title": "t"}));
    let lines = lines_until(&heard, &mut text, |l| answer(l, 1).is_some());
    let created: Value = serde_json::from_str(answer(&lines, 1).unwrap()["body"].as_str().unwrap()).unwrap();
    let session = created["id"].as_str().unwrap().to_string();
    request(&ex, 2, "POST", &format!("/session/{session}/prompt_async?directory={}", root.display()), serde_json::json!({"parts": [{"type": "text", "text": "go"}]}));
    let lines = lines_until(&heard, &mut text, |l| l.iter().any(|v| v["event"]["payload"]["type"] == "session.idle"));
    // Then the server answered the create with 200 and the prompt with 204, and its events came as frames, the text among them
    assert_eq!((answer(&lines, 1).unwrap()["status"].as_u64(), answer(&lines, 2).unwrap()["status"].as_u64()), (Some(200), Some(204)));
    let kinds: Vec<&str> = lines.iter().filter_map(|l| l["event"]["payload"]["type"].as_str()).collect();
    assert!(kinds.contains(&"session.status") && kinds.contains(&"message.part.delta") && kinds.contains(&"message.updated"), "{kinds:?}");
    // And every request carried a valid credential, which is nowhere in what the session said
    let requests = cli.opencode_requests("opencode");
    assert!(!requests.is_empty() && requests.iter().all(|r| r["authorized"] == true), "{requests:?}");
    let password = cli.opencode_password("opencode").unwrap();
    assert!(password.len() >= 32 && !text.contains(&password));
    assert!(ex.cancel("a1"));
}

#[test]
fn given_an_opencode_session_when_a_line_names_no_route_the_adapter_drives_then_it_is_refused_and_nothing_reaches_the_server() {
    // Given an executor whose opencode is the scripted server, a session whose server is ready
    let ((_r, root), (_h, home)) = (dir(), dir());
    let cli = exec_testkit::ScriptedCli::install();
    cli.script("opencode", &exec_testkit::Script::of(vec![]));
    let ex = Arc::new(LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home)).with_cli_path(cli.dir().to_str().unwrap()));
    let heard = start_with(&ex, "a1", SessionOpen { session: "o-1".into(), program: "opencode".into(), args: Vec::new(), cwd: None });
    assert!(matches!(next(&heard), Heard::Started));
    let mut text = String::new();
    lines_until(&heard, &mut text, |l| l.iter().any(|v| v["ready"] == true));
    // When a request off the list, a PUT and a line that is no request are sent
    let refused: Vec<_> = [r#"{"id":2,"method":"GET","path":"/file?path=/etc/passwd"}"#, r#"{"id":3,"method":"PUT","path":"/session/x"}"#, "not json"].iter().map(|line| send(&ex, "o-1", line).unwrap_err().code).collect();
    // Then the others are INVALID_ARGS, and the server saw nothing of them
    assert!(refused.iter().all(|c| *c == Code::InvalidArgs), "{refused:?}");
    assert!(cli.opencode_requests("opencode").iter().all(|r| r["path"] == "/global/event"), "{:?}", cli.opencode_requests("opencode"));
    // And a cancel ends the session with its server
    assert!(ex.cancel("a1"));
    loop {
        if let Heard::Ended(ended) = next(&heard) {
            assert!(ended.unwrap().cancelled);
            break;
        }
    }
}

#[test]
#[ignore = "runs the real opencode of the PATH (no model turn): `cargo test -- --ignored real_opencode`"]
fn given_the_real_opencode_when_a_session_is_opened_then_the_bridge_reaches_its_server_and_event_stream_with_the_credential_it_made() {
    // Given an executor whose PATH is the real opencode's directory, and a session opened for it in a home of its own
    let ((_r, root), (_h, home)) = (dir(), dir());
    let real = std::env::var("OPENCODE_BIN_DIR").unwrap_or_else(|_| "/root/.local/bin".into());
    let ex = Arc::new(LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home)).with_cli_path(&real));
    let heard = start_with(&ex, "a1", SessionOpen { session: "o-1".into(), program: "opencode".into(), args: Vec::new(), cwd: None });
    let mut text = String::new();
    // When the server is ready, a session is created and a prompt that runs no model (noReply) is sent
    lines_until(&heard, &mut text, |l| l.iter().any(|v| v["ready"] == true || v["closed"].is_string()));
    request(&ex, 1, "GET", "/global/health", Value::Null);
    request(&ex, 2, "POST", &format!("/session?directory={}", root.display()), serde_json::json!({"title": "real"}));
    let lines = lines_until(&heard, &mut text, |l| answer(l, 1).is_some() && answer(l, 2).is_some());
    let created: Value = serde_json::from_str(answer(&lines, 2).unwrap()["body"].as_str().unwrap()).unwrap();
    let session = created["id"].as_str().unwrap().to_string();
    request(&ex, 3, "POST", &format!("/session/{session}/prompt_async?directory={}", root.display()), serde_json::json!({"noReply": true, "parts": [{"type": "text", "text": "hi"}]}));
    let lines = lines_until(&heard, &mut text, |l| l.iter().any(|v| v["event"]["payload"]["type"] == "message.part.updated" && v["event"]["payload"]["properties"]["part"]["text"] == "hi"));
    // Then the health answered healthy, the create 200 with a session id, the prompt 204, and the prompt's own events came through the stream
    assert!(answer(&lines, 1).unwrap()["body"].as_str().unwrap().contains("\"healthy\":true"));
    assert_eq!((answer(&lines, 2).unwrap()["status"].as_u64(), answer(&lines, 3).unwrap()["status"].as_u64()), (Some(200), Some(204)));
    assert!(session.starts_with("ses_"), "{session}");
    assert!(ex.cancel("a1"));
}
