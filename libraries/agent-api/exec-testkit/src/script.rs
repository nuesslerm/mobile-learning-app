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
    /// Calls an executor tool through the MCP server the CLI was started with (`--mcp-config` of Claude Code, `OPENCODE_CONFIG_CONTENT` of
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
    /// a version probe never shifts which turn a real start plays. Without it a `--version` start is an ordinary start.
    #[serde(default)]
    pub version: Option<String>,
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
        Script { turns, version: None }
    }

    pub fn printing_version(mut self, text: &str) -> Script {
        self.version = Some(text.into());
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
