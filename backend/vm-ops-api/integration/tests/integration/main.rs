//! vm-ops-api's integration suite: one binary, one module per area of the cases in docs/plans/vm-ops-api-rs/plan.md
//! section 3. Run it through the package's `test:integration`, which names the service binary
//! (`VM_OPS_API_CMD`). The helpers below are the ones every area's cases share.
mod agent_shell;
mod auth;
mod clis_install;
mod helpers;
mod pipeline;
mod ports;
mod rails;
mod reads;
mod redeploy;
mod self_redeploy;
mod startup;
mod state;
mod swap_script;

use serde_json::{json, Value};
use vm_ops_api_integration::{node_tar, now_ms, unstamped, Reply, Service, TarEntry, World};

/// A case's world, with this package's `fake-tool` as its `docker` and `wf`.
pub fn world() -> World {
    World::new(env!("CARGO_BIN_EXE_fake-tool"))
}

/// agent-shell's app container with two siblings the picker passes over: a longer name and the tailscale sidecar.
pub const AGENT_SHELL: &str = r#"[
    {"Id":"as2","Names":["/agent-shell-sessions"],"State":"running","Status":"Up 1 hour","Image":"ns/agent-shell:sha-1"},
    {"Id":"as1","Names":["/agent-shell"],"State":"running","Status":"Up 1 hour","Image":"ns/agent-shell:sha-1"},
    {"Id":"as3","Names":["/agent-shell-tailscale"],"State":"running","Status":"Up 1 hour","Image":"tailscale/tailscale:v1"}
]"#;
pub const EXITED_AGENT_SHELL: &str =
    r#"[{"Id":"as1","Names":["/agent-shell"],"State":"exited","Status":"Exited (137) 5 minutes ago","Image":"ns/agent-shell:sha-1"}]"#;
pub const NO_AGENT_SHELL: &str = r#"[
    {"Id":"as3","Names":["/agent-shell-tailscale"],"State":"running","Status":"Up 1 hour","Image":"tailscale/tailscale:v1"},
    {"Id":"l1","Names":["/leetcode-backend"],"State":"running","Status":"Up 1 hour","Image":"ns/leetcode-backend:sha-1"}
]"#;
/// The error of the 503 every agent-shell route gives when the Engine lists no running agent-shell.
pub const NOT_RUNNING: &str = "agent-shell container is not running";
pub const JOB: &str = "0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab";

/// A GET written byte for byte, signed over `target` as wf signs it.
pub fn get(svc: &Service, target: &str) -> Reply {
    sent(svc, "GET", target, &vm_ops_api_integration::auth("GET", target, &now_ms().to_string(), b""))
}

/// A GET with no auth at all.
pub fn public(svc: &Service, target: &str) -> Reply {
    sent(svc, "GET", target, &[])
}

/// A request with no body (an empty one for POST) and exactly these headers.
pub fn sent(svc: &Service, method: &str, target: &str, headers: &[(String, String)]) -> Reply {
    let body: Option<&[u8]> = (method == "POST").then_some(b"");
    svc.raw(method, target, headers, body).unwrap_or_else(|e| panic!("{method} {target}: {e}"))
}

/// A POST of `body` written byte for byte, signed over `target` and the body as the service hashes it: decoded as UTF-8,
/// a byte that is no UTF-8 read as U+FFFD. wf's bodies are UTF-8, so for them that is the body itself.
pub fn post(svc: &Service, target: &str, body: &[u8]) -> Reply {
    let decoded = String::from_utf8_lossy(body);
    let headers = vm_ops_api_integration::auth("POST", target, &now_ms().to_string(), decoded.as_bytes());
    svc.raw("POST", target, &headers, Some(body)).unwrap_or_else(|e| panic!("POST {target}: {e}"))
}

/// Asserts a JSON reply: its status, `application/json` and exactly `body` as JSON.stringify prints it (key order,
/// number forms, no key for an undefined value).
pub fn expect_json(r: &Reply, status: u16, body: &Value) {
    assert_eq!((r.status, r.body.as_str()), (status, body.to_string().as_str()));
    assert_eq!(r.header("content-type"), Some("application/json"), "{}", r.body);
}

/// Asserts the 503 of an agent-shell route with no agent-shell listed: a JSON reply like every other.
pub fn expect_not_running(r: &Reply) {
    expect_json(r, 503, &json!({ "error": NOT_RUNNING }));
}

/// The fake Engine's requests so far, as `METHOD target`.
pub fn engine_calls(svc: &Service) -> Vec<String> {
    let requests = svc.world().engine_requests();
    requests.iter().map(|r| format!("{} {}", r["method"].as_str().unwrap_or(""), r["target"].as_str().unwrap_or(""))).collect()
}

/// The JSON body of each exec create the Engine received, in order.
pub fn exec_creates(svc: &Service) -> Vec<Value> {
    let requests = svc.world().engine_requests();
    let creates = requests.iter().filter(|r| r["method"] == "POST" && r["target"].as_str().is_some_and(|t| t.ends_with("/exec")));
    creates.map(|r| serde_json::from_str(r["body"].as_str().unwrap_or("")).expect("an exec create body is JSON")).collect()
}

/// Asserts that the archive PUTs are, in order, the tars Node writes for `files`: sent as `application/x-tar` with their
/// exact length, and byte for byte the layout of `application/tar.ts` but for the mtime and checksum.
pub fn expect_node_tars(svc: &Service, files: &[TarEntry]) {
    let requests = svc.world().engine_requests();
    let puts: Vec<(usize, &Value)> = requests.iter().enumerate().filter(|(_, r)| r["method"] == "PUT").collect();
    assert_eq!(puts.len(), files.len(), "archive PUTs: {:?}", puts.iter().map(|(_, r)| &r["target"]).collect::<Vec<_>>());
    for ((i, put), file) in puts.into_iter().zip(files) {
        let (sent, node) = (unstamped(&svc.world().fake_engine().request_body(i)), node_tar(file));
        let framing = json!({ "content-type": "application/x-tar", "content-length": node.len().to_string() });
        assert_eq!(put["headers"], framing, "the archive PUT of {}", file.name);
        let at = sent.iter().zip(&node).position(|(a, b)| a != b).unwrap_or(sent.len().min(node.len()));
        let near = |tar: &[u8]| tar[at.min(tar.len())..(at + 16).min(tar.len())].to_vec();
        assert!(
            sent == node,
            "the tar of {} is {} bytes and differs from Node's {} from byte {at}: {:?}, Node's {:?}",
            file.name,
            sent.len(),
            node.len(),
            near(&sent),
            near(&node)
        );
    }
}

pub fn with_engine(list: &str) -> World {
    let mut w = world();
    w.engine(200, list);
    w
}

pub fn state_file(svc: &Service, rel: &str, text: &str) {
    let path = svc.world().dir().join("state").join(rel);
    std::fs::create_dir_all(path.parent().expect("a dir in state/")).expect("the state dir");
    std::fs::write(path, text).expect("a state file");
}
