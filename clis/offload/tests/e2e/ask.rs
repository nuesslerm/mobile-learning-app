//! `wf offload ask` against the `rca-service` and `opencode serve` this run starts on loopback (`rca.rs`), the scripted model behind it
//! (no network, no key): a session continues with `-c`, a
//! run is read-only without `--auto` and edits with it, an agent defined in the repository answers as defined, and `--format json`
//! reports the session and the tokens.
use crate::rca::Model;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// What a finished `wf offload ask` left.
struct RunResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

/// The CLI with the environment of the machine running the tests, not `run_wf`'s isolated one, pointed at this run's service. A client
/// timeout keeps a stuck service from stalling the stage.
fn ask(dir: &Path, args: &[&str]) -> RunResult {
    let out = crate::rca::point(
        Command::new(crate::support::wf_binary())
            .args(["offload", "ask"])
            .args(args)
            .args(["--timeout-ms", "600000", "--caller", &crate::parity_support::caller_of(dir)])
            .current_dir(dir)
            .env("WF_USAGE", "off"),
        Model::Scripted,
    )
    .output()
        .expect("the wf binary starts");
    RunResult { exit_code: out.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&out.stdout).into_owned(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
}

/// A checkout that drops the sessions its asks made when the test is done with it.
struct Checkout(tempfile::TempDir);

impl Checkout {
    fn path(&self) -> &Path {
        self.0.path()
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        crate::parity_support::purge(&crate::parity_support::caller_of(self.0.path()), Model::Scripted);
    }
}

fn checkout() -> Checkout {
    let dir = tempfile::Builder::new().prefix("wf-e2e-ask-").tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    Checkout(dir)
}

fn said(r: &RunResult) -> String {
    r.stdout.to_uppercase()
}

#[test]
fn a_session_continues_with_c_and_the_json_format_reports_it() {
    crate::parity_support::retrying(|| {
        // Given a first turn that sets a word, with a title
        let dir = checkout();
        let first = ask(dir.path(), &["Remember the secret word PELICAN. Reply with only OK.", "--title", "wf e2e", "--format", "json"]);
        assert_eq!(first.exit_code, 0, "{}{}", first.stdout, first.stderr);
        let events: Vec<Value> = first.stdout.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {}", first.stdout))).collect();
        let session = events[0]["sessionID"].as_str().expect("every event names its session").to_string();
        let finish = events.iter().find(|e| e["type"] == "step_finish").expect("the turn ends with a step_finish");
        assert!(finish["part"]["tokens"]["total"].as_f64().is_some_and(|t| t > 0.0), "{finish}");

        // When the next turn continues the checkout's last session
        let second = ask(dir.path(), &["What was the secret word? Reply with just the word.", "-c"]);

        // Then the same session answers and knows the word
        assert_eq!(second.exit_code, 0, "{}{}", second.stdout, second.stderr);
        assert!(said(&second).contains("PELICAN") && second.stderr.contains(&format!("session {session}")), "{}{}", second.stdout, second.stderr);
    });
}

#[test]
fn a_run_edits_the_file_without_auto_and_a_read_only_agent_keeps_it_unchanged() {
    crate::parity_support::retrying(|| {
        // Given a file in the checkout, and the read-only agent defined there
        let dir = checkout();
        std::fs::write(dir.path().join("a.txt"), "original").unwrap();
        std::fs::create_dir_all(dir.path().join(".opencode/agents")).unwrap();
        std::fs::write(dir.path().join(".opencode/agents/read-only.md"), include_str!("../../../../.opencode/agents/read-only.md")).unwrap();
        let prompt = "Overwrite a.txt with the text CHANGED using your file tools, then say DONE.";

        // When a run as the read-only agent is asked to overwrite it, even with --auto, Then the file is as it was
        let read_only = ask(dir.path(), &[prompt, "-f", "a.txt", "--agent", "read-only", "--auto"]);
        assert_eq!(read_only.exit_code, 0, "{}{}", read_only.stdout, read_only.stderr);
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "original");

        // When the same run has no agent (opencode's build agent), Then the file is rewritten in the checkout, as `opencode run` does
        let writes = ask(dir.path(), &[prompt, "-f", "a.txt", "--variant", "high"]);
        assert_eq!(writes.exit_code, 0, "{}{}", writes.stdout, writes.stderr);
        assert!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap().contains("CHANGED"), "{}", writes.stdout);
    });
}

#[test]
fn an_agent_defined_in_the_repository_answers_as_defined() {
    crate::parity_support::retrying(|| {
        // Given an agent file in the checkout
        let dir = checkout();
        std::fs::create_dir_all(dir.path().join(".opencode/agents")).unwrap();
        std::fs::write(
            dir.path().join(".opencode/agents/pirate.md"),
            "---\ndescription: pirate\nmode: primary\ntools:\n  write: false\n  edit: false\n  bash: false\n---\nAlways answer with exactly the word ARRR and nothing else.\n",
        )
        .unwrap();

        // When a run names the agent, Then it answers as the agent file says
        let r = ask(dir.path(), &["Say hello.", "--agent", "pirate"]);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        assert!(said(&r).contains("ARRR"), "{}", r.stdout);
    });
}

/// Fails, naming both versions, unless the installed opencode is the one the fixtures were recorded from
/// (their `opencodeVersion`, which `.github/actions/install-test-deps` installs: the one pin).
fn assert_pinned_opencode(fixture: &str) {
    let recorded: Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/flow/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
    let out = Command::new("opencode").arg("--version").output().expect("opencode is on PATH");
    let installed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(installed, recorded["opencodeVersion"].as_str().unwrap(), "installed opencode {installed} is not the {fixture} version: run `npm install -g opencode-ai@<opencodeVersion>` (or re-record the fixture and bump opencodeVersion)");
}

/// The pinned opencode lists no option the recorded table
/// (`tests/flow/fixtures/opencode-run-options.json`, which the flow tests hold `ask` to) lacks.
#[test]
fn the_recorded_options_are_the_ones_opencode_run_lists_now() {
    assert_pinned_opencode("opencode-run-options.json");
    let help = Command::new("opencode").args(["run", "--help"]).output().expect("opencode is on PATH");
    let text = String::from_utf8_lossy(&help.stdout).to_string() + &String::from_utf8_lossy(&help.stderr);
    let mut listed: Vec<(Option<char>, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let (short, rest) = match line.strip_prefix('-') {
            Some(r) if !r.starts_with('-') => (r.chars().next(), r[1..].trim_start_matches(", ")),
            _ => (None, line),
        };
        if let Some(long) = rest.strip_prefix("--") {
            listed.push((short, long.split_whitespace().next().unwrap_or_default().to_string()));
        }
    }
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/flow/fixtures/opencode-run-options.json")).unwrap();
    let recorded: Value = serde_json::from_str(&fixture).unwrap();
    let recorded: Vec<(Option<char>, String)> = recorded["options"].as_array().unwrap().iter().map(|o| (o["short"].as_str().and_then(|s| s.chars().next()), o["long"].as_str().unwrap().to_string())).collect();
    let mut now = listed;
    let mut then = recorded;
    now.sort();
    then.sort();
    assert_eq!(now, then, "opencode run's options changed: update the fixture and decide what `wf offload ask` does with each new one");
}

/// The commands `opencode --help` lists on the installed binary are the ones the recorded table accounts for, each one `wf offload ask`
/// replaces (`run`) or has a reason it is not applicable to a run on the service.
#[test]
fn the_recorded_commands_are_the_ones_opencode_lists_now() {
    assert_pinned_opencode("opencode-commands.json");
    let help = Command::new("opencode").arg("--help").output().expect("opencode is on PATH");
    let text = String::from_utf8_lossy(&help.stdout).to_string() + &String::from_utf8_lossy(&help.stderr);
    let mut listed: Vec<String> = text
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.trim().strip_prefix("opencode ").and_then(|rest| rest.split_whitespace().next()).map(|name| name.trim_start_matches('[').trim_end_matches(']').to_string()))
        .collect();
    // the default command is the TUI, recorded as `[project]`
    for name in listed.iter_mut() {
        if name == "project" {
            *name = "[project]".into();
        }
    }
    let table: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/flow/fixtures/opencode-commands.json")).unwrap()).unwrap();
    let mut recorded: Vec<String> = table["commands"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
    for command in table["commands"].as_array().unwrap() {
        let note = command["note"].as_str().unwrap_or_default();
        assert!(!note.is_empty() && (command["status"] == "same" || command["status"] == "not-applicable"), "{command}");
    }
    listed.sort();
    recorded.sort();
    assert_eq!(listed, recorded, "opencode lists other commands than tests/flow/fixtures/opencode-commands.json records: add each new one with its status and why");
}

#[test]
fn given_no_rca_host_in_the_environment_when_asks_are_pointed_then_they_go_to_the_loopback_service_of_the_run_and_not_the_deployed_one() {
    use std::io::{Read, Write};
    // Given the parent has no RCA_HOST of its own
    let env: std::collections::HashMap<_, _> = crate::rca::rca_env(Model::Scripted).iter().cloned().collect();
    // When the run's variables are read, Then the host is loopback with no tailnet in the way
    assert_eq!(env["RCA_HOST"], "127.0.0.1");
    assert_eq!(env["WF_TAILNET_SOCKS"], "");
    // And the service answers there
    let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", env["RCA_PORT"])).expect("the service listens on loopback");
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}
