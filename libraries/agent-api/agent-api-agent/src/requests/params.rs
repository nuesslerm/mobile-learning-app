//! The params of `agent.requests`. A field a method does not name, or one that is left out and required, is refused `INVALID_PARAMS`.
use crate::params::{read, required, to_value};
use agent_api_proto::ParamError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How a person answered a tool the turn asked to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Accept,
    /// Accept this and every later tool of the same kind in the thread's session.
    AcceptForSession,
    Decline,
    /// Decline and end the turn.
    Cancel,
}

/// `approval.answer`: the answer to the `approval.requested` event whose `approval` id this is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalAnswerParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub approval: String,
    pub decision: Decision,
}

impl ApprovalAnswerParams {
    pub fn parse(params: &Value) -> Result<ApprovalAnswerParams, ParamError> {
        let parsed: ApprovalAnswerParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("approval", &parsed.approval)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `input.answer`: the text a person gave to the `input.requested` event whose `request` id this is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputAnswerParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub request: String,
    pub answer: String,
}

impl InputAnswerParams {
    pub fn parse(params: &Value) -> Result<InputAnswerParams, ParamError> {
        let parsed: InputAnswerParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("request", &parsed.request)?;
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        to_value(self)
    }
}

/// `input.dismiss`: close a question without answering it. Only a question can be dismissed; an approval needs its answer or the turn's stop.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputDismissParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub request: String,
}

impl InputDismissParams {
    pub fn parse(params: &Value) -> Result<InputDismissParams, ParamError> {
        let parsed: InputDismissParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("request", &parsed.request)?;
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
    fn given_params_that_are_incomplete_or_name_a_field_the_method_lacks_when_read_then_each_is_refused_saying_which() {
        // Given an answer with no decision, one with an unknown decision, a dismiss with no request and an answer with an extra field
        let cases = [
            (ApprovalAnswerParams::parse(&json!({"thread": "t", "approval": "a"})).map(drop), "missing field `decision`"),
            (ApprovalAnswerParams::parse(&json!({"thread": "t", "approval": "a", "decision": "maybe"})).map(drop), "unknown variant `maybe`"),
            (InputDismissParams::parse(&json!({"thread": "t"})).map(drop), "request is required"),
            (InputAnswerParams::parse(&json!({"thread": "t", "request": "r", "answer": "x", "colour": 1})).map(drop), "unknown field `colour`"),
        ];
        // When read, Then each is refused and the message names what is wrong
        for (parsed, says) in cases {
            let refused = parsed.unwrap_err();
            assert!(refused.0.contains(says), "{says}: {}", refused.0);
        }
    }

    #[test]
    fn given_the_four_decisions_when_encoded_and_read_back_then_they_are_the_same_and_written_in_snake_case() {
        for (decision, written) in [(Decision::Accept, "accept"), (Decision::AcceptForSession, "accept_for_session"), (Decision::Decline, "decline"), (Decision::Cancel, "cancel")] {
            // Given an answer with the decision, When encoded and read back, Then it is the same and the decision is written in snake_case
            let params = ApprovalAnswerParams { thread: "t".into(), approval: "a".into(), decision };
            assert_eq!(params.to_value()["decision"], json!(written));
            assert_eq!(ApprovalAnswerParams::parse(&params.to_value()), Ok(params));
        }
        // And an empty answer text is an answer
        assert!(InputAnswerParams::parse(&json!({"thread": "t", "request": "r", "answer": ""})).is_ok());
    }
}
