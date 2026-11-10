//! The `agent.requests` sub-group (plan M4): the answers to what a running turn asks for. A turn that wants to run a tool waits for
//! `approval.answer`, one that asks the person a question waits for `input.answer` or `input.dismiss`. The asking is a thread event of
//! `agent.threads` (`approval.requested`, `input.requested`); the answering is recorded as an event of this group on the same `thread/<id>` topic.
use agent_api_proto::{GroupName, Method};

pub mod events;
pub mod params;
pub mod results;

pub use events::RequestEvent;
pub use params::{ApprovalAnswerParams, Decision, InputAnswerParams, InputDismissParams};
pub use results::{AnswerResult, ApprovalAnswerResult};

pub const GROUP: GroupName = GroupName::new("agent.requests");

pub const APPROVAL_ANSWER: Method = Method::new(GROUP, "approval.answer");
pub const INPUT_ANSWER: Method = Method::new(GROUP, "input.answer");
pub const INPUT_DISMISS: Method = Method::new(GROUP, "input.dismiss");

pub const METHODS: &[Method] = &[APPROVAL_ANSWER, INPUT_ANSWER, INPUT_DISMISS];

/// The codes of the group. `THREAD_NOT_FOUND` and `INVALID_PARAMS` are the shared ones.
pub mod code {
    /// The request is not open on the thread: no such id, or it was answered or dismissed already.
    pub const REQUEST_NOT_OPEN: &str = "REQUEST_NOT_OPEN";
    /// The method does not fit the request: an approval answered as a question, a question answered as an approval, or an approval dismissed.
    pub const WRONG_REQUEST_KIND: &str = "WRONG_REQUEST_KIND";

    pub const ALL: [&str; 2] = [REQUEST_NOT_OPEN, WRONG_REQUEST_KIND];
}

pub mod message {
    pub fn request_not_open(thread: &str, request: &str) -> String {
        format!("request {request} is not open on thread {thread}")
    }

    /// `method` is what was asked; `kind` is what the request is (`approval` or `question`).
    pub fn wrong_kind(method: &str, request: &str, kind: &str) -> String {
        match (method, kind) {
            ("input.dismiss", "approval") => format!("approval {request} needs an answer: answer it or stop the turn"),
            _ => format!("request {request} is {kind}, which {method} does not answer"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::group_fixtures;
    use serde_json::Value;

    #[test]
    fn given_the_recorded_exchanges_when_checked_against_the_contract_then_each_method_has_one_and_every_success_and_refusal_says_what_the_contract_says() {
        // Given the recorded exchanges of the group
        let recorded = group_fixtures("requests");
        // Then there is one file per method, and every request is of the group
        let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
        named.sort_unstable();
        let mut methods: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        methods.sort_unstable();
        assert_eq!(named, methods);
        for exchange in recorded.iter().flat_map(|f| &f.exchanges) {
            let method = exchange.request["method"].as_str().unwrap();
            let params: &Value = &exchange.request["params"];
            if exchange.succeeds() {
                // And a success is a request the params accept
                let parsed = match method {
                    "approval.answer" => ApprovalAnswerParams::parse(params).map(drop),
                    "input.answer" => InputAnswerParams::parse(params).map(drop),
                    _ => InputDismissParams::parse(params).map(drop),
                };
                assert_eq!(parsed, Ok(()), "{}", exchange.name);
            } else {
                // And a refusal is one of the group's codes, a shared one, or THREAD_NOT_FOUND, with the contract's message
                let (code, text) = (exchange.response["error"]["code"].as_str().unwrap(), exchange.response["error"]["message"].as_str().unwrap());
                let (thread, request) = (params["thread"].as_str().unwrap_or_default(), params["approval"].as_str().or(params["request"].as_str()).unwrap_or_default());
                match code {
                    "REQUEST_NOT_OPEN" => assert_eq!(text, message::request_not_open(thread, request), "{}", exchange.name),
                    "WRONG_REQUEST_KIND" => assert!(text.contains(request), "{}", exchange.name),
                    "THREAD_NOT_FOUND" => assert_eq!(text, crate::message::thread_not_found(thread), "{}", exchange.name),
                    "INVALID_PARAMS" => {
                        let parsed = match method {
                            "approval.answer" => ApprovalAnswerParams::parse(params).map(drop),
                            "input.answer" => InputAnswerParams::parse(params).map(drop),
                            _ => InputDismissParams::parse(params).map(drop),
                        };
                        assert_eq!(parsed.unwrap_err().0, text, "{}", exchange.name);
                    }
                    other => panic!("{}: {other}", exchange.name),
                }
            }
        }
    }
}
