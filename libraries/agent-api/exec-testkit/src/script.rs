//! What the scripted CLI is told to do, as the JSON file the test writes beside the program and the program reads, and what the program
//! records of each time it runs. The program and the tests share these types, so the file one writes is the file the other reads.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One thing the program does, in order, during a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// Prints one line of the CLI's stream-json to stdout.
    Line { text: String },
    Sleep { ms: u64 },
    /// Calls a tool: a file or shell tool the CLI runs itself in its directory, or a tool of the service's MCP server (`--mcp-config` of Claude Code, `OPENCODE_CONFIG_CONTENT` of
    /// opencode), and prints what the driver's CLI prints for a tool call: the call, then its result as the service answered it.
    McpCall { id: String, tool: String, args: Value },
    /// Reads one line of stdin, the code a person pastes into a login, and keeps it for `Credential` steps.
    ReadLine,
    /// Writes `content` to `file` below the directory the environment variable `env` names (a provider's directory), as a login CLI writes its
    /// tokens there. `{paste}` in the content is the line `ReadLine` read. A variable that is not set ends the run with code 5.
    Credential { env: String, file: String, content: String },
}

/// One run of the CLI: what it prints, how it ends, and which environment variables the log keeps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    #[serde(default)]
    pub steps: Vec<Step>,
    /// The exit code, once the steps are done and unless `hang`.
    #[serde(default)]
    pub exit_code: i32,
    /// Stays alive after the steps until it is killed.
    #[serde(default)]
    pub hang: bool,
    /// Written to stderr before the program ends or hangs.
    #[serde(default)]
    pub stderr: String,
    /// The names of the environment variables whose values the invocation log keeps: the only ones it keeps.
    #[serde(default)]
    pub log_env: Vec<String>,
    /// The names of environment variables that name a directory: the log keeps the files below each (relative paths, sorted), so a test sees
    /// what the process could read there. A variable that is not set, or a directory that is not there, is logged with no files.
    #[serde(default)]
    pub list_env: Vec<String>,
}

/// The runs of one program, in the order it is started: the first start plays the first turn, the next the second, and a start after the
/// last plays the last again.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Script {
    pub turns: Vec<Turn>,
    /// What the program prints, and ends with code 0, when it is started with `--version` alone: that start plays no turn and is not logged, so
    /// a version probe never shifts which turn a real start plays. Without it a `--version` start is an ordinary start, except for a program that was never scripted: it prints `<name> 0.0.0-mimic`.
    #[serde(default)]
    pub version: Option<String>,
    /// Plays the turn for a start whose first argument is the key (`update`, `upgrade`), in place of the next turn: such a start is not logged and
    /// does not shift which turn a real start plays.
    #[serde(default)]
    pub commands: std::collections::BTreeMap<String, Turn>,
    /// For a start as `app-server` (Codex's JSON-RPC server on stdio): the answer to each method, `{"result": ...}` or `{"error": {...}}`. The
    /// handshake (`initialize`) is always answered; a method not listed is answered with an error. Authored doubles, not recordings.
    #[serde(default)]
    pub app_server: std::collections::BTreeMap<String, Value>,
    /// For a start as `app-server` that plays a conversation: the frames each start plays, in the order the starts come (the first start plays the
    /// first run, a start after the last plays the last again). A run is a recording's frames, or ones a test authored (`Run`).
    #[serde(default)]
    pub app_server_runs: Vec<Vec<Frame>>,
}

/// One step of a conversation with the scripted app-server, as a recording of one has it (`transcripts/codex-app-server`): what the client sends
/// and what the server says after it. The server keeps to the order: it says nothing until the client has sent what the next `Expect` waits for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Frame {
    /// Waits for the client's next message: a request or notification of this `method`, or, with none, the client's answer to a request of the
    /// server's. A message that is not what is waited for ends the run with code 7 and what was expected on stderr.
    Expect { method: Option<String> },
    /// Prints one frame. A response (an `id`-less `result` or `error`, or one with the `id` the recording had) is given the id of the client's request
    /// that was last expected, so a client that numbers its requests another way is answered.
    Emit { frame: Value },
    Sleep { ms: u64 },
    /// The process ends with this exit code at once, as one that crashes does.
    Exit { code: i32 },
}

/// A conversation a test writes by hand: the frames of one start of the scripted app-server, built a step at a time.
#[derive(Debug, Clone, Default)]
pub struct Run(pub Vec<Frame>);

impl Run {
    pub fn new() -> Run {
        Run::default()
    }

    /// `initialize`, its answer, and `initialized`.
    pub fn handshake(self) -> Run {
        self.expect("initialize").ok(serde_json::json!({"userAgent": "scripted-codex", "codexHome": "/scripted", "platformFamily": "unix", "platformOs": "linux"})).expect("initialized")
    }

    pub fn expect(mut self, method: &str) -> Run {
        self.0.push(Frame::Expect { method: Some(method.into()) });
        self
    }

    /// Waits for the client's answer to a request of the server's.
    pub fn expect_answer(mut self) -> Run {
        self.0.push(Frame::Expect { method: None });
        self
    }

    /// The result of the request last expected.
    pub fn ok(mut self, result: Value) -> Run {
        self.0.push(Frame::Emit { frame: serde_json::json!({"result": result}) });
        self
    }

    pub fn notify(mut self, method: &str, params: Value) -> Run {
        self.0.push(Frame::Emit { frame: serde_json::json!({"method": method, "params": params}) });
        self
    }

    /// A request of the server's, under its own id.
    pub fn ask(mut self, id: u64, method: &str, params: Value) -> Run {
        self.0.push(Frame::Emit { frame: serde_json::json!({"id": id, "method": method, "params": params}) });
        self
    }

    pub fn sleeping(mut self, ms: u64) -> Run {
        self.0.push(Frame::Sleep { ms });
        self
    }

    pub fn exiting(mut self, code: i32) -> Run {
        self.0.push(Frame::Exit { code });
        self
    }

    pub fn then(mut self, frames: Vec<Frame>) -> Run {
        self.0.extend(frames);
        self
    }

    /// `thread/start` answered with a thread of this id.
    pub fn thread_start(self, thread: &str) -> Run {
        self.expect("thread/start").ok(serde_json::json!({"thread": {"id": thread, "status": {"type": "idle"}}}))
    }

    /// `thread/resume` answered with the same thread.
    pub fn thread_resume(self, thread: &str) -> Run {
        self.expect("thread/resume").ok(serde_json::json!({"thread": {"id": thread, "status": {"type": "idle"}}}))
    }

    /// `turn/start` answered with this turn, then what a turn that says `text` and ends sends.
    pub fn turn_saying(self, thread: &str, turn: &str, text: &str) -> Run {
        self.turn_begun(thread, turn)
            .notify("item/started", serde_json::json!({"threadId": thread, "turnId": turn, "item": {"type": "agentMessage", "id": "m1", "text": "", "phase": "final_answer"}}))
            .notify("item/agentMessage/delta", serde_json::json!({"threadId": thread, "turnId": turn, "itemId": "m1", "delta": text}))
            .notify("item/completed", serde_json::json!({"threadId": thread, "turnId": turn, "item": {"type": "agentMessage", "id": "m1", "text": text, "phase": "final_answer"}}))
            .turn_ended(thread, turn, "completed")
    }

    /// `turn/start` answered with this turn, and the turn started.
    pub fn turn_begun(self, thread: &str, turn: &str) -> Run {
        self.expect("turn/start")
            .ok(serde_json::json!({"turn": {"id": turn, "items": [], "itemsView": "notLoaded", "status": "inProgress", "error": null}}))
            .notify("turn/started", serde_json::json!({"threadId": thread, "turn": {"id": turn, "items": [], "status": "inProgress", "error": null}}))
    }

    pub fn turn_ended(self, thread: &str, turn: &str, status: &str) -> Run {
        self.notify("thread/status/changed", serde_json::json!({"threadId": thread, "status": {"type": "idle"}})).notify("turn/completed", serde_json::json!({"threadId": thread, "turn": {"id": turn, "items": [], "status": status, "error": null}}))
    }
}

impl From<Run> for Vec<Frame> {
    fn from(run: Run) -> Vec<Frame> {
        run.0
    }
}

impl Turn {
    /// A run that prints `lines` (a recording's) and ends with code 0.
    pub fn replaying(lines: &[String]) -> Turn {
        Turn { steps: lines.iter().map(|text| Step::Line { text: text.clone() }).collect(), ..Turn::default() }
    }

    pub fn then_calling(mut self, id: &str, tool: &str, args: Value) -> Turn {
        self.steps.push(Step::McpCall { id: id.into(), tool: tool.into(), args });
        self
    }

    pub fn then_line(mut self, text: &str) -> Turn {
        self.steps.push(Step::Line { text: text.into() });
        self
    }

    pub fn then_sleeping(mut self, ms: u64) -> Turn {
        self.steps.push(Step::Sleep { ms });
        self
    }

    pub fn exiting(mut self, code: i32) -> Turn {
        self.exit_code = code;
        self
    }

    pub fn hanging(mut self) -> Turn {
        self.hang = true;
        self
    }

    pub fn complaining(mut self, text: &str) -> Turn {
        self.stderr = text.into();
        self
    }

    pub fn logging_env(mut self, names: &[&str]) -> Turn {
        self.log_env = names.iter().map(|n| n.to_string()).collect();
        self
    }

    pub fn listing_env(mut self, names: &[&str]) -> Turn {
        self.list_env = names.iter().map(|n| n.to_string()).collect();
        self
    }

    pub fn then_reading_a_line(mut self) -> Turn {
        self.steps.push(Step::ReadLine);
        self
    }

    pub fn then_writing_credential(mut self, env: &str, file: &str, content: &str) -> Turn {
        self.steps.push(Step::Credential { env: env.into(), file: file.into(), content: content.into() });
        self
    }
}

impl Script {
    pub fn of(turns: Vec<Turn>) -> Script {
        Script { turns, ..Script::default() }
    }

    pub fn printing_version(mut self, text: &str) -> Script {
        self.version = Some(text.into());
        self
    }

    /// Plays `turn` when the program is started with `command` as its first argument.
    pub fn on_command(mut self, command: &str, turn: Turn) -> Script {
        self.commands.insert(command.into(), turn);
        self
    }

    /// Answers `method` of `app-server` with `answer` (`{"result": ...}` or `{"error": {...}}`).
    pub fn answering_app_server(mut self, method: &str, answer: Value) -> Script {
        self.app_server.insert(method.into(), answer);
        self
    }

    /// Plays these conversations as the successive starts of `app-server` (a recording's frames, or a `Run`).
    pub fn playing_app_server(mut self, runs: Vec<Vec<Frame>>) -> Script {
        self.app_server_runs = runs;
        self
    }

    pub fn turn_for(&self, start: usize) -> Turn {
        self.turns.get(start.min(self.turns.len().saturating_sub(1))).cloned().unwrap_or_default()
    }
}

/// How a start of the program was called.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invocation {
    pub program: String,
    /// The arguments after the program's own name.
    pub argv: Vec<String>,
    pub cwd: String,
    /// The `log_env` names and their values, none for a variable that was not set.
    pub env: BTreeMap<String, Option<String>>,
    /// The `list_env` names and the files below the directory each names.
    #[serde(default)]
    pub listing: BTreeMap<String, Vec<String>>,
}

pub const PROGRAMS: [&str; 3] = ["claude", "codex", "opencode"];

/// The file in the program's directory that holds the script of `program`.
pub fn script_file(program: &str) -> String {
    format!("{program}.script.json")
}

/// The file in the program's directory that holds one line per start of `program`.
pub fn log_file(program: &str) -> String {
    format!("{program}.invocations.jsonl")
}

/// The file in the program's directory that holds one line per message the scripted app-server was sent.
pub fn app_server_log_file(program: &str) -> String {
    format!("{program}.app-server.jsonl")
}

/// The file in the program's directory that holds one line per message a live scripted `claude` was sent (its handshake and mode changes too), and the one
/// that counts the messages of the person it has played a turn for, across its starts.
pub fn claude_log_file(program: &str) -> String {
    format!("{program}.messages.jsonl")
}

pub fn claude_turns_file(program: &str) -> String {
    format!("{program}.messages.turns")
}

/// One message the scripted app-server was sent: which start of it (from 1), its process id, and the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Received {
    pub start: usize,
    pub pid: u32,
    pub message: Value,
}
