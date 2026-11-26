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

pub fn upgrade(
    stream: TcpStream,
    request: &Request,
    mut connection: Connection,
    stop: &AtomicBool,
    span: tracing::span::EnteredSpan,
) {
    let Some(mut ws) = accept(stream, request, span) else {
        return;
    };
    let send = |ws: &mut WebSocket<TcpStream>, frames: Vec<String>| {
        frames
            .into_iter()
            .all(|f| ws.send(Message::text(f)).is_ok())
    };
    while !stop.load(Ordering::SeqCst) {
        let frames = match ws.read() {
            Ok(Message::Text(text)) => connection.on_message(text.as_str()),
            Ok(Message::Close(_)) => break,
            Ok(_) => vec![],
            Err(Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                vec![]
            }
            Err(_) => break,
        };
        if !send(&mut ws, frames) || !send(&mut ws, connection.poll()) {
            break;
        }
        let _ = ws.flush();
    }
    let _ = ws.close(None);
}

fn accept(
    mut stream: TcpStream,
    request: &Request,
    span: tracing::span::EnteredSpan,
) -> Option<WebSocket<TcpStream>> {
    let upgrading = request
        .header("upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let Some(key) = request.header("sec-websocket-key").filter(|_| upgrading) else {
        respond(
            &mut stream,
            400,
            &json!({"code": "bad_request", "message": "A WebSocket upgrade is required."}),
        );
        return None;
    };
    let accept = derive_accept_key(key.as_bytes());
    let head = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
    if stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.flush())
        .is_err()
    {
        return None;
    }
    span.record("http.response.status_code", 101);
    drop(span);
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(20)));
    let config = WebSocketConfig::default().max_message_size(Some(1 << 20));
    Some(WebSocket::from_raw_socket(
        stream,
        Role::Server,
        Some(config),
    ))
}

pub fn preview(
    stream: TcpStream,
    request: &Request,
    mut viewer: t3_core::usecases::preview_viewer::Viewer,
    stop: &AtomicBool,
    span: tracing::span::EnteredSpan,
) {
    let Some(mut ws) = accept(stream, request, span) else {
        return;
    };
    let _ = ws.get_mut().set_write_timeout(Some(Duration::from_secs(5)));
    for event in [viewer.control(), viewer.viewport()] {
        if ws.send(Message::text(event.to_string())).is_err() {
            return;
        }
    }
    let mut pending = false;
    let mut next = std::time::Instant::now();
    while !stop.load(Ordering::SeqCst) {
        if !pending && std::time::Instant::now() >= next {
            match viewer.frame() {
                Ok(jpeg) => {
                    if ws.send(Message::binary(jpeg)).is_err() {
                        break;
                    }
                    pending = true;
                    next = std::time::Instant::now() + Duration::from_millis(100);
                }
                _ => break,
            }
        }
        match ws.read() {
            Ok(Message::Text(raw)) => {
                let Ok(msg) = serde_json::from_str::<serde_json::Value>(&raw) else {
                    continue;
                };
                if msg["type"] == "ack" {
                    pending = false;
                    continue;
                }
                match viewer.input(&msg) {
                    Ok(Some(event)) => {
                        if ws.send(Message::text(event.to_string())).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        if ws
                            .send(Message::text(
                                json!({"type":"error","message":e.message()}).to_string(),
                            ))
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => break,
            Err(Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
            _ => {}
        }
    }
    let _ = ws.close(None);
}
