//! The params of `agent.queue`.
use crate::params::{not_empty, read, required, to_value};
use agent_api_proto::ParamError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `queue.list`: the thread's queue, first to start first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    #[serde(default)]
    pub thread: String,
}

impl ListParams {
    pub fn parse(params: &Value) -> Result<ListParams, ParamError> {
        let parsed: ListParams = read(params)?;
        required("thread", &parsed.thread)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `queue.add`: `prompt` runs after the turns before it. `item` is the id the caller chose (a retry of an add that names an id already queued
/// answers that item unchanged, and the id is the id of the turn that runs it). The rest are what the turn runs with; each left out is the thread's
/// own default (`workspace`, `driver`, `model`) or the service's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_mode: Option<String>,
}

impl AddParams {
    pub fn parse(params: &Value) -> Result<AddParams, ParamError> {
        let parsed: AddParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("prompt", &parsed.prompt)?;
        for (name, value) in [("item", &parsed.item), ("workspace", &parsed.workspace), ("driver", &parsed.driver), ("model", &parsed.model), ("runtime_mode", &parsed.runtime_mode), ("interaction_mode", &parsed.interaction_mode)] {
            not_empty(name, value)?;
        }
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `queue.edit`: the new text of a queued prompt, which may not be empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub item: String,
    #[serde(default)]
    pub prompt: String,
}

impl EditParams {
    pub fn parse(params: &Value) -> Result<EditParams, ParamError> {
        let parsed: EditParams = read(params)?;
        for (name, value) in [("thread", &parsed.thread), ("item", &parsed.item), ("prompt", &parsed.prompt)] {
            required(name, value)?;
        }
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `queue.move`: `item` goes before the queued item `before`, or last when `before` is left out or null.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub item: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

impl MoveParams {
    pub fn parse(params: &Value) -> Result<MoveParams, ParamError> {
        let parsed: MoveParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("item", &parsed.item)?;
        not_empty("before", &parsed.before)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `queue.delete`: take an item out of the queue.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub item: String,
}

impl DeleteParams {
    pub fn parse(params: &Value) -> Result<DeleteParams, ParamError> {
        let parsed: DeleteParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("item", &parsed.item)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `queue.resume`: release a queue a forced stop held, so its first item starts when no turn runs. With `item`, that item is sent now: it goes
/// first and starts at once, which is only allowed while no turn runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

impl ResumeParams {
    pub fn parse(params: &Value) -> Result<ResumeParams, ParamError> {
        let parsed: ResumeParams = read(params)?;
        required("thread", &parsed.thread)?;
        not_empty("item", &parsed.item)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `turn.steer`: the running turn goes on with another prompt: `prompt`, or the prompt of the queued `item` (which leaves the queue). Exactly one of
/// the two. `turn` names the turn expected to run, so a late steer cannot reach the next.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteerParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

impl SteerParams {
    pub fn parse(params: &Value) -> Result<SteerParams, ParamError> {
        let parsed: SteerParams = read(params)?;
        required("thread", &parsed.thread)?;
        for (name, value) in [("turn", &parsed.turn), ("prompt", &parsed.prompt), ("item", &parsed.item)] {
            not_empty(name, value)?;
        }
        if parsed.prompt.is_some() == parsed.item.is_some() {
            return Err(ParamError("name exactly one of prompt and item".into()));
        }
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_params_that_are_incomplete_empty_or_name_a_field_the_method_lacks_when_read_then_each_is_refused_saying_which() {
        // Given an add with no prompt, an add with an empty id, an edit that empties the prompt, a move that goes before nothing named, and a field no method has
        let cases = [
            (AddParams::parse(&json!({"thread": "t"})).map(drop), "prompt is required"),
            (AddParams::parse(&json!({"thread": "t", "prompt": "p", "item": ""})).map(drop), "item must not be empty"),
            (EditParams::parse(&json!({"thread": "t", "item": "i", "prompt": ""})).map(drop), "prompt is required"),
            (MoveParams::parse(&json!({"thread": "t", "item": "i", "before": ""})).map(drop), "before must not be empty"),
            (DeleteParams::parse(&json!({"thread": "t", "item": "i", "colour": 1})).map(drop), "unknown field `colour`"),
            (ResumeParams::parse(&json!({})).map(drop), "thread is required"),
        ];
        // When read, Then each is refused and the message names what is wrong
        for (parsed, says) in cases {
            let refused = parsed.unwrap_err();
            assert!(refused.0.contains(says), "{says}: {}", refused.0);
        }
    }

    #[test]
    fn given_a_steer_when_it_names_both_or_neither_of_prompt_and_item_then_it_is_refused_and_one_of_them_is_accepted() {
        // Given steers that name both, neither, a prompt only and an item only
        let both = SteerParams::parse(&json!({"thread": "t", "prompt": "p", "item": "i"}));
        let neither = SteerParams::parse(&json!({"thread": "t"}));
        let prompt = SteerParams::parse(&json!({"thread": "t", "turn": "u1", "prompt": "p"}));
        let item = SteerParams::parse(&json!({"thread": "t", "item": "i"}));
        // Then the first two are refused saying so, and the others are accepted
        assert_eq!((both.unwrap_err().0, neither.unwrap_err().0), ("name exactly one of prompt and item".to_string(), "name exactly one of prompt and item".to_string()));
        assert!(prompt.is_ok() && item.is_ok());
    }

    #[test]
    fn given_params_a_mapper_builds_when_encoded_and_read_back_then_they_are_the_same_and_left_out_fields_stay_out() {
        // Given an add with every field, a move to the end and a resume of one item
        let add = AddParams { thread: "t".into(), item: Some("i1".into()), prompt: "p".into(), attachments: Some(vec!["a".into()]), workspace: Some("w".into()), driver: Some("claudeAgent".into()), model: Some("opus".into()), runtime_mode: Some("full-access".into()), interaction_mode: Some("plan".into()) };
        let last = MoveParams { thread: "t".into(), item: "i1".into(), before: None };
        // When each is encoded and read back, Then it is the same and a field that is none is absent
        assert_eq!(AddParams::parse(&add.to_value()), Ok(add));
        assert_eq!(MoveParams::parse(&last.to_value()), Ok(last.clone()));
        assert_eq!(last.to_value(), json!({"thread": "t", "item": "i1"}));
        assert_eq!(ResumeParams::parse(&ResumeParams { thread: "t".into(), item: Some("i".into()) }.to_value()).unwrap().item.as_deref(), Some("i"));
    }
}
