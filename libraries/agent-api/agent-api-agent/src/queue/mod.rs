//! The `agent.queue` sub-group (plan M4): prompts that wait for the thread's turn to end, and `turn.steer`, which changes the prompt of the turn
//! that runs. The CLIs run one prompt and exit, so a steer ends the CLI that runs and runs a new one in the same turn with the steer prompt, and a
//! queued prompt starts by itself when a turn ends, as a turn whose id is the item's id. A forced stop holds the queue until `queue.resume`.
use agent_api_proto::{GroupName, Method};

pub mod events;
pub mod params;
pub mod results;

pub use events::QueueEvent;
pub use params::{AddParams, DeleteParams, EditParams, ListParams, MoveParams, ResumeParams, SteerParams};
pub use results::{AddResult, DeleteResult, EditResult, ListResult, MoveResult, QueueItem, ResumeResult, SteerResult};

pub const GROUP: GroupName = GroupName::new("agent.queue");

pub const QUEUE_LIST: Method = Method::new(GROUP, "queue.list");
pub const QUEUE_ADD: Method = Method::new(GROUP, "queue.add");
pub const QUEUE_EDIT: Method = Method::new(GROUP, "queue.edit");
pub const QUEUE_MOVE: Method = Method::new(GROUP, "queue.move");
pub const QUEUE_DELETE: Method = Method::new(GROUP, "queue.delete");
pub const QUEUE_RESUME: Method = Method::new(GROUP, "queue.resume");
pub const TURN_STEER: Method = Method::new(GROUP, "turn.steer");

pub const METHODS: &[Method] = &[QUEUE_LIST, QUEUE_ADD, QUEUE_EDIT, QUEUE_MOVE, QUEUE_DELETE, QUEUE_RESUME, TURN_STEER];

/// The codes of the group. `THREAD_NOT_FOUND`, `NO_TURN` (a steer with no turn running) and `INVALID_PARAMS` are the shared ones.
pub mod code {
    /// The item is not in the thread's queue (it started, was deleted, or never was).
    pub const ITEM_NOT_QUEUED: &str = "ITEM_NOT_QUEUED";
    /// The thread is archived or deleted, so nothing of its queue starts.
    pub const THREAD_NOT_ACTIVE: &str = "THREAD_NOT_ACTIVE";
    /// `queue.resume` naming an item while a turn runs: a queued prompt is sent now only while the thread is stopped.
    pub const NOT_STOPPED: &str = "NOT_STOPPED";
    /// `queue.add` for a thread with no workspace or driver of its own, naming none.
    pub const NO_DEFAULTS: &str = "NO_DEFAULTS";
    /// The queue's first item was started while a forced stop still holds the queue.
    pub const QUEUE_HELD: &str = "QUEUE_HELD";

    pub const ALL: [&str; 5] = [ITEM_NOT_QUEUED, THREAD_NOT_ACTIVE, NOT_STOPPED, NO_DEFAULTS, QUEUE_HELD];
}

pub mod message {
    pub fn item_not_queued(item: &str) -> String {
        format!("Run {item} is not queued.")
    }

    /// The item `queue.move` was asked to go before.
    pub fn target_not_queued(item: &str) -> String {
        format!("Queue target {item} is not queued.")
    }

    pub fn thread_not_active(thread: &str) -> String {
        format!("Thread {thread} is not active.")
    }

    pub const NOT_STOPPED: &str = "A queued user message can only be sent now while the thread is stopped.";

    pub fn no_defaults(thread: &str) -> String {
        format!("thread {thread} has no workspace or driver and the queued prompt names none")
    }

    pub fn queue_held(thread: &str) -> String {
        format!("the queue of thread {thread} is held by a forced stop: resume it first")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::group_fixtures;
    use agent_api_proto::ParamError;
    use serde_json::Value;

    fn parse_params(method: &str, params: &Value) -> Result<(), ParamError> {
        match method {
            "queue.list" => ListParams::parse(params).map(drop),
            "queue.add" => AddParams::parse(params).map(drop),
            "queue.edit" => EditParams::parse(params).map(drop),
            "queue.move" => MoveParams::parse(params).map(drop),
            "queue.delete" => DeleteParams::parse(params).map(drop),
            "queue.resume" => ResumeParams::parse(params).map(drop),
            "turn.steer" => SteerParams::parse(params).map(drop),
            other => panic!("{other} is no method of the group"),
        }
    }

    fn parse_result(method: &str, result: &Value) -> Result<(), String> {
        fn read<T: serde::de::DeserializeOwned>(result: &Value) -> Result<(), String> {
            serde_json::from_value::<T>(result.clone()).map(drop).map_err(|e| e.to_string())
        }
        match method {
            "queue.list" => read::<ListResult>(result),
            "queue.add" => read::<AddResult>(result),
            "queue.edit" => read::<EditResult>(result),
            "queue.move" => read::<MoveResult>(result),
            "queue.delete" => read::<DeleteResult>(result),
            "queue.resume" => read::<ResumeResult>(result),
            "turn.steer" => read::<SteerResult>(result),
            other => panic!("{other} is no method of the group"),
        }
    }

    #[test]
    fn given_the_recorded_exchanges_when_checked_against_the_contract_then_each_method_has_a_refusal_and_a_success_where_no_turn_is_needed_and_each_says_what_the_contract_says() {
        // Given the recorded exchanges of the group
        let recorded = group_fixtures("queue");
        // Then there is one file per method
        let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
        named.sort_unstable();
        let mut methods: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        methods.sort_unstable();
        assert_eq!(named, methods);
        for fixture in &recorded {
            // And each has a refusal, and a success unless it needs a running turn, which no request of the API can start without a CLI
            assert!(fixture.exchanges.iter().any(|e| !e.succeeds()), "{}", fixture.method);
            assert!(fixture.method == "turn.steer" || fixture.exchanges.iter().any(|e| e.succeeds()), "{}", fixture.method);
            for exchange in &fixture.exchanges {
                let params: &Value = &exchange.request["params"];
                let what = format!("{}: {}", fixture.method, exchange.name);
                if exchange.succeeds() {
                    // And a success is a request the params accept and a result the type reads
                    assert_eq!((parse_params(&fixture.method, params), parse_result(&fixture.method, &exchange.response["result"])), (Ok(()), Ok(())), "{what}");
                } else {
                    // And a refusal is one of the group's codes or a shared one, with the contract's message
                    let (code, text) = (exchange.response["error"]["code"].as_str().unwrap(), exchange.response["error"]["message"].as_str().unwrap());
                    let thread = params["thread"].as_str().unwrap_or_default();
                    let item = params["item"].as_str().unwrap_or_default();
                    match code {
                        "ITEM_NOT_QUEUED" if text.starts_with("Queue target") => assert_eq!(text, message::target_not_queued(params["before"].as_str().unwrap()), "{what}"),
                        "ITEM_NOT_QUEUED" => assert_eq!(text, message::item_not_queued(item), "{what}"),
                        "THREAD_NOT_ACTIVE" => assert_eq!(text, message::thread_not_active(thread), "{what}"),
                        "NOT_STOPPED" => assert_eq!(text, message::NOT_STOPPED, "{what}"),
                        "NO_DEFAULTS" => assert_eq!(text, message::no_defaults(thread), "{what}"),
                        "QUEUE_HELD" => assert_eq!(text, message::queue_held(thread), "{what}"),
                        "THREAD_NOT_FOUND" => assert_eq!(text, crate::message::thread_not_found(thread), "{what}"),
                        "NO_TURN" => assert_eq!(text, crate::message::no_turn(thread, params["turn"].as_str()), "{what}"),
                        "INVALID_PARAMS" => assert_eq!(parse_params(&fixture.method, params).unwrap_err().0, text, "{what}"),
                        other => panic!("{what}: {other}"),
                    }
                }
            }
        }
    }
}
