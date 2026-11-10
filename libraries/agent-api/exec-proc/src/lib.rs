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

/// The programs `cli_detect` looks for: the executor runs only these, by name, never a path or a word the caller makes up.
pub const AGENT_CLIS: [&str; 3] = ["claude", "codex", "opencode"];
/// The most a `--version` may take before the program counts as not answering.
pub const VERSION_TIMEOUT_MS: u64 = 5_000;
/// The longest version text kept.
pub const VERSION_MAX: usize = 200;

pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// The family's actions, in the contract's order.
pub const ACTIONS: &[Action] = &[
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
        about: "Say which of the named agent CLIs (claude, codex, opencode) the executor has on its path and what each prints for `--version` (run without a shell, at most 5 s each).",
        properties: &[Property { name: "programs", kind: "array", about: "Program names, each one of claude, codex, opencode (1 to 3, no repeats)" }],
        required: &["programs"],
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
        if list.is_empty() || list.len() > AGENT_CLIS.len() {
            return Err(ExecError::invalid_args(format!("`programs` names 1 to {} programs", AGENT_CLIS.len())));
        }
        let mut programs: Vec<String> = Vec::new();
        for item in list {
            let name = item.as_str().ok_or_else(|| ExecError::invalid_args("`programs` must be an array of program names"))?;
            if !AGENT_CLIS.contains(&name) {
                return Err(ExecError::invalid_args(format!("`programs` names only {}, not {name:?}", AGENT_CLIS.join(", "))));
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliDetectResult {
    pub clis: Vec<CliFound>,
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
        let refused: Vec<_> = [json!({"programs": ["rm"]}), json!({"programs": ["/usr/bin/claude"]}), json!({"programs": ["--help"]}), json!({"programs": ["claude", "claude"]}), json!({"programs": []}), json!({"programs": [1]}), json!({}), json!({"programs": ["claude", "codex", "opencode", "claude"]})].iter().map(|a| CliDetect::parse(a).unwrap_err().code).collect();
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
        let found = to_value(&CliFound { program: "claude".into(), installed: true, path: Some("/bin/claude".into()), version: Some("2.1.7".into()), error: None }).to_string();
        let silent = to_value(&CliFound { program: "codex".into(), installed: true, path: Some("/bin/codex".into()), version: None, error: Some("timed out".into()) }).to_string();
        let missing = to_value(&CliFound { program: "opencode".into(), installed: false, path: None, version: None, error: None }).to_string();
        // Then the keys are the contract's, in order
        assert_eq!(found, r#"{"program":"claude","installed":true,"path":"/bin/claude","version":"2.1.7"}"#);
        assert_eq!(silent, r#"{"program":"codex","installed":true,"path":"/bin/codex","error":"timed out"}"#);
        assert_eq!(missing, r#"{"program":"opencode","installed":false}"#);
    }
}
