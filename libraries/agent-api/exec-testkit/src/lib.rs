//! The test kit for what runs the agent CLIs and acts in a workspace (`docs/plans/rca-parity/plan.md`, section 3). It is a dev-dependency of
//! `rca-service`, `rca-exec`, `rca-cli-driver` and the `t3-serve` integration chain, and of nothing that ships (`cargo_deps` checks it).
//!
//! - `ScriptedCli`: one program (`scripted-cli`) installed as `claude`, `codex` and `opencode`. It prints the lines a script gives it (a
//!   recording's stream-json, and the lines of a tool call it makes through the MCP server it was started with), logs its argv, its working
//!   directory and the environment variables the script names, and then exits with the code the script gives or stays until it is killed.
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
//! - `Tap`: a recording WebSocket proxy between a mapper or an executor and the service, which keeps the agent API calls and executor tools
//!   that went through it; `trace` turns windows on it into the method-hit trace of a chain run and the run record `wf test publish` takes.
pub mod cassette;
pub mod cli;
pub mod devserver;
pub mod executor;
pub mod hosted;
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
pub use recordings::Recording;
pub use repo::{git_env, TempRepo};
pub use script::{Invocation, Script, Step, Turn};
pub use service::RcaService;
pub use tap::{Seen, Tap};
pub use trace::{Case, Suite};
