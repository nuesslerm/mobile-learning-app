//! The double of this crate's port: vm-ops-api as `wf` reaches it, for the flow tests of every crate that talks to the VM.
use base64::Engine;
use serde_json::Value;
use wf_testkit::{iso_from_ms, pki, MockReply, MockRequest, MockServer, MockTailnet, RunOpts, Upstream};

/// The rotating-token seed every fake vm-ops-api client is given.
pub const VM_SEED: &str = "seed-for-tests";

/// The CI port's seed, which a client that runs as a CI job is given as `VM_OPS_API_CI_AUTH_TOKEN`, beside `WF_VM_CI_PORT=8081`.
pub const CI_VM_SEED: &str = "ci-seed-for-tests";

/// vm-ops-api as `wf` reaches it: a mock HTTP service behind a mock tailnet SOCKS5 proxy, and a fresh Ed25519 key pair whose
/// private half the client signs with and whose public half checks the signature.
pub struct FakeVm {
    pub server: MockServer,
    pub tailnet: MockTailnet,
    pub private_one_line: String,
    pub public_pem: String,
}

impl FakeVm {
    pub fn start() -> Self {
        let server = MockServer::start(None);
        let tailnet = MockTailnet::start(Upstream::All(server.port));
        let (private, public_pem) = pki::ed25519_pem_pair();
        Self { server, tailnet, private_one_line: private.trim_end().replace('\n', "\\n"), public_pem }
    }

    /// Answers the next calls in order, each written `body\n<status>` (or `drop`: the connection closes unanswered); earlier
    /// requests are forgotten.
    pub fn replies<S: AsRef<str>>(&self, bodies: &[S]) {
        self.queue(bodies, false);
    }

    /// Like `replies`, and the last reply answers every further call.
    pub fn replies_then_last<S: AsRef<str>>(&self, bodies: &[S]) {
        self.queue(bodies, true);
    }

    fn queue<S: AsRef<str>>(&self, bodies: &[S], repeat_last: bool) {
        self.server.clear();
        let parse = |raw: &str| {
            if raw == "drop" {
                return MockReply { drop: true, ..Default::default() };
            }
            let at = raw.rfind('\n').expect("body\\nstatus");
            MockReply { status: Some(raw[at + 1..].parse().unwrap()), body: Some(Value::String(raw[..at].to_string())), ..Default::default() }
        };
        self.server.queue(bodies.iter().map(|raw| parse(raw.as_ref())).collect(), repeat_last);
    }

    /// A clean environment with the vault off and the vm-ops-api credentials and tailnet proxy of this fake.
    pub fn opts(&self) -> RunOpts {
        RunOpts::default()
            .clean_env()
            .env("SECRETS_BACKEND", "macos")
            .env("SECRETS_LOG_BETTERSTACK", "false")
            .env("WF_PREFLIGHT", "off")
            .env("WF_TAILNET_SOCKS", &self.tailnet.address())
            .env("VM_OPS_API_AUTH_TOKEN", VM_SEED)
            .env("VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", &self.private_one_line)
    }

    pub fn requests(&self) -> Vec<MockRequest> {
        self.server.requests()
    }

    /// Asserts `request` is signed for `method`/`path`/`body` with this fake's key and carries the day's bearer token.
    pub fn assert_signed(&self, request: &MockRequest, method: &str, path: &str, body: &str) {
        self.assert_signed_with(VM_SEED, request, method, path, body);
    }

    /// As `assert_signed`, for a bearer token derived from `seed`.
    pub fn assert_signed_with(&self, seed: &str, request: &MockRequest, method: &str, path: &str, body: &str) {
        let timestamp = request.headers["x-signature-timestamp"].clone();
        let canonical = format!("{method}\n{path}\n{timestamp}\n{}", pki::sha256_hex(body.as_bytes()));
        let signature = base64::engine::general_purpose::STANDARD.decode(&request.headers["x-signature"]).expect("a base64 signature");
        assert!(pki::ed25519_verifies(&self.public_pem, canonical.as_bytes(), &signature), "the Ed25519 signature does not verify for {canonical:?}");
        let date = &iso_from_ms(timestamp.parse().unwrap())[..10];
        assert_eq!(request.headers["authorization"], format!("Bearer {}", pki::hmac_sha256_hex(seed, date.as_bytes())));
    }
}

/// The bodies vm-ops-api answers, each key in its order (pinned by the integration suite in `backend/vm-ops-api/integration`),
/// so no fake answers what the service never sends.
pub mod answers {
    use serde_json::{json, Value};

    /// The hash `/health` and every app redeploy report: the first 12 hex characters of the server script's sha256.
    pub const SERVER_SCRIPT_SHA: &str = "5f0c2a9d41e7";

    /// `body\nstatus`, the form `FakeVm::replies` takes.
    pub fn reply(status: u16, body: &Value) -> String {
        format!("{body}\n{status}")
    }

    /// `n` as JSON prints a JS number: a whole number without its `.0`.
    fn js_number(n: f64) -> Value {
        if n.fract() == 0.0 { json!(n as i64) } else { json!(n) }
    }

    /// A job id as the exec-script start mints it: a UUID, the only id its status and cancel routes take.
    pub fn job_id(n: u16) -> String {
        format!("0b9f5c2e-8a1d-4c3e-9f7a-{n:012}")
    }

    /// The app's own container as its compose file names it; ps lists only an app's own names (`APP_CONTAINER_NAMES`).
    pub fn container_name(app: &str) -> &str {
        match app {
            "leetcode-auth-cookie-capture" => "leetcode-capture",
            "vnc-auth-session" => "vnc-auth",
            other => other,
        }
    }

    /// `GET /health` of a service built from `commit` and tagged with its short sha.
    pub fn health(commit: &str) -> Value {
        json!({
            "ok": true, "commit": commit, "startedAt": "2026-10-06T09:00:00.000Z", "host": "main", "debugMode": false,
            "time": "2026-10-06T10:00:00.000Z", "serverScriptSha": SERVER_SCRIPT_SHA, "imageTag": format!("sha-{}", &commit[..commit.len().min(7)]),
        })
    }

    /// `GET /services/<app>/ps`: each container as (name, state, status, image).
    pub fn ps(app: &str, containers: &[(&str, &str, &str, &str)]) -> Value {
        let listed: Vec<Value> = containers.iter().map(|(name, state, status, image)| json!({ "name": name, "state": state, "status": status, "image": image })).collect();
        json!({ "app": app, "containers": listed })
    }

    /// `GET /services/<app>/ps` of an app whose own container runs `image`.
    pub fn running(app: &str, image: &str) -> Value {
        ps(app, &[(container_name(app), "running", "Up 2 hours", image)])
    }

    /// Any app route for a name outside the seven managed apps.
    pub fn unknown_app() -> Value {
        json!({ "error": "unknown app", "knownApps": ["leetcode-backend", "remote-chat-agent", "leetcode-auth-cookie-capture", "agent-shell", "ci-runner", "vnc-auth-session", "reporting-service", "libs-proxy", "t3-serve", "orca-serve"] })
    }

    /// The 502 of a handler that threw.
    pub fn request_failed(detail: &str) -> Value {
        json!({ "error": "request failed", "detail": detail })
    }

    /// The 502 of a route whose Docker container list failed.
    pub fn engine_down() -> Value {
        request_failed(r#"docker API returned 500: {"message":"engine down"}"#)
    }

    pub fn unauthorized() -> Value {
        json!({ "error": "unauthorized" })
    }

    /// `GET /stats`: the host's memory in MB and load, and each running container as (name, memMb, cpuPct), biggest first.
    pub fn stats(mem_total_mb: u64, mem_available_mb: u64, load: [f64; 3], containers: &[(&str, u64, f64)]) -> Value {
        let mut sorted = containers.to_vec();
        sorted.sort_by_key(|c| std::cmp::Reverse(c.1));
        let listed: Vec<Value> = sorted.iter().map(|(name, mem, cpu)| json!({ "name": name, "memMb": mem, "memLimitMb": null, "cpuPct": js_number(*cpu) })).collect();
        json!({
            "host": { "memTotalMb": mem_total_mb, "memAvailableMb": mem_available_mb, "swapTotalMb": 2048, "load": load.map(js_number) },
            "containers": listed,
            "containersMb": sorted.iter().map(|c| c.1).sum::<u64>(),
        })
    }

    /// `GET /services/<app>/logs`: each container's text by its name.
    pub fn logs(app: &str, tail: u32, texts: &[(&str, &str)]) -> Value {
        let by_name: serde_json::Map<String, Value> = texts.iter().map(|(name, text)| (name.to_string(), json!(text))).collect();
        json!({ "app": app, "tail": tail, "logs": by_name })
    }

    /// `GET /redeploy/status`.
    pub fn redeploy_status(log: &str, history: Value) -> Value {
        json!({ "log": log, "stderr": "", "history": history })
    }

    /// `POST /services/<app>/redeploy?tag=&ref=`: 200 when the sibling exits 0, else 502 with the same keys.
    pub fn redeploy(app: &str, tag: &str, git_ref: &str, exit_code: i64, stdout: &str, stderr: &str) -> Value {
        json!({
            "app": app, "tag": tag, "ref": git_ref, "wipeState": false, "exitCode": exit_code,
            "serverScriptSha": SERVER_SCRIPT_SHA, "stdout": stdout, "stderr": stderr,
        })
    }

    /// `POST /services/<app>/retire`: 200 when the sibling exits 0, else 502 with the same keys.
    pub fn retire(app: &str, exit_code: i64, stdout: &str, stderr: &str) -> Value {
        json!({ "app": app, "exitCode": exit_code, "stdout": stdout, "stderr": stderr })
    }

    /// `POST /redeploy?ref=`, answered 202 before the build.
    pub fn self_redeploy(git_ref: &str) -> Value {
        json!({ "started": true, "building": git_ref })
    }

    /// `POST /services/agent-shell/exec-script`, answered 202.
    pub fn job_started(job: &str) -> Value {
        json!({ "jobId": job })
    }

    /// `GET /services/agent-shell/exec-script/<jobId>`: running until it has an exit code.
    pub fn job(job: &str, exit_code: Option<i64>, output: &str) -> Value {
        json!({ "jobId": job, "running": exit_code.is_none(), "exitCode": exit_code, "output": output })
    }

    /// The exec-script cancel, the opencode-log writes.
    pub fn ok() -> Value {
        json!({ "ok": true })
    }

    /// Every agent-shell route while no agent-shell container exists: 503.
    pub fn agent_shell_not_running() -> Value {
        json!({ "error": "agent-shell container is not running" })
    }

    /// `GET /services/agent-shell/artifact?path=`.
    pub fn artifact(path: &str, base64: &str) -> Value {
        json!({ "path": path, "base64": base64 })
    }

    /// `POST /agent-shell/credentials`, which writes under agent-shell's runtime HOME.
    pub fn credentials(tool: &str, relative_path: &str, bytes: usize) -> Value {
        json!({ "ok": true, "tool": tool, "path": format!("/workspace/home/{relative_path}"), "bytes": bytes })
    }

    /// `POST /drill?action=`, answered 202 before the detached sibling acts: the container it will take down.
    pub fn drill_accepted(action: &str, container: &str) -> Value {
        json!({ "accepted": true, "action": action, "container": container })
    }

    /// The watchdog's own log, one `watchdog.recreated` line per (timestamp, container, ok) as the telemetry crate prints it.
    pub fn watchdog_log(events: &[(&str, &str, bool)]) -> String {
        let line = |(timestamp, container, ok): &(&str, &str, bool)| {
            json!({ "timestamp": timestamp, "level": "ERROR", "hostLabel": "main", "container": container, "reason": "unhealthy", "ok": ok, "message": "watchdog.recreated" }).to_string()
        };
        let started = json!({ "timestamp": "2026-10-06T08:00:00.000000Z", "level": "INFO", "hostLabel": "main", "message": "watchdog.started" }).to_string();
        std::iter::once(started).chain(events.iter().map(line)).collect::<Vec<_>>().join("\n")
    }

    /// `GET /services/vm-ops-api/logs`: the control plane's containers, the watchdog's text among them.
    pub fn vm_ops_api_logs(watchdog: &str) -> Value {
        logs("vm-ops-api", 400, &[("vm-ops-api-ops-api-1", ""), ("vm-ops-api-tailscale", ""), ("watchdog", watchdog)])
    }
}
