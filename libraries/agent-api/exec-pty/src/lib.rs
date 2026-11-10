//! The pty actions of the executor contract: a terminal the executor owns at its workspace. `pty_open` starts the executor's shell on a pty,
//! `pty_attach` streams the pty's output as binary frames (`encode_frame`) numbered by byte offset, so a client that lost its stream resumes
//! with `after_offset`. Names are `[A-Za-z0-9_.-]`, chosen by the caller; sizes and environment are checked here, the cwd by the executor.
use exec_proto::args::{opt_string, opt_uint, string};
use exec_proto::{Action, ExecError, ExecResult, Property};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "testing")]
pub mod testing;

pub const PTY_OPEN: &str = "pty_open";
pub const PTY_WRITE: &str = "pty_write";
pub const PTY_RESIZE: &str = "pty_resize";
pub const PTY_CLOSE: &str = "pty_close";
pub const PTY_LIST: &str = "pty_list";
pub const PTY_ATTACH: &str = "pty_attach";

/// The actions that stay open after their request (they stream until the pty ends or a `cancel` names them).
pub const LONG: &[&str] = &[PTY_ATTACH];

pub const DEFAULT_COLS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 24;
pub const MAX_DIMENSION: u16 = 1000;
/// Most input one `pty_write` carries.
pub const WRITE_CAP: usize = 64 * 1024;
/// Output a pty keeps for a later attach; an attach asking for an offset before the oldest kept byte is told so (`reset`).
pub const RETAIN: usize = 1 << 20;
/// The most data one binary frame carries.
pub const FRAME_DATA_CAP: usize = 16 * 1024;
pub const MAX_PTYS: usize = 32;

/// What a pty's shell inherits from the executor: nothing else, so no secret of the executor's environment reaches a terminal.
pub const INHERITED_ENV: &[&str] = &["PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TMPDIR"];

/// The variables a client may set in `pty_open`'s `env`.
pub fn env_allowed(name: &str) -> bool {
    let shouty = |rest: &str| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    matches!(name, "TERM" | "COLORTERM" | "LANG" | "LANGUAGE" | "TZ") || name.strip_prefix("LC_").is_some_and(shouty) || name.strip_prefix("T3CODE_").is_some_and(shouty)
}

/// The family's actions, in the contract's order.
pub const ACTIONS: &[Action] = &[
    Action {
        name: PTY_OPEN,
        about: "Start the executor's shell on a new pty named `pty`; output is read with pty_attach.",
        properties: &[
            Property { name: "pty", kind: "string", about: "Name of the pty: 1-64 of letters, digits, `_`, `-`, `.`" },
            Property { name: "cwd", kind: "string", about: "Directory inside the workspace root or worktree base (default the root)" },
            Property { name: "cols", kind: "integer", about: "Columns (default 80)" },
            Property { name: "rows", kind: "integer", about: "Rows (default 24)" },
            Property { name: "env", kind: "object", about: "Extra variables: TERM, COLORTERM, LANG, LANGUAGE, TZ, LC_*, T3CODE_* (the T3 app's project script variables)" },
        ],
        required: &["pty"],
    },
    Action {
        name: PTY_WRITE,
        about: "Send input to a pty.",
        properties: &[Property { name: "pty", kind: "string", about: "Name of the pty" }, Property { name: "data", kind: "string", about: "Input text, at most 65536 bytes" }],
        required: &["pty", "data"],
    },
    Action {
        name: PTY_RESIZE,
        about: "Set a pty's size.",
        properties: &[Property { name: "pty", kind: "string", about: "Name of the pty" }, Property { name: "cols", kind: "integer", about: "Columns" }, Property { name: "rows", kind: "integer", about: "Rows" }],
        required: &["pty", "cols", "rows"],
    },
    Action { name: PTY_CLOSE, about: "Kill a pty's process group and forget the pty.", properties: &[Property { name: "pty", kind: "string", about: "Name of the pty" }], required: &["pty"] },
    Action { name: PTY_LIST, about: "The executor's ptys with their sizes and offsets.", properties: &[], required: &[] },
    Action {
        name: PTY_ATTACH,
        about: "Stream a pty's output as binary frames from a byte offset until the pty ends or the request is cancelled.",
        properties: &[
            Property { name: "pty", kind: "string", about: "Name of the pty" },
            Property { name: "after_offset", kind: "integer", about: "Send the bytes from this offset on (default the oldest kept byte)" },
        ],
        required: &["pty"],
    },
];

fn name(args: &Value) -> ExecResult<String> {
    let pty = string(args, "pty")?;
    if pty.is_empty() || pty.len() > 64 || pty.starts_with('.') || !pty.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')) {
        return Err(ExecError::invalid_args("`pty` must be 1-64 of letters, digits, `_`, `-`, `.` and not start with `.`"));
    }
    Ok(pty)
}

fn dimension(args: &Value, key: &str, default: Option<u16>) -> ExecResult<u16> {
    match (opt_uint(args, key)?, default) {
        (Some(n), _) if (1..=u64::from(MAX_DIMENSION)).contains(&n) => Ok(n as u16),
        (Some(_), _) => Err(ExecError::invalid_args(format!("`{key}` must be 1-{MAX_DIMENSION}"))),
        (None, Some(d)) => Ok(d),
        (None, None) => Err(ExecError::invalid_args(format!("`{key}` must be a non-negative integer"))),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PtyOpen {
    pub pty: String,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub env: Vec<(String, String)>,
}

impl PtyOpen {
    pub fn parse(args: &Value) -> ExecResult<PtyOpen> {
        let pty = name(args)?;
        let env = match &args["env"] {
            Value::Null => Vec::new(),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| match v.as_str() {
                    Some(v) if env_allowed(k) && !v.contains('\0') => Ok((k.clone(), v.to_string())),
                    Some(_) => Err(ExecError::invalid_args(format!("`env` may not set {k}"))),
                    None => Err(ExecError::invalid_args("`env` values must be strings")),
                })
                .collect::<ExecResult<Vec<_>>>()?,
            _ => return Err(ExecError::invalid_args("`env` must be an object")),
        };
        Ok(PtyOpen { pty, cwd: opt_string(args, "cwd")?, cols: dimension(args, "cols", Some(DEFAULT_COLS))?, rows: dimension(args, "rows", Some(DEFAULT_ROWS))?, env })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PtyWrite {
    pub pty: String,
    pub data: String,
}

impl PtyWrite {
    pub fn parse(args: &Value) -> ExecResult<PtyWrite> {
        let data = string(args, "data")?;
        if data.len() > WRITE_CAP {
            return Err(ExecError::invalid_args(format!("`data` is over {WRITE_CAP} bytes")));
        }
        Ok(PtyWrite { pty: name(args)?, data })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PtyResize {
    pub pty: String,
    pub cols: u16,
    pub rows: u16,
}

impl PtyResize {
    pub fn parse(args: &Value) -> ExecResult<PtyResize> {
        Ok(PtyResize { pty: name(args)?, cols: dimension(args, "cols", None)?, rows: dimension(args, "rows", None)? })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PtyClose {
    pub pty: String,
}

impl PtyClose {
    pub fn parse(args: &Value) -> ExecResult<PtyClose> {
        Ok(PtyClose { pty: name(args)? })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PtyAttach {
    pub pty: String,
    pub after_offset: Option<u64>,
}

impl PtyAttach {
    pub fn parse(args: &Value) -> ExecResult<PtyAttach> {
        Ok(PtyAttach { pty: name(args)?, after_offset: opt_uint(args, "after_offset")? })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyOpened {
    pub pty: String,
    pub pid: u32,
    /// Absolute directory the shell started in.
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyWritten {
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyResized {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyClosed {
    pub closed: bool,
    /// The shell's exit code (128 + signal when killed); none when it was still running and the kill is not yet reaped.
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyInfo {
    pub pty: String,
    pub pid: u32,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    /// Bytes of output the pty has produced.
    pub offset: u64,
    /// Offset of the oldest byte still kept.
    pub base_offset: u64,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyListed {
    pub ptys: Vec<PtyInfo>,
}

/// The result of an attach, sent when it ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtyAttached {
    pub pty: String,
    /// Offset after the last byte this attach sent.
    pub offset: u64,
    pub exited: bool,
    pub exit_code: Option<i32>,
    /// True when a `cancel` or the close of the channel ended the attach, not the pty.
    pub detached: bool,
}

/// The events of a pty's attach, as subscribe frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum PtyEvent {
    /// Sent before the first output frame. `reset` is true when the requested offset was before the oldest kept byte: the stream starts at `start_offset` instead.
    #[serde(rename = "pty.attached")]
    Attached { pty: String, requested_offset: u64, start_offset: u64, reset: bool, offset: u64 },
}

/// An output frame: `[id length: u8][id][offset: u64 big endian][bytes]`, sent as a binary WebSocket message. The id is the attach request's.
pub fn encode_frame(id: &str, offset: u64, data: &[u8]) -> Vec<u8> {
    let id = &id.as_bytes()[..id.len().min(255)];
    let mut frame = Vec::with_capacity(1 + id.len() + 8 + data.len());
    frame.push(id.len() as u8);
    frame.extend_from_slice(id);
    frame.extend_from_slice(&offset.to_be_bytes());
    frame.extend_from_slice(data);
    frame
}

/// `(id, offset, bytes)` of a frame `encode_frame` wrote; none for bytes that are not one.
pub fn decode_frame(frame: &[u8]) -> Option<(String, u64, &[u8])> {
    let id_len = usize::from(*frame.first()?);
    let id = std::str::from_utf8(frame.get(1..1 + id_len)?).ok()?;
    let offset = u64::from_be_bytes(frame.get(1 + id_len..9 + id_len)?.try_into().ok()?);
    Some((id.to_string(), offset, &frame[9 + id_len..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_proto::{to_value, Code};
    use serde_json::json;

    #[test]
    fn given_binary_data_when_framed_and_decoded_then_id_offset_and_every_byte_come_back() {
        // Given every byte value and an offset past 32 bits
        let data: Vec<u8> = (0..=255).collect();
        // When framed and decoded
        let frame = encode_frame("attach-7", 5_000_000_000, &data);
        // Then nothing is lost, and bytes that are no frame decode to none
        assert_eq!(decode_frame(&frame), Some(("attach-7".to_string(), 5_000_000_000, data.as_slice())));
        assert_eq!(decode_frame(&[]), None);
        assert_eq!(decode_frame(&[3, b'a']), None);
        assert_eq!(decode_frame(&encode_frame("x", 1, &[])[..5]), None);
    }

    #[test]
    fn given_open_arguments_when_parsed_then_defaults_apply_and_names_sizes_and_environment_are_checked() {
        // Given a bare open and one with every field
        let bare = PtyOpen::parse(&json!({"pty": "t1"})).unwrap();
        let full = PtyOpen::parse(&json!({"pty": "a.b-c_9", "cwd": "src", "cols": 120, "rows": 40, "env": {"TERM": "xterm", "LC_ALL": "C"}})).unwrap();
        // Then the bare one has the default size and the full one keeps its values
        assert_eq!((bare.cols, bare.rows, bare.cwd, bare.env.len()), (DEFAULT_COLS, DEFAULT_ROWS, None, 0));
        assert_eq!((full.cols, full.rows, full.cwd.as_deref(), full.env.len()), (120, 40, Some("src"), 2));
        // And each bad name, size or variable is INVALID_ARGS naming what is wrong
        for bad in [json!({"pty": ""}), json!({"pty": ".hidden"}), json!({"pty": "a/b"}), json!({"pty": "x".repeat(65)}), json!({"pty": 4}), json!({"pty": "t", "cols": 0}), json!({"pty": "t", "rows": 1001}), json!({"pty": "t", "cols": "8"}), json!({"pty": "t", "env": {"PATH": "/x"}}), json!({"pty": "t", "env": {"LD_PRELOAD": "x"}}), json!({"pty": "t", "env": {"TERM": 1}}), json!({"pty": "t", "env": "TERM=x"}), json!({"pty": "t", "env": {"LC_": "x"}})] {
            assert_eq!(PtyOpen::parse(&bad).unwrap_err().code, Code::InvalidArgs, "{bad}");
        }
    }

    #[test]
    fn given_write_resize_and_attach_arguments_when_parsed_then_limits_hold_and_the_offset_is_optional() {
        // Given a write over the cap, a resize without a size, and an attach with and without an offset
        let over = PtyWrite::parse(&json!({"pty": "t", "data": "x".repeat(WRITE_CAP + 1)}));
        let at_cap = PtyWrite::parse(&json!({"pty": "t", "data": "x".repeat(WRITE_CAP)}));
        let no_size = PtyResize::parse(&json!({"pty": "t"}));
        // Then the cap is the boundary and a resize needs both numbers
        assert!(over.is_err() && at_cap.is_ok() && no_size.is_err());
        assert_eq!(PtyAttach::parse(&json!({"pty": "t"})).unwrap().after_offset, None);
        assert_eq!(PtyAttach::parse(&json!({"pty": "t", "after_offset": 12})).unwrap().after_offset, Some(12));
        assert_eq!(PtyAttach::parse(&json!({"pty": "t", "after_offset": -1})).unwrap_err().code, Code::InvalidArgs);
    }

    #[test]
    fn given_the_results_and_event_when_encoded_then_the_fields_keep_the_contract_names_and_order() {
        // Given an attached event and an attach result
        // When encoded, Then the keys are the contract's, in order
        assert_eq!(to_value(&PtyEvent::Attached { pty: "t".into(), requested_offset: 9, start_offset: 20, reset: true, offset: 40 }).to_string(), r#"{"kind":"pty.attached","pty":"t","requested_offset":9,"start_offset":20,"reset":true,"offset":40}"#);
        assert_eq!(to_value(&PtyAttached { pty: "t".into(), offset: 5, exited: true, exit_code: Some(0), detached: false }).to_string(), r#"{"pty":"t","offset":5,"exited":true,"exit_code":0,"detached":false}"#);
    }

    #[test]
    fn given_the_action_table_when_read_then_every_required_key_is_a_property_and_attach_is_the_only_long_action() {
        // Given the table
        for action in ACTIONS {
            // Then each required argument is described
            assert!(action.required.iter().all(|r| action.properties.iter().any(|p| p.name == *r)), "{}", action.name);
        }
        assert_eq!(LONG, [PTY_ATTACH]);
    }
}
