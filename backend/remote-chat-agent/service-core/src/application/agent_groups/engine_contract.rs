//! The thread engine's lifecycle events and refusals against the contract (`agent-api-agent`'s `requests`, `queue` and `lifecycle` modules). The
//! engine (`remote-chat-agent-core`) builds its neutral events as JSON and words its refusals itself, because a core crate depends on no contract;
//! this is where the two meet: every event a scenario makes the engine record is read as the contract's event and written back as recorded, every
//! kind the contract names is among them, and every message the engine gives is the contract's.

use super::engine_threads::wire;
use agent_api_agent::lifecycle::{self, LifecycleEvent};
use agent_api_agent::queue::{self, QueueEvent};
use agent_api_agent::requests::{self, RequestEvent};
use agent_api_agent::ThreadEvent;
use remote_chat_agent_core::application::engine::Engine;
use remote_chat_agent_core::application::store::MemoryStore;
use remote_chat_agent_core::domain::lifecycle::{msg, SettleTo};
use remote_chat_agent_core::domain::types::{Body, Command, Receipt, Session};
use remote_chat_agent_core::testing::{CountingIds, FixedClock};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

const NOW: u64 = 1_700_000_000_000;

/// What a refusal says: its code and message.
type Refusal = (String, String);
/// What the contract says of it.
type Expected = (&'static str, String);

fn engine() -> Engine {
    Engine::new(Box::new(MemoryStore::default()), "/work".into(), Arc::new(FixedClock(NOW)), Arc::new(CountingIds::default()))
}

fn ask(engine: &Engine, thread: &str, body: Body) -> Receipt {
    engine.dispatch(Command { command_id: "c".into(), thread_id: thread.into(), body }, "agent-api").unwrap()
}

/// A command the scenario expects the engine to accept.
fn ok(engine: &Engine, thread: &str, body: Body) {
    let receipt = ask(engine, thread, body);
    assert!(receipt.accepted, "{thread}: {:?}", receipt.error);
}

fn say(engine: &Engine, thread: &str, turn: Option<&str>, event: Value) {
    ok(engine, thread, Body::NeutralEvent { turn_id: turn.map(String::from), event });
}

fn thread(engine: &Engine, id: &str) {
    ok(engine, id, Body::NeutralCreate { title: format!("title of {id}"), workspace: Some("w1".into()), driver: Some("claudeAgent".into()), model: None });
}

fn turn(engine: &Engine, thread: &str, id: &str, prompt: &str, reply: &str, end: Option<&str>) {
    say(engine, thread, Some(id), json!({"type": "turn.started"}));
    say(engine, thread, Some(id), json!({"type": "message.delta", "role": "user", "text": prompt}));
    say(engine, thread, Some(id), json!({"type": "message.delta", "role": "assistant", "text": reply}));
    if let Some(reason) = end {
        say(engine, thread, Some(id), json!({"type": "turn.finished", "reason": reason}));
    }
}

fn add(item: &str, prompt: &str) -> Body {
    Body::QueueAdd { item: Some(item.into()), prompt: prompt.into(), attachments: vec![], workspace: None, driver: None, model: None, runtime_mode: None, interaction_mode: None }
}

/// Drives every command of the three sub-groups through the engine, the refusals of a few of them too, and answers the threads that were used.
fn scenario(engine: &Engine) -> Vec<&'static str> {
    // A thread that asks and is answered, queues, steers, is force stopped and resumed
    thread(engine, "asker");
    turn(engine, "asker", "u1", "work", "…", None);
    for event in [json!({"type": "approval.requested", "approval": "ap1", "tool": "run_command", "args": {}}), json!({"type": "input.requested", "request": "rq1", "prompt": "Which?"}), json!({"type": "input.requested", "request": "rq2", "prompt": "And?"})] {
        say(engine, "asker", Some("u1"), event);
    }
    ok(engine, "asker", Body::ApprovalAnswer { approval: "ap1".into(), decision: "accept".into() });
    ok(engine, "asker", Body::InputAnswer { request: "rq1".into(), answer: "the second".into() });
    ok(engine, "asker", Body::InputDismiss { request: "rq2".into() });
    for body in [add("q1", "one"), add("q2", "two"), Body::QueueEdit { item: "q1".into(), prompt: "better".into() }, Body::QueueMove { item: "q2".into(), before: Some("q1".into()) }, Body::QueueDelete { item: "q1".into() }, add("q3", "three")] {
        ok(engine, "asker", body);
    }
    ok(engine, "asker", Body::Steer { turn: Some("u1".into()), prompt: None, item: Some("q2".into()) });
    ok(engine, "asker", Body::Steer { turn: None, prompt: Some("go left".into()), item: None });
    ok(engine, "asker", Body::ForceStop { turn: Some("u1".into()) });
    say(engine, "asker", Some("u1"), json!({"type": "turn.finished", "reason": "interrupted"}));
    ok(engine, "asker", Body::QueueResume { item: Some("q3".into()) });
    ok(engine, "asker", Body::QueueStart { item: "q3".into() });
    // A thread that runs a workflow, one whose turn failed and is retried, and one that is settled, snoozed, woken, archived, brought back, deleted
    thread(engine, "worker");
    turn(engine, "worker", "w1", "work", "…", None);
    ok(engine, "worker", Body::WorkflowStop { turn: "w1".into(), workflow: Some("agent-7".into()) });
    thread(engine, "failing");
    turn(engine, "failing", "f1", "try", "oops", Some("failed"));
    ok(engine, "failing", Body::Retry { turn: "f1".into(), new_turn: "f2".into() });
    turn(engine, "failing", "f2", "try", "fine", Some("completed"));
    thread(engine, "resting");
    for to in [SettleTo::Settled, SettleTo::Active] {
        ok(engine, "resting", Body::SettleSet { to, until_ms: None, at_ms: None });
    }
    ok(engine, "resting", Body::SettleSet { to: SettleTo::Snoozed, until_ms: Some(NOW + 60_000), at_ms: None });
    ok(engine, "resting", Body::SettleSet { to: SettleTo::Awake, until_ms: None, at_ms: None });
    ok(engine, "resting", Body::LifecycleArchive { archive: true });
    ok(engine, "resting", Body::LifecycleArchive { archive: false });
    thread(engine, "doomed");
    ok(engine, "doomed", Body::LifecycleDelete { only_if_unused: false });
    // A parent with two turns and a provider session, forked, merged back, consumed, detached and rolled back
    thread(engine, "parent");
    turn(engine, "parent", "p1", "start", "started", Some("completed"));
    turn(engine, "parent", "p2", "more", "moved", Some("completed"));
    ok(engine, "parent", Body::SessionSet { session: Session { cli: "claudeAgent".into(), provider_session_id: Some("s1".into()), status: "ready".into(), active_turn_id: None, last_error: None } });
    ok(engine, "kid", Body::Fork { source: "parent".into(), at_turn: Some("p1".into()), title: None });
    turn(engine, "kid", "k1", "explore", "found it", Some("completed"));
    ok(engine, "parent", Body::MergeBack { source: "kid".into(), turn: None });
    say(engine, "parent", None, json!({"type": "thread.merge_consumed", "source": "kid"}));
    ok(engine, "parent", Body::Detach { session: "s1".into(), reason: Some("stale".into()) });
    ok(engine, "parent", Body::Rollback { turn: "p1".into(), restore_files: false });
    vec!["asker", "worker", "failing", "resting", "doomed", "parent", "kid"]
}

#[test]
fn given_a_scenario_that_uses_every_command_when_the_engines_events_are_read_as_the_contracts_then_each_reads_and_writes_back_as_recorded_and_every_kind_of_the_three_groups_was_made() {
    // Given the engine after a scenario of every command of agent.requests, agent.queue and agent.lifecycle
    let engine = engine();
    let threads = scenario(&engine);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for id in threads {
        let (events, _) = engine.neutral_events(id, 0).unwrap().expect("a thread of the agent API");
        for event in events.iter().filter_map(wire) {
            // When each wire event is read as the event of the group its kind belongs to
            let (thread, turn, seq) = (event.thread.clone().unwrap(), event.turn.clone(), event.seq);
            let written_back = if requests::events::KINDS.contains(&event.kind.as_str()) {
                RequestEvent::from_event(&event).map(|e| e.into_event(&thread, turn.as_deref(), seq))
            } else if queue::events::KINDS.contains(&event.kind.as_str()) {
                QueueEvent::from_event(&event).map(|e| e.into_event(&thread, turn.as_deref(), seq))
            } else if lifecycle::events::KINDS.contains(&event.kind.as_str()) {
                LifecycleEvent::from_event(&event).map(|e| e.into_event(&thread, turn.as_deref(), seq))
            } else {
                ThreadEvent::from_event(&event).map(|e| e.into_event(&thread, turn.as_deref(), seq))
            };
            // Then it is the contract's event and writes back as the engine recorded it
            assert_eq!(written_back.as_ref(), Ok(&event), "{id} seq {seq}: {}", event.kind);
            seen.insert(event.kind.clone());
        }
    }
    // And the scenario made every kind of the three groups
    let named: BTreeSet<String> = requests::events::KINDS.iter().chain(queue::events::KINDS.iter()).chain(lifecycle::events::KINDS.iter()).map(|k| k.to_string()).collect();
    let missing: Vec<&String> = named.difference(&seen).collect();
    assert!(missing.is_empty(), "the engine never made: {missing:?}");
}

#[test]
fn given_the_refusals_the_engine_makes_when_compared_with_the_contract_then_each_code_is_the_groups_or_a_shared_one_and_each_message_is_the_contracts() {
    // Given the engine after the scenario, and refusals asked of its threads
    let engine = engine();
    scenario(&engine);
    ok(&engine, "worker", add("qa", "x"));
    let refused = |thread: &str, body: Body| ask(&engine, thread, body).error.expect("refused");
    let cases: Vec<(Refusal, Expected)> = vec![
        (refused("asker", Body::ApprovalAnswer { approval: "nope".into(), decision: "accept".into() }), ("REQUEST_NOT_OPEN", requests::message::request_not_open("asker", "nope"))),
        (refused("asker", Body::QueueEdit { item: "ghost".into(), prompt: "x".into() }), ("ITEM_NOT_QUEUED", queue::message::item_not_queued("ghost"))),
        (refused("worker", Body::QueueMove { item: "qa".into(), before: Some("ghost".into()) }), ("ITEM_NOT_QUEUED", queue::message::target_not_queued("ghost"))),
        (refused("resting", Body::QueueStart { item: "q1".into() }), ("ITEM_NOT_QUEUED", queue::message::item_not_queued("q1"))),
        (refused("doomed", Body::QueueResume { item: None }), ("THREAD_NOT_ACTIVE", queue::message::thread_not_active("doomed"))),
        (refused("worker", Body::QueueResume { item: Some("q1".into()) }), ("NOT_STOPPED", queue::message::NOT_STOPPED.to_string())),
        (refused("asker", Body::Steer { turn: None, prompt: Some("x".into()), item: None }), ("NO_TURN", agent_api_agent::message::no_turn("asker", None))),
        (refused("resting", Body::LifecycleArchive { archive: false }), ("NOT_ARCHIVED", lifecycle::message::not_archived("resting"))),
        (refused("kid", Body::Fork { source: "parent".into(), at_turn: None, title: None }), ("THREAD_EXISTS", lifecycle::message::thread_exists("kid"))),
        (refused("again", Body::Fork { source: "worker".into(), at_turn: Some("w1".into()), title: None }), ("FORK_SOURCE_RUNNING", lifecycle::message::fork_source_running("w1"))),
        (refused("again", Body::Fork { source: "resting".into(), at_turn: None, title: None }), ("NO_STABLE_TURN", lifecycle::message::no_stable_fork("latest_stable"))),
        (refused("resting", Body::SettleSet { to: SettleTo::Snoozed, until_ms: Some(1), at_ms: None }), ("SNOOZE_NOT_FUTURE", lifecycle::message::snooze_not_future("resting", 1))),
        (refused("worker", Body::ForceStop { turn: Some("w0".into()) }), ("SUPERSEDED", lifecycle::message::superseded("worker", "w0"))),
        (refused("worker", Body::MergeBack { source: "kid".into(), turn: None }), ("NOT_A_FORK", lifecycle::message::not_a_fork("kid", "worker"))),
        (refused("parent", Body::Rollback { turn: "ghost".into(), restore_files: false }), ("TURN_NOT_FOUND", lifecycle::message::turn_not_found("parent", "ghost"))),
        (refused("parent", Body::Detach { session: "s1".into(), reason: None }), ("SESSION_NOT_OWNED", lifecycle::message::session_not_owned("parent", "s1"))),
        (refused("failing", Body::Retry { turn: "f1".into(), new_turn: "f3".into() }), ("NOT_RETRYABLE", lifecycle::message::not_retryable("f1", "it is not the newest turn"))),
        (refused("nobody", Body::QueueDelete { item: "q".into() }), ("THREAD_NOT_FOUND", agent_api_agent::message::thread_not_found("nobody"))),
    ];
    let known: BTreeSet<&str> = requests::code::ALL.iter().chain(queue::code::ALL.iter()).chain(lifecycle::code::ALL.iter()).chain(agent_api_agent::code::ALL.iter()).copied().collect();
    for ((code, message), (expected_code, expected_message)) in cases {
        // When each refusal is compared with the contract, Then the code is one the contract names, and is the one expected, with the contract's words
        assert!(known.contains(code.as_str()), "{code} is no code of the contract");
        assert_eq!((code.as_str(), message), (expected_code, expected_message));
    }
}

#[test]
fn given_every_message_the_engine_words_when_compared_with_the_contracts_for_the_same_arguments_then_they_are_the_same() {
    // Given the messages of the engine and of the contract, for the same ids
    let pairs: Vec<(String, String)> = vec![
        (msg::thread_not_found("t"), agent_api_agent::message::thread_not_found("t")),
        (msg::turn_running("t"), agent_api_agent::message::turn_running("t")),
        (msg::no_turn("t", Some("u")), agent_api_agent::message::no_turn("t", Some("u"))),
        (msg::no_turn("t", None), agent_api_agent::message::no_turn("t", None)),
        (msg::request_not_open("t", "r"), requests::message::request_not_open("t", "r")),
        (msg::wrong_kind("input.dismiss", "r", "approval"), requests::message::wrong_kind("input.dismiss", "r", "approval")),
        (msg::wrong_kind("approval.answer", "r", "a question"), requests::message::wrong_kind("approval.answer", "r", "a question")),
        (msg::item_not_queued("i"), queue::message::item_not_queued("i")),
        (msg::target_not_queued("i"), queue::message::target_not_queued("i")),
        (msg::thread_not_active("t"), queue::message::thread_not_active("t")),
        (msg::NOT_STOPPED.to_string(), queue::message::NOT_STOPPED.to_string()),
        (msg::queue_held("t"), queue::message::queue_held("t")),
        (msg::no_defaults("t"), queue::message::no_defaults("t")),
        (msg::already_archived("t"), lifecycle::message::already_archived("t")),
        (msg::not_archived("t"), lifecycle::message::not_archived("t")),
        (msg::thread_exists("t"), lifecycle::message::thread_exists("t")),
        (msg::fork_source_running("u"), lifecycle::message::fork_source_running("u")),
        (msg::no_stable_fork("run"), lifecycle::message::no_stable_fork("run")),
        (msg::no_stable_merge("latest_stable"), lifecycle::message::no_stable_merge("latest_stable")),
        (msg::snooze_not_future("t", 5), lifecycle::message::snooze_not_future("t", 5)),
        (msg::snooze_request_open("t"), lifecycle::message::snooze_request_open("t")),
        (msg::snooze_queued("t"), lifecycle::message::snooze_queued("t")),
        (msg::snooze_archived("t"), lifecycle::message::snooze_archived("t")),
        (msg::superseded("t", "u"), lifecycle::message::superseded("t", "u")),
        (msg::not_a_fork("s", "t"), lifecycle::message::not_a_fork("s", "t")),
        (msg::TARGET_BUSY.to_string(), lifecycle::message::TARGET_BUSY.to_string()),
        (msg::merge_pending("t"), lifecycle::message::merge_pending("t")),
        (msg::source_not_finished("u", "running"), lifecycle::message::source_not_finished("u", "running")),
        (msg::turn_not_found("t", "u"), lifecycle::message::turn_not_found("t", "u")),
        (msg::session_not_owned("t", "s"), lifecycle::message::session_not_owned("t", "s")),
        (msg::not_retryable("u", "it did not fail"), lifecycle::message::not_retryable("u", "it did not fail")),
    ];
    // When each pair is compared, Then the two are the same words
    for (engine_says, contract_says) in pairs {
        assert_eq!(engine_says, contract_says);
    }
}
