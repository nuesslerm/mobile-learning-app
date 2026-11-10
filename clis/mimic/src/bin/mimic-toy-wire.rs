//! The toy `ws+http` reference of the tap flow tests: a `mimic-driver/1` server whose target is a small HTTP and WebSocket server in this
//! process, reached through a `Tap`. `http` ops (`{method, path, body}`) and `ws` ops (`{path, frames: [text or {"binary": n}]}`) are
//! made as a client would, and the steps are what the tap saw. The server makes up an id and a time per reply. `--tweak change-value`
//! plants a behaviour change, for a candidate.
use mimic_core::cassette::{Outcome, Status};
use mimic_core::ports::{Driver, DriverError, Hello, Observed, Request, Rng};
use mimic_core::presets;
use mimic_core::testing::driver_info;
use mimic_core::wire::{encode_frame, steps, FrameParser, Wire, OP_BINARY, OP_CLOSE, OP_TEXT};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use wf_mimic::adapters::{entropy, serve, Carry, Tap};

fn uuid(rng: &mut dyn Rng) -> String {
    let (a, b) = (rng.next_u64(), rng.next_u64());
    format!("{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}", a >> 32, (a >> 16) & 0xffff, a & 0xfff, (b >> 48) & 0xfff, b & 0xffff_ffff_ffff)
}

fn reply(tweak: bool, what: &str, rng: &mut dyn Rng) -> Value {
    let what = if tweak { "changed" } else { what };
    json!({"echo": what, "session": uuid(rng), "at": "2026-11-06T10:11:12.345Z"})
}

fn head(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 2048]);
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            return (String::from_utf8_lossy(&buf[..end]).into_owned(), buf[end + 4..].to_vec());
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return (String::new(), Vec::new()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn answer(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let _ = write!(stream, "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
}

fn serve_connection(mut stream: TcpStream, tweak: bool) {
    let mut rng = entropy();
    let (text, rest) = head(&mut stream);
    let (start, lower) = (text.lines().next().unwrap_or("").to_string(), text.to_lowercase());
    let path = start.split_whitespace().nth(1).unwrap_or("/").to_string();
    if !lower.contains("upgrade: websocket") {
        let length = lower.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
        let mut body = rest;
        while body.len() < length {
            let mut chunk = [0u8; 512];
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.extend_from_slice(&chunk[..n]),
            }
        }
        match path.as_str() {
            "/ws" => answer(&mut stream, "426 Upgrade Required", "application/json", &json!({"code": "upgrade_required", "at": "2026-11-06T10:11:12.345Z"}).to_string()),
            p if p.starts_with("/echo") => {
                let said: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                answer(&mut stream, "200 OK", "application/json", &reply(tweak, said["say"].as_str().unwrap_or(""), &mut rng).to_string());
            }
            _ => answer(&mut stream, "404 Not Found", "text/plain", "no such route"),
        }
        return;
    }
    let _ = write!(stream, "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: toy\r\n\r\n");
    let (mut parser, mut chunk) = (FrameParser::default(), [0u8; 2048]);
    let mut pending = rest;
    loop {
        for m in parser.push(&pending) {
            let out = match m.opcode {
                OP_CLOSE => {
                    let _ = stream.write_all(&encode_frame(OP_CLOSE, &m.payload, None));
                    return;
                }
                OP_BINARY => encode_frame(OP_BINARY, &m.payload.iter().rev().copied().collect::<Vec<u8>>(), None),
                _ => {
                    let asked: Value = serde_json::from_slice(&m.payload).unwrap_or(Value::Null);
                    let body = json!({"id": asked["id"], "result": reply(tweak, asked["method"].as_str().unwrap_or(""), &mut rng)});
                    encode_frame(OP_TEXT, body.to_string().as_bytes(), None)
                }
            };
            let _ = stream.write_all(&out);
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => pending = chunk[..n].to_vec(),
        }
    }
}

fn start_server(tweak: bool) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let addr = listener.local_addr().expect("its address");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve_connection(stream, tweak));
        }
    });
    addr
}

struct WireToy {
    tap: Tap,
    keys: u32,
}

impl WireToy {
    fn connect(&self) -> TcpStream {
        TcpStream::connect(self.tap.url().trim_start_matches("http://")).expect("the tap is listening")
    }

    fn http(&self, input: &Value) {
        let (method, path) = (input["method"].as_str().unwrap_or("GET"), input["path"].as_str().unwrap_or("/"));
        let body = input.get("body").map(Value::to_string).unwrap_or_default();
        let mut s = self.connect();
        let _ = write!(s, "{method} {path} HTTP/1.1\r\nhost: toy\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        let _ = s.read_to_end(&mut Vec::new());
    }

    fn ws(&mut self, input: &Value) {
        let path = input["path"].as_str().unwrap_or("/ws");
        let mut s = self.connect();
        let _ = write!(s, "GET {path} HTTP/1.1\r\nhost: toy\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-key: dG95\r\nsec-websocket-version: 13\r\nsec-websocket-extensions: permessage-deflate\r\n\r\n");
        let (answer, rest) = head(&mut s);
        if !answer.starts_with("HTTP/1.1 101") {
            let _ = s.read_to_end(&mut Vec::new());
            return;
        }
        let mut parser = FrameParser::default();
        let mut have = parser.push(&rest).len();
        // one frame at a time, as a client of a request and answer protocol does: the order the tap sees is then the order of causes
        for frame in input["frames"].as_array().cloned().unwrap_or_default() {
            self.keys += 1;
            let key = self.keys.to_be_bytes();
            let wire = match frame["binary"].as_u64() {
                Some(n) => encode_frame(OP_BINARY, &(0..n as u8).collect::<Vec<u8>>(), Some(key)),
                None => encode_frame(OP_TEXT, frame.to_string().as_bytes(), Some(key)),
            };
            let _ = s.write_all(&wire);
            let want = have + 1;
            while have < want {
                let mut chunk = [0u8; 2048];
                match s.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => have += parser.push(&chunk[..n]).len(),
                }
            }
        }
        let _ = s.write_all(&encode_frame(OP_CLOSE, &[&1000u16.to_be_bytes()[..], b"done"].concat(), Some([1, 2, 3, 4])));
        let _ = s.read_to_end(&mut Vec::new());
    }
}

impl Driver for WireToy {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        Ok(Hello { driver: driver_info("toy-wire"), target_version: Some("toy-wire-1.0".into()), coverage: false })
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        let mark = self.tap.mark();
        match request.op.as_deref() {
            Some("http") => self.http(&request.input),
            Some("ws") => self.ws(&request.input),
            other => return Err(DriverError(format!("toy-wire has no op {other:?}"))),
        }
        let compiled = presets::t3().compile().map_err(|e| DriverError(e.to_string()))?;
        let steps = steps(&self.tap.since(mark), &mut compiled.normalizer());
        Ok(Observed { steps, effects: Vec::new(), outcome: Outcome { status: Status::Ok, code: None, reference_accepts: Some(true) } })
    }
}

fn main() {
    let tweak = std::env::args().skip(1).any(|a| a == "change-value");
    let tap = Tap::to(start_server(tweak), Carry::Both).expect("the tap listens");
    if let Err(e) = serve(&mut WireToy { tap, keys: 0 }, std::io::stdin().lock(), std::io::stdout().lock()) {
        eprintln!("mimic-toy-wire: {e}");
        std::process::exit(1);
    }
}
