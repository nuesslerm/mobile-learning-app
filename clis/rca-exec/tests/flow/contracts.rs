//! The recorded exchanges of the executor contract (`libraries/agent-api/exec-*/tests/fixtures`), replayed through the real channel client on a
//! real directory: what rca sends and what the executor must answer, frame by frame. The service replays the same files against its own side
//! (`backend/remote-chat-agent/rs/tests/flow/contracts.rs`).
use super::{accept, recv, send, until_result, until_result_events, wait_for, Seen};
use exec_proto::testing::{Exchange, Fixture, Setup};
use rca_exec::adapters::exec_channel::ExecChannel;
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::ToolExecutor;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

fn fixtures() -> Vec<Fixture> {
    [exec_fs::testing::fixtures(), exec_proc::testing::fixtures(), exec_git::testing::fixtures(), exec_pty::testing::fixtures()].concat()
}

fn contract_actions() -> BTreeSet<&'static str> {
    [exec_fs::ACTIONS, exec_proc::ACTIONS, exec_git::ACTIONS, exec_pty::ACTIONS].into_iter().flatten().map(|a| a.name).collect()
}

const DATE: &str = "2020-01-01T00:00:00Z";

fn git_output(dir: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_AUTHOR_DATE", DATE)
        .env("GIT_COMMITTER_DATE", DATE)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let done = git_output(dir, args);
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
}

fn write_all(dir: &Path, files: &BTreeMap<String, String>) {
    for (path, content) in files {
        let at = dir.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, content).unwrap();
    }
}

fn empty(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(path).unwrap();
        } else {
            std::fs::remove_file(path).unwrap();
        }
    }
}

/// A program that prints `text` for `--version`, as an agent CLI does.
fn fake_cli(bin: &Path, program: &str, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = bin.join(program);
    std::fs::write(&path, format!("#!/bin/sh\nprintf '%s\\n' '{text}'\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Empty directories with what the exchange asks for: the root's files, then (when asked) a git repository that has committed them, then the
/// worktrees it names under the base, then the edits; the agent CLIs on the executor's path (`bin`) and the files of its home.
fn prepare(dir: &Path, base: &Path, (bin, home): (&Path, &Path), setup: &Setup) {
    empty(dir);
    empty(base);
    empty(bin);
    empty(home);
    write_all(home, &setup.home);
    for (program, text) in &setup.clis {
        fake_cli(bin, program, text);
    }
    write_all(dir, &setup.files);
    if setup.commit {
        git(dir, &["init", "-q", "-b", "main"]);
        for (key, value) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
            git(dir, &["config", key, value]);
        }
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "first"]);
    }
    for (name, branch) in &setup.worktrees {
        git(dir, &["worktree", "add", "-q", "-b", branch, base.join(name).to_str().unwrap()]);
    }
    if setup.remote {
        let remote = base.join("remote.git");
        git(base, &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
        git(dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(dir, &["push", "-q", "-u", "origin", "main"]);
        for number in &setup.remote_pulls {
            git(dir, &["switch", "-q", "-c", &format!("pull-{number}")]);
            std::fs::write(dir.join(format!("pull-{number}.txt")), format!("pull request {number}\n")).unwrap();
            git(dir, &["add", "-A"]);
            git(dir, &["commit", "-q", "-m", &format!("pull request {number}")]);
            git(dir, &["push", "-q", "origin", &format!("HEAD:refs/pull/{number}/head")]);
            git(dir, &["switch", "-q", "main"]);
            git(dir, &["branch", "-q", "-D", &format!("pull-{number}")]);
        }
    }
    if let Some(c) = &setup.conflict {
        let at = dir.join(&c.path);
        git(dir, &["switch", "-q", "-c", "other"]);
        std::fs::write(&at, &c.theirs).unwrap();
        git(dir, &["commit", "-q", "-am", "theirs"]);
        git(dir, &["switch", "-q", "main"]);
        std::fs::write(&at, &c.ours).unwrap();
        git(dir, &["commit", "-q", "-am", "ours"]);
        assert!(!git_output(dir, &["merge", "other"]).status.success(), "the merge was to conflict");
    }
    write_all(dir, &setup.edit);
    write_all(base, &setup.worktree_edit);
}

/// The output of an action, one string per stream: stdout and stderr interleave as the process wrote them, so only each stream's own order is stable.
fn by_stream(frames: impl IntoIterator<Item = (String, String)>) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (stream, data) in frames {
        out.entry(stream).or_default().push_str(&data);
    }
    out
}

#[test]
fn every_action_of_the_contract_has_recorded_exchanges_and_cancel_has_none_that_fails() {
    // Given the actions the contract crates name and the fixtures they ship
    let recorded = fixtures();
    // Then each action has one fixture, and no fixture names an action the contract does not
    let mut named: Vec<&str> = recorded.iter().map(|f| f.action.as_str()).collect();
    named.sort_unstable();
    assert_eq!(named, contract_actions().into_iter().collect::<Vec<_>>());
    for fixture in &recorded {
        // And each of its exchanges is a request for that action, with an id of its own, and the first one succeeds
        // (an action on a pty records its refusals only: the one pty a replay opens belongs to the pty_open recording)
        let needs_a_pty = [exec_pty::PTY_WRITE, exec_pty::PTY_RESIZE, exec_pty::PTY_ATTACH].contains(&fixture.action.as_str());
        assert!(!fixture.exchanges.is_empty() && (needs_a_pty || fixture.exchanges[0].succeeds()), "{}", fixture.action);
        let ids: BTreeSet<&str> = fixture.exchanges.iter().map(Exchange::id).collect();
        assert_eq!(ids.len(), fixture.exchanges.len(), "{}: ids repeat", fixture.action);
        assert!(fixture.exchanges.iter().all(|e| e.tool() == fixture.action && e.response["id"] == e.request["id"]), "{}", fixture.action);
        // And a cancel can never fail, so none of its exchanges does
        if fixture.action == exec_proc::CANCEL {
            assert!(fixture.exchanges.iter().all(Exchange::succeeds), "cancel has a failing exchange");
        }
    }
}

#[test]
fn every_executor_contract_crate_that_ships_recordings_has_them_among_the_inputs_of_this_flow() {
    // Given the executor contract crates that keep recorded exchanges, and this crate's manifest
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let crates = std::fs::read_dir(root.join("../../libraries/agent-api")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("exec-") && e.path().join("tests/fixtures").is_dir());
    for krate in crates {
        // When an exchange of one is edited, Then the flow reruns only if its directory is an input of the flow task
        let name = krate.file_name().to_string_lossy().into_owned();
        assert!(manifest.contains(&format!("libraries/agent-api/{name}/tests/fixtures/**")), "{name}'s recordings are not in [package.metadata.flow] inputs");
    }
}

#[test]
fn every_recorded_exchange_is_answered_over_the_channel_with_its_recorded_output_and_result() {
    // Given an in-test rca, an executor on a directory attached to it, and every recorded exchange
    let (dir, base, bin, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let recorded = fixtures();
    let rca = {
        let (root, base, recorded) = (dir.path().to_path_buf(), base.path().to_path_buf(), recorded.clone());
        let (bin, home) = (bin.path().to_path_buf(), home.path().to_path_buf());
        std::thread::spawn(move || {
            let mut ws = accept(&listener, &Seen::default());
            let mut answers = Vec::new();
            for exchange in recorded.iter().flat_map(|f| &f.exchanges) {
                // When rca sends the request into the directory the exchange sets up
                prepare(&root, &base, (&bin, &home), &exchange.setup);
                send(&mut ws, exchange.request.clone());
                let (stream, events, frame) = until_result_events(&mut ws, exchange.id());
                answers.push((stream, events, frame));
            }
            ws.close(Some(tungstenite::protocol::CloseFrame { code: 1000.into(), reason: "".into() })).unwrap();
            while !matches!(ws.read(), Err(tungstenite::Error::ConnectionClosed)) {}
            answers
        })
    };
    let executor: Arc<dyn ToolExecutor> = Arc::new(LocalExecutor::new(dir.path()).unwrap().with_worktree_base(base.path()).unwrap().with_cli_path(bin.path().to_str().unwrap()).with_skill_home(Some(home.path())));
    let channel = ExecChannel::open(&url, "contracts", "tok", executor).unwrap();
    let answers = rca.join().unwrap();
    channel.wait().unwrap();
    // Then every answer is the recorded one: the output of each stream and the result or error frame
    let exchanges: Vec<&Exchange> = recorded.iter().flat_map(|f| &f.exchanges).collect();
    assert_eq!(answers.len(), exchanges.len());
    for (exchange, (stream, events, frame)) in exchanges.iter().zip(answers) {
        let recorded_stream = by_stream(exchange.stream.iter().map(|s| (s["stream"].as_str().unwrap().to_string(), s["data"].as_str().unwrap().to_string())));
        assert_eq!(by_stream(stream), recorded_stream, "{}: {}", exchange.tool(), exchange.name);
        assert_eq!(events, exchange.events, "{}: {}", exchange.tool(), exchange.name);
        assert_eq!(exchange.masked(frame), exchange.masked(exchange.response.clone()), "{}: {}", exchange.tool(), exchange.name);
    }
}

#[test]
fn text_that_is_not_json_is_answered_with_an_error_frame_that_has_no_id_and_the_channel_keeps_serving() {
    // Given an executor attached to an in-test rca
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let rca = std::thread::spawn(move || {
        let mut ws = accept(&listener, &Seen::default());
        // When rca sends text that is not JSON, then a request that names no tool
        ws.send(tungstenite::Message::text("not json")).unwrap();
        let garbage = recv(&mut ws);
        send(&mut ws, json!({"id": "n1", "args": {}}));
        let (_, toolless) = until_result(&mut ws, "n1");
        ws.close(Some(tungstenite::protocol::CloseFrame { code: 1000.into(), reason: "".into() })).unwrap();
        while !matches!(ws.read(), Err(tungstenite::Error::ConnectionClosed)) {}
        (garbage, toolless)
    });
    let channel = ExecChannel::open(&url, "contracts", "tok", Arc::new(LocalExecutor::new(dir.path()).unwrap())).unwrap();
    let (garbage, toolless) = rca.join().unwrap();
    wait_for("the channel to end", || !channel.is_open());
    // Then the first is INVALID_ARGS with an empty id, and the second is UNKNOWN_TOOL by its own id
    assert_eq!((garbage["id"].clone(), garbage["ok"].clone(), garbage["error"]["code"].clone()), (json!(""), json!(false), json!("INVALID_ARGS")));
    assert!(garbage["error"]["message"].as_str().unwrap().starts_with("not JSON: "), "{garbage}");
    assert_eq!((toolless["ok"].clone(), toolless["error"]["code"].clone()), (json!(false), json!("UNKNOWN_TOOL")));
}
