//! The HTTP service a caller talks to, as a loopback double: it answers each request with a route's reply and records
//! what arrived.
use std::sync::mpsc;
use std::time::Duration;
use tiny_http::{Header, Response, Server};

pub type Reply = Response<std::io::Cursor<Vec<u8>>>;

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

pub struct FakeService {
    pub url: String,
    seen: mpsc::Receiver<Seen>,
}

impl FakeService {
    /// Serves until the test process ends; `route` answers each request.
    pub fn start(route: impl Fn(&Seen) -> Reply + Send + 'static) -> FakeService {
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", server.server_addr().to_ip().unwrap().port());
        let (tx, seen) = mpsc::channel();
        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
                let got = Seen {
                    method: request.method().to_string(),
                    url: request.url().to_string(),
                    headers: request.headers().iter().map(|h| (h.field.to_string().to_ascii_lowercase(), h.value.to_string())).collect(),
                    body,
                };
                let _ = tx.send(got.clone());
                let _ = request.respond(route(&got));
            }
        });
        FakeService { url, seen }
    }

    /// The next request that arrived (the proof a call reached the service).
    pub fn next(&self) -> Seen {
        self.seen.recv_timeout(Duration::from_secs(5)).expect("a request reached the fake service")
    }
}

pub fn text(body: &str, status: u16) -> Reply {
    Response::from_string(body).with_status_code(status)
}

pub fn redirect(to: &str) -> Reply {
    Response::from_string("").with_status_code(302).with_header(Header::from_bytes("Location", to).unwrap())
}

/// A port nothing listens on.
pub fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("cli-http-flow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
