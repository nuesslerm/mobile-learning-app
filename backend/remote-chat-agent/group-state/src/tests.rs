//! The state groups over a store in memory: reads and writes per scope, the snapshot and live events of `state.subscribe`, a resume within the
//! kept tail, and the reset that answers a caller older than the tail or ahead of the stream.
use crate::testing::MemoryStore;
use crate::{StateGroup, StateHub, StateWatchGroup};
use agent_api_proto::Event;
use async_trait::async_trait;
use rca_agent_core::{AgentApi, Build, Connection, EventSink, Group};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const BUILD: Build<'static> = Build { version: "1", commit: "c" };

struct Queue(mpsc::UnboundedSender<Event>);

#[async_trait]
impl EventSink for Queue {
    fn send(&self, event: &Event) -> bool {
        self.0.send(event.clone()).is_ok()
    }

    async fn send_replay(&self, event: &Event) -> bool {
        self.send(event)
    }
}

fn api(store: MemoryStore, keep: usize) -> AgentApi {
    let hub = Arc::new(StateHub::keeping(Arc::new(store), keep));
    AgentApi::new(vec![Arc::new(StateGroup::new(hub.clone())) as Arc<dyn Group>, Arc::new(StateWatchGroup::new(hub))]).unwrap()
}

async fn call(api: &AgentApi, method: &str, params: Value) -> Value {
    api.dispatch_text(BUILD, &json!({"id": 1, "method": method, "params": params}).to_string()).await
}

async fn set(api: &AgentApi, scope: &str, key: &str, value: Value) {
    let answered = call(api, "state.set", json!({"scope": scope, "key": key, "value": value})).await;
    assert_eq!(answered, json!({"id": 1, "result": {}}));
}

/// A subscriber's socket: the answer to `state.subscribe`, and the events its stream pushes afterwards.
struct Subscriber {
    answer: Value,
    events: mpsc::UnboundedReceiver<Event>,
    stream: tokio::task::JoinHandle<()>,
}

async fn subscribe(api: &AgentApi, params: Value) -> Subscriber {
    let (tx, events) = mpsc::unbounded_channel();
    let connection = Connection::new(Arc::new(Queue(tx)));
    let frame = json!({"id": 1, "method": "state.subscribe", "params": params}).to_string();
    let answer = api.dispatch_on(BUILD, &frame, &connection).await;
    let mut jobs = connection.take_following();
    let stream = tokio::spawn(match jobs.pop() {
        Some(followed) => followed.job,
        None => Box::pin(async {}),
    });
    Subscriber { answer, events, stream }
}

impl Subscriber {
    async fn next(&mut self) -> Event {
        tokio::time::timeout(Duration::from_secs(5), self.events.recv()).await.expect("an event within 5 s").expect("the stream is open")
    }

    async fn silent(&mut self) -> bool {
        tokio::time::timeout(Duration::from_millis(100), self.events.recv()).await.is_err()
    }
}

fn seqs_and_keys(events: &[Event]) -> Vec<(u64, String)> {
    events.iter().map(|e| (e.seq, e.data["key"].as_str().unwrap_or_default().to_string())).collect()
}

#[tokio::test]
async fn given_a_value_set_for_a_device_when_it_is_read_then_it_comes_back_and_another_scope_does_not_see_it() {
    // Given a value set in one scope
    let api = api(MemoryStore::default(), 8);
    set(&api, "phone-a", "quick", json!(["ls"])).await;
    // When it is read in that scope and in another
    // Then only the first has it
    assert_eq!(call(&api, "state.get", json!({"scope": "phone-a", "key": "quick"})).await, json!({"id": 1, "result": {"value": ["ls"]}}));
    assert_eq!(call(&api, "state.get", json!({"scope": "phone-b", "key": "quick"})).await["result"], json!({"value": null}));
}

#[tokio::test]
async fn given_a_subscriber_when_a_value_is_written_then_it_gets_the_values_now_and_the_change_as_an_event_numbered_from_the_seq_it_stands_at() {
    // Given a scope holding one value, and a second scope
    let api = api(MemoryStore::default(), 8);
    set(&api, "phone-a", "theme", json!("light")).await;
    // When a subscriber follows the first scope
    let mut sub = subscribe(&api, json!({"scope": "phone-a"})).await;
    // Then it is answered with the values and the seq they stand at, and no reset
    assert_eq!(sub.answer, json!({"id": 1, "result": {"scope": "phone-a", "values": {"theme": "light"}, "seq": 1, "reset": false}}));
    // And a write to the scope arrives as an event with the next seq, while a write to another scope does not
    set(&api, "phone-b", "theme", json!("other")).await;
    set(&api, "phone-a", "theme", json!("dark")).await;
    let event = sub.next().await;
    assert_eq!(event.to_value(), json!({"event": {"seq": 2, "type": "state.changed", "scope": "phone-a", "key": "theme", "value": "dark"}}));
    assert!(sub.silent().await);
    sub.stream.abort();
}

#[tokio::test]
async fn given_a_dropped_subscriber_when_it_resumes_by_its_last_seq_then_it_gets_only_what_it_missed_and_then_the_live_changes() {
    // Given a subscriber that saw seq 1 and dropped, and three writes it missed
    let api = api(MemoryStore::default(), 8);
    set(&api, "d", "a", json!(1)).await;
    let first = subscribe(&api, json!({"scope": "d"})).await;
    first.stream.abort();
    for (key, n) in [("b", 2), ("c", 3), ("a", 4)] {
        set(&api, "d", key, json!(n)).await;
    }
    // When it resumes after seq 1
    let mut sub = subscribe(&api, json!({"scope": "d", "after_seq": 1})).await;
    // Then the answer is no reset and carries no values (the events do), and the missed writes arrive in order
    assert_eq!(sub.answer["result"], json!({"scope": "d", "values": {}, "seq": 4, "reset": false}));
    let missed = [sub.next().await, sub.next().await, sub.next().await];
    assert_eq!(seqs_and_keys(&missed), [(2, "b".to_string()), (3, "c".to_string()), (4, "a".to_string())]);
    // And a later write is delivered live with the next seq
    set(&api, "d", "e", json!(5)).await;
    assert_eq!(seqs_and_keys(&[sub.next().await]), [(5, "e".to_string())]);
    sub.stream.abort();
}

#[tokio::test]
async fn given_more_writes_than_the_tail_keeps_when_a_subscriber_resumes_past_it_then_it_is_reset_to_the_values_as_they_are() {
    // Given a tail of 2 and five writes to one scope (key k overwritten, so the values differ from the events)
    let api = api(MemoryStore::default(), 2);
    for n in 1..=5 {
        set(&api, "d", if n < 5 { "k" } else { "z" }, json!(n)).await;
    }
    // When a subscriber resumes after seq 1 (older than the tail), after seq 3 (inside it), and after seq 9 (ahead of the stream)
    let old = subscribe(&api, json!({"scope": "d", "after_seq": 1})).await;
    let mut inside = subscribe(&api, json!({"scope": "d", "after_seq": 3})).await;
    let ahead = subscribe(&api, json!({"scope": "d", "after_seq": 9})).await;
    // Then the old and the ahead ones are reset to the values and the seq, and the one inside the tail is replayed the rest
    assert_eq!(old.answer["result"], json!({"scope": "d", "values": {"k": 4, "z": 5}, "seq": 5, "reset": true}));
    assert_eq!(ahead.answer["result"], json!({"scope": "d", "values": {"k": 4, "z": 5}, "seq": 5, "reset": true}));
    assert_eq!(inside.answer["result"]["reset"], json!(false));
    assert_eq!(seqs_and_keys(&[inside.next().await, inside.next().await]), [(4, "k".to_string()), (5, "z".to_string())]);
    // And a subscriber reset past the tail follows the stream from the seq it was given
    let mut old = old;
    set(&api, "d", "y", json!(6)).await;
    assert_eq!(seqs_and_keys(&[old.next().await]), [(6, "y".to_string())]);
    for s in [old.stream, inside.stream, ahead.stream] {
        s.abort();
    }
}

#[tokio::test]
async fn given_a_subscriber_that_falls_behind_the_tail_when_the_stream_notices_then_it_ends_so_the_peer_resubscribes() {
    // Given a stream opened at seq 0 on a tail of 1
    let api = api(MemoryStore::default(), 1);
    let (tx, events) = mpsc::unbounded_channel();
    let connection = Connection::new(Arc::new(Queue(tx)));
    let frame = json!({"id": 1, "method": "state.subscribe", "params": {"scope": "d", "after_seq": 0}}).to_string();
    api.dispatch_on(BUILD, &frame, &connection).await;
    // When four writes land before the stream ever runs, and then it does
    for n in 0..4 {
        set(&api, "d", "k", json!(n)).await;
    }
    let followed = connection.take_following().pop().unwrap();
    // Then it finds the events it needs gone and ends without sending a partial history
    tokio::time::timeout(Duration::from_secs(5), followed.job).await.expect("the stream ends");
    let mut events = events;
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn given_bad_params_and_a_full_store_when_called_then_each_is_refused_with_its_code_and_a_refused_write_is_no_event() {
    // Given a service whose store refuses writes
    let full = api(MemoryStore::full(), 8);
    let mut sub = subscribe(&full, json!({"scope": "a"})).await;
    // When a write is made, and params are missing
    // Then the write is STATE_UNAVAILABLE with the store's reason, the params are INVALID_PARAMS, and the subscriber sees nothing
    let refused = call(&full, "state.set", json!({"scope": "a", "key": "k", "value": 1})).await;
    assert_eq!(refused["error"], json!({"code": "STATE_UNAVAILABLE", "message": "disk full"}));
    assert_eq!(call(&full, "state.get", json!({"scope": "a"})).await["error"]["code"], json!("INVALID_PARAMS"));
    assert_eq!(call(&full, "state.subscribe", json!({})).await["error"], json!({"code": "INVALID_PARAMS", "message": "scope is required"}));
    assert_eq!(call(&full, "state.subscribe", json!({"scope": "a", "after_seq": -1})).await["error"]["code"], json!("INVALID_PARAMS"));
    assert!(sub.silent().await);
    assert_eq!(sub.answer["result"]["seq"], json!(0));
    sub.stream.abort();
}

#[tokio::test]
async fn given_a_client_that_followed_an_earlier_run_when_the_service_restarts_and_writes_past_its_seq_then_it_is_reset_to_the_values() {
    // Given a client that followed a run up to seq 40, and a restarted service (a later base) on the same store that has since written 50 values
    let store = Arc::new(MemoryStore::default());
    let first = StateHub::starting_at(store.clone(), 256, 1_000);
    for n in 0..40 {
        first.set("d", &format!("k{n}"), json!(n)).unwrap();
    }
    let seen = 1_000 + 40;
    let second = Arc::new(StateHub::starting_at(store, 256, 2_000_000));
    let restarted = AgentApi::new(vec![Arc::new(StateGroup::new(second.clone())) as Arc<dyn Group>, Arc::new(StateWatchGroup::new(second))]).unwrap();
    for n in 40..90 {
        set(&restarted, "d", &format!("k{n}"), json!(n)).await;
    }
    // When the client resubscribes after the seq it saw
    let resumed = subscribe(&restarted, json!({"scope": "d", "after_seq": seen})).await;
    // Then it is reset, and given every value of the scope instead of only the events past its seq
    assert_eq!(resumed.answer["result"]["reset"], json!(true));
    assert_eq!(resumed.answer["result"]["values"].as_object().unwrap().len(), 90);
    resumed.stream.abort();
}
