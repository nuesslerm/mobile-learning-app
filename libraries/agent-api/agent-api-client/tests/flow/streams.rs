//! Subscriptions: the events a thread's stream delivers once and in order, across a socket that drops, a service that replays what the client
//! has, a resubscription the service refuses, and a service that never comes back.
use super::fake::{script, Conn, Fake};
use super::{config, method};
use agent_api_agent::{SubscribeParams, SubscribeResult};
use agent_api_client::{code, Client, Subscribe};
use agent_api_proto::{thread_topic, Event};
use serde_json::{json, Value};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);

/// The recorded events of thread t1, which the services below play.
fn recorded() -> Vec<Event> {
    agent_api_agent::testing::thread_events()
}

fn subscription(thread: &str, after_seq: u64) -> Subscribe {
    Subscribe::new(thread_topic(thread), method("thread.subscribe"), SubscribeParams { thread: thread.into(), after_seq }.to_value(), "after_seq")
}

/// Answers the subscribe request the client sends next, as the service would: where the replay starts and where the thread stands.
fn accept_subscribe(conn: &mut Conn, last_seq: u64) -> SubscribeParams {
    let request = conn.request().expect("a subscribe request");
    let params = SubscribeParams::parse(&request.params).expect("the contract's own params");
    conn.answer(&request, serde_json::to_value(SubscribeResult::new(&params.thread, params.after_seq, last_seq)).unwrap());
    params
}

fn seqs(events: &[Event]) -> Vec<u64> {
    events.iter().map(|e| e.seq).collect()
}

/// Every event the client has before `wait` runs out.
fn drain(client: &mut Client, wait: Duration) -> Vec<Event> {
    std::iter::from_fn(|| client.next_event(wait).unwrap()).collect()
}

#[test]
fn a_subscription_delivers_each_event_of_its_thread_once_in_order_and_one_of_another_thread_is_not_held_back() {
    // Given a service that answers the subscribe of thread t1 and sends the first four recorded events, then one of thread t2
    let events = recorded();
    let other = Event { thread: Some("t2".into()), ..events[0].clone() };
    let fake = Fake::start(vec![script({
        let (events, other) = (events.clone(), other.clone());
        move |conn| {
            accept_subscribe(conn, 4);
            events[..4].iter().for_each(|e| conn.event(e));
            conn.event(&other);
            conn.linger();
        }
    })]);
    let mut client = Client::connect(config(&fake, 1)).unwrap();
    // When the client subscribes to t1 from its start and reads
    let answer = client.subscribe(subscription("t1", 0)).unwrap();
    let got = drain(&mut client, Duration::from_millis(300));
    // Then it has the service's answer, the four events of t1 in order and the one of t2, and the last seq it saw of t1
    assert_eq!(answer, json!({"topic": "thread/t1", "after_seq": 0, "last_seq": 4}));
    assert_eq!(got, [&events[..4], std::slice::from_ref(&other)].concat());
    assert_eq!((client.last_seq("thread/t1"), client.last_seq("thread/t2")), (Some(4), None));
    // And nothing more comes
    assert_eq!(client.next_event(Duration::from_millis(50)).unwrap(), None);
}

#[test]
fn a_socket_that_drops_in_the_middle_of_a_stream_is_reopened_and_the_stream_resumes_after_the_last_event_the_client_saw() {
    // Given a service that sends three events and drops, and on the next connection answers a subscribe and sends the rest
    let events = recorded();
    let fake = Fake::start(vec![
        script({
            let events = events.clone();
            move |conn| {
                accept_subscribe(conn, 3);
                events[..3].iter().for_each(|e| conn.event(e));
                conn.drop_now();
            }
        }),
        script({
            let events = events.clone();
            move |conn| {
                let params = accept_subscribe(conn, 6);
                events.iter().filter(|e| e.seq > params.after_seq).take(3).for_each(|e| conn.event(e));
                conn.linger();
            }
        }),
    ]);
    let mut client = Client::connect(config(&fake, 5)).unwrap();
    client.subscribe(subscription("t1", 0)).unwrap();
    // When the client reads on, through the drop
    let got = drain(&mut client, Duration::from_millis(500));
    // Then it has events 1 to 6 once each, in order, and the second connection was asked to resume after seq 3, the last it had
    assert_eq!(seqs(&got), [1, 2, 3, 4, 5, 6]);
    let resubscribe: Vec<Value> = fake.seen().into_iter().filter(|s| s.connection == 1).map(|s| s.request.params).collect();
    assert_eq!(resubscribe, [json!({"thread": "t1", "after_seq": 3})]);
}

#[test]
fn events_a_service_replays_that_the_client_already_has_are_dropped_and_the_stream_goes_on_from_the_first_it_does_not() {
    // Given a service that, after a drop, answers the resubscribe by replaying from seq 2 though the client has 3
    let events = recorded();
    let fake = Fake::start(vec![
        script({
            let events = events.clone();
            move |conn| {
                accept_subscribe(conn, 3);
                events[..3].iter().for_each(|e| conn.event(e));
                conn.drop_now();
            }
        }),
        script({
            let events = events.clone();
            move |conn| {
                accept_subscribe(conn, 5);
                events[1..5].iter().for_each(|e| conn.event(e));
                conn.linger();
            }
        }),
    ]);
    let mut client = Client::connect(config(&fake, 5)).unwrap();
    client.subscribe(subscription("t1", 0)).unwrap();
    // When the client reads on
    let got = drain(&mut client, Duration::from_millis(500));
    // Then each seq is there once: the replay of 2 and 3 is dropped
    assert_eq!(seqs(&got), [1, 2, 3, 4, 5]);
}

#[test]
fn a_replay_that_comes_before_the_answer_to_the_subscribe_is_counted_against_the_position_the_client_asked_from() {
    // Given a service that sends events 2 to 4 before it answers a subscribe made after seq 2
    let events = recorded();
    let fake = Fake::start(vec![script({
        let events = events.clone();
        move |conn| {
            let request = conn.request().unwrap();
            events[1..4].iter().for_each(|e| conn.event(e));
            conn.answer(&request, serde_json::to_value(SubscribeResult::new("t1", 2, 4)).unwrap());
            conn.linger();
        }
    })]);
    let mut client = Client::connect(config(&fake, 1)).unwrap();
    // When the client subscribes after seq 2
    client.subscribe(subscription("t1", 2)).unwrap();
    // Then it has events 3 and 4: seq 2 was not after 2
    assert_eq!(seqs(&drain(&mut client, Duration::from_millis(200))), [3, 4]);
}

#[test]
fn a_service_that_does_not_come_back_is_given_up_on_after_the_attempts_the_backoff_allows_and_the_error_says_so() {
    // Given a service that answers a subscribe, sends two events and drops, and then closes every connection at once
    let events = recorded();
    let fake = Fake::start(vec![script({
        let events = events.clone();
        move |conn| {
            accept_subscribe(conn, 2);
            events[..2].iter().for_each(|e| conn.event(e));
            conn.drop_now();
        }
    })]);
    let mut client = Client::connect(config(&fake, 3)).unwrap();
    client.subscribe(subscription("t1", 0)).unwrap();
    let first_two: Vec<Event> = (0..2).map(|_| client.next_event(WAIT).unwrap().expect("an event")).collect();
    assert_eq!(seqs(&first_two), [1, 2]);
    // When the client waits for more
    let gave_up = client.next_event(WAIT).unwrap_err();
    // Then it gave up with DISCONNECTED after three attempts, having tried the service that many more times
    assert_eq!(gave_up.code, code::DISCONNECTED, "{gave_up}");
    assert!(gave_up.message.contains("gave up after 3 attempts"), "{gave_up}");
    assert_eq!(fake.tcp_connections(), 1 + 3);
    // And a call afterwards tries again, and gives up the same way
    let again = client.call(method("thread.list"), json!({})).unwrap_err();
    assert_eq!((again.code.as_str(), fake.tcp_connections()), (code::DISCONNECTED, 1 + 3 + 3), "{again}");
}

#[test]
fn a_subscription_the_service_refuses_when_the_client_resumes_it_is_reported_with_the_services_code_and_no_longer_kept() {
    // Given a service that drops the connection after an event, and refuses the resubscribe as THREAD_NOT_FOUND
    let events = recorded();
    let fake = Fake::start(vec![
        script({
            let events = events.clone();
            move |conn| {
                accept_subscribe(conn, 1);
                conn.event(&events[0]);
                conn.drop_now();
            }
        }),
        script(|conn| {
            let request = conn.request().unwrap();
            conn.refuse(&request, "THREAD_NOT_FOUND", "no thread has the id t1");
            conn.linger();
        }),
    ]);
    let mut client = Client::connect(config(&fake, 3)).unwrap();
    client.subscribe(subscription("t1", 0)).unwrap();
    assert_eq!(seqs(&[client.next_event(WAIT).unwrap().unwrap()]), [1]);
    // When the client waits for more
    let refused = client.next_event(WAIT).unwrap_err();
    // Then the error is the service's, says which stream, and the stream is dropped
    assert_eq!(refused.code, "THREAD_NOT_FOUND");
    assert!(refused.message.contains("resuming thread/t1") && refused.message.contains("no thread has the id t1"), "{refused}");
    assert_eq!((refused.from_service(), client.last_seq("thread/t1")), (true, None));
}

#[test]
fn a_subscription_the_service_refuses_at_the_start_is_not_kept_and_unsubscribing_drops_the_events_still_queued() {
    // Given a service that refuses the first subscribe, and answers the second and sends two events with the next call's answer
    let events = recorded();
    let fake = Fake::start(vec![script({
        let events = events.clone();
        move |conn| {
            let request = conn.request().unwrap();
            conn.refuse(&request, "THREAD_NOT_FOUND", "no thread has the id nope");
            accept_subscribe(conn, 2);
            let call = conn.request().unwrap();
            events[..2].iter().for_each(|e| conn.event(e));
            conn.answer(&call, json!({"threads": []}));
            conn.linger();
        }
    })]);
    let mut client = Client::connect(config(&fake, 1)).unwrap();
    // When a subscribe is refused
    let refused = client.subscribe(subscription("nope", 0)).unwrap_err();
    // Then it is the service's error and nothing is kept
    assert_eq!((refused.code.as_str(), client.last_seq("thread/nope")), ("THREAD_NOT_FOUND", None));
    // And when a thread is subscribed, two of its events arrive with a call's answer, and the client unsubscribes before reading them
    client.subscribe(subscription("t1", 0)).unwrap();
    client.call(method("thread.list"), json!({})).unwrap();
    assert!(client.unsubscribe("thread/t1") && !client.unsubscribe("thread/t1"));
    // Then they are not delivered
    assert_eq!(client.next_event(Duration::from_millis(100)).unwrap(), None);
}
