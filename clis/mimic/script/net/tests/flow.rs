//! The adapters against a toy server on loopback: raw status, repeated headers, chunked and gzip bodies, no redirect, a refused upgrade,
//! and the interpreter driving a WebSocket session through `TcpTransport` with the system clock.
use flate2::write::GzEncoder;
use flate2::Compression;
use mimic_core::cassette::{Dir, Step};
use mimic_script_core::case::Case;
use mimic_script_core::testing::FixedTarget;
use mimic_script_core::{Interpreter, Policy};
use mimic_script_net::{SystemClock, TcpTransport};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

fn respond(stream: &mut TcpStream, status: &str, headers: &[&str], body: &[u8]) {
    let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for h in headers {
        head.push_str(h);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
}

/// A stream that first hands back the request bytes already read, so the WebSocket handshake can read them again.
struct Replay {
    head: std::io::Cursor<Vec<u8>>,
    stream: TcpStream,
}

impl Read for Replay {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.head.read(buf)? {
            0 => self.stream.read(buf),
            n => Ok(n),
        }
    }
}

impl Write for Replay {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

fn serve(stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut raw = Vec::new();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    raw.extend_from_slice(line.as_bytes());
    let path = line.split(' ').nth(1).unwrap_or("/").to_string();
    let method = line.split(' ').next().unwrap_or("").to_string();
    let (mut upgrade, mut length) = (false, 0usize);
    let mut seen: Vec<String> = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        raw.extend_from_slice(header.as_bytes());
        if header.trim().is_empty() {
            break;
        }
        let lower = header.to_lowercase();
        upgrade |= lower.starts_with("upgrade: websocket");
        if let Some(n) = lower.strip_prefix("content-length:") {
            length = n.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("content-length:") || lower.starts_with("content-type:") {
            seen.push(lower.trim().to_string());
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let mut stream = stream;
    match path.as_str() {
        "/chunked" => respond(&mut stream, "200 OK", &["Transfer-Encoding: chunked"], b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"),
        "/dup" => respond(&mut stream, "204 No Content", &["Set-Cookie: a=1", "set-cookie: b=2"], b""),
        "/redirect" => respond(&mut stream, "302 Found", &["Location: /elsewhere"], b""),
        "/echo" => respond(&mut stream, "200 OK", &["Content-Type: text/plain"], format!("{method} {}", String::from_utf8_lossy(&body)).as_bytes()),
        "/seen" => respond(&mut stream, "200 OK", &["Content-Type: text/plain"], seen.join(";").as_bytes()),
        "/gzip" => {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(br#"{"zipped":true}"#).unwrap();
            respond(&mut stream, "200 OK", &["Content-Encoding: gzip"], &encoder.finish().unwrap());
        }
        "/refuse" if upgrade => respond(&mut stream, "403 Forbidden", &[], b"no"),
        _ if upgrade => {
            let mut ws = tungstenite::accept(Replay { head: std::io::Cursor::new(raw), stream }).unwrap();
            while let Ok(message) = ws.read() {
                match message {
                    tungstenite::Message::Text(t) => {
                        let frame: Value = serde_json::from_str(t.as_str()).unwrap_or(Value::Null);
                        if frame["_tag"] == "Request" {
                            let id = frame["id"].clone();
                            ws.send(tungstenite::Message::text(json!({"_tag": "Chunk", "requestId": id, "values": [1]}).to_string())).unwrap();
                            ws.send(tungstenite::Message::text(json!({"_tag": "Exit", "requestId": id, "exit": {"_tag": "Success"}}).to_string())).unwrap();
                        }
                    }
                    tungstenite::Message::Close(_) => break,
                    _ => {}
                }
            }
        }
        _ => respond(&mut stream, "404 Not Found", &[], b""),
    }
}

fn toy() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream));
        }
    });
    addr
}

fn run(addr: SocketAddr, script: Value) -> Vec<Step> {
    let mut interpreter = Interpreter::new(Box::new(TcpTransport::new(addr)), Box::new(FixedTarget::new("/tmp/ws", "cred")), Box::new(SystemClock::new()), Policy { settle_ms: 60, ..Policy::default() });
    interpreter.run_case(&Case::parse(&json!({"script": script})).unwrap()).unwrap().steps
}

#[test]
fn given_answers_with_chunked_gzip_and_repeated_headers_when_called_then_the_bodies_are_decoded_and_the_headers_kept() {
    let addr = toy();

    let steps = run(addr, json!([{"http": {"path": "/chunked"}}, {"http": {"path": "/dup"}}, {"http": {"path": "/gzip"}}]));

    assert_eq!(steps[1].payload["body"], json!("hello world"));
    assert_eq!(steps[3].payload, json!({"status": 204, "headers": {"set-cookie": "a=1, b=2", "connection": "close"}, "body": null}));
    assert_eq!(steps[5].payload["body"], json!(r#"{"zipped":true}"#));
}

#[test]
fn given_a_redirect_when_called_then_it_is_not_followed_and_a_body_goes_out_with_its_length() {
    let addr = toy();

    let steps = run(addr, json!([{"http": {"path": "/redirect"}}, {"http": {"method": "POST", "path": "/echo", "body": "payload"}}]));

    assert_eq!((steps[1].payload["status"].clone(), steps[1].payload["headers"]["location"].clone()), (json!(302), json!("/elsewhere")));
    assert_eq!(steps[3].payload["body"], json!("POST payload"));
}

#[test]
fn given_a_bodiless_post_and_an_untyped_string_body_when_sent_then_they_go_out_as_fetch_sends_them() {
    let addr = toy();

    let steps = run(addr, json!([
        {"http": {"method": "POST", "path": "/seen"}},
        {"http": {"method": "POST", "path": "/seen", "body": "x"}},
        {"http": {"method": "POST", "path": "/seen", "body": "x", "headers": {"content-type": "application/json"}}},
        {"http": {"method": "GET", "path": "/seen"}}
    ]));

    let seen = |at: usize| steps[at].payload["body"].as_str().unwrap_or("").to_string();
    assert_eq!(seen(1), "content-length: 0");
    assert_eq!(seen(3), "content-length: 1;content-type: text/plain;charset=utf-8");
    assert_eq!(seen(5), "content-length: 1;content-type: application/json");
    assert_eq!(seen(7), "");
}

#[test]
fn given_a_server_that_answers_a_request_with_chunks_when_a_session_runs_then_frames_arrive_in_order_with_acks_and_the_close_ends_it() {
    let addr = toy();

    let steps = run(addr, json!([{"ws-open": {"path": "/ws"}}, {"ws-send": {"_tag": "Request", "id": "1", "tag": "t"}}, {"ws-close": {}}]));

    let seen: Vec<(Dir, String)> = steps.iter().map(|s| (s.dir, s.payload["_tag"].as_str().unwrap_or(s.channel.as_deref().unwrap_or("")).to_string())).collect();
    assert_eq!(&seen[..5], [(Dir::S2c, "ws:open".into()), (Dir::C2s, "Request".into()), (Dir::S2c, "Chunk".into()), (Dir::C2s, "Ack".into()), (Dir::S2c, "Exit".into())]);
    assert_eq!(steps.last().unwrap().payload, json!({"code": null, "reason": "", "tcp": true}));
}

#[test]
fn given_a_refused_upgrade_when_opened_then_the_real_status_is_a_step_and_a_send_fails_for_want_of_a_socket() {
    let addr = toy();

    let steps = run(addr, json!([{"ws-open": {"path": "/refuse"}}, {"ws-send": {"id": "1"}}]));

    assert_eq!(steps[0].payload, json!({"path": "/refuse", "status": 403}));
    assert_eq!(steps.last().unwrap().payload, json!({"transport_error": "no socket"}));
}

#[test]
fn given_nothing_listening_when_called_then_the_failure_is_a_recorded_transport_error() {
    let dead = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();

    let steps = run(dead, json!([{"http": {"path": "/x"}}]));

    assert_eq!(steps[0].payload, json!({"transport_error": "ECONNREFUSED"}));
}

#[test]
fn given_an_open_socket_when_closed_at_once_then_the_close_ends_the_script() {
    let addr = toy();

    let steps = run(addr, json!([{"ws-open": {"path": "/ws"}}, {"ws-close": {}}]));

    assert_eq!(steps.last().unwrap().payload, json!({"code": null, "reason": "", "tcp": true}));
}
