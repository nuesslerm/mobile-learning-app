//! A mapper's side of the agent socket for the flows: the handshake with the service's own answer when it fails, and a client that matches an
//! answer to its request by id and keeps the events that arrive meanwhile in the order the service sent them.
use agent_api_proto::Event;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

pub type Socket = WebSocketStream<TcpStream>;

/// How long a flow waits for a frame the service should send.
pub const WAIT: Duration = Duration::from_secs(20);

/// A WebSocket handshake that failed with the service's own answer: its status.
pub async fn handshake(port: u16, path: &str, headers: &[(&str, String)]) -> Result<Socket, u16> {
    let mut request = format!("ws://127.0.0.1:{port}{path}").into_client_request().unwrap();
    for (name, value) in headers {
        request.headers_mut().insert(tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(name.as_bytes()).unwrap(), value.parse().unwrap());
    }
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("the service is listening");
    match tokio_tungstenite::client_async(request, tcp).await {
        Ok((stream, _)) => Ok(stream),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => Err(response.status().as_u16()),
        Err(e) => panic!("the handshake failed: {e}"),
    }
}

/// A mapper's side of the socket: a request is answered by the frame with its id, and the events that arrive meanwhile are kept in order.
pub struct Agent {
    socket: Socket,
    next: u64,
    events: VecDeque<Event>,
}

impl Agent {
    pub async fn open(port: u16, bearer: &str) -> Agent {
        let socket = handshake(port, "/agent/v1/socket", &[("authorization", format!("Bearer {bearer}"))]).await.unwrap_or_else(|status| panic!("the agent socket was refused with {status}"));
        Agent { socket, next: 1, events: VecDeque::new() }
    }

    async fn frame(&mut self) -> Value {
        let read = async {
            loop {
                match self.socket.next().await.expect("the socket stays open").expect("a frame") {
                    Message::Text(text) => return serde_json::from_str::<Value>(&text).expect("a JSON frame"),
                    _ => continue,
                }
            }
        };
        tokio::time::timeout(WAIT, read).await.expect("a frame arrives")
    }

    pub async fn ask(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.socket.send(Message::Text(json!({"id": id, "method": method, "params": params}).to_string().into())).await.unwrap();
        loop {
            let frame = self.frame().await;
            if frame.get("event").is_some() {
                self.events.push_back(Event::from_value(&frame).expect("an event frame"));
            } else if frame["id"] == json!(id) {
                return frame;
            }
        }
    }

    /// The next event of the stream, in the order the service sent it.
    pub async fn event(&mut self) -> Event {
        if let Some(event) = self.events.pop_front() {
            return event;
        }
        let frame = self.frame().await;
        Event::from_value(&frame).unwrap_or_else(|e| panic!("{frame} is no event: {e}"))
    }

    /// The events up to and including the first of `kind`.
    pub async fn events_through(&mut self, kind: &str) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            let event = self.event().await;
            let done = event.kind == kind;
            seen.push(event);
            if done {
                return seen;
            }
        }
    }
}
