//! `TcpCandidate`: the `Candidate` port over a loopback socket. It sends a recorded client's requests to a server under check as
//! HTTP/1.1 and as WebSocket frames, and hands back what the server answers.
use super::conn::dial;
use super::http1::{bodiless, body, read_head, status_of};
use mimic_core::wire::{encode_frame, FrameParser, WsMessage, OP_BINARY, OP_CLOSE, OP_TEXT};
use mimic_proxy_core::application::ports::{Candidate, HttpAnswer};
use mimic_proxy_core::domain::ordering::Payload;
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const OP_PING: u8 = 9;
const OP_PONG: u8 = 10;
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
const SOCKET_KEY: &str = "bWltaWMtcHJveHktY2hlY2s=";
const MASK: [u8; 4] = [0x6d, 0x69, 0x6d, 0x63];

struct Socket {
    stream: TcpStream,
    parser: FrameParser,
    waiting: VecDeque<WsMessage>,
}

pub struct TcpCandidate {
    addr: SocketAddr,
    socket: Option<Socket>,
}

fn told(e: std::io::Error) -> String {
    format!("the candidate did not answer: {e}")
}

impl TcpCandidate {
    pub fn new(addr: SocketAddr) -> TcpCandidate {
        TcpCandidate { addr, socket: None }
    }

    fn connect(&self) -> Result<TcpStream, String> {
        let stream = dial(self.addr).map_err(|e| format!("the candidate is not listening on {}: {e}", self.addr))?;
        stream.set_read_timeout(Some(ANSWER_TIMEOUT)).and_then(|()| stream.set_write_timeout(Some(ANSWER_TIMEOUT))).map_err(told)?;
        Ok(stream)
    }
}

impl Candidate for TcpCandidate {
    fn http(&mut self, method: &str, target: &str, headers: &[(String, String)], request: &[u8]) -> Result<HttpAnswer, String> {
        let mut stream = self.connect()?;
        let mut head = format!("{method} {target} HTTP/1.1\r\nHost: {}\r\n", self.addr);
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        write!(stream, "{head}Content-Length: {}\r\nConnection: close\r\n\r\n", request.len()).and_then(|()| stream.write_all(request)).map_err(told)?;
        let (answer, early) = read_head(&mut stream).map_err(told)?;
        let status = status_of(&answer.start);
        let answered = if bodiless(status, method) { Vec::new() } else { body(&mut stream, &answer, early, true).map_err(told)?.observed().to_vec() };
        Ok(HttpAnswer { status, headers: answer.headers, body: answered })
    }

    fn open(&mut self, target: &str) -> Result<u16, String> {
        let mut stream = self.connect()?;
        write!(stream, "GET {target} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {SOCKET_KEY}\r\nSec-WebSocket-Version: 13\r\n\r\n", self.addr).map_err(told)?;
        let (answer, early) = read_head(&mut stream).map_err(told)?;
        let status = status_of(&answer.start);
        if status == 101 {
            let mut parser = FrameParser::default();
            let waiting = parser.push(&early).into();
            self.socket = Some(Socket { stream, parser, waiting });
        }
        Ok(status)
    }

    fn send(&mut self, message: &Payload) -> Result<(), String> {
        let socket = self.socket.as_mut().ok_or("no socket is open")?;
        let frame = match message {
            Payload::Json(value) => encode_frame(OP_TEXT, value.to_string().as_bytes(), Some(MASK)),
            Payload::Text(text) => encode_frame(OP_TEXT, text.as_bytes(), Some(MASK)),
            Payload::Binary(bytes) => encode_frame(OP_BINARY, bytes, Some(MASK)),
        };
        socket.stream.write_all(&frame).map_err(told)
    }

    fn receive(&mut self, wait_ms: u64) -> Result<Option<Payload>, String> {
        let Some(socket) = self.socket.as_mut() else { return Ok(None) };
        socket.stream.set_read_timeout(Some(Duration::from_millis(wait_ms.max(1)))).map_err(told)?;
        let mut chunk = [0u8; 8192];
        loop {
            while let Some(message) = socket.waiting.pop_front() {
                match message.opcode {
                    OP_CLOSE => {
                        self.socket = None;
                        return Ok(None);
                    }
                    OP_PING => socket.stream.write_all(&encode_frame(OP_PONG, &message.payload, Some(MASK))).map_err(told)?,
                    OP_PONG => {}
                    OP_TEXT => {
                        return Ok(Some(match serde_json::from_slice(&message.payload) {
                            Ok(value) => Payload::Json(value),
                            Err(_) => Payload::Text(String::from_utf8_lossy(&message.payload).into_owned()),
                        }))
                    }
                    _ => return Ok(Some(Payload::Binary(message.payload))),
                }
            }
            match socket.stream.read(&mut chunk) {
                Ok(0) => {
                    self.socket = None;
                    return Ok(None);
                }
                Ok(n) => socket.waiting.extend(socket.parser.push(&chunk[..n])),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(None),
                Err(e) => return Err(told(e)),
            }
        }
    }

    fn close(&mut self) {
        if let Some(mut socket) = self.socket.take() {
            let _ = socket.stream.write_all(&encode_frame(OP_CLOSE, &1000u16.to_be_bytes(), Some(MASK)));
        }
    }
}
