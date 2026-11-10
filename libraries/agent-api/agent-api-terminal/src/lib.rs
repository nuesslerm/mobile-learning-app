//! The `terminal` group of the agent API: shells the workspace's executor runs on ptys. A terminal is named by the caller (`terminal`) within a
//! workspace. Its output is a byte stream numbered by offset: `terminal.attach` answers where the stream stands and then pushes events whose
//! `seq` is the offset after their last byte, so a caller that lost the stream attaches again with `after_offset`. The service keeps a bounded,
//! non-persisted tail of each stream; an attach older than the tail is answered by a `terminal.snapshot` with `reset: true` instead of the bytes
//! it missed. Output travels as base64 in `data`.
use agent_api_proto::{text, Event, GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub mod b64;

#[cfg(feature = "testing")]
pub mod testing;

pub const GROUP: GroupName = GroupName::new("terminal");

pub const OPEN: Method = Method::new(GROUP, "terminal.open");
pub const WRITE: Method = Method::new(GROUP, "terminal.write");
pub const RESIZE: Method = Method::new(GROUP, "terminal.resize");
pub const CLOSE: Method = Method::new(GROUP, "terminal.close");
pub const CLEAR: Method = Method::new(GROUP, "terminal.clear");
pub const LIST: Method = Method::new(GROUP, "terminal.list");
pub const RENAME: Method = Method::new(GROUP, "terminal.rename");
pub const ATTACH: Method = Method::new(GROUP, "terminal.attach");
pub const SUBSCRIBE: Method = Method::new(GROUP, "terminal.subscribe");

pub const METHODS: &[Method] = &[OPEN, WRITE, RESIZE, CLOSE, CLEAR, LIST, RENAME, ATTACH, SUBSCRIBE];

pub mod code {
    /// No terminal of that name in the workspace.
    pub const UNKNOWN_TERMINAL: &str = "UNKNOWN_TERMINAL";
    /// The terminal's shell has ended: it takes no input.
    pub const TERMINAL_EXITED: &str = "TERMINAL_EXITED";
    /// The attach asked for an offset the stream has not reached.
    pub const OFFSET_AHEAD: &str = "OFFSET_AHEAD";
}

/// Most output the service keeps of one terminal for a later attach.
pub const RETAIN: usize = 256 * 1024;
/// Most bytes one `terminal.output` event carries.
pub const CHUNK: usize = 16 * 1024;
/// Longest terminal name and title.
pub const NAME_MAX: usize = 48;
pub const TITLE_MAX: usize = 128;
pub const MAX_COLS: u64 = 1000;
pub const MAX_ROWS: u64 = 500;

/// The event kinds of a terminal's streams. `terminal.upsert` and `terminal.remove` are those of `terminal.subscribe`, the others of an attach.
pub mod kind {
    pub const SNAPSHOT: &str = "terminal.snapshot";
    pub const OUTPUT: &str = "terminal.output";
    pub const EXITED: &str = "terminal.exited";
    pub const CLOSED: &str = "terminal.closed";
    pub const CLEARED: &str = "terminal.cleared";
    pub const UPSERT: &str = "terminal.upsert";
    pub const REMOVE: &str = "terminal.remove";
}

fn required(params: &Value, key: &str) -> Result<String, ParamError> {
    text(params, key).map(str::to_string).ok_or_else(|| ParamError(format!("{key} is required")))
}

/// A terminal's name: `[A-Za-z0-9_.-]`, at most `NAME_MAX`, not starting with a dot.
pub fn terminal_param(params: &Value) -> Result<String, ParamError> {
    let name = required(params, "terminal")?;
    if name.len() > NAME_MAX || name.starts_with('.') || !name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')) {
        return Err(ParamError(format!("terminal must be 1-{NAME_MAX} of letters, digits, `_`, `-`, `.` and not start with `.`")));
    }
    Ok(name)
}

fn dimension(params: &Value, key: &str, max: u64) -> Result<Option<u16>, ParamError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) if (1..=max).contains(&n) => Ok(Some(n as u16)),
            _ => Err(ParamError(format!("{key} must be an integer from 1 to {max}"))),
        },
    }
}

fn optional_text(params: &Value, key: &str) -> Result<Option<String>, ParamError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s.clone())),
        Some(_) => Err(ParamError(format!("{key} must be a non-empty string"))),
    }
}

/// The identity every method but `list` and `subscribe` takes: a terminal of a workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub workspace: String,
    pub terminal: String,
}

impl Session {
    pub fn parse(params: &Value) -> Result<Session, ParamError> {
        Ok(Session { workspace: required(params, "workspace")?, terminal: terminal_param(params)? })
    }
}

/// `workspace` of the methods that take no terminal.
pub fn workspace_param(params: &Value) -> Result<String, ParamError> {
    required(params, "workspace")
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenParams {
    pub session: Session,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub env: Map<String, Value>,
    pub title: Option<String>,
}

impl OpenParams {
    pub fn parse(params: &Value) -> Result<OpenParams, ParamError> {
        let env = match params.get("env") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(env)) if env.values().all(Value::is_string) => env.clone(),
            Some(_) => return Err(ParamError("env must be an object of strings".into())),
        };
        let title = optional_text(params, "title")?;
        if title.as_ref().is_some_and(|t| t.chars().count() > TITLE_MAX) {
            return Err(ParamError(format!("title is over {TITLE_MAX} characters")));
        }
        Ok(OpenParams { session: Session::parse(params)?, cwd: optional_text(params, "cwd")?, cols: dimension(params, "cols", MAX_COLS)?, rows: dimension(params, "rows", MAX_ROWS)?, env, title })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WriteParams {
    pub session: Session,
    pub data: String,
}

impl WriteParams {
    pub fn parse(params: &Value) -> Result<WriteParams, ParamError> {
        let data = params.get("data").and_then(Value::as_str).filter(|d| !d.is_empty()).ok_or_else(|| ParamError("data is required".into()))?;
        if data.len() > 64 * 1024 {
            return Err(ParamError("data is over 65536 bytes".into()));
        }
        Ok(WriteParams { session: Session::parse(params)?, data: data.to_string() })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResizeParams {
    pub session: Session,
    pub cols: u16,
    pub rows: u16,
}

impl ResizeParams {
    pub fn parse(params: &Value) -> Result<ResizeParams, ParamError> {
        let need = |v: Option<u16>, key: &str| v.ok_or_else(|| ParamError(format!("{key} is required")));
        Ok(ResizeParams { session: Session::parse(params)?, cols: need(dimension(params, "cols", MAX_COLS)?, "cols")?, rows: need(dimension(params, "rows", MAX_ROWS)?, "rows")? })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenameParams {
    pub session: Session,
    pub title: String,
}

impl RenameParams {
    pub fn parse(params: &Value) -> Result<RenameParams, ParamError> {
        let title = required(params, "title")?;
        if title.chars().count() > TITLE_MAX {
            return Err(ParamError(format!("title is over {TITLE_MAX} characters")));
        }
        Ok(RenameParams { session: Session::parse(params)?, title })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachParams {
    pub session: Session,
    pub after_offset: Option<u64>,
}

impl AttachParams {
    pub fn parse(params: &Value) -> Result<AttachParams, ParamError> {
        let after_offset = match params.get("after_offset") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| ParamError("after_offset must be a non-negative integer".into()))?),
        };
        Ok(AttachParams { session: Session::parse(params)?, after_offset })
    }
}

/// The params of `terminal.subscribe`: the workspace whose terminals to follow, and the last metadata `seq` the caller saw.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeParams {
    pub workspace: String,
    pub after_seq: Option<u64>,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        let after_seq = match params.get("after_seq") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| ParamError("after_seq must be a non-negative integer".into()))?),
        };
        Ok(SubscribeParams { workspace: workspace_param(params)?, after_seq })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Exited,
}

/// What the service knows of a terminal: the result of `open`, the rows of `list` and the body of `terminal.upsert`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub workspace: String,
    pub terminal: String,
    pub title: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
    /// Bytes of output produced so far, and the offset of the oldest byte the service still keeps.
    pub offset: u64,
    pub base_offset: u64,
    /// Milliseconds since the epoch of the last change.
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenResult {
    #[serde(flatten)]
    pub terminal: TerminalInfo,
    /// False when the terminal existed and was running: it is returned as it is.
    pub opened: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    pub workspace: String,
    pub terminals: Vec<TerminalInfo>,
}

/// The answer to `terminal.attach`: where the replay starts and ends. The replay and then the live output follow as events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachResult {
    pub workspace: String,
    pub terminal: String,
    pub after_offset: Option<u64>,
    /// First offset the replay covers (the caller's, or the oldest kept byte when it asked for none or an older one).
    pub start_offset: u64,
    /// Offset the stream stands at when the answer was made.
    pub offset: u64,
    /// True when `after_offset` was before the oldest kept byte: a `terminal.snapshot` with `reset` replaces the bytes missed.
    pub reset: bool,
    pub cols: u16,
    pub rows: u16,
    pub status: Status,
    pub exit_code: Option<i32>,
}

/// The answer to `terminal.subscribe`: the terminals now, and the `seq` of the metadata stream they stand at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub workspace: String,
    pub terminals: Vec<TerminalInfo>,
    pub seq: u64,
    /// True when `after_seq` was older than the events kept: `terminals` replaces what the caller held.
    pub reset: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Done {
    pub workspace: String,
    pub terminal: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Written {
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resized {
    pub cols: u16,
    pub rows: u16,
}

fn stream_event(kind: &str, seq: u64, workspace: &str, terminal: &str, extra: Value) -> Event {
    let mut data = Map::new();
    data.insert("workspace".into(), json!(workspace));
    data.insert("terminal".into(), json!(terminal));
    if let Value::Object(extra) = extra {
        data.extend(extra);
    }
    Event { seq, thread: None, turn: None, kind: kind.to_string(), data }
}

/// Output of the stream from `offset`; its `seq` is the offset after it.
pub fn output_event(workspace: &str, terminal: &str, offset: u64, bytes: &[u8]) -> Event {
    stream_event(kind::OUTPUT, offset + bytes.len() as u64, workspace, terminal, json!({"offset": offset, "data": b64::encode(bytes)}))
}

/// The kept tail from `start_offset` to `offset`, with the terminal as it stands. `reset` says the caller's offset was older than the tail.
pub fn snapshot_event(info: &TerminalInfo, start_offset: u64, bytes: &[u8], reset: bool) -> Event {
    let end = start_offset + bytes.len() as u64;
    stream_event(kind::SNAPSHOT, end, &info.workspace, &info.terminal, json!({"start_offset": start_offset, "offset": end, "data": b64::encode(bytes), "reset": reset, "cols": info.cols, "rows": info.rows, "status": info.status, "exit_code": info.exit_code}))
}

pub fn exited_event(workspace: &str, terminal: &str, offset: u64, exit_code: Option<i32>) -> Event {
    stream_event(kind::EXITED, offset, workspace, terminal, json!({"exit_code": exit_code}))
}

pub fn closed_event(workspace: &str, terminal: &str, offset: u64) -> Event {
    stream_event(kind::CLOSED, offset, workspace, terminal, json!({}))
}

pub fn cleared_event(workspace: &str, terminal: &str, offset: u64) -> Event {
    stream_event(kind::CLEARED, offset, workspace, terminal, json!({}))
}

/// A change of the metadata stream, numbered `seq` in the workspace's own count.
pub fn upsert_event(seq: u64, info: &TerminalInfo) -> Event {
    stream_event(kind::UPSERT, seq, &info.workspace, &info.terminal, json!({"info": info}))
}

pub fn remove_event(seq: u64, workspace: &str, terminal: &str) -> Event {
    stream_event(kind::REMOVE, seq, workspace, terminal, json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> TerminalInfo {
        TerminalInfo { workspace: "w".into(), terminal: "t".into(), title: "t".into(), cwd: "/r".into(), cols: 80, rows: 24, status: Status::Running, exit_code: None, pid: Some(7), offset: 3, base_offset: 0, updated_at: 5 }
    }

    #[test]
    fn given_the_params_of_open_when_read_then_names_sizes_and_env_are_checked_and_each_missing_key_is_named() {
        // Given a minimal open and a full one
        let min = OpenParams::parse(&json!({"workspace": "w", "terminal": "term-1"})).unwrap();
        let full = OpenParams::parse(&json!({"workspace": "w", "terminal": "a.b_c-9", "cwd": "src", "cols": 120, "rows": 40, "env": {"TERM": "xterm"}, "title": "build"})).unwrap();
        // Then the minimal one has defaults and the full one keeps its values
        assert_eq!((min.cwd, min.cols, min.rows, min.env.len(), min.title), (None, None, None, 0, None));
        assert_eq!((full.cwd.as_deref(), full.cols, full.rows, full.env.len(), full.title.as_deref()), (Some("src"), Some(120), Some(40), 1, Some("build")));
        // And each bad key is refused with its name
        assert_eq!(OpenParams::parse(&json!({"terminal": "t"})).unwrap_err().0, "workspace is required");
        assert_eq!(OpenParams::parse(&json!({"workspace": "w"})).unwrap_err().0, "terminal is required");
        for bad in [json!({"terminal": ".x"}), json!({"terminal": "a/b"}), json!({"terminal": "x".repeat(NAME_MAX + 1)})] {
            let mut params = bad;
            params["workspace"] = json!("w");
            assert!(OpenParams::parse(&params).unwrap_err().0.starts_with("terminal must be"), "{params}");
        }
        let with = |extra: Value| {
            let mut params = json!({"workspace": "w", "terminal": "t"});
            params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            OpenParams::parse(&params).unwrap_err().0
        };
        assert_eq!(with(json!({"cols": 0})), "cols must be an integer from 1 to 1000");
        assert_eq!(with(json!({"rows": 501})), "rows must be an integer from 1 to 500");
        assert_eq!(with(json!({"env": {"A": 1}})), "env must be an object of strings");
        assert_eq!(with(json!({"cwd": ""})), "cwd must be a non-empty string");
    }

    #[test]
    fn given_write_resize_rename_attach_and_subscribe_params_when_read_then_limits_hold_and_offsets_are_optional() {
        let s = |extra: Value| {
            let mut params = json!({"workspace": "w", "terminal": "t"});
            params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            params
        };
        assert_eq!(WriteParams::parse(&s(json!({"data": "ls\n"}))).unwrap().data, "ls\n");
        assert_eq!(WriteParams::parse(&s(json!({}))).unwrap_err().0, "data is required");
        assert!(WriteParams::parse(&s(json!({"data": "x".repeat(65537)}))).is_err());
        assert_eq!(ResizeParams::parse(&s(json!({"cols": 100}))).unwrap_err().0, "rows is required");
        assert_eq!(ResizeParams::parse(&s(json!({"cols": 100, "rows": 30}))).unwrap().rows, 30);
        assert_eq!(RenameParams::parse(&s(json!({"title": "x".repeat(TITLE_MAX + 1)}))).unwrap_err().0, "title is over 128 characters");
        assert_eq!(AttachParams::parse(&s(json!({}))).unwrap().after_offset, None);
        assert_eq!(AttachParams::parse(&s(json!({"after_offset": 12}))).unwrap().after_offset, Some(12));
        assert_eq!(AttachParams::parse(&s(json!({"after_offset": -1}))).unwrap_err().0, "after_offset must be a non-negative integer");
        assert_eq!(SubscribeParams::parse(&json!({"workspace": "w", "after_seq": 4})).unwrap().after_seq, Some(4));
    }

    #[test]
    fn given_output_and_snapshot_events_when_encoded_then_seq_is_the_offset_after_the_bytes_and_data_is_base64_that_decodes_back() {
        // Given non-UTF-8 output at offset 10 and a snapshot of the tail
        let bytes = [0u8, 255, 104, 105];
        let out = output_event("w", "t", 10, &bytes);
        let snap = snapshot_event(&info(), 4, &bytes, true);
        // Then seq is the offset after the last byte, in both, and the data decodes to the bytes
        assert_eq!((out.seq, out.thread.clone(), out.topic()), (14, None, None));
        assert_eq!(b64::decode(out.data["data"].as_str().unwrap()), Some(bytes.to_vec()));
        assert_eq!((snap.seq, snap.data["start_offset"].clone(), snap.data["offset"].clone(), snap.data["reset"].clone(), snap.data["status"].clone()), (8, json!(4), json!(8), json!(true), json!("running")));
        assert_eq!(out.to_value()["event"]["type"], json!("terminal.output"));
    }

    #[test]
    fn given_the_metadata_events_when_encoded_then_the_upsert_carries_the_info_and_the_remove_names_the_terminal() {
        let up = upsert_event(3, &info());
        assert_eq!((up.seq, up.kind.as_str(), up.data["info"]["terminal"].clone()), (3, "terminal.upsert", json!("t")));
        let rm = remove_event(4, "w", "t");
        assert_eq!((rm.seq, rm.kind.as_str(), rm.data["terminal"].clone()), (4, "terminal.remove", json!("t")));
        assert_eq!(serde_json::to_value(OpenResult { terminal: info(), opened: true }).unwrap()["opened"], json!(true));
    }
}
