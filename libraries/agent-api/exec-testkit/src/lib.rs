//! The test kit for what runs the agent CLIs and acts in a workspace (`docs/plans/rca-parity/plan.md`, section 3). It is a dev-dependency of
//! `rca-service`, `rca-exec`, `rca-cli-driver` and the `t3-serve` integration chain, and of nothing that ships (`wf check --layout` checks it: a testkit is reachable through dev-dependencies only).
//!
//! - `ScriptedCli`: one program (`scripted-cli`) installed as `claude`, `codex` and `opencode`. It prints the lines a script gives it (a
//!   recording's stream-json, and the lines of a tool call it makes through the MCP server it was started with), logs its argv, its working
//!   directory and the environment variables the script names, and then exits with the code the script gives or stays until it is killed.
//! - `app_server`: what the scripted `codex app-server` plays: the recordings of `transcripts/codex-app-server` (Codex's JSON-RPC conversation, as another client's run of the
//!   real server wrote it) and the runs a test writes with `Run`; `ScriptedCli::app_server_log` is what the server was sent.
//! - `HostedClis`: one program (`fake-hosted`) installed as `gh` and `glab` that replays the output the real CLIs print for an argv (written from
//!   their documented JSON), logs every start with its stdin, and exits with a chosen code, floods its output or hangs, so a test never needs a
//!   real account or token.
//! - `devserver`: the program `fake-devserver`, a workspace's dev server that listens on `$PORT` and answers with the environment it was started
//!   with, so a test sees what the executor gave a preview process.
//! - `RcaExec`: the real `rca-exec` binary (built from `clis/` once per process, or the one `RCA_EXEC_BIN` names) serving a `TempRepo` and attached
//!   to an rca service, so a file the repository holds was written by the executor.
//! - `TempRepo`: a git repository on `main` with a first commit, and git that reads no configuration of the machine.
//! - `recordings`: the stream-json each CLI prints, with where every recording came from (the cassette set `cassettes/exec-recordings`).
//! - `RcaService`: the real `rca-service` binary (built from `backend/remote-chat-agent` once per process, or the one `RCA_SERVICE_BIN` names),
//!   on a free port with a secret and a `PATH` that finds the scripted CLIs.
//! - `OpencodeServer`: the real `opencode serve` of the `PATH` on a free loopback port, with the provider key in its environment only, for the
//!   service's `POST /runs` (what `wf rca ask` drives).
//! - `FakeModel`: the program `fake-model`, an OpenAI-compatible chat-completions server that answers the rca e2e prompts from a script;
//!   `OpencodeServer::start_scripted` points an opencode at it, so the run needs no network and no key.
//! - `Tap`: a recording WebSocket proxy between a mapper or an executor and the service, which keeps the agent API calls and executor tools
//!   that went through it; `trace` turns windows on it into the method-hit trace of a chain run and the run record `wf test publish` takes.
pub mod app_server;
pub mod cassette;
pub mod claude;
pub mod cli;
pub mod devserver;
pub mod executor;
pub mod hosted;
pub mod model;
pub mod opencode;
pub mod opencode_live;
pub mod recordings;
pub mod repo;
pub mod script;
pub mod service;
pub mod tap;
pub mod trace;

pub use cassette::{Cassette, Interaction};
pub use cli::ScriptedCli;
pub use executor::RcaExec;
pub use hosted::{HostedClis, Rule, Start};
pub use model::FakeModel;
pub use opencode::OpencodeServer;
pub use recordings::Recording;
pub use repo::{git_env, TempRepo};
pub use script::{Frame, Invocation, Run, Script, Step, Turn};
pub use service::RcaService;
pub use tap::{Seen, Tap};
pub use trace::{attribute, write_replay_record, Case, Suite};

/// A stub that never ends on its own: it ends when its parent does (the test that spawned it), so a hung stub cannot outlive its test
/// and hold the lock `cargo` runs under, and at the latest after ten minutes.
pub fn hang_until_orphaned() -> ! {
    let parent = std::os::unix::process::parent_id();
    for _ in 0..600 {
        if std::os::unix::process::parent_id() != parent {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    std::process::exit(124)
}

/// Give a traced fixture time to flush completed spans before its process is reaped.
pub fn stop_fixture(child: &mut std::process::Child, signal: &str) {
    if std::env::var_os("WF_PERFORMANCE_FIXTURE").is_some() && child.try_wait().ok().flatten().is_none() {
        let _ = std::process::Command::new("kill").args([signal, &child.id().to_string()]).status();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() { return; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}
