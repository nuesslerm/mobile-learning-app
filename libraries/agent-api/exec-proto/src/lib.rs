//! The executor channel's wire (`docs/plans/rca-client/protocol.md`), shared by the service that asks and the executor that acts: a request
//! frame `{id, tool, args}` is answered by output frames `{id, stream, data}` and one result frame `{id, ok, result}` or
//! `{id, ok: false, error: {code, message}}`. Pure: no I/O, so both sides parse and encode the same bytes.
use serde_json::{json, Value};

pub mod args;

#[cfg(feature = "testing")]
pub mod testing;

/// The WebSocket close code of a channel that a newer channel for the same workspace replaced; it ends all of that executor's streams.
pub const REPLACED: u16 = 4001;

/// Every code an action can fail with, whoever raises it: the executor, the service when no executor answers, the MCP bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    PathOutsideRoot,
    NotFound,
    NotAFile,
    /// A create found the path taken.
    AlreadyExists,
    EditNotFound,
    EditAmbiguous,
    InvalidArgs,
    UnknownTool,
    Io,
    /// No channel is open for the workspace.
    NoExecutor,
    /// The channel closed while the action was pending.
    ExecutorGone,
    /// The MCP bridge refused a `tools/call` (unknown tool, a required argument missing) before any executor was asked.
    InvalidArguments,
}

impl Code {
    pub const ALL: [Code; 12] = [Code::PathOutsideRoot, Code::NotFound, Code::NotAFile, Code::AlreadyExists, Code::EditNotFound, Code::EditAmbiguous, Code::InvalidArgs, Code::UnknownTool, Code::Io, Code::NoExecutor, Code::ExecutorGone, Code::InvalidArguments];

    pub fn as_str(self) -> &'static str {
        match self {
            Code::PathOutsideRoot => "PATH_OUTSIDE_ROOT",
            Code::NotFound => "NOT_FOUND",
            Code::NotAFile => "NOT_A_FILE",
            Code::AlreadyExists => "ALREADY_EXISTS",
            Code::EditNotFound => "EDIT_NOT_FOUND",
            Code::EditAmbiguous => "EDIT_AMBIGUOUS",
            Code::InvalidArgs => "INVALID_ARGS",
            Code::UnknownTool => "UNKNOWN_TOOL",
            Code::Io => "IO_ERROR",
            Code::NoExecutor => "NO_EXECUTOR",
            Code::ExecutorGone => "EXECUTOR_GONE",
            Code::InvalidArguments => "INVALID_ARGUMENTS",
        }
    }

    /// The code a wire string names; an executor may answer a code this build does not know.
    pub fn parse(code: &str) -> Option<Code> {
        Code::ALL.into_iter().find(|c| c.as_str() == code)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecError {
    pub code: Code,
    pub message: String,
}

impl ExecError {
    pub fn new(code: Code, message: impl Into<String>) -> ExecError {
        ExecError { code, message: message.into() }
    }

    pub fn invalid_args(message: impl Into<String>) -> ExecError {
        ExecError::new(Code::InvalidArgs, message)
    }
}

pub type ExecResult<T> = Result<T, ExecError>;

/// A result as the wire carries it, its fields in declaration order.
pub fn to_value<T: serde::Serialize>(result: &T) -> Value {
    serde_json::to_value(result).unwrap_or(Value::Null)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    pub fn as_str(self) -> &'static str {
        match self {
            Stream::Stdout => "stdout",
            Stream::Stderr => "stderr",
        }
    }
}

/// One argument of an action: its name, JSON type and what it means.
#[derive(Debug, Clone, Copy)]
pub struct Property {
    pub name: &'static str,
    pub kind: &'static str,
    pub about: &'static str,
}

/// An executor action: its wire name (`tool` of a request frame) and its argument table, which the MCP bridge publishes as the tool's schema.
#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub name: &'static str,
    pub about: &'static str,
    pub properties: &'static [Property],
    pub required: &'static [&'static str],
}

/// A request frame: the service asks the executor to perform `tool` with `args`.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: String,
    pub tool: String,
    pub args: Value,
}

impl Request {
    pub fn new(id: impl Into<String>, tool: &str, args: Value) -> Request {
        Request { id: id.into(), tool: tool.to_string(), args }
    }

    pub fn to_value(&self) -> Value {
        json!({"id": self.id, "tool": self.tool, "args": self.args})
    }

    pub fn encode(&self) -> String {
        self.to_value().to_string()
    }

    /// A frame's parts as the executor reads them: a missing id or tool is empty and missing args are null, so the action fails by its own
    /// code (`UNKNOWN_TOOL`, `INVALID_ARGS`) and not by the framing.
    pub fn from_value(frame: &Value) -> Request {
        Request { id: frame["id"].as_str().unwrap_or("").to_string(), tool: frame["tool"].as_str().unwrap_or("").to_string(), args: frame["args"].clone() }
    }

    pub fn parse(text: &str) -> ExecResult<Request> {
        serde_json::from_str::<Value>(text).map(|frame| Request::from_value(&frame)).map_err(|e| ExecError::invalid_args(format!("not JSON: {e}")))
    }
}

/// What an executor answered `ok: false`.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

/// A chunk of a running action's output.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputFrame {
    pub id: String,
    pub stream: String,
    pub data: String,
}

/// An event of a `subscribe` action's stream, which outlives the request that opened it; `seq` is the stream's own sequence, so a
/// re-subscribe after a reconnect can ask for what follows the last one seen. The type only: no action sends it yet.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeFrame {
    pub id: String,
    pub seq: u64,
    pub event: Value,
}

/// A frame from the executor to the service.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Output(OutputFrame),
    Done { id: String, outcome: Result<Value, ErrorBody> },
    Event(SubscribeFrame),
}

impl Reply {
    /// `None` for what is not a frame of a request: text that is not JSON, a frame whose id is not a string, one that is neither output,
    /// a result nor an event.
    pub fn parse(text: &str) -> Option<Reply> {
        let frame: Value = serde_json::from_str(text).ok()?;
        let id = frame["id"].as_str()?.to_string();
        if frame["stream"].is_string() {
            return Some(Reply::Output(OutputFrame { id, stream: frame["stream"].as_str().unwrap_or_default().to_string(), data: frame["data"].as_str().unwrap_or_default().to_string() }));
        }
        if frame["ok"] == true {
            return Some(Reply::Done { id, outcome: Ok(frame["result"].clone()) });
        }
        if frame["ok"] == false {
            let text = |k: &str| frame["error"][k].as_str().unwrap_or_default().to_string();
            return Some(Reply::Done { id, outcome: Err(ErrorBody { code: text("code"), message: text("message") }) });
        }
        match frame["seq"].as_u64() {
            Some(seq) if !frame["event"].is_null() => Some(Reply::Event(SubscribeFrame { id, seq, event: frame["event"].clone() })),
            _ => None,
        }
    }
}

pub fn output_frame(id: &str, stream: Stream, data: &str) -> String {
    json!({"id": id, "stream": stream.as_str(), "data": data}).to_string()
}

pub fn result_frame(id: &str, result: &Value) -> String {
    json!({"id": id, "ok": true, "result": result}).to_string()
}

pub fn error_frame(id: &str, code: &str, message: &str) -> String {
    json!({"id": id, "ok": false, "error": {"code": code, "message": message}}).to_string()
}

pub fn subscribe_frame(frame: &SubscribeFrame) -> String {
    json!({"id": frame.id, "seq": frame.seq, "event": frame.event}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_every_code_when_its_string_is_parsed_then_it_comes_back_and_no_two_share_a_string() {
        // Given the codes of the contract
        for code in Code::ALL {
            // When each one's wire string is read back
            let back = Code::parse(code.as_str());
            // Then it is the same code
            assert_eq!(back, Some(code));
        }
        // And no two codes share a string, and a string the contract does not name is none
        let mut strings: Vec<&str> = Code::ALL.iter().map(|c| c.as_str()).collect();
        strings.sort_unstable();
        strings.dedup();
        assert_eq!(strings.len(), Code::ALL.len());
        assert_eq!(Code::parse("NO_SUCH_CODE"), None);
    }

    #[test]
    fn given_the_frames_an_executor_writes_when_they_are_parsed_then_each_is_what_was_encoded() {
        // Given one output, one success, one failure and one subscribe frame, as the encoders write them
        let output = output_frame("a1", Stream::Stderr, "oops\n");
        let done = result_frame("a1", &json!({"exit_code": 0}));
        let failed = error_frame("a2", Code::NotFound.as_str(), "x does not exist");
        let event = subscribe_frame(&SubscribeFrame { id: "s1".into(), seq: 7, event: json!({"kind": "git.status"}) });
        // When they are parsed
        let parsed: Vec<Option<Reply>> = [output, done, failed, event].iter().map(|t| Reply::parse(t)).collect();
        // Then each is the frame it was
        assert_eq!(parsed[0], Some(Reply::Output(OutputFrame { id: "a1".into(), stream: "stderr".into(), data: "oops\n".into() })));
        assert_eq!(parsed[1], Some(Reply::Done { id: "a1".into(), outcome: Ok(json!({"exit_code": 0})) }));
        assert_eq!(parsed[2], Some(Reply::Done { id: "a2".into(), outcome: Err(ErrorBody { code: "NOT_FOUND".into(), message: "x does not exist".into() }) }));
        assert_eq!(parsed[3], Some(Reply::Event(SubscribeFrame { id: "s1".into(), seq: 7, event: json!({"kind": "git.status"}) })));
    }

    #[test]
    fn given_text_that_is_no_frame_of_a_request_when_parsed_then_it_is_ignored() {
        // Given text that is not JSON, a frame without a string id, and one that says neither output, ok nor event
        let ignored = ["not json", r#"{"id": 5, "ok": true}"#, r#"{"ok": true}"#, r#"{"id": "a", "note": 1}"#, r#"{"id": "a", "seq": 1}"#];
        // When each is parsed, Then none is a reply
        for text in ignored {
            assert_eq!(Reply::parse(text), None, "{text}");
        }
    }

    #[test]
    fn given_a_result_frame_without_its_fields_when_parsed_then_the_missing_parts_are_null_and_empty() {
        // Given an ok frame with no result and a failure with no error body
        let ok = Reply::parse(r#"{"id": "a", "ok": true}"#);
        let failed = Reply::parse(r#"{"id": "b", "ok": false}"#);
        // Then the result is null and the code and message are empty, not a refusal of the frame
        assert_eq!(ok, Some(Reply::Done { id: "a".into(), outcome: Ok(Value::Null) }));
        assert_eq!(failed, Some(Reply::Done { id: "b".into(), outcome: Err(ErrorBody { code: String::new(), message: String::new() }) }));
    }

    #[test]
    fn given_a_request_frame_when_encoded_and_parsed_then_it_round_trips_and_text_that_is_not_json_fails_with_invalid_args() {
        // Given a request
        let request = Request::new("a7", "read_file", json!({"path": "a.txt"}));
        // When it is encoded and parsed again, Then it is the same request
        assert_eq!(Request::parse(&request.encode()), Ok(request));
        // And a frame without parts reads as empty ones, while text that is not JSON is INVALID_ARGS
        assert_eq!(Request::from_value(&json!({})), Request { id: String::new(), tool: String::new(), args: Value::Null });
        assert_eq!(Request::parse("{").unwrap_err().code, Code::InvalidArgs);
    }
}
