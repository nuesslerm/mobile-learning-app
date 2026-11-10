//! Calls: the answers as the contract records them, the bearer, the schemes, and every way a call can fail.
use super::fake::{script, Fake};
use super::{config, method};
use agent_api_client::{code, Client, Config};
use agent_api_proto::Request;
use serde_json::{json, Value};
use std::time::Duration;

/// The recorded request of each exchange of the contract and the answer recorded for it.
fn recorded() -> Vec<(String, Value, Value)> {
    let all: Vec<_> = agent_api_agent::testing::fixtures().into_iter().flat_map(|f| f.exchanges).map(|e| (e.request["method"].as_str().unwrap().to_string(), e.request["params"].clone(), e.response)).collect();
    // A request recorded twice must have one answer, or the replay below could not tell which is meant
    for (method, params, response) in &all {
        let same: Vec<_> = all.iter().filter(|(m, p, _)| m == method && p == params).collect();
        assert!(same.iter().all(|(_, _, r)| r == response), "{method} {params} is recorded with two answers");
    }
    all
}

#[test]
fn every_exchange_the_contract_records_is_answered_to_the_client_as_recorded_and_the_service_saw_the_params_it_sent() {
    // Given a service that answers each request with the answer recorded for its method and params
    let answers = recorded();
    let fake = Fake::start(vec![script({
        let answers = answers.clone();
        move |conn| {
            while let Some(request) = conn.request() {
                let (_, _, response) = answers.iter().find(|(m, p, _)| *m == request.method && *p == request.params).unwrap_or_else(|| panic!("nothing recorded for {} {}", request.method, request.params));
                let mut frame = response.clone();
                frame["id"] = request.id.clone();
                conn.send(frame);
            }
        }
    })]);
    let mut client = Client::connect(config(&fake, 1)).unwrap();
    for (name, params, response) in &answers {
        // When the client calls each recorded request
        let answer = client.call(method(name), params.clone());
        // Then it gets the result recorded, or an error with the recorded code and message, which came from the service
        match (answer, response.get("result")) {
            (Ok(result), Some(recorded)) => assert_eq!(&result, recorded, "{name} {params}"),
            (Err(error), None) => assert_eq!((error.code.as_str(), error.message.as_str(), error.from_service()), (response["error"]["code"].as_str().unwrap(), response["error"]["message"].as_str().unwrap(), true), "{name} {params}"),
            (other, _) => panic!("{name} {params}: {other:?}, recorded {response}"),
        }
    }
    // And the service saw each request with the method and params sent, under ids that are unique and rise
    drop(client);
    let seen = fake.seen();
    assert_eq!(seen.iter().map(|s| (s.request.method.clone(), s.request.params.clone())).collect::<Vec<_>>(), answers.iter().map(|(m, p, _)| (m.clone(), p.clone())).collect::<Vec<_>>());
    let ids: Vec<u64> = seen.iter().map(|s| s.request.id.as_u64().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
}

#[test]
fn a_bearer_the_service_refuses_is_unauthorized_and_the_right_one_is_what_the_service_saw() {
    // Given a service that wants the device bearer
    let fake = Fake::start(vec![script(|conn| conn.linger())]);
    // When the client connects with another bearer
    let mut wrong = config(&fake, 1);
    wrong.bearer = "stolen".into();
    let refused = Client::connect(wrong).err().expect("the wrong bearer is refused");
    // Then the error is UNAUTHORIZED and the service saw that bearer
    assert_eq!(refused.code, code::UNAUTHORIZED, "{refused}");
    assert_eq!(fake.handshakes(), [Some("Bearer stolen".to_string())]);
    // And a client with the right one connects, and is the first to be served
    let client = Client::connect(config(&fake, 1)).unwrap();
    assert!(client.is_connected());
    assert_eq!(fake.handshakes()[1].as_deref(), Some("Bearer device-bearer"));
}

#[test]
fn a_url_that_is_not_ws_or_names_nothing_that_listens_is_a_connect_error_and_the_service_is_not_called_for_the_first() {
    // Given a service, and a port nothing listens on
    let fake = Fake::start(vec![]);
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    for (url, says) in [(fake.url.replace("ws://", "wss://"), "wss"), (fake.url.replace("ws://", "http://"), "http"), (format!("ws://{closed}"), "")] {
        // When the client connects to each
        let refused = Client::connect(Config::new(url.clone(), "b")).err().unwrap_or_else(|| panic!("{url} connected"));
        // Then each is CONNECT, and the scheme errors name the scheme
        assert_eq!(refused.code, code::CONNECT, "{url}: {refused}");
        assert!(refused.message.contains(says), "{url}: {refused}");
    }
    // And the service saw no connection from the two that are not ws
    assert_eq!(fake.tcp_connections(), 0);
}

#[test]
fn events_that_come_before_an_answer_are_queued_in_order_and_the_call_still_returns_its_own_answer() {
    // Given a service that sends two events of a thread before it answers a request
    let events: Vec<_> = agent_api_agent::testing::thread_events().into_iter().take(2).collect();
    let fake = Fake::start(vec![script({
        let events = events.clone();
        move |conn| {
            let request = conn.request().unwrap();
            events.iter().for_each(|e| conn.event(e));
            conn.answer(&request, json!({"threads": []}));
            conn.linger();
        }
    })]);
    let mut client = Client::connect(config(&fake, 1)).unwrap();
    // When the client calls
    let result = client.call(method("thread.list"), json!({})).unwrap();
    // Then it has the result, and the events come out in the order they were sent
    assert_eq!(result, json!({"threads": []}));
    let first = client.next_event(Duration::from_secs(1)).unwrap();
    let second = client.next_event(Duration::from_secs(1)).unwrap();
    assert_eq!([first, second], [Some(events[0].clone()), Some(events[1].clone())]);
}

#[test]
fn a_call_nobody_answers_is_a_timeout_that_leaves_the_socket_open_and_its_late_answer_is_not_taken_for_the_next() {
    // Given a service that answers the first request only after the second has arrived, and the second at once
    let fake = Fake::start(vec![script(|conn| {
        let first = conn.request().unwrap();
        let second = conn.request().unwrap();
        conn.answer(&first, json!({"late": true}));
        conn.answer(&second, json!({"threads": []}));
        conn.linger();
    })]);
    let mut short = config(&fake, 1);
    short.call_timeout = Duration::from_millis(150);
    let mut client = Client::connect(short).unwrap();
    // When the first call waits longer than it may
    let timed_out = client.call(method("thread.get"), json!({"thread": "t1"})).unwrap_err();
    // Then it is a TIMEOUT and the socket is still open
    assert_eq!((timed_out.code.as_str(), client.is_connected()), (code::TIMEOUT, true), "{timed_out}");
    // And the next call gets its own answer, not the late one that arrives first
    assert_eq!(client.call(method("thread.list"), json!({})).unwrap(), json!({"threads": []}));
}

#[test]
fn a_socket_that_drops_after_a_request_was_written_is_disconnected_and_the_request_is_not_sent_again() {
    // Given a service that reads a request and drops the connection, then answers on the one the client opens next
    let fake = Fake::start(vec![
        script(|conn| {
            conn.request().unwrap();
            conn.drop_now();
        }),
        script(|conn| {
            while let Some(request) = conn.request() {
                conn.answer(&request, json!({"threads": []}));
            }
        }),
    ]);
    let mut client = Client::connect(config(&fake, 3)).unwrap();
    // When a call is dropped in the middle
    let dropped = client.call(method("turn.start"), json!({"thread": "t1", "workspace": "w1", "driver": "codex", "prompt": "p"})).unwrap_err();
    // Then it is DISCONNECTED and the socket is closed
    assert_eq!((dropped.code.as_str(), client.is_connected()), (code::DISCONNECTED, false), "{dropped}");
    // And the next call reopens the socket and is the only request the second connection sees: the turn was not started again
    assert_eq!(client.call(method("thread.list"), json!({})).unwrap(), json!({"threads": []}));
    let seen: Vec<(usize, String)> = fake.seen().into_iter().map(|s| (s.connection, s.request.method)).collect();
    assert_eq!(seen, [(0, "turn.start".to_string()), (1, "thread.list".to_string())]);
}

#[test]
fn a_frame_that_is_no_agent_api_frame_is_a_protocol_error_that_ends_that_socket_and_the_client_goes_on_with_a_new_one() {
    // Given a service that answers a request with text that is not JSON, and answers properly on the next connection
    let fake = Fake::start(vec![
        script(|conn| {
            conn.request().unwrap();
            conn.send_text("this is not a frame");
        }),
        script(|conn| {
            let request = conn.request().unwrap();
            conn.answer(&request, json!({"threads": []}));
        }),
    ]);
    let mut client = Client::connect(config(&fake, 3)).unwrap();
    // When the client calls and reads the text
    let broken = client.call(method("thread.list"), json!({})).unwrap_err();
    // Then it is PROTOCOL, and the next call works on a new socket
    assert_eq!(broken.code, code::PROTOCOL, "{broken}");
    assert_eq!(client.call(method("thread.list"), json!({})).unwrap(), json!({"threads": []}));
    assert_eq!(fake.seen().iter().map(|s| s.connection).collect::<Vec<_>>(), [0, 1]);
}

#[test]
fn a_request_frame_is_the_contracts_with_a_fresh_id_each_time() {
    // Given a service that echoes the request it got as the result
    let fake = Fake::start(vec![script(|conn| {
        while let Some(request) = conn.request() {
            conn.answer(&request, Request::to_value(&request));
        }
    })]);
    let mut client = Client::connect(config(&fake, 1)).unwrap();
    // When the client calls twice
    let (a, b) = (client.call(method("thread.get"), json!({"thread": "t1"})).unwrap(), client.call(method("thread.get"), json!({"thread": "t1"})).unwrap());
    // Then each frame is {id, method, params} and the ids differ
    assert_eq!((a["method"].clone(), a["params"].clone()), (json!("thread.get"), json!({"thread": "t1"})));
    assert_ne!(a["id"], b["id"]);
}
