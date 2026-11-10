//! The envelope round trip the event enums of the sub-groups share: the enum's `type` is the event's kind and its other fields sit beside the
//! envelope's, as `ThreadEvent` does.
use agent_api_proto::{Event, ParseError};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};

/// `event` on the envelope of `thread`; the fields keep the order the enum declares them in.
pub(crate) fn encode<T: Serialize>(event: &T, thread: &str, turn: Option<&str>, seq: u64) -> Event {
    let (kind, data) = match serde_json::to_value(event) {
        Ok(Value::Object(fields)) => {
            let kind = fields.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
            (kind, fields.into_iter().filter(|(name, _)| name != "type").collect::<Map<String, Value>>())
        }
        _ => (String::new(), Map::new()),
    };
    Event { seq, thread: Some(thread.to_string()), turn: turn.map(str::to_string), kind, data }
}

/// The enum a wire event is: refused when it has no thread, is of a kind outside `kinds`, or lacks a field.
pub(crate) fn decode<T: DeserializeOwned>(event: &Event, kinds: &[&str]) -> Result<T, ParseError> {
    if event.thread.is_none() {
        return Err(ParseError("a thread event needs a thread".into()));
    }
    if !kinds.contains(&event.kind.as_str()) {
        return Err(ParseError(format!("{} is not an event of this group", event.kind)));
    }
    let mut fields = Map::new();
    fields.insert("type".into(), Value::String(event.kind.clone()));
    fields.extend(event.data.clone());
    serde_json::from_value(Value::Object(fields)).map_err(|e| ParseError(format!("{}: {e}", event.kind)))
}
