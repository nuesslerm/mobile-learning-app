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
    SessionOpen { session: session.into(), program: "codex".into(), cwd: None }
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
    let (tx, rx) = mpsc::channel();
    let (ex, id, args) = (Arc::clone(ex), id.to_string(), open(session));
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
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
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
    assert_eq!(ended.unwrap().cancelled, true);
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
