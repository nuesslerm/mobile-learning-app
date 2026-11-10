//! The `agent.threads` sub-group of the agent API (`docs/plans/rca-parity/plan.md`, section 2, and the owner's method names in
//! `docs/plans/rca-client/architecture.md`): threads and turns of the agent CLIs, the methods that start, read and follow them, and the
//! neutral events a turn emits. `thread.*` is answered by the service from its engine log, `turn.start` and `turn.interrupt` drive a CLI.
//!
//! The sub-groups of M4 are modules of this crate with their own params, results, codes, events and fixtures: `requests` (`agent.requests`),
//! `queue` (`agent.queue`) and `lifecycle` (`agent.lifecycle`). Their events are other kinds on the same `thread/<id>` topic; the thirteen of
//! `events` are unchanged.
//!
//! - **Params** (`params`) refuse a field they do not name with `INVALID_PARAMS`, so a mapper's typo is an error and not a silent default.
//! - **Results** (`results`) are read by a client that ignores a field it does not know.
//! - **Events** (`events`) are the thirteen neutral ones, on the envelope of `agent-api-proto` (`{event: {seq, thread, turn, type, ...}}`);
//!   `seq` is the thread's engine sequence, and `thread.subscribe {thread, after_seq}` replays what a client missed before it follows.
//!   Not here: the `reset` event of a stream that cannot replay (a proposed addition, plan decision 7).
use agent_api_proto::{GroupName, Method};

pub mod events;
pub mod lifecycle;
pub mod params;
pub mod queue;
pub mod requests;
pub mod results;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod wire;

pub use events::{FileOp, FinishReason, OutputStream, Role, ThreadEvent, Usage};
pub use params::{CreateParams, GetParams, HistoryParams, InterruptParams, ListParams, StartParams, SubscribeParams, TranscriptParams};
pub use results::{HistoryResult, InterruptResult, ListResult, StartResult, Status, SubscribeResult, ThreadView, TranscriptItem, TranscriptResult};

pub const GROUP: GroupName = GroupName::new("agent.threads");

pub const THREAD_CREATE: Method = Method::new(GROUP, "thread.create");
pub const THREAD_LIST: Method = Method::new(GROUP, "thread.list");
pub const THREAD_GET: Method = Method::new(GROUP, "thread.get");
pub const THREAD_SUBSCRIBE: Method = Method::new(GROUP, "thread.subscribe");
pub const THREAD_HISTORY: Method = Method::new(GROUP, "thread.history");
pub const THREAD_TRANSCRIPT: Method = Method::new(GROUP, "thread.transcript");
pub const TURN_START: Method = Method::new(GROUP, "turn.start");
pub const TURN_INTERRUPT: Method = Method::new(GROUP, "turn.interrupt");

/// Every method of the group, the thread methods first. `turn.start` and `turn.interrupt` are the ones that drive a CLI.
pub const METHODS: &[Method] = &[THREAD_CREATE, THREAD_LIST, THREAD_GET, THREAD_SUBSCRIBE, THREAD_HISTORY, THREAD_TRANSCRIPT, TURN_START, TURN_INTERRUPT];

/// The error codes of the group. A malformed param is `INVALID_PARAMS` and a workspace with no executor is `NO_EXECUTOR`, both shared.
pub mod code {
    /// No thread has the id the request names.
    pub const THREAD_NOT_FOUND: &str = "THREAD_NOT_FOUND";
    /// `turn.start` on a thread whose turn is still running.
    pub const TURN_RUNNING: &str = "TURN_RUNNING";
    /// `turn.interrupt` on a thread with no running turn, or naming a turn that is not the running one.
    pub const NO_TURN: &str = "NO_TURN";
    /// `turn.start` naming a driver the service does not run.
    pub const UNKNOWN_DRIVER: &str = "UNKNOWN_DRIVER";
    /// The service could not read or write its thread log; nothing was changed that the answer does not say.
    pub const THREADS_UNAVAILABLE: &str = "THREADS_UNAVAILABLE";

    /// Every code the group answers with itself.
    pub const ALL: [&str; 5] = [THREAD_NOT_FOUND, TURN_RUNNING, NO_TURN, UNKNOWN_DRIVER, THREADS_UNAVAILABLE];
}

/// The message each code is answered with, so the service and the recorded exchanges say the same.
pub mod message {
    pub fn thread_not_found(thread: &str) -> String {
        format!("no thread has the id {thread}")
    }

    pub fn turn_running(thread: &str) -> String {
        format!("thread {thread} already has a running turn")
    }

    /// `turn` is the turn an interrupt named, when it named one.
    pub fn no_turn(thread: &str, turn: Option<&str>) -> String {
        match turn {
            Some(turn) => format!("thread {thread} is not running turn {turn}"),
            None => format!("thread {thread} has no running turn"),
        }
    }

    pub fn unknown_driver(driver: &str) -> String {
        format!("no driver named {driver}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{fixtures, thread_events};
    use agent_api_proto::{ParamError, Request};
    use serde_json::{json, Value};
    use std::collections::BTreeSet;

    fn parse_params(method: &str, params: &Value) -> Result<(), ParamError> {
        match method {
            "thread.create" => CreateParams::parse(params).map(drop),
            "thread.list" => ListParams::parse(params).map(drop),
            "thread.get" => GetParams::parse(params).map(drop),
            "thread.subscribe" => SubscribeParams::parse(params).map(drop),
            "thread.history" => HistoryParams::parse(params).map(drop),
            "thread.transcript" => TranscriptParams::parse(params).map(drop),
            "turn.start" => StartParams::parse(params).map(drop),
            "turn.interrupt" => InterruptParams::parse(params).map(drop),
            other => panic!("{other} is no method of the group"),
        }
    }

    fn parse_result(method: &str, result: &Value) -> Result<(), String> {
        let read = |r: Result<(), serde_json::Error>| r.map_err(|e| e.to_string());
        match method {
            "thread.create" | "thread.get" => ThreadView::from_result(result).map(drop).map_err(|e| e.0),
            "thread.list" => read(serde_json::from_value::<ListResult>(result.clone()).map(drop)),
            "thread.subscribe" => read(serde_json::from_value::<SubscribeResult>(result.clone()).map(drop)),
            "thread.history" => serde_json::from_value::<HistoryResult>(result.clone()).map_err(|e| e.to_string()).and_then(|r| r.events().map(drop).map_err(|e| e.0)),
            "thread.transcript" => read(serde_json::from_value::<TranscriptResult>(result.clone()).map(drop)),
            "turn.start" => read(serde_json::from_value::<StartResult>(result.clone()).map(drop)),
            "turn.interrupt" => read(serde_json::from_value::<InterruptResult>(result.clone()).map(drop)),
            other => panic!("{other} is no method of the group"),
        }
    }

    #[test]
    fn given_the_recorded_exchanges_when_checked_against_the_contract_then_every_method_has_a_success_and_a_refusal_and_each_says_what_the_contract_says() {
        // Given the recorded exchanges of the group
        let recorded = fixtures();
        // Then there is one file per method, no more
        let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
        named.sort_unstable();
        let mut methods: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        methods.sort_unstable();
        assert_eq!(named, methods);
        let codes: BTreeSet<&str> = code::ALL.iter().copied().chain(["INVALID_PARAMS", "NO_EXECUTOR"]).collect();
        for fixture in &recorded {
            let method = fixture.method.as_str();
            // And each has an exchange that succeeds and one that is refused
            assert!(fixture.exchanges.iter().any(|e| e.succeeds()) && fixture.exchanges.iter().any(|e| !e.succeeds()), "{method}");
            for exchange in &fixture.exchanges {
                let what = format!("{method}: {}", exchange.name);
                // And every request, and every request that sets it up, is a request of the group, and the service calls no executor action
                for request in exchange.before.iter().chain([&exchange.request]) {
                    let parsed = Request::from_value(request).unwrap_or_else(|_| panic!("{what}: not a request"));
                    assert!(METHODS.iter().any(|m| m.name() == parsed.method), "{what}: {}", parsed.method);
                }
                // (a turn that starts is the exception: the executor records the files first, with `git_checkpoint`)
                let expected: &[&str] = if method == "turn.start" && exchange.succeeds() { &["git_checkpoint"] } else { &[] };
                assert_eq!((exchange.request["method"].as_str(), exchange.actions.iter().map(String::as_str).collect::<Vec<_>>()), (Some(method), expected.to_vec()), "{what}");
                if exchange.succeeds() {
                    // And a success is a request the params accept and a result the type reads
                    assert_eq!(parse_params(method, &exchange.request["params"]), Ok(()), "{what}");
                    assert_eq!(parse_result(method, &exchange.response["result"]), Ok(()), "{what}");
                } else {
                    // And a refusal is one of the group's codes (or a shared one), with the message the contract gives it
                    let (code, text) = (exchange.response["error"]["code"].as_str().unwrap(), exchange.response["error"]["message"].as_str().unwrap());
                    assert!(codes.contains(code), "{what}: {code}");
                    let params = &exchange.request["params"];
                    let thread = params["thread"].as_str().unwrap_or_default();
                    match code {
                        "INVALID_PARAMS" => assert_eq!(parse_params(method, params).unwrap_err().0, text, "{what}"),
                        "THREAD_NOT_FOUND" => assert_eq!(text, message::thread_not_found(thread), "{what}"),
                        "TURN_RUNNING" => assert_eq!(text, message::turn_running(thread), "{what}"),
                        "NO_TURN" => assert_eq!(text, message::no_turn(thread, params["turn"].as_str()), "{what}"),
                        "UNKNOWN_DRIVER" => assert_eq!(text, message::unknown_driver(params["driver"].as_str().unwrap()), "{what}"),
                        _ => assert_eq!(text, format!("no executor is attached for workspace {}", params["workspace"].as_str().unwrap()), "{what}"),
                    }
                }
            }
        }
    }

    #[test]
    fn given_the_recorded_events_of_two_turns_when_read_then_every_kind_appears_in_seq_order_and_each_is_a_thread_event_that_writes_back_as_recorded() {
        // Given the recorded events of thread t1
        let events = thread_events();
        // Then the seq runs 1.. without a gap, every one of the thirteen kinds appears, and each is of thread t1
        assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), (1..=events.len() as u64).collect::<Vec<_>>());
        let kinds: BTreeSet<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, BTreeSet::from_iter(events::KINDS));
        assert!(events.iter().all(|e| e.topic().as_deref() == Some("thread/t1")));
        for event in &events {
            // And each reads as a thread event and writes back the frame it came from
            let read = ThreadEvent::from_event(event).unwrap_or_else(|e| panic!("seq {}: {e}", event.seq));
            assert_eq!(read.into_event("t1", event.turn.as_deref(), event.seq), *event, "seq {}", event.seq);
        }
        // And the transcript of them is what a person read: two turns, the second only an error
        let transcript = TranscriptResult::fold("t1", &events);
        assert_eq!((transcript.last_seq, transcript.items.len()), (20, 9));
        assert!(matches!(transcript.items.last(), Some(TranscriptItem::Error { code, .. }) if code == "RATE_LIMITED"));
        assert_eq!(json!(transcript.items[5]), json!({"kind": "tool", "seq": 10, "turn": "u1", "call": "c2", "tool": "run_command", "args": {"command": "npm test"}, "output": "1 failing\n", "ok": false, "result": {"exit_code": 1}}));
    }

    #[test]
    fn given_the_methods_of_the_group_when_listed_then_there_are_eight_of_this_group_with_the_owners_names() {
        // Given the group's methods
        // Then they are the owner's eight names, each of this group and none twice
        let names: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        assert_eq!(names, ["thread.create", "thread.list", "thread.get", "thread.subscribe", "thread.history", "thread.transcript", "turn.start", "turn.interrupt"]);
        assert!(METHODS.iter().all(|m| m.group() == GROUP) && GROUP.as_str() == "agent.threads");
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), METHODS.len());
    }
}
