//! A Chromium double on loopback: the DevTools HTTP endpoint (`/json/version`, `/json/list`) and a WebSocket that
//! answers each CDP command with the test's handler, records every command, and sends events the handler or the test
//! push.
use serde_json::{json, Value};
use std::net::TcpListener;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::Message;

#[derive(Clone, Debug)]
pub struct Call {
    pub method: String,
    pub params: Value,
    pub session: Option<String>,
}

/// Answers one command: the result, or the browser's error message. Events pushed to `events` go out before the reply.
/// An error of `NO_REPLY` sends nothing, like a browser that never answers.
pub type Handler = dyn Fn(&Call, &mut Vec<Value>) -> Result<Value, String> + Send + Sync;

pub const NO_REPLY: &str = "no reply";

pub struct FakeBrowser {
    /// `http://127.0.0.1:<port>`, what `Attached::attach` and the launch line take.
    pub endpoint: String,
    pub port: u16,
    pub browser_ws: String,
    pub page_ws: String,
    calls: Arc<Mutex<Vec<Call>>>,
    outboxes: Arc<Mutex<Vec<mpsc::Sender<String>>>>,
}

impl FakeBrowser {
    pub fn start(handler: impl Fn(&Call, &mut Vec<Value>) -> Result<Value, String> + Send + Sync + 'static) -> FakeBrowser {
        let handler: Arc<Handler> = Arc::new(handler);
        let ws = TcpListener::bind("127.0.0.1:0").unwrap();
        let ws_port = ws.local_addr().unwrap().port();
        let browser_ws = format!("ws://127.0.0.1:{ws_port}/devtools/browser/fake");
        let page_ws = format!("ws://127.0.0.1:{ws_port}/devtools/page/T1");
        let calls: Arc<Mutex<Vec<Call>>> = Arc::default();
        let outboxes: Arc<Mutex<Vec<mpsc::Sender<String>>>> = Arc::default();
        {
            let (calls, outboxes) = (Arc::clone(&calls), Arc::clone(&outboxes));
            std::thread::spawn(move || {
                for stream in ws.incoming().flatten() {
                    let (tx, rx) = mpsc::channel();
                    outboxes.lock().unwrap().push(tx);
                    let (calls, handler) = (Arc::clone(&calls), Arc::clone(&handler));
                    std::thread::spawn(move || serve_socket(stream, rx, &calls, &*handler));
                }
            });
        }
        let http = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = http.server_addr().to_ip().unwrap().port();
        let (version, list) = (
            json!({ "Browser": "HeadlessChrome/141.0.7390.37", "webSocketDebuggerUrl": browser_ws }).to_string(),
            json!([{ "id": "T1", "type": "page", "url": "about:blank", "webSocketDebuggerUrl": page_ws }]).to_string(),
        );
        std::thread::spawn(move || {
            for request in http.incoming_requests() {
                let reply = match request.url() {
                    "/json/version" => tiny_http::Response::from_string(version.clone()),
                    "/json/list" => tiny_http::Response::from_string(list.clone()),
                    "/json/garbled" => tiny_http::Response::from_string("<html>"),
                    _ => tiny_http::Response::from_string("").with_status_code(404),
                };
                let _ = request.respond(reply);
            }
        });
        FakeBrowser { endpoint: format!("http://127.0.0.1:{port}"), port, browser_ws, page_ws, calls, outboxes }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub fn calls_to(&self, method: &str) -> Vec<Call> {
        self.calls().into_iter().filter(|c| c.method == method).collect()
    }

    /// An event the browser sends on its own, to every open connection.
    pub fn emit(&self, event: Value) {
        for tx in self.outboxes.lock().unwrap().iter() {
            let _ = tx.send(event.to_string());
        }
    }

    /// The first command matching `pick` once it has arrived (a fire-and-forget answer from the client's own thread).
    pub fn wait_for(&self, pick: impl Fn(&Call) -> bool) -> Call {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(c) = self.calls().into_iter().find(|c| pick(c)) {
                return c;
            }
            assert!(Instant::now() < deadline, "the expected command never reached the browser; got {:?}", self.calls().iter().map(|c| &c.method).collect::<Vec<_>>());
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn serve_socket(stream: std::net::TcpStream, outbox: mpsc::Receiver<String>, calls: &Mutex<Vec<Call>>, handler: &Handler) {
    let Ok(mut socket) = tungstenite::accept(stream) else { return };
    socket.get_ref().set_read_timeout(Some(Duration::from_millis(5))).unwrap();
    loop {
        while let Ok(event) = outbox.try_recv() {
            if socket.send(Message::text(event)).is_err() {
                return;
            }
        }
        let text = match socket.read() {
            Ok(Message::Text(text)) => text.to_string(),
            Ok(Message::Close(_)) => return,
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(_) => return,
        };
        let request: Value = serde_json::from_str(&text).unwrap();
        let call = Call { method: request["method"].as_str().unwrap_or("").to_string(), params: request["params"].clone(), session: request["sessionId"].as_str().map(str::to_string) };
        calls.lock().unwrap().push(call.clone());
        let mut events = Vec::new();
        let mut reply = match handler(&call, &mut events) {
            Ok(result) => json!({ "id": request["id"], "result": result }),
            Err(message) if message == NO_REPLY => continue,
            Err(message) => json!({ "id": request["id"], "error": { "code": -32601, "message": message } }),
        };
        if let Some(s) = &call.session {
            reply["sessionId"] = json!(s);
        }
        for event in events {
            if socket.send(Message::text(event.to_string())).is_err() {
                return;
            }
        }
        if socket.send(Message::text(reply.to_string())).is_err() {
            return;
        }
    }
}

/// The attach handshake of a browser with one blank page `T1`, attached as session `S1`.
pub fn attach_reply(call: &Call) -> Option<Value> {
    match call.method.as_str() {
        "Target.getTargets" => Some(json!({ "targetInfos": [
            { "targetId": "D1", "type": "page", "url": "devtools://devtools/bundled/inspector.html" },
            { "targetId": "T1", "type": "page", "url": "about:blank" },
        ] })),
        "Target.attachToTarget" => Some(json!({ "sessionId": "S1" })),
        "Page.enable" | "Runtime.enable" | "Network.enable" => Some(json!({})),
        _ => None,
    }
}

pub fn value(v: Value) -> Result<Value, String> {
    Ok(json!({ "result": { "type": "object", "value": v } }))
}

/// A grayscale PNG of `w` x `h` whose pixel at (x, y) is `f(x, y)`.
pub fn png_gray(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    let pixels: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| f(x, y)).collect();
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w as u32, h as u32);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    out
}

pub fn png_size(png: &[u8]) -> (u32, u32) {
    (u32::from_be_bytes([png[16], png[17], png[18], png[19]]), u32::from_be_bytes([png[20], png[21], png[22], png[23]]))
}

/// A port nothing listens on.
pub fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}
