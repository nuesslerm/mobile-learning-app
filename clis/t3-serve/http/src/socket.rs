//! The `/ws` upgrade and the socket loop: read a frame (20 ms at most), answer it, then flush what live events queued.
use crate::http::{respond, Request};
use serde_json::json;
use std::io::{ErrorKind, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use t3_core::rpc::Connection;
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::{Error, Message, WebSocket};

pub fn upgrade(mut stream: TcpStream, request: &Request, mut connection: Connection, stop: &AtomicBool) {
    let upgrading = request.header("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let Some(key) = request.header("sec-websocket-key").filter(|_| upgrading) else {
        return respond(&mut stream, 400, &json!({"code": "bad_request", "message": "A WebSocket upgrade is required."}));
    };
    let accept = derive_accept_key(key.as_bytes());
    let head = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
    if stream.write_all(head.as_bytes()).and_then(|()| stream.flush()).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(20)));
    let config = WebSocketConfig::default().max_message_size(Some(1 << 20));
    let mut ws = WebSocket::from_raw_socket(stream, Role::Server, Some(config));
    let send = |ws: &mut WebSocket<TcpStream>, frames: Vec<String>| frames.into_iter().all(|f| ws.send(Message::text(f)).is_ok());
    while !stop.load(Ordering::SeqCst) {
        let frames = match ws.read() {
            Ok(Message::Text(text)) => connection.on_message(text.as_str()),
            Ok(Message::Close(_)) => break,
            Ok(_) => vec![],
            Err(Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => vec![],
            Err(_) => break,
        };
        if !send(&mut ws, frames) || !send(&mut ws, connection.poll()) {
            break;
        }
        let _ = ws.flush();
    }
    let _ = ws.close(None);
}
