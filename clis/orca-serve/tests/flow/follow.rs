//! A followed stream over a real socket: the events of an attach come once and in order, and a dropped socket resumes after the last offset.
use agent_api_proto::Request;
use agent_api_terminal::output_event;
use orca_core::AgentApi;
use orca_serve::adapters::rca::RcaAgent;
use serde_json::{json, Value};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

fn read_request(ws: &mut WebSocket<TcpStream>, seen: &Mutex<Vec<Request>>) -> Request {
    loop {
        if let Message::Text(text) = ws.read().unwrap() {
            let request = Request::parse(&text).unwrap();
            seen.lock().unwrap().push(request.clone());
            return request;
        }
    }
}

fn push(ws: &mut WebSocket<TcpStream>, offset: u64, bytes: &[u8]) {
    ws.send(Message::Text(output_event("w", "t", offset, bytes).encode().into())).unwrap();
}

#[test]
fn given_an_attach_whose_socket_drops_after_two_events_when_followed_then_all_three_come_in_order_and_the_reconnect_resumes_after_offset_9() {
    // Given a fake agent socket that answers terminal.attach, pushes the output ending at offsets 5 and 9, drops, and after the reconnect pushes the output ending at 12
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let authority = listener.local_addr().unwrap().to_string();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let server = {
        let seen = seen.clone();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let request = read_request(&mut ws, &seen);
            ws.send(Message::Text(json!({"id": request.id, "result": {"offset": 0}}).to_string().into())).unwrap();
            push(&mut ws, 0, b"hello");
            push(&mut ws, 5, b"abcd");
            std::thread::sleep(Duration::from_millis(150));
            let _ = ws.get_ref().shutdown(Shutdown::Both);
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let request = read_request(&mut ws, &seen);
            ws.send(Message::Text(json!({"id": request.id, "result": {"offset": 9}}).to_string().into())).unwrap();
            push(&mut ws, 9, b"xyz");
            while ws.read().is_ok() {}
        })
    };
    let agent = RcaAgent::new(&authority, "bearer");
    // When it is followed and read for three events
    let (answer, mut stream) = agent.follow("terminal.attach", "terminal/w/t", json!({"workspace": "w", "terminal": "t", "after_offset": 0}), "after_offset").unwrap();
    let mut events = vec![];
    while events.len() < 3 {
        events.push(stream.next(Duration::from_secs(5)).unwrap().expect("an event before the wait ran out"));
    }
    drop(stream);
    server.join().unwrap();
    // Then it has the service's answer and the three events in order, and the second attach asked to start after offset 9
    assert_eq!(answer, json!({"offset": 0}));
    assert_eq!(events.iter().map(|e| (e.seq, e.kind.as_str())).collect::<Vec<_>>(), [(5, "terminal.output"), (9, "terminal.output"), (12, "terminal.output")]);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.iter().map(|r| r.params["after_offset"].clone()).collect::<Vec<Value>>(), [json!(0), json!(9)]);
}
