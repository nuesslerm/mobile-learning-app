//! An in-process agent API server: it accepts connections one after another and plays one script on each, the way a service would answer
//! (recorded exchanges, events, a drop). Every request it receives and every handshake it sees is kept for the test to read.
use agent_api_proto::{Event, Request};
use serde_json::{json, Value};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tungstenite::handshake::server::{ErrorResponse, Request as HttpRequest, Response as HttpResponse};
use tungstenite::{Message, WebSocket};

pub const BEARER: &str = "device-bearer";

pub type Script = Box<dyn FnOnce(&mut Conn) + Send>;

pub fn script(play: impl FnOnce(&mut Conn) + Send + 'static) -> Script {
    Box::new(play)
}

/// A request the server received, and on which connection (0 is the first that completed its handshake).
#[derive(Debug, Clone)]
pub struct Seen {
    pub connection: usize,
    pub request: Request,
}

#[derive(Default)]
struct Log {
    seen: Mutex<Vec<Seen>>,
    /// The `authorization` header of each handshake, accepted or not.
    handshakes: Mutex<Vec<Option<String>>>,
    /// Every TCP connection the server accepted, whatever became of it.
    tcp: AtomicUsize,
}

pub struct Fake {
    pub url: String,
    log: Arc<Log>,
    stop: Arc<AtomicBool>,
    port: u16,
    thread: Option<JoinHandle<()>>,
}

pub struct Conn {
    ws: WebSocket<TcpStream>,
    index: usize,
    log: Arc<Log>,
}

impl Fake {
    /// Serves `scripts` in order, one per connection whose handshake carried `BEARER`. Once they are used up the server stays up and closes
    /// every connection at once, so a client that reconnects finds a service that no longer answers.
    pub fn start(scripts: Vec<Script>) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (log, stop) = (Arc::new(Log::default()), Arc::new(AtomicBool::new(false)));
        let thread = {
            let (log, stop) = (log.clone(), stop.clone());
            std::thread::spawn(move || serve(listener, scripts, log, stop))
        };
        Fake { url: format!("ws://127.0.0.1:{port}"), log, stop, port, thread: Some(thread) }
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.log.seen.lock().unwrap().clone()
    }

    pub fn handshakes(&self) -> Vec<Option<String>> {
        self.log.handshakes.lock().unwrap().clone()
    }

    pub fn tcp_connections(&self) -> usize {
        self.log.tcp.load(Ordering::SeqCst)
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The accept that waits is woken by a connection of our own
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// The handshake callback's error is tungstenite's own response type
#[allow(clippy::result_large_err)]
fn serve(listener: TcpListener, scripts: Vec<Script>, log: Arc<Log>, stop: Arc<AtomicBool>) {
    let mut scripts = scripts.into_iter();
    let mut index = 0;
    loop {
        let Ok((stream, _)) = listener.accept() else { return };
        if stop.load(Ordering::SeqCst) {
            return;
        }
        log.tcp.fetch_add(1, Ordering::SeqCst);
        let Some(script) = scripts.next() else {
            // Nothing left to play: the connection is closed as it comes
            let _ = stream.shutdown(Shutdown::Both);
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let handshakes = log.clone();
        let accepted = tungstenite::accept_hdr(stream, move |req: &HttpRequest, resp: HttpResponse| -> Result<HttpResponse, ErrorResponse> {
            let auth = req.headers().get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
            let good = auth.as_deref() == Some(&format!("Bearer {BEARER}"));
            handshakes.handshakes.lock().unwrap().push(auth);
            if good {
                Ok(resp)
            } else {
                Err(tungstenite::http::Response::builder().status(401).body(Some("unauthorized".to_string())).unwrap())
            }
        });
        match accepted {
            Ok(ws) => {
                script(&mut Conn { ws, index, log: log.clone() });
                index += 1;
            }
            // A refused handshake plays no script: the next connection gets it
            Err(_) => {
                let rest: Vec<Script> = std::iter::once(script).chain(scripts).collect();
                scripts = rest.into_iter();
            }
        }
    }
}

impl Conn {
    /// The next request the client sends, none when it closed, dropped or sent nothing for ten seconds.
    pub fn request(&mut self) -> Option<Request> {
        loop {
            match self.ws.read() {
                Ok(Message::Text(text)) => {
                    let request = Request::parse(&text).unwrap_or_else(|_| panic!("the client sent a frame that is no request: {text}"));
                    self.log.seen.lock().unwrap().push(Seen { connection: self.index, request: request.clone() });
                    return Some(request);
                }
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    pub fn send(&mut self, frame: Value) {
        self.ws.send(Message::Text(frame.to_string().into())).unwrap();
    }

    pub fn send_text(&mut self, text: &str) {
        self.ws.send(Message::Text(text.into())).unwrap();
    }

    pub fn answer(&mut self, request: &Request, result: Value) {
        self.send(json!({"id": request.id, "result": result}));
    }

    pub fn refuse(&mut self, request: &Request, code: &str, message: &str) {
        self.send(json!({"id": request.id, "error": {"code": code, "message": message}}));
    }

    pub fn event(&mut self, event: &Event) {
        self.send_text(&event.encode());
    }

    /// Ends the connection with no closing handshake, as a crash or a lost network does.
    pub fn drop_now(&mut self) {
        let _ = self.ws.get_ref().shutdown(Shutdown::Both);
    }

    /// Keeps the connection until the client closes it.
    pub fn linger(&mut self) {
        while self.request().is_some() {}
    }
}
