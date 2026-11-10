//! The rca agent API's wire (`GET /agent/v1/socket`, `docs/plans/rca-client/architecture.md`): JSON text frames. A request is
//! `{id, method, params}`, answered by `{id, result}` or `{id, error: {code, message}}`; an event is `{event: {seq, thread, turn, type, ...}}`.
//! Pure: the service and every mapper parse and encode with these, and each capability group names its methods in its own crate.
use serde_json::{json, Map, Value};
use std::fmt;

#[cfg(feature = "testing")]
pub mod testing;

/// The agent API's protocol version; a mapper refuses a service that answers another.
pub const PROTOCOL: u32 = 1;

/// The error codes every group shares. A group names its own beside its methods.
pub mod code {
    pub const INVALID_REQUEST: &str = "INVALID_REQUEST";
    pub const UNKNOWN_METHOD: &str = "UNKNOWN_METHOD";
    pub const INVALID_PARAMS: &str = "INVALID_PARAMS";
}

/// A capability group, as `service.status` lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupName(&'static str);

impl GroupName {
    pub const fn new(name: &'static str) -> GroupName {
        GroupName(name)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// A method and the group it belongs to. Each group crate holds its own as constants, so no crate lists every method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Method {
    group: GroupName,
    name: &'static str,
}

impl Method {
    pub const fn new(group: GroupName, name: &'static str) -> Method {
        Method { group, name }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn group(self) -> GroupName {
        self.group
    }
}

/// A frame that is no valid frame of its kind.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

/// What a response says went wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Result { id: Value, result: Value },
    Error { id: Value, error: ErrorBody },
}

impl Response {
    pub fn result(id: Value, result: Value) -> Response {
        Response::Result { id, result }
    }

    pub fn error(id: Value, code: &str, message: impl Into<String>) -> Response {
        Response::Error { id, error: ErrorBody { code: code.to_string(), message: message.into() } }
    }

    pub fn id(&self) -> &Value {
        match self {
            Response::Result { id, .. } | Response::Error { id, .. } => id,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Response::Result { id, result } => json!({ "id": id, "result": result }),
            Response::Error { id, error } => json!({ "id": id, "error": { "code": error.code, "message": error.message } }),
        }
    }

    pub fn encode(&self) -> String {
        self.to_value().to_string()
    }

    pub fn from_value(frame: &Value) -> Result<Response, ParseError> {
        let id = frame.get("id").cloned().ok_or_else(|| ParseError("a response needs an id".into()))?;
        if let Some(error) = frame.get("error") {
            let text = |key: &str| error.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| ParseError(format!("an error needs a {key}")));
            return Ok(Response::Error { id, error: ErrorBody { code: text("code")?, message: text("message")? } });
        }
        frame.get("result").cloned().map(|result| Response::Result { id, result }).ok_or_else(|| ParseError("a response needs a result or an error".into()))
    }

    pub fn parse(text: &str) -> Result<Response, ParseError> {
        Response::from_value(&serde_json::from_str::<Value>(text).map_err(|e| ParseError(format!("not JSON: {e}")))?)
    }
}

/// A request frame. `params` is an empty object when the frame has none.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

impl Request {
    pub fn new(id: impl Into<Value>, method: Method, params: Value) -> Request {
        Request { id: id.into(), method: method.name().to_string(), params }
    }

    /// The request a frame holds, or the response that refuses it: an id that is not a string or a number is echoed as it came.
    pub fn from_value(frame: &Value) -> Result<Request, Box<Response>> {
        let id = frame.get("id").cloned().unwrap_or(Value::Null);
        if !(id.is_string() || id.is_number()) {
            return Err(Box::new(Response::error(id, code::INVALID_REQUEST, "a request needs an id, a string or a number")));
        }
        let Some(method) = frame.get("method").and_then(Value::as_str) else { return Err(Box::new(Response::error(id, code::INVALID_REQUEST, "a request needs a method"))) };
        let params = frame.get("params").cloned().unwrap_or_else(|| json!({}));
        Ok(Request { id, method: method.to_string(), params })
    }

    pub fn parse(text: &str) -> Result<Request, Box<Response>> {
        match serde_json::from_str::<Value>(text) {
            Ok(frame) => Request::from_value(&frame),
            Err(_) => Err(Box::new(Response::error(Value::Null, code::INVALID_REQUEST, "a frame is one JSON request"))),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({ "id": self.id, "method": self.method, "params": self.params })
    }

    pub fn encode(&self) -> String {
        self.to_value().to_string()
    }
}

/// A param that is missing or of the wrong kind: answered `INVALID_PARAMS`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamError(pub String);

impl ParamError {
    pub fn response(self, id: Value) -> Response {
        Response::error(id, code::INVALID_PARAMS, self.0)
    }
}

/// A param that is a non-empty string.
pub fn text<'a>(params: &'a Value, name: &str) -> Option<&'a str> {
    params.get(name).and_then(Value::as_str).filter(|v| !v.is_empty())
}

/// The topic of a thread's events, `thread/<id>`: the stream `thread.subscribe` opens and a client resumes by its last `seq`.
pub fn thread_topic(thread: &str) -> String {
    format!("thread/{thread}")
}

/// One event of a thread, in the order of its `seq`: `{event: {seq, thread?, turn?, type, ...data}}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub seq: u64,
    pub thread: Option<String>,
    pub turn: Option<String>,
    pub kind: String,
    pub data: Map<String, Value>,
}

impl Event {
    /// The stream whose `seq` this event carries: `thread/<id>` for an event of a thread, none for an event of no stream.
    pub fn topic(&self) -> Option<String> {
        self.thread.as_deref().map(thread_topic)
    }

    pub fn to_value(&self) -> Value {
        let mut event = Map::new();
        event.insert("seq".into(), json!(self.seq));
        if let Some(thread) = &self.thread {
            event.insert("thread".into(), json!(thread));
        }
        if let Some(turn) = &self.turn {
            event.insert("turn".into(), json!(turn));
        }
        event.insert("type".into(), json!(self.kind));
        event.extend(self.data.clone());
        json!({ "event": event })
    }

    pub fn encode(&self) -> String {
        self.to_value().to_string()
    }

    pub fn from_value(frame: &Value) -> Result<Event, ParseError> {
        let event = frame.get("event").and_then(Value::as_object).ok_or_else(|| ParseError("an event frame needs an event object".into()))?;
        let seq = event.get("seq").and_then(Value::as_u64).ok_or_else(|| ParseError("an event needs a numeric seq".into()))?;
        let kind = event.get("type").and_then(Value::as_str).ok_or_else(|| ParseError("an event needs a type".into()))?.to_string();
        let own = |key: &str| event.get(key).and_then(Value::as_str).map(str::to_string);
        let data = event.iter().filter(|(k, _)| !["seq", "thread", "turn", "type"].contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        Ok(Event { seq, thread: own("thread"), turn: own("turn"), kind, data })
    }
}

/// A frame from the service: the answer to a request, or an event.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Response(Response),
    Event(Event),
}

impl Incoming {
    pub fn parse(text: &str) -> Result<Incoming, ParseError> {
        let frame: Value = serde_json::from_str(text).map_err(|e| ParseError(format!("not JSON: {e}")))?;
        if frame.get("event").is_some() {
            Event::from_value(&frame).map(Incoming::Event)
        } else {
            Response::from_value(&frame).map(Incoming::Response)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP: GroupName = GroupName::new("demo");
    const PING: Method = Method::new(GROUP, "demo.ping");

    #[test]
    fn given_a_request_when_encoded_and_parsed_then_it_is_the_same_and_a_frame_without_params_has_an_empty_object() {
        // Given a request built for a method, and a frame that leaves params out
        let request = Request::new(7, PING, json!({"n": 1}));
        // When the first is encoded and parsed, and the second parsed
        let back = Request::parse(&request.encode());
        let bare = Request::parse(r#"{"id": "a", "method": "demo.ping"}"#);
        // Then the first is unchanged and the second has params {}
        assert_eq!(back, Ok(request));
        assert_eq!(bare.unwrap().params, json!({}));
    }

    #[test]
    fn given_frames_that_are_no_request_when_parsed_then_each_is_refused_by_a_response_that_echoes_the_id_as_it_came() {
        // Given text that is not JSON, a frame with no id, with an id that is neither string nor number, and one with no method
        let cases = [("not json", Value::Null, "a frame is one JSON request"), (r#"{"method": "demo.ping"}"#, Value::Null, "a request needs an id, a string or a number"), (r#"{"id": [1], "method": "demo.ping"}"#, json!([1]), "a request needs an id, a string or a number"), (r#"{"id": 5}"#, json!(5), "a request needs a method")];
        for (text, id, message) in cases {
            // When each is parsed
            let refused = *Request::parse(text).unwrap_err();
            // Then the response is INVALID_REQUEST with that message and the id as it came
            assert_eq!(refused, Response::error(id, code::INVALID_REQUEST, message), "{text}");
        }
        // And a frame that is not an object at all is refused the same way
        assert_eq!(Request::from_value(&json!(5)).unwrap_err().to_value()["error"]["code"], json!("INVALID_REQUEST"));
    }

    #[test]
    fn given_the_two_kinds_of_response_when_encoded_then_the_keys_are_the_contract_and_parsing_reads_them_back() {
        // Given a result and an error response
        let ok = Response::result(json!(1), json!({"value": null}));
        let failed = Response::error(json!("s"), "NO_EXECUTOR", "no executor is attached for workspace w2");
        // When they are encoded
        // Then the keys are id and result, or id and error with code and message, and parsing returns what was encoded
        assert_eq!(ok.encode(), r#"{"id":1,"result":{"value":null}}"#);
        assert_eq!(failed.encode(), r#"{"id":"s","error":{"code":"NO_EXECUTOR","message":"no executor is attached for workspace w2"}}"#);
        assert_eq!((Response::parse(&ok.encode()), Response::parse(&failed.encode())), (Ok(ok), Ok(failed)));
        assert!(Response::parse(r#"{"id": 1}"#).is_err() && Response::parse("{").is_err());
    }

    #[test]
    fn given_an_event_when_encoded_and_parsed_then_the_data_stays_beside_the_envelope_fields() {
        // Given an event of a thread with data of its own
        let mut data = Map::new();
        data.insert("role".into(), json!("assistant"));
        data.insert("text".into(), json!("hi"));
        let event = Event { seq: 12, thread: Some("t1".into()), turn: None, kind: "message.delta".into(), data };
        // When it is encoded
        let frame = event.encode();
        // Then the envelope fields and the data are siblings, a turn that is none is left out, and a mapper reads the frame as an event
        assert_eq!(frame, r#"{"event":{"seq":12,"thread":"t1","type":"message.delta","role":"assistant","text":"hi"}}"#);
        assert_eq!(Incoming::parse(&frame), Ok(Incoming::Event(event)));
        assert!(matches!(Incoming::parse(r#"{"id": 1, "result": {}}"#), Ok(Incoming::Response(_))));
        assert!(Incoming::parse(r#"{"event": {"type": "x"}}"#).is_err());
    }

    #[test]
    fn given_events_of_a_thread_and_of_none_when_their_topics_are_read_then_only_the_thread_has_one() {
        // Given an event of thread t1 and an event that names no thread
        let of = |thread: Option<&str>| Event { seq: 1, thread: thread.map(String::from), turn: None, kind: "x".into(), data: Map::new() };
        // When their topics are read, Then the first is thread/t1, the stream thread.subscribe names, and the second has none
        assert_eq!((of(Some("t1")).topic(), of(None).topic()), (Some("thread/t1".to_string()), None));
        assert_eq!(thread_topic("a b"), "thread/a b");
    }

    #[test]
    fn given_a_method_of_a_group_when_matched_then_it_is_equal_only_to_itself() {
        // Given a method and the group it was made for
        // Then it reports both, and one of the same group with another name is a different method
        assert_eq!((PING.name(), PING.group().as_str()), ("demo.ping", "demo"));
        assert_ne!(PING, Method::new(GROUP, "demo.pong"));
        assert_eq!(text(&json!({"a": "", "b": "x", "c": 1}), "a"), None);
        assert_eq!((text(&json!({"b": "x"}), "b"), text(&json!({"c": 1}), "c")), (Some("x"), None));
    }
}
