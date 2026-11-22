//! The live Codex session against the real `codex app-server`: the driver's `CodexSessions` over `Processes` (the service's own process adapter, so the
//! environment is the production one: `CODEX_HOME` is `<root>/codex`), two cheap turns of one thread with a close and a resume between them.
//! Opt in: the test is ignored, and `wf mimic exec --clis real:codex-ci` runs it with the second Codex login restored into `CODEX_HOME`, the pinned
//! binary in `MIMIC_REAL_CLI_DIR`, and the refreshed login written back to the vault afterwards. With `RCA_REAL_RESULT_OUT` set a finished run also
//! writes its shape (events seen per turn, never the reply) there.

use rca_cli_driver::application::codex_sessions::CodexSessions;
use rca_cli_driver::application::ports::SessionDriver;
use rca_cli_driver::domain::{ExecEvent, TurnRequest, Verdict};
use remote_chat_agent_host::Processes;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

const WORKSPACE: &str = "real-codex-proof";
const THREAD: &str = "proof-thread";
const TURN_LIMIT: Duration = Duration::from_secs(150);
const FIRST: &str = "Reply with the single word ok. Use no tool.";
const SECOND: &str = "Which single word did you reply with before? Answer with that word only. Use no tool.";

struct Turn {
    kinds: Vec<&'static str>,
    session: Option<String>,
    text: String,
    failed: Option<String>,
}

fn kind(event: &ExecEvent) -> &'static str {
    match event {
        ExecEvent::Session(_) => "session",
        ExecEvent::Text(_) => "text",
        ExecEvent::Reasoning(_) => "reasoning",
        ExecEvent::ToolStarted { .. } | ExecEvent::ToolFinished { .. } => "tool",
        ExecEvent::Approval { .. } => "approval",
        ExecEvent::Done => "done",
        ExecEvent::Error(_) => "error",
    }
}

fn request(prompt: &str, resume: Option<&str>, cwd: &Path) -> TurnRequest {
    TurnRequest {
        thread_id: THREAD.into(),
        run_id: "proof".into(),
        instance_id: "codex".into(),
        model: std::env::var("WF_CODEX_CI_MODEL").ok().filter(|m| !m.trim().is_empty()),
        cwd: cwd.display().to_string(),
        prompt: prompt.into(),
        runtime_mode: "approval-required".into(),
        interaction_mode: "default".into(),
        resume: resume.map(str::to_string),
    }
}

/// The turn's events to its end, an approval it asks for declined so it never waits for nobody.
async fn turn(sessions: &CodexSessions, linker: &Processes, req: &TurnRequest) -> Result<Turn, String> {
    let mut events = sessions.turn(WORKSPACE, linker, req).await?;
    let mut seen = Turn { kinds: Vec::new(), session: None, text: String::new(), failed: None };
    let run = async {
        while let Some(event) = events.recv().await {
            seen.kinds.push(kind(&event));
            match event {
                ExecEvent::Session(id) => seen.session = Some(id),
                ExecEvent::Text(text) => seen.text.push_str(&text),
                ExecEvent::Approval { id, .. } => {
                    sessions.answer(THREAD, &id, Verdict::Decline);
                }
                ExecEvent::Error(why) => {
                    seen.failed = Some(why);
                    break;
                }
                ExecEvent::Done => break,
                _ => {}
            }
        }
    };
    tokio::time::timeout(TURN_LIMIT, run).await.map_err(|_| format!("the turn did not end within {} seconds", TURN_LIMIT.as_secs()))?;
    Ok(seen)
}

/// Both turns, or why the proof failed: nothing here panics, so the caller closes the session whatever happened.
async fn scenario(sessions: &CodexSessions, linker: &Processes, cwd: &Path) -> Result<serde_json::Value, String> {
    // Given a workspace with no server: the first turn starts it, starts a thread and announces its id
    let first = turn(sessions, linker, &request(FIRST, None, cwd)).await?;
    if let Some(why) = &first.failed {
        return Err(format!("the first turn ended in an error: {why}"));
    }
    let id = first.session.clone().ok_or("the first turn announced no session id")?;
    if first.kinds.last() != Some(&"done") || !first.text.to_lowercase().contains("ok") {
        return Err(format!("the first turn did not answer ok and finish: {:?}", first.kinds));
    }
    // When the session is closed (the server and everything it started end) and the same thread runs again with its id
    if !sessions.close(WORKSPACE) {
        return Err("close found no server to end".into());
    }
    let second = turn(sessions, linker, &request(SECOND, Some(&id), cwd)).await?;
    if let Some(why) = &second.failed {
        return Err(format!("the resumed turn ended in an error: {why}"));
    }
    // Then the resumed thread kept the conversation: no new session was announced, and the answer is the word of the first turn
    if second.session.is_some() || second.kinds.last() != Some(&"done") || !second.text.to_lowercase().contains("ok") {
        return Err(format!("the resumed turn did not continue the conversation: {:?}", second.kinds));
    }
    Ok(json!({"schema": "real-codex-session/1", "turns": [{"events": first.kinds, "session_announced": true}, {"events": second.kinds, "session_announced": false, "resumed": true}], "closed": true}))
}

fn record(result: &serde_json::Value) {
    if let Some(path) = std::env::var_os("RCA_REAL_RESULT_OUT").filter(|p| !p.is_empty()) {
        std::fs::write(path, serde_json::to_string_pretty(result).unwrap()).expect("the result file is written");
    }
}

/// The providers' root the login sits under (`<root>/codex`), and the pinned binary's directory put before the system's `PATH`.
fn rig() -> Result<(PathBuf, Vec<(String, String)>), String> {
    let home = PathBuf::from(std::env::var_os("CODEX_HOME").ok_or("CODEX_HOME is not set")?);
    if !home.join("auth.json").is_file() {
        return Err(format!("{} holds no auth.json", home.display()));
    }
    let root = home.parent().filter(|_| home.file_name().is_some_and(|n| n == "codex")).ok_or("CODEX_HOME is not a `codex` directory of a providers' root")?.to_path_buf();
    let mut vars: Vec<(String, String)> = std::env::vars().collect();
    if let Some(dir) = std::env::var_os("MIMIC_REAL_CLI_DIR").filter(|d| !d.is_empty()) {
        let system = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(std::iter::once(PathBuf::from(dir)).chain(std::env::split_paths(&system))).map_err(|e| e.to_string())?;
        vars.retain(|(name, _)| name != "PATH");
        vars.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    Ok((root, vars))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a real model: run by `wf mimic exec --clis real:codex-ci` with the second Codex login restored"]
async fn given_the_real_codex_app_server_when_two_cheap_turns_run_across_a_close_then_the_session_announces_answers_resumes_and_ends() {
    let (root, vars) = match rig() {
        Ok(rig) => rig,
        Err(why) => {
            eprintln!("skipped: {why} (this test runs under `wf mimic exec --clis real:codex-ci`)");
            return;
        }
    };
    let cwd = tempfile::tempdir().unwrap();
    let sessions = CodexSessions::new(Duration::ZERO);
    let linker = Processes::with_inherited(root.display().to_string(), vars);
    let outcome = scenario(&sessions, &linker, cwd.path()).await;
    sessions.close(WORKSPACE);
    match outcome {
        Ok(result) => record(&result),
        Err(why) => panic!("{why}"),
    }
}
