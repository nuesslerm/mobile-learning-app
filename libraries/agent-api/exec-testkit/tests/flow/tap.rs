//! The tap between a client and a service: a real WebSocket proxy over loopback, in front of a fake service that answers the agent API and
//! asks executor tools the way the rca service does.
use exec_testkit::{Seen, Tap};
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::HeaderValue;
use tungstenite::{Message, WebSocket};

/// What the fake service saw of the handshakes: the path and the `authorization` header of each.
type Handshakes = Arc<Mutex<Vec<(String, Option<String>)>>>;

// tungstenite's handshake callback fixes the error type: a whole HTTP response
#[allow(clippy::result_large_err)]
/// A service that takes connections one after another. The agent socket answers each `{id, method}` with a result (an error for
/// `turn.start`) and, after `thread.subscribe`, pushes an event; the executor channel asks `write_file` and `read_file` at once and waits
/// for their results. Any other path, and a bearer other than `good`, is refused with 401.
fn service(good: &'static str) -> (SocketAddr, Handshakes) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Handshakes = Arc::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let log = log.clone();
            std::thread::spawn(move || {
                let mut path = String::new();
                let accepted = tungstenite::accept_hdr(stream, |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
                    path = request.uri().path().to_string();
                    let bearer = request.headers().get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
                    log.lock().unwrap().push((path.clone(), bearer.clone()));
                    if bearer.as_deref() == Some(&format!("Bearer {good}")) {
                        Ok(response)
                    } else {
                        let mut refusal = ErrorResponse::new(Some("no".to_string()));
                        *refusal.status_mut() = tungstenite::http::StatusCode::UNAUTHORIZED;
                        Err(refusal)
                    }
                });
                let Ok(mut socket) = accepted else { return };
                if path == "/exec/v2/executors" {
                    for (id, tool) in [("a", "write_file"), ("b", "read_file")] {
                        socket.send(Message::text(json!({"id": id, "tool": tool, "args": {}}).to_string())).unwrap();
                    }
                }
                while let Ok(Message::Text(text)) = socket.read() {
                    let frame: Value = serde_json::from_str(text.as_str()).unwrap();
                    match frame["method"].as_str() {
                        Some("turn.start") => socket.send(Message::text(json!({"id": frame["id"], "error": {"code": "TURN_RUNNING", "message": "busy"}}).to_string())).unwrap(),
                        Some(method) => {
                            socket.send(Message::text(json!({"id": frame["id"], "result": {"method": method}}).to_string())).unwrap();
                            if method == "thread.subscribe" {
                                socket.send(Message::text(json!({"event": "turn.started", "topic": "thread/t1", "seq": 1}).to_string())).unwrap();
                            }
                        }
                        None => {}
                    }
                }
            });
        }
    });
    (addr, seen)
}

fn connect(tap: &Tap, path: &str, bearer: &str) -> Result<WebSocket<TcpStream>, tungstenite::Error> {
    let address = tap.url().trim_start_matches("http://").to_string();
    let mut request = format!("ws://{address}{path}").into_client_request().unwrap();
    request.headers_mut().insert("authorization", HeaderValue::from_str(&format!("Bearer {bearer}")).unwrap());
    let stream = TcpStream::connect(&address).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    tungstenite::client(request, stream).map(|(socket, _)| socket).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(e) => e,
        other => panic!("{other:?}"),
    })
}

fn ask(socket: &mut WebSocket<TcpStream>, id: u64, method: &str) -> Value {
    socket.send(Message::text(json!({"id": id, "method": method, "params": {}}).to_string())).unwrap();
    loop {
        let Message::Text(text) = socket.read().unwrap() else { continue };
        let frame: Value = serde_json::from_str(text.as_str()).unwrap();
        if frame["id"] == json!(id) {
            return frame;
        }
    }
}

#[test]
fn given_a_client_of_the_agent_socket_when_it_calls_through_the_tap_then_it_gets_the_services_answers_and_the_tap_saw_each_call_in_its_window() {
    // Given a service and a tap in front of it
    let (service, handshakes) = service("device-bearer");
    let tap = Tap::to(service);
    // When a client calls three methods through the tap, the second of which the service refuses, and one event is pushed meanwhile
    let mut socket = connect(&tap, "/agent/v1/socket", "device-bearer").expect("the tap lets the client in");
    let first_window = tap.mark();
    let created = ask(&mut socket, 1, "thread.create");
    let refused = ask(&mut socket, 2, "turn.start");
    let second_window = tap.mark();
    let subscribed = ask(&mut socket, 3, "thread.subscribe");
    let event = socket.read().unwrap();
    // Then each answer is the service's own, the event arrived, and the client's bearer reached the service on the same path
    assert_eq!((created["result"]["method"].clone(), refused["error"]["code"].clone(), subscribed["result"]["method"].clone()), (json!("thread.create"), json!("TURN_RUNNING"), json!("thread.subscribe")));
    assert!(event.into_text().unwrap().as_str().contains("turn.started"));
    assert_eq!(handshakes.lock().unwrap().as_slice(), [("/agent/v1/socket".to_string(), Some("Bearer device-bearer".to_string()))]);
    // And the tap saw the calls in the order the service answered them, and a window holds only what came after its mark
    let agent = |method: &str, ok: bool, code: Option<&str>| Seen::Agent { method: method.into(), ok, code: code.map(Into::into) };
    assert_eq!(tap.since(first_window), [agent("thread.create", true, None), agent("turn.start", false, Some("TURN_RUNNING")), agent("thread.subscribe", true, None)]);
    assert_eq!(tap.since(second_window), [agent("thread.subscribe", true, None)]);
}

#[test]
fn given_an_executor_when_it_answers_the_tools_the_service_asks_through_the_tap_then_the_tap_saw_each_tool_with_its_outcome() {
    // Given a tap in front of a service that asks write_file and read_file as soon as an executor attaches
    let (service, _) = service("secret");
    let tap = Tap::to(service);
    let mark = tap.mark();
    // When the executor attaches through the tap and answers the first tool ok and the second with an error
    let mut socket = connect(&tap, "/exec/v2/executors?workspace=w1", "secret").expect("the tap lets the executor in");
    let mut asked = Vec::new();
    while asked.len() < 2 {
        let Message::Text(text) = socket.read().unwrap() else { continue };
        asked.push(serde_json::from_str::<Value>(text.as_str()).unwrap());
    }
    socket.send(Message::text(json!({"id": "a", "stream": "stdout", "data": "x"}).to_string())).unwrap();
    socket.send(Message::text(json!({"id": "a", "ok": true, "result": {}}).to_string())).unwrap();
    socket.send(Message::text(json!({"id": "b", "ok": false, "error": {"code": "NOT_FOUND", "message": "no"}}).to_string())).unwrap();
    for _ in 0..200 {
        if tap.since(mark).len() == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Then the service's requests came through unchanged, and the tap saw both tools at their result frames
    assert_eq!(asked.iter().map(|a| a["tool"].as_str().unwrap()).collect::<Vec<_>>(), ["write_file", "read_file"]);
    assert_eq!(tap.since(mark), [Seen::Executor { tool: "write_file".into(), ok: true, code: None }, Seen::Executor { tool: "read_file".into(), ok: false, code: Some("NOT_FOUND".into()) }]);
}

#[test]
fn given_a_bearer_the_service_refuses_when_a_client_connects_through_the_tap_then_it_gets_the_services_401_and_nothing_is_seen() {
    // Given a tap in front of a service that takes one bearer
    let (service, handshakes) = service("device-bearer");
    let tap = Tap::to(service);
    // When a client connects with another bearer
    let refused = connect(&tap, "/agent/v1/socket", "stolen");
    // Then the handshake fails with the service's own status, the service saw the attempt, and the tap saw no call
    match refused {
        Err(tungstenite::Error::Http(response)) => assert_eq!(response.status().as_u16(), 401),
        other => panic!("expected the service's 401, got {:?}", other.map(|_| ())),
    }
    assert_eq!(handshakes.lock().unwrap().len(), 1);
    assert!(tap.since(0).is_empty());
}
