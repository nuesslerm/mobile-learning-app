//! `wf test backends up|down` with stand-in binaries (a python server answering /health on $PORT, or one that dies): what it prints, what it
//! leaves running, and what `down` stops. The real build and start of rca-service and leetcode-backend is a hosted run's.
use crate::support::{last_json, run_wf, RunOpts};
use serde_json::{json, Value};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const SERVER: &str = "#!/usr/bin/env python3\nimport http.server, os\nclass H(http.server.BaseHTTPRequestHandler):\n    def do_GET(self):\n        self.send_response(200 if self.path == '/health' else 404)\n        self.end_headers()\n        self.wfile.write(b'{\"status\":\"ok\"}')\n    def log_message(self, *a): pass\nhttp.server.ThreadingHTTPServer(('127.0.0.1', int(os.environ['PORT'])), H).serve_forever()\n";
const DIES: &str = "#!/bin/sh\necho 'config missing' >&2\nexit 3\n";

struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Sandbox { root: tempfile::Builder::new().prefix("e2e-backends-").tempdir().unwrap() }
    }

    fn stub(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.path().join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    fn wf(&self, args: &[&str], rca: &Path, leetcode: &Path) -> (i32, Value) {
        let state = self.state().display().to_string();
        let mut all = vec!["test", "backends"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--dir", &state]);
        let opts = RunOpts::default().env("RCA_SERVICE_BIN", &rca.display().to_string()).env("LEETCODE_BACKEND_BIN", &leetcode.display().to_string());
        let r = run_wf(&all, &opts);
        (r.exit_code, last_json(&r))
    }
}

fn up(url: &str) -> bool {
    TcpStream::connect(url.trim_start_matches("http://")).is_ok()
}

fn alive(pid: u64) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| !s.rsplit_once(')').is_some_and(|(_, rest)| rest.trim().starts_with('Z')))
}

fn health(url: &str) -> u16 {
    use std::io::{Read, Write};
    let mut s = TcpStream::connect(url.trim_start_matches("http://")).unwrap();
    s.write_all(b"GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split(' ').nth(1).unwrap().parse().unwrap()
}

#[test]
fn given_two_backends_when_up_runs_then_one_json_names_urls_pids_and_env_and_down_stops_them() {
    // Given stand-in binaries for rca-service and leetcode-backend
    let sb = Sandbox::new();
    let (rca, leetcode) = (sb.stub("rca", SERVER), sb.stub("leetcode", SERVER));
    // When up runs
    let (code, out) = sb.wf(&["up"], &rca, &leetcode);
    // Then it prints ok with a url, pid and health url for each, in a fixed order
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["ok"], json!(true));
    let backends = out["backends"].as_array().unwrap();
    let names: Vec<&str> = backends.iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["rca-service", "leetcode-backend"]);
    for b in backends {
        let mut keys: Vec<&str> = b.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["bin", "env", "health", "host", "log", "name", "pid", "port", "url"], "{b}");
        // And the health probe answers 200 on the url it printed
        assert_eq!(health(b["url"].as_str().unwrap()), 200);
        assert_eq!(b["health"], json!(format!("{}/health", b["url"].as_str().unwrap())));
    }
    // And the env a test points at each carries its host, port and a secret of this run
    let rca_env = &backends[0]["env"];
    assert_eq!((rca_env["RCA_HOST"].as_str(), rca_env["RCA_PORT"].as_str()), (Some("127.0.0.1"), Some(backends[0]["port"].to_string().as_str())));
    assert_eq!(rca_env["CHAT_AGENT_API_SECRET"].as_str().unwrap().len(), 32);
    assert_eq!(backends[1]["env"]["EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL"], backends[1]["url"]);
    assert_eq!(backends[1]["env"]["EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN"].as_str().unwrap().len(), 32);
    // And a second up refuses while they run
    let (code, again) = sb.wf(&["up"], &rca, &leetcode);
    assert_eq!((code, again["ok"].clone()), (1, json!(false)), "{again}");
    assert!(again["error"].as_str().unwrap().contains("already up"), "{again}");

    // When down runs
    let pids: Vec<u64> = backends.iter().map(|b| b["pid"].as_u64().unwrap()).collect();
    let urls: Vec<String> = backends.iter().map(|b| b["url"].as_str().unwrap().to_string()).collect();
    let (code, down) = sb.wf(&["down"], &rca, &leetcode);
    // Then it names each as stopped, the processes are gone and nothing listens
    assert_eq!(code, 0, "{down}");
    let stopped: Vec<(&str, &str)> = down["stopped"].as_array().unwrap().iter().map(|s| (s["name"].as_str().unwrap(), s["how"].as_str().unwrap())).collect();
    assert_eq!(stopped, [("rca-service", "term"), ("leetcode-backend", "term")]);
    assert!(pids.iter().all(|p| !alive(*p)));
    assert!(urls.iter().all(|u| !up(u)));
    // And a down with nothing running is still ok
    let (code, none) = sb.wf(&["down"], &rca, &leetcode);
    assert_eq!((code, none["stopped"].clone()), (0, json!([])));
}

#[test]
fn given_a_backend_that_dies_when_up_runs_then_it_fails_naming_it_and_stops_the_ones_already_started() {
    // Given an rca-service that serves and a leetcode-backend that exits at once
    let sb = Sandbox::new();
    let (rca, leetcode) = (sb.stub("rca", SERVER), sb.stub("leetcode", DIES));
    // When up runs
    let (code, out) = sb.wf(&["up"], &rca, &leetcode);
    // Then it fails with the backend named and its log's last lines
    assert_eq!((code, out["ok"].clone()), (1, json!(false)), "{out}");
    let error = out["error"].as_str().unwrap();
    assert!(error.contains("leetcode-backend") && error.contains("config missing"), "{error}");
    // And nothing is left running or recorded
    let (_, down) = sb.wf(&["down"], &rca, &leetcode);
    assert_eq!(down["stopped"], json!([]));
}

#[test]
fn given_only_one_backend_named_when_up_runs_then_just_that_one_starts_and_an_unknown_name_is_refused() {
    let sb = Sandbox::new();
    let (rca, leetcode) = (sb.stub("rca", SERVER), sb.stub("leetcode", SERVER));
    // When only the leetcode backend is asked for
    let (code, out) = sb.wf(&["up", "--only", "leetcode-backend"], &rca, &leetcode);
    // Then it is the only one printed
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["backends"].as_array().unwrap().len(), 1);
    assert_eq!(out["backends"][0]["name"], json!("leetcode-backend"));
    sb.wf(&["down"], &rca, &leetcode);
    // And a name that is no backend is refused before anything starts
    let (code, bad) = sb.wf(&["up", "--only", "nope"], &rca, &leetcode);
    assert_eq!((code, bad["ok"].clone()), (1, json!(false)), "{bad}");
}

#[test]
fn given_a_named_binary_that_is_no_file_when_up_runs_then_it_fails_naming_the_variable() {
    let sb = Sandbox::new();
    let leetcode = sb.stub("leetcode", SERVER);
    let (code, out) = sb.wf(&["up"], &sb.root.path().join("missing"), &leetcode);
    assert_eq!(code, 1, "{out}");
    assert!(out["error"].as_str().unwrap().contains("RCA_SERVICE_BIN="), "{out}");
}

#[test]
fn given_the_leetcode_backend_up_when_its_env_is_read_then_every_name_is_one_the_web_export_bakes_and_the_app_reads() {
    // Given the leetcode backend started on its own, as the browser e2e job does
    let sb = Sandbox::new();
    let (rca, leetcode) = (sb.stub("rca", SERVER), sb.stub("leetcode", SERVER));
    let (code, out) = sb.wf(&["up", "--only", "leetcode-backend"], &rca, &leetcode);
    assert_eq!(code, 0, "{out}");
    // When the job reads its EXPO_PUBLIC_ env (what `wf test prepare --web` bakes into the export)
    let env = out["backends"][0]["env"].as_object().unwrap();
    let mut names: Vec<&str> = env.keys().map(String::as_str).collect();
    names.sort();
    // Then they are exactly the backend's base url and token, a loopback url and a token of this run
    assert_eq!(names, ["EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL", "EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN"]);
    assert!(env["EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL"].as_str().unwrap().starts_with("http://127.0.0.1:"));
    assert_eq!(env["EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN"].as_str().unwrap().len(), 32);
    // And the app reads each name
    let config = std::fs::read_to_string(wf_kit::root::repo_root().join("frontend/libraries/app-config/src/appConfig.ts")).unwrap();
    assert!(names.iter().all(|n| config.contains(n)), "appConfig.ts does not read every name in {names:?}");
    sb.wf(&["down"], &rca, &leetcode);
}
