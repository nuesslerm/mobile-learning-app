//! The params of each method of the group. A request that names a field none of these has, that gives a field the wrong kind, or that leaves
//! out one that is required is answered `INVALID_PARAMS` (`ParamError::response`), with a message that names the field.
use agent_api_proto::ParamError;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

/// The most events one `thread.history` page holds when the request names no `limit`.
pub const DEFAULT_HISTORY_LIMIT: u32 = 200;

pub(crate) fn read<T: DeserializeOwned>(params: &Value) -> Result<T, ParamError> {
    let params = if params.is_null() { json!({}) } else { params.clone() };
    serde_json::from_value(params).map_err(|e| ParamError(e.to_string()))
}

pub(crate) fn required(name: &str, value: &str) -> Result<(), ParamError> {
    if value.is_empty() {
        return Err(ParamError(format!("{name} is required")));
    }
    Ok(())
}

pub(crate) fn not_empty(name: &str, value: &Option<String>) -> Result<(), ParamError> {
    match value {
        Some(text) if text.is_empty() => Err(ParamError(format!("{name} must not be empty"))),
        _ => Ok(()),
    }
}

pub(crate) fn to_value<T: Serialize>(params: &T) -> Value {
    serde_json::to_value(params).unwrap_or_else(|_| json!({}))
}

/// `thread.create`: a thread with no turn yet. `thread` is the id the caller chose (the service names the thread when it is left out; a create
/// for an id that exists answers that thread unchanged, so a retry is safe). `workspace`, `driver` and `model` are defaults for its turns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl CreateParams {
    pub fn parse(params: &Value) -> Result<CreateParams, ParamError> {
        let parsed: CreateParams = read(params)?;
        for (name, value) in [("thread", &parsed.thread), ("title", &parsed.title), ("workspace", &parsed.workspace), ("driver", &parsed.driver), ("model", &parsed.model)] {
            not_empty(name, value)?;
        }
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `thread.list`: every thread, newest activity first. It takes no param.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {}

impl ListParams {
    pub fn parse(params: &Value) -> Result<ListParams, ParamError> {
        read(params)
    }

    pub fn to_value(&self) -> Value {
        json!({})
    }
}

/// `thread.get`: one thread.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetParams {
    #[serde(default)]
    pub thread: String,
}

impl GetParams {
    pub fn parse(params: &Value) -> Result<GetParams, ParamError> {
        let parsed: GetParams = read(params)?;
        required("thread", &parsed.thread)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `thread.subscribe`: follow a thread's events from the one after `after_seq` (0 for all of them): the service sends what happened since,
/// in order, then each new event as it happens.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscribeParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub after_seq: u64,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        let parsed: SubscribeParams = read(params)?;
        required("thread", &parsed.thread)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `thread.history`: a page of the events after `after_seq`, oldest first, at most `limit` of them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub after_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl HistoryParams {
    pub fn parse(params: &Value) -> Result<HistoryParams, ParamError> {
        let parsed: HistoryParams = read(params)?;
        required("thread", &parsed.thread)?;
        if parsed.limit == Some(0) {
            return Err(ParamError("limit must be at least 1".into()));
        }
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }

    /// The page size the request asks for.
    pub fn page(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_HISTORY_LIMIT)
    }
}

/// `thread.transcript`: the thread's settled items, read as the rows a person sees.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptParams {
    #[serde(default)]
    pub thread: String,
}

impl TranscriptParams {
    pub fn parse(params: &Value) -> Result<TranscriptParams, ParamError> {
        let parsed: TranscriptParams = read(params)?;
        required("thread", &parsed.thread)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `turn.start`: run `prompt` on `thread` with `driver` in `workspace` (whose executor must be attached). `runtime_mode` and `interaction_mode`
/// pass to the driver as its permission mode (`approval-required`, `auto-accept-edits`, `full-access`; `default`, `plan`). `attachments` are
/// ids from the attachments group, which no service lists yet: a turn that names one is refused until it does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_mode: Option<String>,
}

impl StartParams {
    pub fn parse(params: &Value) -> Result<StartParams, ParamError> {
        let parsed: StartParams = read(params)?;
        for (name, value) in [("thread", &parsed.thread), ("workspace", &parsed.workspace), ("driver", &parsed.driver), ("prompt", &parsed.prompt)] {
            required(name, value)?;
        }
        for (name, value) in [("model", &parsed.model), ("runtime_mode", &parsed.runtime_mode), ("interaction_mode", &parsed.interaction_mode)] {
            not_empty(name, value)?;
        }
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `turn.interrupt`: stop the running turn of `thread`; `turn` names the one expected to be running, so a late interrupt cannot stop the next.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterruptParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
}

impl InterruptParams {
    pub fn parse(params: &Value) -> Result<InterruptParams, ParamError> {
        let parsed: InterruptParams = read(params)?;
        required("thread", &parsed.thread)?;
        not_empty("turn", &parsed.turn)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_params_with_a_field_the_method_does_not_have_when_read_then_each_method_refuses_it_naming_the_field() {
        // Given params that add a field `colour` to what each method takes
        let with = |base: Value| {
            let mut params = base;
            params["colour"] = json!("red");
            params
        };
        let cases: [(&str, Result<(), ParamError>); 8] = [
            ("create", CreateParams::parse(&with(json!({}))).map(drop)),
            ("list", ListParams::parse(&with(json!({}))).map(drop)),
            ("get", GetParams::parse(&with(json!({"thread": "t"}))).map(drop)),
            ("subscribe", SubscribeParams::parse(&with(json!({"thread": "t"}))).map(drop)),
            ("history", HistoryParams::parse(&with(json!({"thread": "t"}))).map(drop)),
            ("transcript", TranscriptParams::parse(&with(json!({"thread": "t"}))).map(drop)),
            ("start", StartParams::parse(&with(json!({"thread": "t", "workspace": "w", "driver": "codex", "prompt": "p"}))).map(drop)),
            ("interrupt", InterruptParams::parse(&with(json!({"thread": "t"}))).map(drop)),
        ];
        for (method, parsed) in cases {
            // When each is read, Then it is refused, and the message names the field
            let refused = parsed.expect_err(method);
            assert!(refused.0.contains("unknown field `colour`"), "{method}: {}", refused.0);
        }
    }

    #[test]
    fn given_params_missing_a_required_field_or_giving_it_empty_when_read_then_the_message_says_which_is_required() {
        // Given a start that lacks each required field in turn, and a get with an empty thread
        let full = json!({"thread": "t", "workspace": "w", "driver": "codex", "prompt": "p"});
        for field in ["thread", "workspace", "driver", "prompt"] {
            let mut partial = full.clone();
            partial.as_object_mut().unwrap().remove(field);
            // When read, Then each is refused with `<field> is required`
            assert_eq!(StartParams::parse(&partial).unwrap_err().0, format!("{field} is required"));
        }
        assert_eq!(GetParams::parse(&json!({"thread": ""})).unwrap_err().0, "thread is required");
        // And an optional field that is given empty is refused, one that is left out is not
        assert_eq!(StartParams::parse(&json!({"thread": "t", "workspace": "w", "driver": "d", "prompt": "p", "model": ""})).unwrap_err().0, "model must not be empty");
        assert_eq!(CreateParams::parse(&json!({"title": ""})).unwrap_err().0, "title must not be empty");
        assert!(CreateParams::parse(&json!({})).is_ok() && CreateParams::parse(&Value::Null).is_ok());
    }

    #[test]
    fn given_fields_of_the_wrong_kind_when_read_then_they_are_refused_and_a_negative_or_zero_bound_is_too() {
        // Given a subscribe whose after_seq is negative, then a string, a history whose limit is zero, and attachments that are not ids
        let cases = [
            SubscribeParams::parse(&json!({"thread": "t", "after_seq": -1})).map(drop),
            SubscribeParams::parse(&json!({"thread": "t", "after_seq": "3"})).map(drop),
            HistoryParams::parse(&json!({"thread": "t", "limit": 0})).map(drop),
            StartParams::parse(&json!({"thread": "t", "workspace": "w", "driver": "d", "prompt": "p", "attachments": [1]})).map(drop),
            GetParams::parse(&json!({"thread": 7})).map(drop),
        ];
        // When read, Then every one is refused
        assert!(cases.iter().all(Result::is_err), "{cases:?}");
        assert_eq!(HistoryParams::parse(&json!({"thread": "t", "limit": 0})).unwrap_err().0, "limit must be at least 1");
    }

    #[test]
    fn given_params_a_mapper_builds_when_encoded_and_read_back_then_they_are_the_same_and_left_out_fields_stay_out() {
        // Given a start with every field, a start with only the required ones, and a history with no limit
        let all = StartParams { thread: "t1".into(), workspace: "w1".into(), driver: "claudeAgent".into(), model: Some("opus".into()), prompt: "fix it".into(), attachments: Some(vec!["a1".into()]), runtime_mode: Some("auto-accept-edits".into()), interaction_mode: Some("plan".into()) };
        let minimal = StartParams { thread: "t1".into(), workspace: "w1".into(), driver: "codex".into(), prompt: "p".into(), ..StartParams::default() };
        let history = HistoryParams { thread: "t1".into(), after_seq: 4, limit: None };
        // When each is encoded and read back
        // Then they round-trip, and a field that is none is absent from the frame
        assert_eq!(StartParams::parse(&all.to_value()), Ok(all));
        assert_eq!(StartParams::parse(&minimal.to_value()), Ok(minimal.clone()));
        assert_eq!(minimal.to_value(), json!({"thread": "t1", "workspace": "w1", "driver": "codex", "prompt": "p"}));
        assert_eq!((HistoryParams::parse(&history.to_value()), history.page()), (Ok(history), DEFAULT_HISTORY_LIMIT));
        assert_eq!(SubscribeParams::parse(&json!({"thread": "t"})).unwrap().after_seq, 0);
    }
}
