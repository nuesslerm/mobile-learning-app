//! The preview actions of the executor contract: the workspace's dev server, started, listed and stopped where the workspace is. A started
//! server listens on a port the caller names inside `PORT_MIN..=PORT_MAX`, runs in its own process group in a directory under the root, and
//! inherits only `INHERITED_ENV` from the executor (no token, no proxy): what else it sees the request names. `device_*` and `browser_*` need a
//! real emulator or browser and are not part of the contract (`docs/gaps/preview-device-browser-actions-not-built.md`).
use exec_proto::args::{opt_string, opt_uint, string};
use exec_proto::{Action, ExecError, ExecResult, Property};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "testing")]
pub mod testing;

pub const PREVIEW_START: &str = "preview_start";
pub const PREVIEW_STOP: &str = "preview_stop";
pub const PREVIEW_LIST: &str = "preview_list";

/// The actions that wait on a process: they run beside the reader, not in it.
pub const LONG: &[&str] = &[PREVIEW_START];

pub const PORT_MIN: u16 = 3000;
pub const PORT_MAX: u16 = 9999;
/// Servers an executor runs at once.
pub const MAX_RUNNING: usize = 4;
/// Exited servers `preview_list` still reports.
pub const MAX_EXITED: usize = 16;
pub const READY_DEFAULT_MS: u64 = 15_000;
pub const READY_MAX_MS: u64 = 60_000;
pub const COMMAND_MAX: usize = 4096;
pub const NAME_MAX: usize = 64;
pub const ENV_MAX: usize = 32;
/// The last output of a server `preview_list` carries, in bytes.
pub const TAIL_MAX: usize = 2048;

/// The variables a server inherits from the executor. A token, a proxy and the executor's own configuration directories are not among them.
pub const INHERITED_ENV: &[&str] = &["PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TMPDIR"];

/// Set by the executor, so a request cannot name them; the others cannot load code into the server's startup.
pub const RESERVED_ENV: &[&str] = &["PORT", "HOST", "BROWSER", "BASH_ENV", "ENV", "IFS", "SHELLOPTS", "PS4", "LD_PRELOAD", "LD_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES", "NODE_OPTIONS"];

/// The family's actions, in the contract's order.
pub const ACTIONS: &[Action] = &[
    Action {
        name: PREVIEW_START,
        about: "Start the workspace's dev server on a port of 3000-9999 and return once it accepts connections; refused when the port is taken, the server exits first or is not listening in time.",
        properties: &[
            Property { name: "command", kind: "string", about: "Shell command, at most 4096 bytes; it is told its port in PORT" },
            Property { name: "port", kind: "integer", about: "The port the server listens on, 3000 to 9999" },
            Property { name: "cwd", kind: "string", about: "Directory relative to the workspace root" },
            Property { name: "name", kind: "string", about: "A label of at most 64 letters, digits, dots, dashes and underscores" },
            Property { name: "env", kind: "object", about: "Extra environment variables (at most 32); the server inherits nothing else but PATH, HOME and the locale" },
            Property { name: "ready_timeout_ms", kind: "integer", about: "How long to wait for the port (default 15000, at most 60000)" },
        ],
        required: &["command", "port"],
    },
    Action { name: PREVIEW_STOP, about: "Stop a started server and its processes by id.", properties: &[Property { name: "id", kind: "string", about: "Id `preview_start` returned" }], required: &["id"] },
    Action { name: PREVIEW_LIST, about: "The servers this executor started: the running ones and the last that exited, with the tail of their output.", properties: &[], required: &[] },
];

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewStart {
    pub command: String,
    pub port: u16,
    pub cwd: Option<String>,
    pub name: Option<String>,
    pub env: Vec<(String, String)>,
    pub ready_timeout_ms: u64,
}

fn env_name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase() || c == '_') && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

impl PreviewStart {
    pub fn parse(args: &Value) -> ExecResult<PreviewStart> {
        let command = string(args, "command")?;
        if command.trim().is_empty() || command.len() > COMMAND_MAX || command.contains('\0') {
            return Err(ExecError::invalid_args(format!("`command` must be text of 1 to {COMMAND_MAX} bytes")));
        }
        let port = args["port"].as_u64().ok_or_else(|| ExecError::invalid_args("`port` must be an integer"))?;
        if !(PORT_MIN as u64..=PORT_MAX as u64).contains(&port) {
            return Err(ExecError::invalid_args(format!("`port` must be between {PORT_MIN} and {PORT_MAX}")));
        }
        let name = opt_string(args, "name")?;
        if let Some(name) = &name {
            if name.is_empty() || name.len() > NAME_MAX || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
                return Err(ExecError::invalid_args(format!("`name` must be 1 to {NAME_MAX} letters, digits, dots, dashes or underscores")));
            }
        }
        let env = match &args["env"] {
            Value::Null => Vec::new(),
            Value::Object(map) if map.len() > ENV_MAX => return Err(ExecError::invalid_args(format!("`env` holds at most {ENV_MAX} variables"))),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| {
                    if !env_name_ok(k) || RESERVED_ENV.contains(&k.as_str()) {
                        return Err(ExecError::invalid_args(format!("`env` may not set {k}")));
                    }
                    let value = v.as_str().filter(|v| !v.contains('\0')).ok_or_else(|| ExecError::invalid_args("`env` values must be strings"))?;
                    Ok((k.clone(), value.to_string()))
                })
                .collect::<ExecResult<Vec<_>>>()?,
            _ => return Err(ExecError::invalid_args("`env` must be an object")),
        };
        let ready_timeout_ms = opt_uint(args, "ready_timeout_ms")?.unwrap_or(READY_DEFAULT_MS);
        if ready_timeout_ms == 0 || ready_timeout_ms > READY_MAX_MS {
            return Err(ExecError::invalid_args(format!("`ready_timeout_ms` must be between 1 and {READY_MAX_MS}")));
        }
        Ok(PreviewStart { command, port: port as u16, cwd: opt_string(args, "cwd")?, name, env, ready_timeout_ms })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewStop {
    pub id: String,
}

/// One request of the family, read from its tool name and args.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewOp {
    Start(PreviewStart),
    Stop(PreviewStop),
    List,
}

impl PreviewOp {
    /// None for a tool that is not a preview action.
    pub fn parse(tool: &str, args: &Value) -> Option<ExecResult<PreviewOp>> {
        match tool {
            PREVIEW_START => Some(PreviewStart::parse(args).map(PreviewOp::Start)),
            PREVIEW_STOP => Some(string(args, "id").map(|id| PreviewOp::Stop(PreviewStop { id }))),
            PREVIEW_LIST => Some(Ok(PreviewOp::List)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Exited,
}

/// A server, as `preview_start` and `preview_list` report it. `url` is where the executor's machine reaches it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewInfo {
    pub id: String,
    pub name: String,
    pub command: String,
    pub cwd: String,
    pub port: u16,
    pub url: String,
    pub pid: u32,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub started_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exited_ms: Option<u64>,
    pub tail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stopped {
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    pub previews: Vec<PreviewInfo>,
}

pub fn url_of(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_proto::Code;
    use serde_json::json;

    #[test]
    fn given_a_start_request_when_parsed_then_the_defaults_fill_in_and_the_environment_keeps_its_order() {
        // Given a full request and a bare one
        let full = PreviewStart::parse(&json!({"command": "pnpm dev", "port": 4000, "cwd": "web", "name": "web-1", "env": {"B": "2", "A": "1"}, "ready_timeout_ms": 500})).unwrap();
        let bare = PreviewStart::parse(&json!({"command": "pnpm dev", "port": PORT_MIN})).unwrap();
        // Then the variables come in the order given and the bare one has the default wait and nothing else
        assert_eq!((full.env, full.cwd, full.name, full.ready_timeout_ms), (vec![("B".into(), "2".into()), ("A".into(), "1".into())], Some("web".into()), Some("web-1".into()), 500));
        assert_eq!((bare.ready_timeout_ms, bare.env.is_empty(), bare.cwd, bare.name), (READY_DEFAULT_MS, true, None, None));
    }

    #[test]
    fn given_ports_around_the_bounds_when_parsed_then_only_those_inside_are_accepted() {
        for (port, ok) in [(PORT_MIN as u64 - 1, false), (PORT_MIN as u64, true), (PORT_MAX as u64, true), (PORT_MAX as u64 + 1, false), (0, false), (65536, false)] {
            let parsed = PreviewStart::parse(&json!({"command": "x", "port": port}));
            assert_eq!(parsed.is_ok(), ok, "port {port}: {parsed:?}");
        }
        let text = PreviewStart::parse(&json!({"command": "x", "port": "4000"})).unwrap_err();
        assert!(text.code == Code::InvalidArgs && text.message.contains("`port`"), "{text:?}");
    }

    #[test]
    fn given_an_environment_that_names_a_reserved_or_malformed_variable_when_parsed_then_it_is_refused_by_name() {
        // Given requests that set the port, a loader hook, a lower-case name and a number
        for (name, value) in [("PORT", json!("1")), ("LD_PRELOAD", json!("x.so")), ("NODE_OPTIONS", json!("--require x")), ("path", json!("x")), ("A B", json!("x"))] {
            let refused = PreviewStart::parse(&json!({"command": "x", "port": 4000, "env": {name: value}})).unwrap_err();
            // Then each is refused naming the variable
            assert!(refused.message.contains(name), "{name}: {refused:?}");
        }
        assert!(PreviewStart::parse(&json!({"command": "x", "port": 4000, "env": {"A": 1}})).unwrap_err().message.contains("strings"));
        assert!(PreviewStart::parse(&json!({"command": "x", "port": 4000, "env": "A=1"})).unwrap_err().message.contains("object"));
    }

    #[test]
    fn given_a_command_a_name_and_a_wait_out_of_range_when_parsed_then_each_is_refused() {
        let long = "x".repeat(COMMAND_MAX + 1);
        for args in [json!({"command": "  ", "port": 4000}), json!({"command": long, "port": 4000}), json!({"command": "x", "port": 4000, "name": "a b"}), json!({"command": "x", "port": 4000, "name": ""}), json!({"command": "x", "port": 4000, "ready_timeout_ms": 0}), json!({"command": "x", "port": 4000, "ready_timeout_ms": READY_MAX_MS + 1})] {
            assert_eq!(PreviewStart::parse(&args).unwrap_err().code, Code::InvalidArgs, "{args}");
        }
    }

    #[test]
    fn given_the_tools_when_parsed_as_ops_then_only_the_three_preview_actions_are_ops() {
        assert!(matches!(PreviewOp::parse(PREVIEW_STOP, &json!({"id": "p1"})), Some(Ok(PreviewOp::Stop(PreviewStop { id }))) if id == "p1"));
        assert!(matches!(PreviewOp::parse(PREVIEW_STOP, &json!({})), Some(Err(_))));
        assert!(matches!(PreviewOp::parse(PREVIEW_LIST, &json!({})), Some(Ok(PreviewOp::List))));
        assert!(PreviewOp::parse("run_command", &json!({})).is_none());
        assert_eq!(ACTIONS.iter().map(|a| a.name).collect::<Vec<_>>(), [PREVIEW_START, PREVIEW_STOP, PREVIEW_LIST]);
        assert!(!INHERITED_ENV.iter().any(|v| v.contains("TOKEN") || v.contains("KEY") || v.contains("PROXY")));
    }

    #[test]
    fn given_a_listed_server_when_encoded_then_the_fields_keep_the_contract_names_and_an_unexited_one_has_no_exit() {
        let running = PreviewInfo { id: "preview-1".into(), name: "web".into(), command: "x".into(), cwd: ".".into(), port: 4000, url: url_of(4000), pid: 7, state: State::Running, exit_code: None, started_ms: 5, exited_ms: None, tail: String::new() };
        assert_eq!(exec_proto::to_value(&running).to_string(), r#"{"id":"preview-1","name":"web","command":"x","cwd":".","port":4000,"url":"http://127.0.0.1:4000/","pid":7,"state":"running","started_ms":5,"tail":""}"#);
    }
}
