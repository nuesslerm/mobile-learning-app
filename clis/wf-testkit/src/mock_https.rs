use std::collections::HashMap;
use std::path::PathBuf;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HttpsRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Default)]
pub struct Rule {
    pub user: Option<String>,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub status: Option<u16>,
    pub body: String,
}

struct State {
    requests: Vec<HttpsRequest>,
    status: u16,
    reply: String,
    rules: Vec<Rule>,
}

/// A mock HTTPS endpoint on a thread (an OTLP or BetterStack ingest, for one). The certificate is made with `rcgen` for
/// 127.0.0.1 under a throwaway CA; a test points the CLI at it with the CA bundle variable of the implementation under test.
/// Port of `mock-https-server.ts`.
pub struct MockHttps {
    pub port: u16,
    /// PEM file of the CA, for `SSL_CERT_FILE` and its kin.
    pub ca: PathBuf,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

impl MockHttps {
    pub fn start() -> Self {
        use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
        let dir = tempfile::Builder::new().prefix("wf-mock-https-").tempdir().expect("temp dir");
        let pki = super::pki::ca_and_localhost_leaf();
        std::fs::write(dir.path().join("ca.pem"), &pki.ca_pem).unwrap();
        let certs = CertificateDer::pem_slice_iter(pki.cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>().expect("certificates");
        let key = PrivateKeyDer::from_pem_slice(pki.key_pem.as_bytes()).expect("a private key");
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("protocol versions")
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .expect("a server certificate"),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a listener");
        let port = listener.local_addr().expect("address").port();
        let state = Arc::new(Mutex::new(State { requests: Vec::new(), status: 200, reply: "{}".into(), rules: Vec::new() }));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (state, stop) = (state.clone(), stop.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    let Ok(conn) = rustls::ServerConnection::new(config.clone()) else { continue };
                    serve(rustls::StreamOwned::new(conn, stream), &state);
                }
            })
        };
        Self { port, ca: dir.path().join("ca.pem"), state, stop, worker: Some(worker), _dir: dir }
    }

    pub fn requests(&self) -> Vec<HttpsRequest> {
        self.state.lock().unwrap().requests.clone()
    }

    pub fn clear(&self) {
        self.state.lock().unwrap().requests.clear();
    }

    pub fn status(&self, code: u16) {
        self.state.lock().unwrap().status = code;
    }

    /// The body every later request is answered with (unless a rule matches).
    pub fn reply(&self, body: &str) {
        self.state.lock().unwrap().reply = body.to_string();
    }

    /// The first rule whose user (basic auth) matches and whose body contains every `includes` and none of `excludes` wins.
    pub fn rules(&self, list: Vec<Rule>) {
        self.state.lock().unwrap().rules = list;
    }
}

/// One request per connection: read the head and the `Content-Length` body, answer, close.
fn serve(mut tls: rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>, state: &Mutex<State>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        match tls.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, url) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
    let headers: HashMap<String, String> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let mut body = buf[head_end..].to_vec();
    if headers.get("transfer-encoding").is_some_and(|t| t.contains("chunked")) {
        while !body.ends_with(b"0\r\n\r\n") {
            match tls.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.extend_from_slice(&chunk[..n]),
            }
        }
        body = dechunk(&body);
    } else {
        let want: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        while body.len() < want {
            match tls.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.extend_from_slice(&chunk[..n]),
            }
        }
    }
    let body = String::from_utf8_lossy(&body).into_owned();
    let (status, reply) = {
        let mut state = state.lock().unwrap();
        let user = headers
            .get("authorization")
            .map(|auth| auth.trim_start_matches("Basic ").to_string())
            .and_then(|encoded| decode_basic_user(&encoded))
            .unwrap_or_default();
        let rule = state.rules.iter().find(|r| r.user.as_ref().is_none_or(|u| *u == user) && r.includes.iter().all(|t| body.contains(t)) && !r.excludes.iter().any(|t| body.contains(t))).cloned();
        state.requests.push(HttpsRequest { method, url, headers, body });
        match rule {
            Some(rule) => (rule.status.unwrap_or(200), rule.body),
            None => (state.status, state.reply.clone()),
        }
    };
    let response = format!("HTTP/1.1 {status} \r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
    let _ = tls.write_all(response.as_bytes());
    tls.conn.send_close_notify();
    let _ = tls.flush();
}

fn dechunk(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(at) = rest.windows(2).position(|w| w == b"\r\n") {
        let size = usize::from_str_radix(String::from_utf8_lossy(&rest[..at]).split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
        rest = &rest[at + 2..];
        if size == 0 || rest.len() < size {
            break;
        }
        out.extend_from_slice(&rest[..size]);
        rest = rest.get(size + 2..).unwrap_or(&[]);
    }
    out
}

/// The user of a base64 `user:password` pair.
fn decode_basic_user(encoded: &str) -> Option<String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for byte in encoded.bytes().filter(|b| *b != b'=') {
        bits = (bits << 6) | TABLE.iter().position(|t| *t == byte)? as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    String::from_utf8(out).ok().map(|text| text.split(':').next().unwrap_or("").to_string())
}

impl Drop for MockHttps {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
