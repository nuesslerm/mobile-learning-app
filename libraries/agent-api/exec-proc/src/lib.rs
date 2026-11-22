//! The process actions of the executor contract (`docs/plans/rca-client/protocol.md`): `run_command`, which streams its output as output
//! frames before its result, `cancel`, which stops a running one by id and cannot fail, and `cli_detect`, which says which agent CLIs the
//! executor has and their versions.
use exec_proto::args::{opt_string, opt_uint, string};
use exec_proto::{Action, ExecError, ExecResult, Property};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "testing")]
pub mod testing;

pub const RUN_COMMAND: &str = "run_command";
pub const CANCEL: &str = "cancel";
pub const CLI_DETECT: &str = "cli_detect";
/// A CLI's own login, run at the executor with the executor's own home, so what the CLI writes lands where its turns read it.
pub const LOGIN_START: &str = "login_start";
pub const LOGIN_SEND: &str = "login_send";
pub const LOGIN_STATUS: &str = "login_status";
pub const LOGIN_CANCEL: &str = "login_cancel";
pub const LOGOUT: &str = "logout";
/// An agent CLI's own self-update, run at the executor.
pub const CLI_UPDATE: &str = "cli_update";
/// One request to a `codex app-server` started for it and stopped after the answer, under the executor's own Codex login.
pub const CODEX_APP_SERVER: &str = "codex_app_server_request";
/// A live conversation process of an agent CLI (Codex's app-server, Claude Code's stream-json process, opencode's HTTP server), started once and talked to for as long as it runs: `session_open` streams its
/// output lines and ends when it does, `session_send` writes one line to its input, and cancelling the open action stops it.
pub const SESSION_OPEN: &str = "session_open";
pub const SESSION_SEND: &str = "session_send";
/// One read of the executor's own Claude account (its usage), under the login the CLI keeps in the executor's home.
pub const CLAUDE_ACCOUNT: &str = "claude_account_request";
/// One sample of the processes below the executor: what the phone's resource view shows of the workspace host.
pub const PROC_SAMPLE: &str = "proc_sample";
/// One request to a stdio MCP server the workspace declares in its `.mcp.json`, started for it and stopped after the answer.
pub const MCP_REQUEST: &str = "mcp_request";

/// The most processes one `proc_sample` lists; the rest is counted in `truncated`.
pub const SAMPLE_MAX: usize = 512;
/// The workspace file that declares MCP servers, in the shape Claude Code reads (`{"mcpServers": {name: {command, args, env}}}`).
pub const MCP_CONFIG: &str = ".mcp.json";
/// The requests `mcp_request` sends; the MCP methods an app needs to list tools, call one and read a `ui://` resource.
pub const MCP_METHODS: [&str; 3] = ["tools/list", "tools/call", "resources/read"];
/// How long a server may take to start and answer, by default and at most.
pub const MCP_TIMEOUT_MS: u64 = 20_000;
pub const MCP_TIMEOUT_MAX_MS: u64 = 60_000;
/// The largest answer kept, in bytes of JSON.
pub const MCP_ANSWER_MAX: usize = 1 << 20;
/// The largest `params` sent, in bytes of JSON: it stays under a pipe buffer, so the write to a server that reads nothing cannot block past the deadline.
pub const MCP_PARAMS_MAX: usize = 32 * 1024;
/// The protocol version the executor says it speaks when it initializes a server.
pub const MCP_PROTOCOL: &str = "2025-06-18";

/// The arguments of each agent CLI's self-update. Codex has none of its own (it is updated by the package manager that installed it), so
/// `cli_update` refuses it. Written from the CLIs' documentation, not run against them here.
pub const UPDATE_ARGS: [(&str, &[&str]); 2] = [("claude", &["update"]), ("opencode", &["upgrade"])];
/// The longest an update may run, and the most of its output kept (the tail).
pub const UPDATE_TIMEOUT_MS: u64 = 300_000;
pub const UPDATE_OUTPUT_MAX: usize = 4096;

/// The methods of Codex's app-server the executor sends, by name: `account/read` and `account/rateLimits/read` are the ones Orca reads an account
/// and its limits with; the spend of a reset credit is the one the plan names for it. The last name is not checked against a pinned Codex binary
/// (none is in the sandbox): the scripted app-server of the flow tests is the only thing that answers it so far.
pub const CODEX_METHODS: [&str; 3] = ["account/read", "account/rateLimits/read", "account/rateLimitResetCredit/consume"];
/// How long the app-server may take for its handshake and the one answer, by default and at most.
pub const APP_SERVER_TIMEOUT_MS: u64 = 20_000;
pub const APP_SERVER_TIMEOUT_MAX_MS: u64 = 60_000;
/// Where the Claude account's usage is read, and the longest the read may take.
pub const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const CLAUDE_REQUESTS: [&str; 1] = ["usage"];
/// The file, relative to the executor's home, that holds the Claude login's token. The token never leaves the executor.
pub const CLAUDE_CREDENTIALS: &str = ".claude/.credentials.json";

/// The arguments of each agent CLI's login: a URL and a code on its output, no browser or display at the executor. Written from the CLIs'
/// documentation, not run against them here: the real-CLI checks of the plan verify them for each pinned version.
pub const LOGIN_ARGS: [(&str, &[&str]); 3] = [("claude", &["auth", "login"]), ("codex", &["login", "--device-auth"]), ("opencode", &["auth", "login"])];
/// The arguments of each agent CLI's logout.
pub const LOGOUT_ARGS: [(&str, &[&str]); 3] = [("claude", &["auth", "logout"]), ("codex", &["logout"]), ("opencode", &["auth", "logout"])];
/// The file, relative to the executor's home, whose presence says the CLI holds a login.
pub const CREDENTIAL_FILES: [(&str, &str); 3] = [("claude", ".claude/.credentials.json"), ("codex", ".codex/auth.json"), ("opencode", ".local/share/opencode/auth.json")];
/// The most output one login keeps; older lines are dropped from the front.
pub const LOGIN_OUTPUT_MAX: usize = 64 << 10;
/// The most logins one executor runs at once.
pub const LOGINS_MAX: usize = 8;

/// The programs `cli_detect` looks for: the executor runs only these, by name, never a path or a word the caller makes up.
pub const AGENT_CLIS: [&str; 3] = ["claude", "codex", "opencode"];
/// The tools a host's checks name besides the agent CLIs: git, and the GitHub and GitLab CLIs.
pub const TOOL_CLIS: [&str; 3] = ["git", "gh", "glab"];
/// Every program `cli_detect` looks for.
pub const DETECTABLE: [&str; 6] = ["claude", "codex", "opencode", "git", "gh", "glab"];
/// The most a `--version` may take before the program counts as not answering.
pub const VERSION_TIMEOUT_MS: u64 = 5_000;
/// The longest version text kept.
pub const VERSION_MAX: usize = 200;

pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// The actions that stay open and take a thread of their own at the executor.
pub const LONG: &[&str] = &[SESSION_OPEN];
/// The longest line `session_send` writes, and the most sessions one executor keeps open at once.
pub const SESSION_LINE_MAX: usize = 1 << 20;
pub const SESSIONS_MAX: usize = 64;
/// What an agent CLI is started with by `session_open`, by program: the arguments are the CLI's own. The caller adds only the per-session flags of
/// `SESSION_FLAGS`, each checked.
pub const SESSION_ARGS: [(&str, &[&str]); 3] = [
    ("codex", &["app-server"]),
    ("opencode", &["serve", "--hostname", "127.0.0.1", "--port", "0"]),
    ("claude", &["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--replay-user-messages", "--permission-prompt-tool", "stdio"]),
];
/// The flags a caller may add to a session's CLI, by program, each with one value: Claude Code's conversation (a new id, or one to resume), its permission
/// mode, its model and the tool servers it is told of.
pub const SESSION_FLAGS: [(&str, &[&str]); 3] = [("codex", &[]), ("opencode", &[]), ("claude", &["--session-id", "--resume", "--permission-mode", "--model", "--mcp-config"])];
/// The permission modes `--permission-mode` may name, and the longest value a flag may carry.
pub const PERMISSION_MODES: [&str; 6] = ["default", "acceptEdits", "bypassPermissions", "plan", "dontAsk", "auto"];
pub const SESSION_FLAG_VALUE_MAX: usize = 16 * 1024;

/// The family's actions, in the contract's order.
pub const ACTIONS: &[Action] = &[
    Action {
        name: PROC_SAMPLE,
        about: "Sample the executor's own process and every process below it (never any other): pid, parent, name, state, threads, resident memory and CPU time of each, at most 512, and their totals.",
        properties: &[],
        required: &[],
    },
    Action {
        name: MCP_REQUEST,
        about: "Start a stdio MCP server named in the workspace's .mcp.json, send it one request (tools/list, tools/call or resources/read) and return its result; the server is stopped after it. A server with a url is refused.",
        properties: &[
            Property { name: "server", kind: "string", about: "A key of `mcpServers` in .mcp.json" },
            Property { name: "method", kind: "string", about: "One of tools/list, tools/call, resources/read" },
            Property { name: "params", kind: "object", about: "The request's params (an object, default empty)" },
            Property { name: "timeout_ms", kind: "integer", about: "Give up after this many milliseconds (default 20000, at most 60000)" },
        ],
        required: &["server", "method"],
    },
    Action {
        name: RUN_COMMAND,
        about: "Run a shell command in the workspace and return its output; a non-zero exit code is an error.",
        properties: &[
            Property { name: "command", kind: "string", about: "Shell command" },
            Property { name: "cwd", kind: "string", about: "Directory relative to the workspace root" },
            Property { name: "timeout_ms", kind: "integer", about: "Kill the command after this many milliseconds (default 120000)" },
            Property { name: "env", kind: "object", about: "Extra environment variables" },
        ],
        required: &["command"],
    },
    Action { name: CANCEL, about: "Cancel a running action by its id.", properties: &[Property { name: "id", kind: "string", about: "Id of the running action" }], required: &["id"] },
    Action {
        name: CLI_DETECT,
        about: "Say which of the named CLIs (the agent CLIs claude, codex, opencode, and git, gh, glab) the executor has on its path and what each prints for `--version` (run without a shell, at most 5 s each).",
        properties: &[Property { name: "programs", kind: "array", about: "Program names, each one of claude, codex, opencode, git, gh, glab (1 to 6, no repeats)" }],
        required: &["programs"],
    },
    Action {
        name: LOGIN_START,
        about: "Start an agent CLI's own login (claude, codex or opencode) at the executor, with the executor's home, and return the id of the flow; read what it prints with login_status.",
        properties: &[Property { name: "program", kind: "string", about: "One of claude, codex, opencode" }],
        required: &["program"],
    },
    Action {
        name: LOGIN_SEND,
        about: "Send one line (a pasted code, an answer to a prompt) to a running login's input.",
        properties: &[Property { name: "flow", kind: "string", about: "Id login_start returned" }, Property { name: "text", kind: "string", about: "The line to send, without its newline" }],
        required: &["flow", "text"],
    },
    Action {
        name: LOGIN_STATUS,
        about: "What a login has printed since `after` (a byte offset into its output, 0 for all of it), whether it ended and how, and whether its CLI now holds a login.",
        properties: &[Property { name: "flow", kind: "string", about: "Id login_start returned" }, Property { name: "after", kind: "integer", about: "Output offset to read from (default 0)" }],
        required: &["flow"],
    },
    Action { name: LOGIN_CANCEL, about: "Stop a running login and forget it.", properties: &[Property { name: "flow", kind: "string", about: "Id login_start returned" }], required: &["flow"] },
    Action {
        name: LOGOUT,
        about: "Run an agent CLI's own logout at the executor and say whether it now holds a login.",
        properties: &[Property { name: "program", kind: "string", about: "One of claude, codex, opencode" }],
        required: &["program"],
    },
    Action {
        name: CLI_UPDATE,
        about: "Run an agent CLI's own self-update (claude, opencode) at the executor and return the tail of its output and the version it now reports; codex has none and is refused.",
        properties: &[Property { name: "program", kind: "string", about: "One of claude, opencode" }],
        required: &["program"],
    },
    Action {
        name: SESSION_OPEN,
        about: "Start a live conversation process of an agent CLI (codex: `codex app-server`, claude: `claude -p` over stream-json, opencode: `opencode serve` on a loopback port under a password only the executor holds) in the workspace, with the executor's own login, and stream its stdout lines until it ends; for opencode the lines are the executor's bridge to the server (`SessionHttp`), never the server's own output. Cancelling the action stops the process and everything it started.",
        properties: &[
            Property { name: "session", kind: "string", about: "An id the caller chooses, for session_send to name (1 to 128 characters, not in use)" },
            Property { name: "program", kind: "string", about: "One of the agent CLIs that run as a session: codex, claude, opencode" },
            Property { name: "args", kind: "array", about: "Flags and their values for the CLI, from the program's list: claude takes --session-id or --resume (a UUID), --permission-mode, --model and --mcp-config" },
            Property { name: "cwd", kind: "string", about: "Directory relative to the workspace root" },
        ],
        required: &["session", "program"],
    },
    Action {
        name: SESSION_SEND,
        about: "Write one line (a JSON message, without its newline) to the input of a session that session_open started; for an opencode session the line is an HTTP request for its server, answered on the session's output.",
        properties: &[Property { name: "session", kind: "string", about: "Id session_open was given" }, Property { name: "line", kind: "string", about: "The line to write, without its newline" }],
        required: &["session", "line"],
    },
    Action {
        name: CODEX_APP_SERVER,
        about: "Start `codex app-server` at the executor, send it one request (account/read, account/rateLimits/read or the reset-credit spend) and return its answer; the server is stopped after it.",
        properties: &[
            Property { name: "method", kind: "string", about: "One of account/read, account/rateLimits/read, account/rateLimitResetCredit/consume" },
            Property { name: "params", kind: "object", about: "The request's params (an object, default empty)" },
            Property { name: "timeout_ms", kind: "integer", about: "Give up after this many milliseconds (default 20000, at most 60000)" },
        ],
        required: &["method"],
    },
    Action {
        name: CLAUDE_ACCOUNT,
        about: "Read the executor's own Claude account: `usage` asks the account's usage windows under the login in the executor's home and returns the HTTP status and the body; the token is never returned.",
        properties: &[Property { name: "request", kind: "string", about: "`usage`" }],
        required: &["request"],
    },
];

#[derive(Debug, Clone, PartialEq)]
pub struct RunCommand {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout_ms: u64,
    pub env: Vec<(String, String)>,
}

impl RunCommand {
    pub fn parse(args: &Value) -> ExecResult<RunCommand> {
        let env = match &args["env"] {
            Value::Null => Vec::new(),
            Value::Object(map) => map.iter().map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())).ok_or_else(|| ExecError::invalid_args("`env` values must be strings"))).collect::<ExecResult<Vec<_>>>()?,
            _ => return Err(ExecError::invalid_args("`env` must be an object")),
        };
        Ok(RunCommand { command: string(args, "command")?, cwd: opt_string(args, "cwd")?, timeout_ms: opt_uint(args, "timeout_ms")?.unwrap_or(DEFAULT_TIMEOUT_MS), env })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    /// A process killed by a signal reports `128 + signal`.
    pub exit_code: i32,
    pub timed_out: bool,
    pub cancelled: bool,
    pub truncated: bool,
}

/// The id of the running action to stop; none when the request named none, which cancels nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Cancel {
    pub id: Option<String>,
}

impl Cancel {
    pub fn parse(args: &Value) -> Cancel {
        Cancel { id: args["id"].as_str().map(str::to_string) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelResult {
    pub cancelled: bool,
}

/// `cli_detect`: the programs to look for, in the order the answer lists them.
#[derive(Debug, Clone, PartialEq)]
pub struct CliDetect {
    pub programs: Vec<String>,
}

impl CliDetect {
    pub fn parse(args: &Value) -> ExecResult<CliDetect> {
        let list = args["programs"].as_array().ok_or_else(|| ExecError::invalid_args("`programs` must be an array of program names"))?;
        if list.is_empty() || list.len() > DETECTABLE.len() {
            return Err(ExecError::invalid_args(format!("`programs` names 1 to {} programs", DETECTABLE.len())));
        }
        let mut programs: Vec<String> = Vec::new();
        for item in list {
            let name = item.as_str().ok_or_else(|| ExecError::invalid_args("`programs` must be an array of program names"))?;
            if !DETECTABLE.contains(&name) {
                return Err(ExecError::invalid_args(format!("`programs` names only {}, not {name:?}", DETECTABLE.join(", "))));
            }
            if programs.iter().any(|p| p == name) {
                return Err(ExecError::invalid_args(format!("`programs` names {name} twice")));
            }
            programs.push(name.to_string());
        }
        Ok(CliDetect { programs })
    }
}

/// What the executor found for one program. `path` is where it lies on the executor's path; `version` the first line of its `--version`
/// (at most `VERSION_MAX` bytes). A program that is found but does not answer has `error` (`timed out`, `exited with <code>`, or the
/// reason it did not start) and no version; a program that is not found has only `installed: false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliFound {
    pub program: String,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// For an agent CLI that is installed: whether its credential file is in the executor's home (`CREDENTIAL_FILES`); none for the tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_in: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliDetectResult {
    pub clis: Vec<CliFound>,
}

fn agent_program(args: &Value) -> ExecResult<String> {
    let program = string(args, "program")?;
    if !AGENT_CLIS.contains(&program.as_str()) {
        return Err(ExecError::invalid_args(format!("`program` is one of {}, not {program:?}", AGENT_CLIS.join(", "))));
    }
    Ok(program)
}

/// `login_start` and `logout`: the agent CLI to run the flow of.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginProgram {
    pub program: String,
}

impl LoginProgram {
    pub fn parse(args: &Value) -> ExecResult<LoginProgram> {
        Ok(LoginProgram { program: agent_program(args)? })
    }

    /// The arguments of its login, and of its logout.
    pub fn login_args(&self) -> &'static [&'static str] {
        LOGIN_ARGS.iter().find(|(p, _)| *p == self.program).map_or(&[], |(_, a)| a)
    }

    pub fn logout_args(&self) -> &'static [&'static str] {
        LOGOUT_ARGS.iter().find(|(p, _)| *p == self.program).map_or(&[], |(_, a)| a)
    }

    pub fn credential_file(&self) -> &'static str {
        CREDENTIAL_FILES.iter().find(|(p, _)| *p == self.program).map_or("", |(_, f)| f)
    }
}

/// `login_send`.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginSend {
    pub flow: String,
    pub text: String,
}

impl LoginSend {
    pub fn parse(args: &Value) -> ExecResult<LoginSend> {
        let text = string(args, "text")?;
        if text.len() > 4096 || text.contains('\n') {
            return Err(ExecError::invalid_args("`text` is one line of at most 4096 bytes"));
        }
        Ok(LoginSend { flow: string(args, "flow")?, text })
    }
}

/// `login_status`.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginStatus {
    pub flow: String,
    pub after: u64,
}

impl LoginStatus {
    pub fn parse(args: &Value) -> ExecResult<LoginStatus> {
        Ok(LoginStatus { flow: string(args, "flow")?, after: opt_uint(args, "after")?.unwrap_or(0) })
    }
}

/// `login_cancel`.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginCancel {
    pub flow: String,
}

impl LoginCancel {
    pub fn parse(args: &Value) -> ExecResult<LoginCancel> {
        Ok(LoginCancel { flow: string(args, "flow")? })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginStarted {
    pub flow: String,
}

/// What a login has done: `output` is what it printed from `after`, `next` the offset to ask from next. A login that has ended says whether it
/// succeeded (`ok`, its exit code was 0) and, when it did not, `error` is the tail of its output; `signed_in` is whether the CLI's credential
/// file is there now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginState {
    pub output: String,
    pub next: u64,
    pub ended: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub signed_in: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginCancelled {
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoggedOut {
    pub signed_in: bool,
}

/// `cli_update`: the CLI to update (the same names as a login's).
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateProgram {
    pub program: String,
}

impl UpdateProgram {
    pub fn parse(args: &Value) -> ExecResult<UpdateProgram> {
        let program = agent_program(args)?;
        if !UPDATE_ARGS.iter().any(|(p, _)| *p == program) {
            return Err(ExecError::invalid_args(format!("{program} has no update command of its own; update it with the package manager that installed it")));
        }
        Ok(UpdateProgram { program })
    }

    pub fn args(&self) -> &'static [&'static str] {
        UPDATE_ARGS.iter().find(|(p, _)| *p == self.program).map_or(&[], |(_, a)| a)
    }
}

/// What an update did: whether the CLI exited 0, its exit code, the tail of its output and the first line of its `--version` afterwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliUpdated {
    pub program: String,
    pub ok: bool,
    pub exit_code: i32,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// `session_open`: the id the caller names the session by, the agent CLI to run and where.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionOpen {
    pub session: String,
    pub program: String,
    /// The per-session flags and their values, as `SESSION_FLAGS` allows them.
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

impl SessionOpen {
    pub fn parse(args: &Value) -> ExecResult<SessionOpen> {
        let session = string(args, "session")?;
        if session.is_empty() || session.len() > 128 {
            return Err(ExecError::invalid_args("`session` must hold 1 to 128 characters"));
        }
        let program = string(args, "program")?;
        if !SESSION_ARGS.iter().any(|(p, _)| *p == program) {
            let known: Vec<&str> = SESSION_ARGS.iter().map(|(p, _)| *p).collect();
            return Err(ExecError::invalid_args(format!("`program` is one of {}, not {program:?}", known.join(", "))));
        }
        let given = match &args["args"] {
            Value::Null => Vec::new(),
            Value::Array(items) => items.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| ExecError::invalid_args("`args` holds strings only"))).collect::<ExecResult<Vec<_>>>()?,
            _ => return Err(ExecError::invalid_args("`args` must be an array of strings")),
        };
        check_session_flags(&program, &given)?;
        Ok(SessionOpen { session, program, args: given, cwd: opt_string(args, "cwd")? })
    }

    /// The arguments the CLI is started with: its own, then the caller's flags.
    pub fn argv(&self) -> Vec<String> {
        let own = SESSION_ARGS.iter().find(|(p, _)| *p == self.program).map_or(&[][..], |(_, a)| a);
        own.iter().map(|a| a.to_string()).chain(self.args.iter().cloned()).collect()
    }
}

fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5 && [8, 4, 4, 4, 12].iter().zip(&parts).all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A session's flags are pairs of one flag of the program's list and its value, each flag once; a conversation is named once, by a new id or by one to resume.
fn check_session_flags(program: &str, given: &[String]) -> ExecResult<()> {
    let allowed = SESSION_FLAGS.iter().find(|(p, _)| *p == program).map_or(&[][..], |(_, f)| f);
    if !given.len().is_multiple_of(2) {
        return Err(ExecError::invalid_args("`args` is flag and value pairs"));
    }
    let mut seen: Vec<&str> = Vec::new();
    for pair in given.chunks(2) {
        let (flag, value) = (pair[0].as_str(), pair[1].as_str());
        if !allowed.contains(&flag) {
            return Err(ExecError::invalid_args(format!("{program} takes no flag {flag:?} in a session (it takes {})", if allowed.is_empty() { "none".to_string() } else { allowed.join(", ") })));
        }
        if seen.contains(&flag) {
            return Err(ExecError::invalid_args(format!("{flag} is given twice")));
        }
        seen.push(flag);
        let fine = match flag {
            "--session-id" | "--resume" => is_uuid(value),
            "--permission-mode" => PERMISSION_MODES.contains(&value),
            "--model" => !value.is_empty() && value.len() <= 128 && value.chars().all(|c| c.is_ascii_alphanumeric() || "-_.:/[]".contains(c)),
            _ => value.len() <= SESSION_FLAG_VALUE_MAX && serde_json::from_str::<Value>(value).is_ok_and(|v| v.is_object()),
        };
        if !fine {
            return Err(ExecError::invalid_args(format!("the value of {flag} is not acceptable")));
        }
    }
    if seen.contains(&"--session-id") && seen.contains(&"--resume") {
        return Err(ExecError::invalid_args("--session-id and --resume name the conversation twice"));
    }
    Ok(())
}

/// How a session ended: the exit code of its process (`128 + signal` when a signal ended it) and whether cancelling the action ended it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEnded {
    pub exit_code: i32,
    pub cancelled: bool,
}

/// `session_send`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSend {
    pub session: String,
    pub line: String,
}

impl SessionSend {
    pub fn parse(args: &Value) -> ExecResult<SessionSend> {
        let line = string(args, "line")?;
        if line.len() > SESSION_LINE_MAX || line.contains('\n') {
            return Err(ExecError::invalid_args(format!("`line` is one line of at most {SESSION_LINE_MAX} bytes")));
        }
        Ok(SessionSend { session: string(args, "session")?, line })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSent {
    pub written: usize,
}

/// The routes of an opencode server a session's request may name (the prefix of the path, then nothing, `/` or `?`): what the adapter drives, and
/// nothing that writes configuration to the workspace (`/config` is read only here).
pub const SESSION_HTTP_ROUTES: [&str; 6] = ["/session", "/permission", "/question", "/global/health", "/config", "/agent"];
const SESSION_HTTP_METHODS: [&str; 4] = ["GET", "POST", "PATCH", "DELETE"];

/// What `session_send` carries to an opencode session, as one line: `{"id": 7, "method": "POST", "path": "/session?directory=%2Fw", "body": {...}}`.
/// The executor makes the request to its own server, adding the credential only it holds, and answers on the session's output with
/// `{"id": 7, "status": 200, "body": "<text>"}` (`status` 0 and `error` when the request could not be made). The same output carries
/// `{"ready": true}` once the server is up and its event stream is open, then `{"event": <frame of /global/event>}` for every frame of it (the heartbeat
/// left out), and `{"closed": "<why>"}` when that stream ends.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionHttp {
    pub id: u64,
    pub method: String,
    pub path: String,
    pub body: Option<Value>,
}

impl SessionHttp {
    pub fn parse(line: &str) -> ExecResult<SessionHttp> {
        let v: Value = serde_json::from_str(line).map_err(|e| ExecError::invalid_args(format!("the line is no JSON request: {e}")))?;
        let id = v["id"].as_u64().ok_or_else(|| ExecError::invalid_args("`id` must be a non-negative integer"))?;
        let method = string(&v, "method")?;
        if !SESSION_HTTP_METHODS.contains(&method.as_str()) {
            return Err(ExecError::invalid_args(format!("`method` is one of {}", SESSION_HTTP_METHODS.join(", "))));
        }
        let path = string(&v, "path")?;
        let (route, query) = path.split_once('?').unwrap_or((path.as_str(), ""));
        let routed = SESSION_HTTP_ROUTES.iter().any(|r| route == *r || route.strip_prefix(r).is_some_and(|rest| rest.starts_with('/')));
        let plain = |c: char| c.is_ascii_graphic() && c != '#' && c != '\\';
        if !routed || !path.chars().all(plain) || route.contains('%') || route.split('/').any(|segment| segment == ".." || segment == ".") || query.contains("..%2") {
            return Err(ExecError::invalid_args(format!("`path` is no route of the server a session may use: {path:?}")));
        }
        if (route == "/config" || route.starts_with("/config/")) && method != "GET" {
            return Err(ExecError::invalid_args("`/config` is read only here"));
        }
        let body = match &v["body"] {
            Value::Null => None,
            other => Some(other.clone()),
        };
        Ok(SessionHttp { id, method, path, body })
    }
}

/// `codex_app_server_request`.
#[derive(Debug, Clone, PartialEq)]
pub struct CodexRequest {
    pub method: String,
    pub params: Value,
    pub timeout_ms: u64,
}

impl CodexRequest {
    pub fn parse(args: &Value) -> ExecResult<CodexRequest> {
        let method = string(args, "method")?;
        if !CODEX_METHODS.contains(&method.as_str()) {
            return Err(ExecError::invalid_args(format!("`method` is one of {}, not {method:?}", CODEX_METHODS.join(", "))));
        }
        let params = match &args["params"] {
            Value::Null => Value::Object(Default::default()),
            p @ Value::Object(_) => p.clone(),
            _ => return Err(ExecError::invalid_args("`params` must be an object")),
        };
        let timeout_ms = opt_uint(args, "timeout_ms")?.unwrap_or(APP_SERVER_TIMEOUT_MS).clamp(1, APP_SERVER_TIMEOUT_MAX_MS);
        Ok(CodexRequest { method, params, timeout_ms })
    }
}

/// The app-server's answer to the request: its `result` as it sent it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexAnswer {
    pub result: Value,
}

/// `claude_account_request`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeRequest {
    pub request: String,
}

impl ClaudeRequest {
    pub fn parse(args: &Value) -> ExecResult<ClaudeRequest> {
        let request = string(args, "request")?;
        if !CLAUDE_REQUESTS.contains(&request.as_str()) {
            return Err(ExecError::invalid_args(format!("`request` is one of {}, not {request:?}", CLAUDE_REQUESTS.join(", "))));
        }
        Ok(ClaudeRequest { request })
    }
}

/// The Claude account service's answer: its HTTP status and its body (JSON when it is, else the text as a string).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeAnswer {
    pub status: u16,
    pub body: Value,
}

/// One process of a `proc_sample`. `cpu_ticks` is user plus system time in clock ticks (`ticks_per_second` of the sample).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub state: String,
    pub threads: u32,
    pub rss_kib: u64,
    pub cpu_ticks: u64,
}

/// `proc_sample`'s answer: the processes by pid, the sums over all of them (also over the ones cut), and the clock the ticks count in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcSample {
    pub sampled_at_ms: u64,
    pub ticks_per_second: u64,
    pub processes: Vec<ProcInfo>,
    pub count: u64,
    pub total_rss_kib: u64,
    pub total_cpu_ticks: u64,
    pub truncated: bool,
}

/// `mcp_request`.
#[derive(Debug, Clone, PartialEq)]
pub struct McpRequest {
    pub server: String,
    pub method: String,
    pub params: Value,
    pub timeout_ms: u64,
}

impl McpRequest {
    pub fn parse(args: &Value) -> ExecResult<McpRequest> {
        let server = string(args, "server")?;
        if server.is_empty() || server.len() > 128 {
            return Err(ExecError::invalid_args("`server` must hold 1 to 128 characters"));
        }
        let method = string(args, "method")?;
        if !MCP_METHODS.contains(&method.as_str()) {
            return Err(ExecError::invalid_args(format!("`method` is one of {}, not {method:?}", MCP_METHODS.join(", "))));
        }
        let params = match &args["params"] {
            Value::Null => Value::Object(Default::default()),
            p @ Value::Object(_) => p.clone(),
            _ => return Err(ExecError::invalid_args("`params` must be an object")),
        };
        if params.to_string().len() > MCP_PARAMS_MAX {
            return Err(ExecError::invalid_args(format!("`params` is longer than {MCP_PARAMS_MAX} bytes")));
        }
        let timeout_ms = opt_uint(args, "timeout_ms")?.unwrap_or(MCP_TIMEOUT_MS).clamp(1, MCP_TIMEOUT_MAX_MS);
        Ok(McpRequest { server, method, params, timeout_ms })
    }
}

/// The server's answer to the request: its `result` as it sent it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpAnswer {
    pub result: Value,
}

/// The first non-empty line of a program's `--version` output, cut to `VERSION_MAX` bytes at a character boundary; none when there is none.
pub fn version_line(output: &str) -> Option<String> {
    let line = output.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut end = line.len().min(VERSION_MAX);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    Some(line[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_proto::{to_value, Code};
    use serde_json::json;

    #[test]
    fn given_the_arguments_of_an_mcp_request_when_read_then_a_known_method_and_a_server_are_required_and_the_timeout_is_clamped() {
        // Given a minimal request, one with a method the executor does not send, one without a server, one with params that are no object and one with a long timeout
        let minimal = McpRequest::parse(&json!({"server": "app", "method": "tools/list"})).unwrap();
        let unknown = McpRequest::parse(&json!({"server": "app", "method": "prompts/list"}));
        let no_server = McpRequest::parse(&json!({"method": "tools/list"}));
        let bad_params = McpRequest::parse(&json!({"server": "app", "method": "tools/call", "params": [1]}));
        let long = McpRequest::parse(&json!({"server": "app", "method": "tools/call", "timeout_ms": 10_000_000})).unwrap();
        let huge = McpRequest::parse(&json!({"server": "app", "method": "tools/call", "params": {"x": "a".repeat(MCP_PARAMS_MAX)}}));
        // Then the minimal one has empty params and the default timeout, the others are INVALID_ARGS, and the timeout is cut to the maximum
        assert_eq!((minimal.params, minimal.timeout_ms), (json!({}), MCP_TIMEOUT_MS));
        assert!([unknown, no_server, bad_params, huge].into_iter().all(|r| r.unwrap_err().code == exec_proto::Code::InvalidArgs));
        assert_eq!(long.timeout_ms, MCP_TIMEOUT_MAX_MS);
    }

    #[test]
    fn given_a_session_open_request_when_parsed_then_the_cli_keeps_its_own_arguments_and_only_the_listed_flags_with_good_values_are_added() {
        // Given a claude session with a new conversation, a mode, a model and a tool server
        let id = "6b0b2f0e-6d0e-4a0a-9a52-8d54b4b9a4de";
        let open = |program: &str, args: Value| SessionOpen::parse(&json!({"session": "s", "program": program, "args": args}));
        let ok = open("claude", json!(["--session-id", id, "--permission-mode", "plan", "--model", "claude-sonnet-5-5", "--mcp-config", "{\"mcpServers\":{}}"])).unwrap();
        // When its arguments are built
        let argv = ok.argv();
        // Then the CLI's own come first (stream-json both ways, the stdio permission prompt), and the caller's follow in order
        assert_eq!(&argv[..3], ["-p", "--input-format", "stream-json"]);
        assert!(argv.windows(2).any(|w| w == ["--permission-prompt-tool", "stdio"]));
        assert_eq!(&argv[argv.len() - 8..argv.len() - 6], ["--session-id", id]);
        // And a flag off the list, a bad value, a repeat, an odd count, both ways to name a conversation and flags for codex are INVALID_ARGS
        let refused = [
            open("claude", json!(["--dangerously-skip-permissions", "x"])),
            open("claude", json!(["--permission-mode", "yolo"])),
            open("claude", json!(["--resume", "not-a-uuid"])),
            open("claude", json!(["--model", "a b"])),
            open("claude", json!(["--mcp-config", "[1]"])),
            open("claude", json!(["--model", "a", "--model", "b"])),
            open("claude", json!(["--model"])),
            open("claude", json!(["--session-id", id, "--resume", id])),
            open("claude", json!("--model")),
            open("codex", json!(["--model", "x"])),
        ];
        assert!(refused.iter().all(|r| r.as_ref().is_err_and(|e| e.code == Code::InvalidArgs)), "{refused:?}");
        assert_eq!(open("codex", json!([])).unwrap().argv(), ["app-server"]);
    }

    #[test]
    fn given_a_run_command_request_when_parsed_then_the_timeout_defaults_and_the_environment_keeps_its_order() {
        // Given a command with two variables and one with only a command
        let full = RunCommand::parse(&json!({"command": "ls", "cwd": "src", "timeout_ms": 5, "env": {"B": "2", "A": "1"}})).unwrap();
        let bare = RunCommand::parse(&json!({"command": "ls"})).unwrap();
        // Then the variables come in the order given, and the bare one has the default timeout and no environment
        assert_eq!((full.env, full.cwd, full.timeout_ms), (vec![("B".to_string(), "2".to_string()), ("A".to_string(), "1".to_string())], Some("src".to_string()), 5));
        assert_eq!((bare.timeout_ms, bare.env.is_empty(), bare.cwd), (DEFAULT_TIMEOUT_MS, true, None));
    }

    #[test]
    fn given_a_malformed_environment_when_parsed_then_it_is_refused_before_the_command_is_checked() {
        // Given requests with no command and an environment that is a string, and one whose value is a number
        let not_object = RunCommand::parse(&json!({"env": "A=1"})).unwrap_err();
        let not_text = RunCommand::parse(&json!({"env": {"A": 1}})).unwrap_err();
        // Then each is INVALID_ARGS naming the environment, not the missing command
        assert!(not_object.code == Code::InvalidArgs && not_object.message.contains("`env` must be an object"), "{not_object:?}");
        assert!(not_text.message.contains("`env` values must be strings"), "{not_text:?}");
        assert!(RunCommand::parse(&json!({})).unwrap_err().message.contains("`command`"));
    }

    #[test]
    fn given_a_cancel_request_when_parsed_then_it_never_fails_and_a_missing_id_names_nothing() {
        // Given a cancel with an id, one with none, and one whose id is a number
        let named = Cancel::parse(&json!({"id": "job-1"}));
        let none = Cancel::parse(&json!({}));
        let number = Cancel::parse(&json!({"id": 4}));
        // Then only the string id names a running action
        assert_eq!((named.id, none.id, number.id), (Some("job-1".to_string()), None, None));
    }

    #[test]
    fn given_the_results_when_encoded_then_the_fields_keep_the_contract_names_and_order() {
        // Given a run result and a cancel result
        // When they are encoded, Then the keys are the contract's, in order
        assert_eq!(to_value(&RunResult { exit_code: 137, timed_out: false, cancelled: true, truncated: false }).to_string(), r#"{"exit_code":137,"timed_out":false,"cancelled":true,"truncated":false}"#);
        assert_eq!(to_value(&CancelResult { cancelled: false }).to_string(), r#"{"cancelled":false}"#);
    }

    #[test]
    fn given_a_cli_detect_request_when_parsed_then_only_the_named_agent_clis_are_read_once_each_in_order() {
        // Given a request for two CLIs, one for a word that is no agent CLI, a path, an option, a repeat, an empty list and no list
        let ok = CliDetect::parse(&json!({"programs": ["opencode", "claude"]})).unwrap();
        assert_eq!(CliDetect::parse(&json!({"programs": ["git", "gh", "glab"]})).unwrap().programs, ["git", "gh", "glab"]);
        let refused: Vec<_> = [json!({"programs": ["rm"]}), json!({"programs": ["/usr/bin/claude"]}), json!({"programs": ["--help"]}), json!({"programs": ["claude", "claude"]}), json!({"programs": []}), json!({"programs": [1]}), json!({}), json!({"programs": ["claude", "codex", "opencode", "git", "gh", "glab", "claude"]})].iter().map(|a| CliDetect::parse(a).unwrap_err().code).collect();
        // Then the first keeps its order, and every other is INVALID_ARGS
        assert_eq!(ok.programs, ["opencode", "claude"]);
        assert!(refused.iter().all(|c| *c == Code::InvalidArgs), "{refused:?}");
    }

    #[test]
    fn given_version_output_when_read_then_the_first_line_with_text_is_kept_and_cut_at_a_character_boundary() {
        // Given output with blank lines first, one that is only blank, and a long line of two-byte characters
        let long = "é".repeat(VERSION_MAX);
        // When the version line is read
        // Then the first line with text is trimmed, blank output has none, and the cut never splits a character
        assert_eq!(version_line("\n  \n 2.1.7 (Claude Code) \nmore"), Some("2.1.7 (Claude Code)".to_string()));
        assert_eq!(version_line(" \n\t\n"), None);
        assert_eq!(version_line(&long).map(|v| v.len()), Some(VERSION_MAX));
    }

    #[test]
    fn given_a_found_and_a_missing_cli_when_encoded_then_a_missing_one_carries_only_its_name_and_installed_false() {
        // Given a program that answered, one that did not, and one that is not there
        let found = to_value(&CliFound { program: "claude".into(), installed: true, path: Some("/bin/claude".into()), version: Some("2.1.7".into()), error: None, signed_in: None }).to_string();
        let silent = to_value(&CliFound { program: "codex".into(), installed: true, path: Some("/bin/codex".into()), version: None, error: Some("timed out".into()), signed_in: None }).to_string();
        let missing = to_value(&CliFound { program: "opencode".into(), installed: false, path: None, version: None, error: None, signed_in: None }).to_string();
        // Then the keys are the contract's, in order
        assert_eq!(found, r#"{"program":"claude","installed":true,"path":"/bin/claude","version":"2.1.7"}"#);
        assert_eq!(silent, r#"{"program":"codex","installed":true,"path":"/bin/codex","error":"timed out"}"#);
        assert_eq!(missing, r#"{"program":"opencode","installed":false}"#);
    }

    #[test]
    fn given_provider_account_requests_when_parsed_then_only_the_listed_programs_methods_and_reads_are_accepted() {
        // Given an update for claude and for codex, requests of the app-server with a method of the list, one off it and params that are no object
        let claude = UpdateProgram::parse(&json!({"program": "claude"})).unwrap();
        let codex = UpdateProgram::parse(&json!({"program": "codex"})).unwrap_err();
        let read = CodexRequest::parse(&json!({"method": "account/rateLimits/read", "timeout_ms": 999_999})).unwrap();
        let off = CodexRequest::parse(&json!({"method": "thread/start"})).unwrap_err();
        let bad = CodexRequest::parse(&json!({"method": "account/read", "params": []})).unwrap_err();
        // Then claude has its own update arguments, codex is refused, the timeout is held to its most and the params default to an object
        assert_eq!(claude.args(), ["update"]);
        assert!(codex.code == Code::InvalidArgs && codex.message.contains("no update command"), "{codex:?}");
        assert_eq!((read.timeout_ms, read.params), (APP_SERVER_TIMEOUT_MAX_MS, json!({})));
        assert!(off.message.contains("account/read") && bad.message.contains("`params` must be an object"));
        assert_eq!(ClaudeRequest::parse(&json!({"request": "usage"})).unwrap().request, "usage");
        assert_eq!(ClaudeRequest::parse(&json!({"request": "token"})).unwrap_err().code, Code::InvalidArgs);
    }

    #[test]
    fn given_session_requests_when_parsed_then_only_a_named_session_of_a_listed_program_opens_and_a_line_is_one_line() {
        // Given an open of codex, one of a program with no session mode, one with no id, one with an id that is too long, and lines with and without a newline
        let open = SessionOpen::parse(&json!({"session": "s1", "program": "codex", "cwd": "sub"})).unwrap();
        let refused: Vec<_> = [json!({"session": "s1", "program": "gemini"}), json!({"program": "codex"}), json!({"session": "", "program": "codex"}), json!({"session": "x".repeat(129), "program": "codex"})].iter().map(|a| SessionOpen::parse(a).unwrap_err().code).collect();
        let sent = SessionSend::parse(&json!({"session": "s1", "line": "{\"id\":1}"})).unwrap();
        let two_lines = SessionSend::parse(&json!({"session": "s1", "line": "a\nb"})).unwrap_err();
        // Then the open starts the CLI with its own arguments, the others are INVALID_ARGS, and a line with a newline is refused
        assert_eq!((open.argv(), open.cwd.as_deref()), (vec!["app-server".to_string()], Some("sub")));
        assert!(refused.iter().all(|c| *c == Code::InvalidArgs), "{refused:?}");
        assert_eq!(sent.line, "{\"id\":1}");
        assert!(two_lines.code == Code::InvalidArgs && two_lines.message.contains("one line"));
        assert_eq!(LONG, [SESSION_OPEN]);
    }

    #[test]
    fn given_a_line_for_an_opencode_session_when_parsed_then_only_a_route_the_adapter_drives_is_a_request() {
        // Given requests on the session, permission and question routes, a bare route with a query, and ones off the list
        let call = |line: Value| SessionHttp::parse(&line.to_string());
        let create = call(json!({"id": 1, "method": "POST", "path": "/session?directory=%2Fw%2Fp", "body": {"title": "t"}})).unwrap();
        let reply = call(json!({"id": 2, "method": "POST", "path": "/permission/per_1/reply?directory=%2Fw", "body": {"reply": "once"}})).unwrap();
        let health = call(json!({"id": 0, "method": "GET", "path": "/global/health"})).unwrap();
        let refused = [
            call(json!({"id": 3, "method": "GET", "path": "/file?path=x"})),
            call(json!({"id": 3, "method": "GET", "path": "/session/../auth/x"})),
            call(json!({"id": 3, "method": "GET", "path": "/session/%2e%2e/auth/x"})),
            call(json!({"id": 3, "method": "GET", "path": "/sessions"})),
            call(json!({"id": 3, "method": "GET", "path": "/session x"})),
            call(json!({"id": 3, "method": "PATCH", "path": "/config"})),
            call(json!({"id": 3, "method": "PUT", "path": "/session/s"})),
            call(json!({"id": -1, "method": "GET", "path": "/session"})),
            SessionHttp::parse("not json"),
        ];
        // Then the three are requests with their body, and the others are INVALID_ARGS
        assert_eq!((create.id, create.method.as_str(), create.body), (1, "POST", Some(json!({"title": "t"}))));
        assert_eq!((reply.path.as_str(), health.body), ("/permission/per_1/reply?directory=%2Fw", None));
        assert!(refused.iter().all(|r| r.as_ref().is_err_and(|e| e.code == Code::InvalidArgs)), "{refused:?}");
        assert_eq!(SessionOpen::parse(&json!({"session": "s", "program": "opencode"})).unwrap().argv(), ["serve", "--hostname", "127.0.0.1", "--port", "0"]);
    }
}
