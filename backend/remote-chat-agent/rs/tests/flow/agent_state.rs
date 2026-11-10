//! `state` and `state.watch` end to end over the agent socket: a value one mapper writes reaches another mapper that follows its scope as an
//! event with a `seq`, a mapper that dropped resumes by its last `seq` and is sent what it missed, and one older than the tail the service keeps
//! is reset to the values as they are.

use crate::harness::{service, Env, Options};
use crate::mapper::Agent;
use serde_json::{json, Value};

async fn set(agent: &mut Agent, scope: &str, key: &str, value: Value) {
    let answer = agent.ask("state.set", json!({"scope": scope, "key": key, "value": value})).await;
    assert_eq!(answer["result"], json!({}), "state.set {key}: {answer}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_mapper_following_a_scope_when_another_mapper_writes_to_it_then_the_follower_gets_the_change_as_an_event_with_the_next_seq_and_nothing_of_another_scope() {
    // Given a service, a mapper that follows the scope of phone-a (holding one value), and a second mapper
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let token = svc.mint("state flow").await;
    let (mut writer, mut follower) = (Agent::open(port, &token).await, Agent::open(port, &token).await);
    set(&mut writer, "phone-a", "theme", json!("light")).await;
    let subscribed = follower.ask("state.subscribe", json!({"scope": "phone-a"})).await;
    assert_eq!(subscribed["result"], json!({"scope": "phone-a", "values": {"theme": "light"}, "seq": 1, "reset": false}));
    // When the second mapper writes to another scope and then to the followed one
    set(&mut writer, "phone-b", "theme", json!("other")).await;
    set(&mut writer, "phone-a", "theme", json!("dark")).await;
    // Then the follower's next event is the change of its own scope, numbered 2, and the value is readable
    assert_eq!(follower.event().await.to_value(), json!({"event": {"seq": 2, "type": "state.changed", "scope": "phone-a", "key": "theme", "value": "dark"}}));
    assert_eq!(writer.ask("state.get", json!({"scope": "phone-a", "key": "theme"})).await["result"], json!({"value": "dark"}));
    // And the service lists the sub-group
    let status = follower.ask("service.status", json!({})).await;
    assert!(status["result"]["groups"].as_array().unwrap().contains(&json!("state.watch")), "{status}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_follower_that_dropped_when_it_resumes_by_its_last_seq_then_it_is_sent_what_it_missed_in_order_and_the_writes_after_that_live() {
    // Given a follower that saw seq 1 of a scope and then dropped its socket, and three writes it missed
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let token = svc.mint("state flow").await;
    let mut writer = Agent::open(port, &token).await;
    set(&mut writer, "d", "a", json!(1)).await;
    let mut first = Agent::open(port, &token).await;
    let seen = first.ask("state.subscribe", json!({"scope": "d"})).await["result"]["seq"].as_u64().unwrap();
    drop(first);
    for (key, n) in [("b", 2), ("c", 3), ("a", 4)] {
        set(&mut writer, "d", key, json!(n)).await;
    }
    // When it reconnects and subscribes after the seq it saw
    let mut again = Agent::open(port, &token).await;
    let resumed = again.ask("state.subscribe", json!({"scope": "d", "after_seq": seen})).await;
    // Then it is told there is no reset and receives the three missed changes in order
    assert_eq!(resumed["result"], json!({"scope": "d", "values": {}, "seq": 4, "reset": false}));
    let mut got = Vec::new();
    for _ in 0..3 {
        let event = again.event().await;
        got.push((event.seq, event.data["key"].clone(), event.data["value"].clone()));
    }
    assert_eq!(got, [(2, json!("b"), json!(2)), (3, json!("c"), json!(3)), (4, json!("a"), json!(4))]);
    // And a write made after it resumed arrives live with seq 5
    set(&mut writer, "d", "e", json!(5)).await;
    assert_eq!(again.event().await.seq, 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_more_writes_than_the_tail_keeps_when_a_follower_resumes_past_it_then_the_answer_is_a_reset_with_the_values_as_they_are() {
    // Given 300 writes to one scope over five keys, more than the 256 events the service keeps
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let token = svc.mint("state flow").await;
    let mut writer = Agent::open(port, &token).await;
    for n in 1..=300u64 {
        set(&mut writer, "d", &format!("k{}", n % 5), json!(n)).await;
    }
    // When a follower resumes after seq 10 (past the tail) and another after seq 100 (inside it)
    let mut past = Agent::open(port, &token).await;
    let reset = past.ask("state.subscribe", json!({"scope": "d", "after_seq": 10})).await;
    let mut inside = Agent::open(port, &token).await;
    let resumed = inside.ask("state.subscribe", json!({"scope": "d", "after_seq": 100})).await;
    // Then the first is answered with every key's latest value at seq 300 and no replay, and the second is replayed the 200 it missed
    assert_eq!(reset["result"], json!({"scope": "d", "values": {"k0": 300, "k1": 296, "k2": 297, "k3": 298, "k4": 299}, "seq": 300, "reset": true}));
    assert_eq!(resumed["result"]["reset"], json!(false));
    assert_eq!(inside.event().await.seq, 101);
    // And the reset follower is sent the next write, numbered 301
    set(&mut writer, "d", "k1", json!(301)).await;
    assert_eq!(past.event().await.seq, 301);
}
