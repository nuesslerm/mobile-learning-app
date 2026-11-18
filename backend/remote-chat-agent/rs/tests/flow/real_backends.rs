//! One finished turn of a real agent CLI through the whole chain (the service, the real `rca-exec`, the CLI run at the workspace host with its own
//! file tools), per backend. Opt in: the tests are ignored, and the hosted `real-backends` job runs the one its leg names with the CLI's pinned
//! binary first on `PATH` and its credential in the environment (`RCA_REAL_CLI_DIR`, `RCA_REAL_CREDENTIAL_ENV`, and for Codex `CODEX_OAUTH_JSON_B64`).

use crate::harness::{service_spawning, Env, Options, SECRET};
use crate::mapper::Agent;
use exec_testkit::{RcaExec, TempRepo};
use rca_cli_driver::testing::ScriptedSpawner;
use serde_json::json;
use std::time::Duration;

const PROMPT: &str = "Create a file named hello.txt in the current directory containing the word hi, using your file tools. Then reply with exactly the word PONG.";

fn executor_env() -> (std::ffi::OsString, Vec<(String, String)>, tempfile::TempDir) {
    let dir = std::env::var_os("RCA_REAL_CLI_DIR").expect("RCA_REAL_CLI_DIR names the directory of the pinned CLI");
    let system = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(std::iter::once(std::path::PathBuf::from(dir)).chain(std::env::split_paths(&system))).unwrap();
    let names = std::env::var("RCA_REAL_CREDENTIAL_ENV").unwrap_or_default();
    let env = names.split(',').filter(|n| !n.is_empty()).map(|n| (n.to_string(), std::env::var(n).unwrap_or_else(|_| panic!("{n} is not set")))).collect();
    let home = tempfile::tempdir().unwrap();
    if let Ok(login) = std::env::var("CODEX_OAUTH_JSON_B64") {
        let bytes = base64_decode(login.trim());
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        std::fs::write(home.path().join(".codex/auth.json"), bytes).unwrap();
    }
    (path, env, home)
}

fn base64_decode(text: &str) -> Vec<u8> {
    let out = std::process::Command::new("base64").arg("-d").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().and_then(|mut c| {
        use std::io::Write;
        c.stdin.take().unwrap().write_all(text.as_bytes())?;
        c.wait_with_output()
    });
    out.expect("base64 decodes the login").stdout
}

async fn real_turn(driver: &str, model: Option<&str>) {
    // Given the service, the real executor with the pinned CLI and its credential, and a mapper following a new thread
    let env = Env::new(&[]);
    let (path, creds, home) = executor_env();
    env.use_path(&path);
    let repo = TempRepo::new();
    let svc = service_spawning(&env, Options { at_workspace: true, ..Options::default() }, ScriptedSpawner::new());
    let port = svc.listen().await;
    let _exec = tokio::task::block_in_place(|| RcaExec::serve_with_env(&format!("http://127.0.0.1:{port}"), "w1", &repo, SECRET, &[], &path, Some(home.path()), &creds));
    let token = svc.mint("real backend").await;
    let mut agent = Agent::open(port, &token).await;
    let made = agent.ask("thread.create", json!({"thread": "t1", "workspace": "w1", "driver": driver})).await;
    assert!(made.get("result").is_some(), "{made}");
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    // When a turn asks it to write a file and answer
    let mut start = json!({"thread": "t1", "workspace": "w1", "driver": driver, "prompt": PROMPT, "runtime_mode": "full-access"});
    if let Some(model) = model {
        start["model"] = json!(model);
    }
    let started = agent.ask("turn.start", start).await;
    assert!(started.get("result").is_some(), "{started}");
    let events = tokio::time::timeout(Duration::from_secs(420), agent.events_through("turn.finished")).await.expect("the turn finished within seven minutes");
    // Then it finished completed, its answer carries the word, and the file its own tool wrote is in the workspace's repository
    let finished = events.last().unwrap();
    assert_eq!(finished.data["reason"], json!("completed"), "{driver}: {:?}", events.iter().map(|e| (e.kind.clone(), e.data.clone())).collect::<Vec<_>>());
    let answer: String = events.iter().filter(|e| e.kind == "message.delta" && e.data["role"] == json!("assistant")).filter_map(|e| e.data["text"].as_str()).collect();
    assert!(answer.to_uppercase().contains("PONG"), "{driver} answered {answer:?}");
    assert!(repo.path().join("hello.txt").exists(), "{driver} wrote no hello.txt: {:?}", events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
#[ignore = "a real model: run by the real-backends job with RCA_REAL_CLI_DIR and the credential"]
async fn given_the_pinned_claude_when_a_turn_writes_a_file_then_it_finishes_completed_with_the_file_in_the_workspace() {
    real_turn("claudeAgent", None).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
#[ignore = "a real model: run by the real-backends job with RCA_REAL_CLI_DIR and the credential"]
async fn given_the_pinned_codex_when_a_turn_writes_a_file_then_it_finishes_completed_with_the_file_in_the_workspace() {
    real_turn("codex", None).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
#[ignore = "a real model: run by the real-backends job with RCA_REAL_CLI_DIR and the credential"]
async fn given_the_pinned_opencode_when_a_turn_writes_a_file_then_it_finishes_completed_with_the_file_in_the_workspace() {
    real_turn("opencode", Some("opencode-go/deepseek-v4.1-flash")).await;
}
