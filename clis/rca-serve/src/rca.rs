use rca_mobile_core::{Agent, Failure, ReadWrite, Result};
use serde_json::{json, Value};
use std::{
    io::ErrorKind,
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tungstenite::{client::IntoClientRequest, Message, WebSocket};

pub struct Rca {
    pub upstream: String,
    pub bearer: String,
}

const TIMEOUT: Duration = Duration::from_secs(20);

fn disconnected(error: impl std::fmt::Display) -> Failure {
    Failure::new("DISCONNECTED", error.to_string())
}

impl Rca {
    fn open(&self, method: &str, params: Value) -> Result<WebSocket<TcpStream>> {
        let authority = self
            .upstream
            .trim_end_matches('/')
            .strip_prefix("ws://")
            .filter(|s| !s.is_empty() && !s.contains('/'))
            .ok_or_else(|| Failure::new("CONFIG", "Expected a ws:// RCA host and port"))?;
        let mut request = format!("ws://{authority}/agent/v1/socket")
            .into_client_request()
            .map_err(disconnected)?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", self.bearer)
                .parse()
                .map_err(|_| Failure::new("CONFIG", "Invalid upstream bearer"))?,
        );
        let stream = authority
            .to_socket_addrs()
            .map_err(disconnected)?
            .find_map(|addr| TcpStream::connect_timeout(&addr, TIMEOUT).ok())
            .ok_or_else(|| Failure::new("DISCONNECTED", "Cannot reach RCA"))?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(disconnected)?;
        stream
            .set_write_timeout(Some(TIMEOUT))
            .map_err(disconnected)?;
        let (mut socket, _) = tungstenite::client(request, stream).map_err(disconnected)?;
        socket
            .send(Message::Text(
                json!({"id":1,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(disconnected)?;
        Ok(socket)
    }

    fn next(socket: &mut WebSocket<TcpStream>) -> Result<Option<Value>> {
        match socket.read() {
            Ok(Message::Text(text)) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| Failure::new("BAD_RESPONSE", e.to_string())),
            Ok(Message::Ping(_)) => {
                socket.flush().map_err(disconnected)?;
                Ok(None)
            }
            Ok(Message::Pong(_)) => Ok(None),
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
            {
                Ok(None)
            }
            Ok(_) => Err(Failure::new("DISCONNECTED", "RCA closed the connection")),
            Err(e) => Err(disconnected(e)),
        }
    }

    fn response(frame: &Value) -> Result<&Value> {
        if let Some(error) = frame.get("error") {
            return Err(Failure::new(
                error["code"].as_str().unwrap_or("UPSTREAM"),
                error["message"]
                    .as_str()
                    .unwrap_or("RCA refused this request"),
            ));
        }
        frame
            .get("result")
            .ok_or_else(|| Failure::new("BAD_RESPONSE", "Missing RCA result"))
    }

    pub fn follow(
        &self,
        thread: &str,
        mut after: u64,
        send: mpsc::Sender<Result<Value>>,
    ) -> Result<()> {
        let mut socket = self.open(
            "thread.subscribe",
            json!({"thread":thread,"after_seq":after}),
        )?;
        socket
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(1)))
            .map_err(disconnected)?;
        while !send.is_closed() {
            let Some(frame) = Self::next(&mut socket)? else {
                continue;
            };
            if frame["id"] == 1 {
                Self::response(&frame)?;
                continue;
            }
            let event = &frame["event"];
            if event["thread"] != thread {
                continue;
            }
            if let Some(seq) = event["seq"].as_u64().filter(|seq| *seq > after) {
                if send.blocking_send(Ok(frame)).is_err() {
                    break;
                }
                after = seq;
            }
        }
        Ok(())
    }
}

impl Agent for Rca {
    fn request(&self, action: ReadWrite, params: Value) -> Result<Value> {
        use ReadWrite::*;
        let method = match action {
            Repositories => "repo.list",
            CreateWorkspace => "worktree.create",
            Threads => "thread.list",
            CreateThread => "thread.create",
            GetThread => "thread.get",
            History => "thread.history",
            Transcript => "thread.transcript",
            Family => "thread.family",
            Issues => "issue.list",
            GetContext => "state.get",
            SetContext => "state.set",
            Models => "models.list",
            AddTurn => "queue.add",
            ResumeQueue => "queue.resume",
            Approval => "approval.answer",
            Input => "input.answer",
        };
        let mut socket = self.open(method, params)?;
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if let Some(frame) = Self::next(&mut socket)? {
                if frame["id"] == 1 {
                    return Self::response(&frame).cloned();
                }
            }
        }
        Err(Failure::new(
            "TIMEOUT",
            format!("RCA did not answer {method}"),
        ))
    }
}
