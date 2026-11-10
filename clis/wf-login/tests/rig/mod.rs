//! The mock services and helpers the `wf secret login` journeys share: the owner's authed requests, a recording HTTP double, the listener's container.
use crate::support::{create_sh_stub, spawn_bin, spawn_wf, RunOpts, Spawned};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const OWNER: &str = "4242";
pub const SECRET: &str = "trigger-secret";
pub const AUTHED: [(&str, &str); 3] = [("x-auth-trigger-secret", SECRET), ("x-auth-owner-id", OWNER), ("content-type", "application/json")];

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("body {:?}: {e}", self.text()))
    }
}

pub fn fetch(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&str>) -> Resp {
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).proxy(None).max_redirects(0).timeout_global(Some(Duration::from_secs(90))).build().into();
    let mut request = ureq::http::Request::builder().method(method).uri(format!("http://127.0.0.1:{port}{path}"));
    for (k, v) in headers {
        request = request.header(*k, *v);
    }
    let mut response = match body {
        Some(body) => agent.run(request.body(body.as_bytes().to_vec()).unwrap()),
        None => agent.run(request.body(()).unwrap()),
    }
    .unwrap_or_else(|e| panic!("{method} {path}: {e}"));
    let headers = response.headers().iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or_default().to_string())).collect();
    Resp { status: response.status().as_u16(), headers, body: response.body_mut().read_to_vec().unwrap_or_default() }
}

#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub fn ok(body: &str) -> Answer {
    Answer { status: 200, headers: vec![], body: body.as_bytes().to_vec() }
}

/// An HTTP service on a thread that records each request and answers it with `handler`.
pub struct Recorder {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<Recorded>>>,
    pub stop: Arc<AtomicBool>,
    pub worker: Option<std::thread::JoinHandle<()>>,
}

impl Recorder {
    pub fn start(handler: impl Fn(&Recorded) -> Answer + Send + 'static) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let (seen, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
        let (sink, flag) = (seen.clone(), stop.clone());
        let worker = std::thread::spawn(move || loop {
            match server.recv_timeout(Duration::from_millis(50)) {
                Ok(Some(mut request)) => {
                    let mut body = String::new();
                    let _ = request.as_reader().read_to_string(&mut body);
                    let headers = request.headers().iter().map(|h| (h.field.as_str().as_str().to_ascii_lowercase(), h.value.as_str().to_string())).collect();
                    let recorded = Recorded { method: request.method().as_str().to_string(), url: request.url().to_string(), headers, body };
                    sink.lock().unwrap().push(recorded.clone());
                    let answer = handler(&recorded);
                    let mut response = tiny_http::Response::from_data(answer.body).with_status_code(answer.status);
                    for (k, v) in answer.headers {
                        response.add_header(tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap());
                    }
                    let _ = request.respond(response);
                }
                Ok(None) if flag.load(Ordering::SeqCst) => break,
                Ok(None) => {}
                Err(_) => break,
            }
        });
        Self { port, seen, stop, worker: Some(worker) }
    }

    pub fn seen(&self) -> Vec<Recorded> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The desktop container around the listener: a HOME, stub `tailscale` (logging), `bws` and whatever a test adds.
pub struct Container {
    pub dir: tempfile::TempDir,
    pub bin: PathBuf,
    pub home: PathBuf,
}

pub struct Listener {
    pub proc: Spawned,
    pub port: u16,
    pub gate: u16,
}

pub fn number(value: &Value) -> u16 {
    value.as_u64().map(|n| n as u16).or_else(|| value.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

pub fn text(value: &Value) -> String {
    value.as_str().map(String::from).unwrap_or_else(|| value.to_string())
}

pub fn logs_of(out: &str) -> Vec<Value> {
    out.lines().filter(|l| l.starts_with('{')).filter_map(|l| serde_json::from_str(l).ok()).collect()
}

impl Container {
    pub fn new() -> Container {
        let dir = tempfile::Builder::new().prefix("wf-flow-listener-").tempdir().unwrap();
        let (bin, home) = (dir.path().join("bin"), dir.path().join("home"));
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir(&home).unwrap();
        create_sh_stub(&bin.join("bws"), "printf '%s' \"${BWS_LIST:-[]}\"");
        let c = Container { dir, bin, home };
        c.stub("tailscale", &format!("echo \"$@\" >> \"{}/tailscale.log\"", c.dir.path().display()));
        c
    }

    pub fn stub(&self, name: &str, body: &str) {
        create_sh_stub(&self.bin.join(name), body);
    }

    pub fn tailscale_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("tailscale.log")).map(|t| t.trim().lines().map(String::from).collect()).unwrap_or_default()
    }

    /// Starts the listener (VNC_AUTH_LISTENER_BIN, the image's own build of it, else `wf secret login listener`): both ports once
    /// announced, or the output if it exits first.
    pub fn start(&self, env: &[(&str, Option<&str>)]) -> Result<Listener, String> {
        let mut merged: Vec<(String, String)> = vec![
            ("HOME".into(), self.home.display().to_string()),
            ("SECRETS_BACKEND".into(), "bitwarden".into()),
            ("SECRETS_LOG_BETTERSTACK".into(), "false".into()),
            ("BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN".into(), "read".into()),
            ("VNC_AUTH_TRIGGER_SECRET".into(), SECRET.into()),
            ("VNC_AUTH_OWNER_CHAT_ID".into(), format!("{OWNER},9999")),
            ("VNC_AUTH_LISTENER_PORT".into(), "0".into()),
            ("VNC_AUTH_GATE_PORT".into(), "0".into()),
            ("VNC_AUTH_LINK_BASE_URL".into(), "https://vnc.example.ts.net".into()),
            ("VNC_AUTH_CDP_ENDPOINT".into(), "http://127.0.0.1:9".into()),
        ];
        for (key, value) in env {
            merged.retain(|(k, _)| k != key);
            if let Some(value) = value {
                merged.push((key.to_string(), value.to_string()));
            }
        }
        let mut opts = RunOpts::default().bin_dir(&self.bin).cwd(self.dir.path());
        for (key, value) in &merged {
            opts = opts.env(key, value);
        }
        let mut proc = match std::env::var_os("VNC_AUTH_LISTENER_BIN") {
            Some(bin) => spawn_bin(&PathBuf::from(bin), &[], &opts),
            None => spawn_wf(&["secret", "login", "listener"], &opts),
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let logs = logs_of(&proc.out());
            let find = |msg: &str| logs.iter().find(|l| l["msg"] == msg).cloned();
            if let (Some(control), Some(gate)) = (find("vnc_auth.listener.listening"), find("vnc_auth.gate.listening")) {
                return Ok(Listener { port: number(&control["port"]), gate: number(&gate["gatePort"]), proc });
            }
            if let Some(code) = proc.wait(Duration::from_millis(0)) {
                return Err(format!("listener exited {code} before listening: {}", proc.out()));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Err(format!("listener never announced its ports: {}", proc.out()))
    }

    pub fn up(&self, env: &[(&str, Option<&str>)]) -> Listener {
        self.start(env).unwrap_or_else(|e| panic!("{e}"))
    }
}

impl Listener {
    pub fn logs(&self) -> Vec<Value> {
        logs_of(&self.proc.out())
    }

    pub fn logged(&self, msg: &str) -> Option<Value> {
        self.logs().into_iter().find(|l| l["msg"] == msg)
    }

    pub fn messages(&self) -> Vec<String> {
        self.logs().iter().map(|l| text(&l["msg"])).collect()
    }

    pub fn wait_for(&self, msg: &str, within: Duration) -> Value {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Some(hit) = self.logged(msg) {
                return hit;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("never logged {msg}: {:?}", self.messages());
    }

    pub fn post(&self, path: &str, body: Value, headers: &[(&str, &str)]) -> Resp {
        fetch(self.port, "POST", path, headers, Some(&body.to_string()))
    }

    pub fn authed(&self, path: &str, body: Value) -> Resp {
        self.post(path, body, &AUTHED)
    }

    pub fn gate(&self, path: &str, headers: &[(&str, &str)]) -> Resp {
        fetch(self.gate, "GET", path, headers, None)
    }
}

pub fn token_of(link_url: &Value) -> String {
    let url = link_url.as_str().unwrap_or_default();
    url.split_once("?t=").map(|(_, t)| t.split('&').next().unwrap_or("").to_string()).unwrap_or_default()
}

