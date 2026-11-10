use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    server: Arc<tiny_http::Server>,
    worker: Option<std::thread::JoinHandle<()>>,
    _dir: tempfile::TempDir,
}

impl MockHttps {
    pub fn start() -> Self {
        let dir = tempfile::Builder::new().prefix("wf-mock-https-").tempdir().expect("temp dir");
        let pki = super::pki::ca_and_localhost_leaf();
        std::fs::write(dir.path().join("ca.pem"), &pki.ca_pem).unwrap();
        let config = tiny_http::SslConfig { certificate: pki.cert_pem.into_bytes(), private_key: pki.key_pem.into_bytes() };
        let server = Arc::new(tiny_http::Server::https(("127.0.0.1", 0), config).expect("an HTTPS server"));
        let port = server.server_addr().to_ip().expect("ip address").port();
        let state = Arc::new(Mutex::new(State { requests: Vec::new(), status: 200, reply: "{}".into(), rules: Vec::new() }));
        let worker = {
            let (server, state) = (server.clone(), state.clone());
            std::thread::spawn(move || {
                for mut request in server.incoming_requests() {
                    let mut body = String::new();
                    let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
                    let headers: HashMap<String, String> = request.headers().iter().map(|h| (h.field.to_string().to_ascii_lowercase(), h.value.to_string())).collect();
                    let (status, reply) = {
                        let mut state = state.lock().unwrap();
                        let user = headers
                            .get("authorization")
                            .map(|auth| auth.trim_start_matches("Basic ").to_string())
                            .and_then(|encoded| decode_basic_user(&encoded))
                            .unwrap_or_default();
                        let rule = state.rules.iter().find(|r| r.user.as_ref().is_none_or(|u| *u == user) && r.includes.iter().all(|t| body.contains(t)) && !r.excludes.iter().any(|t| body.contains(t))).cloned();
                        state.requests.push(HttpsRequest { method: request.method().to_string(), url: request.url().to_string(), headers, body });
                        match rule {
                            Some(rule) => (rule.status.unwrap_or(200), rule.body),
                            None => (state.status, state.reply.clone()),
                        }
                    };
                    let mut response = tiny_http::Response::from_string(reply).with_status_code(status);
                    response.add_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap());
                    let _ = request.respond(response);
                }
            })
        };
        Self { port, ca: dir.path().join("ca.pem"), state, server, worker: Some(worker), _dir: dir }
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
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
