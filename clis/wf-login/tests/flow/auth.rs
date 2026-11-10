//! `wf secret login`: logging a tool in here or through the shared desktop, the capture verbs that drive the desktop's listener,
//! and the listener itself (`wf secret login listener`): its control channel, startup, token gate and credential relay. The tools' own login commands, `tailscale`, `s6-setuidgid` and `bws` are stubs; the
//! listener, the backend, vm-ops-api and the delivery sink are mock HTTP services.
use crate::rig::*;
use crate::support::{create_sh_stub, MockReply, MockServer, RunOpts, RunResult};
use wf_vmops::testing::answers;
use base64::Engine;
use regex::Regex;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

/// A throwaway HOME, a stub dir with `bws` answering an empty vault, and a mock listener and backend on one port.
struct Desk {
    dir: tempfile::TempDir,
    home: PathBuf,
    bin: PathBuf,
    server: MockServer,
}

impl Desk {
    fn new() -> Desk {
        let dir = tempfile::Builder::new().prefix("wf-flow-login-").tempdir().unwrap();
        let (home, bin) = (dir.path().join("home"), dir.path().join("bin"));
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&bin).unwrap();
        create_sh_stub(&bin.join("bws"), "printf '%s' \"${BWS_LIST:-[]}\"");
        Desk { dir, home, bin, server: MockServer::start(None) }
    }

    fn stub(&self, name: &str, script: &str) {
        create_sh_stub(&self.bin.join(name), script);
    }

    /// `wf secret login <args>`; `None` removes a default variable.
    fn login(&self, args: &[&str], env: &[(&str, Option<String>)]) -> RunResult {
        self.server.clear();
        let mut merged: Vec<(String, String)> = vec![
            ("HOME".into(), self.home.display().to_string()),
            ("SECRETS_BACKEND".into(), "bitwarden".into()),
            ("SECRETS_LOG_BETTERSTACK".into(), "false".into()),
            ("BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN".into(), "read".into()),
            ("VNC_AUTH_LISTENER_URL".into(), format!("http://127.0.0.1:{}", self.server.port)),
            ("WF_TAILNET_SOCKS".into(), String::new()),
            ("VNC_AUTH_TRIGGER_SECRET".into(), "trigger-secret".into()),
            ("VNC_AUTH_OWNER_CHAT_ID".into(), "4242".into()),
            ("VNC_AUTH_CAPTURE_TIMEOUT_MS".into(), "400".into()),
            ("VNC_AUTH_POLL_INTERVAL_MS".into(), "50".into()),
        ];
        for (key, value) in env {
            merged.retain(|(k, _)| k != key);
            if let Some(value) = value {
                merged.push((key.to_string(), value.clone()));
            }
        }
        // the registry's secrets reach `wf` from the vault, which the bws stub serves, never from its environment
        const VAULT_KEYS: [&str; 3] = ["VNC_AUTH_TRIGGER_SECRET", "VNC_AUTH_OWNER_CHAT_ID", "VM_GATEWAY_AUTH_SEED"];
        let (vaulted, merged): (Vec<_>, Vec<_>) = merged.into_iter().partition(|(k, _)| VAULT_KEYS.contains(&k.as_str()));
        let listing: Vec<Value> = vaulted.iter().map(|(k, v)| json!({ "id": k, "key": k, "value": v })).collect();
        let mut full = vec!["secret", "login"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().bin_dir(&self.bin).cwd(self.dir.path()).clean_env().env("BWS_LIST", &Value::Array(listing).to_string());
        for (key, value) in &merged {
            opts = opts.env(key, value);
        }
        crate::support::run_wf(&full, &opts)
    }

    fn urls(&self) -> Vec<String> {
        self.server.requests().into_iter().map(|q| q.url).collect()
    }
}

fn session_for(tool: &str) -> Value {
    json!({ "sessionId": "s1", "tool": tool, "status": "awaiting-login", "linkUrl": "https://link.example/abc" })
}

#[test]
fn wf_lists_the_tools_with_a_login_names_them_for_an_unknown_one_and_refuses_the_retired_remote_flag() {
    let d = Desk::new();
    // When the tools are listed, an unknown one is asked for, and the retired --remote flag is used
    let list = d.login(&["list"], &[]);
    let unknown = d.login(&["nope"], &[]);
    let retired = d.login(&["--remote", "claude"], &[]);
    // Then every tool with an auth descriptor is printed in registry order, the unknown one names them, and the flag is refused
    assert_eq!((list.exit_code, list.stdout.as_str()), (0, "claude\ncodex\nopencode\ngemini\ngcloud\nleetcode\n"));
    assert_eq!((unknown.exit_code, unknown.stderr.as_str()), (1, "Unknown tool: nope\nKnown tools: claude, codex, opencode, gemini, gcloud, leetcode\n"));
    assert!(retired.exit_code == 2 && retired.stderr.contains("--remote was removed") && d.server.requests().is_empty(), "{}", retired.stderr);
}

#[test]
fn a_person_logs_a_tool_in_through_the_shared_desktop_and_drives_its_capture() {
    let d = Desk::new();
    d.stub("claude", "exit 0");

    // When a login starts and the listener says it finished
    let ready = ("POST /auth/credential", MockReply::json(200, json!({ "ready": true })));
    d.server.routes(&[("POST /auth/start", MockReply::json(200, session_for("claude"))), ready.clone(), ("POST /auth/done", MockReply::json(200, json!({})))]);
    let finished = d.login(&["claude"], &[]);
    let requests = d.server.requests();
    // Then the link is printed, the session is closed, nothing is copied, and a stale credential here is never what decides
    assert_eq!(finished.exit_code, 0, "{}", finished.stderr);
    assert_eq!(
        finished.stdout,
        "Open this link on any device to complete claude login in the shared desktop:\nhttps://link.example/abc\nThis link is reusable across devices and expires in 12 hours.\n\nWaiting for the login to finish...\nclaude: authenticated ✓ (the login stays on the VM; agent-shell has its credential)\n"
    );
    assert_eq!((requests[0].url.as_str(), serde_json::from_str::<Value>(&requests[0].body).unwrap()), ("/auth/start", json!({ "tool": "claude" })));
    assert_eq!((requests[1].url.as_str(), serde_json::from_str::<Value>(&requests[1].body).unwrap()), ("/auth/credential", json!({ "sessionId": "s1", "copy": false })));
    assert_eq!((requests[2].url.as_str(), serde_json::from_str::<Value>(&requests[2].body).unwrap()), ("/auth/done", json!({ "sessionId": "s1" })));
    assert!(!d.home.join(".claude/.credentials.json").exists());

    // When the listener never says it finished
    d.server.routes(&[("POST /auth/start", MockReply::json(200, session_for("claude")))]);
    let timed_out = d.login(&["claude"], &[]);
    // Then it times out with exit 1 and never signals done
    assert_eq!(timed_out.exit_code, 1);
    assert!(timed_out.stderr.contains("claude: login did not complete within the timeout"), "{}", timed_out.stderr);
    assert_eq!(d.urls().into_iter().filter(|u| u != "/auth/credential").collect::<Vec<_>>(), ["/auth/start"]);

    // When leetcode finishes, and codex finishes but /auth/done fails
    d.server.routes(&[("POST /auth/start", MockReply::json(200, session_for("leetcode"))), ready.clone(), ("POST /auth/done", MockReply::json(200, json!({})))]);
    let leetcode = d.login(&["leetcode"], &[]);
    d.server.routes(&[("POST /auth/start", MockReply::json(200, session_for("codex"))), ready, ("POST /auth/done", MockReply::text(500, "boom"))]);
    let codex = d.login(&["codex"], &[]);
    // Then each is confirmed, and a failing done call does not undo a finished login
    assert_eq!(leetcode.exit_code, 0, "{}", leetcode.stderr);
    assert!(leetcode.stdout.contains("leetcode: authenticated ✓"), "{}", leetcode.stdout);
    assert_eq!(codex.exit_code, 0, "{}", codex.stderr);
    assert!(codex.stdout.contains("codex: authenticated ✓"), "{}", codex.stdout);

    // When the listener refuses the start, the tool is unknown, or the listener credentials are missing
    d.server.routes(&[("POST /auth/start", MockReply::text(409, "in flight"))]);
    let refused = d.login(&["claude"], &[]);
    let unknown = d.login(&["nope"], &[]);
    let unknown_calls = d.server.requests().len();
    let no_creds = d.login(&["claude"], &[("VNC_AUTH_TRIGGER_SECRET", None)]);
    // Then each is explained and the last two call nothing
    assert_eq!(refused.exit_code, 1);
    assert!(refused.stderr.contains("Failed to start a login session: auth/start failed: 409 in flight"), "{}", refused.stderr);
    assert!(unknown.exit_code == 1 && unknown.stderr.contains("Unknown tool: nope") && unknown_calls == 0, "{}", unknown.stderr);
    assert!(no_creds.exit_code == 1 && no_creds.stderr.contains("VNC_AUTH_TRIGGER_SECRET and VNC_AUTH_OWNER_CHAT_ID must be set in this container") && d.server.requests().is_empty(), "{}", no_creds.stderr);

    // When a capture starts with neither, both or a malformed spec
    let neither = d.login(&["capture"], &[]);
    let both = d.login(&["capture", "--spec", "a.json", "--spec-json", "{}"], &[]);
    let malformed = d.login(&["capture", "--spec-json", "{nope"], &[]);
    // Then a worked example is shown, and nothing reaches the listener
    assert_eq!(neither.exit_code, 1);
    assert!(neither.stderr.contains("Provide exactly one of --spec <path> or --spec-json <json>.") && neither.stderr.contains("\"loginUrl\": \"https://example.com/accounts/login/\"") && neither.stderr.contains("\"url\": \"${MY_BACKEND_BASE_URL}/api/session\""), "{}", neither.stderr);
    assert!(both.exit_code == 1 && both.stderr.contains("Provide exactly one of"), "{}", both.stderr);
    assert!(malformed.exit_code == 1 && malformed.stderr.contains("Could not read the spec as JSON:") && d.server.requests().is_empty(), "{}", malformed.stderr);

    // When a capture starts from an inline spec, from a file, and through a listener URL with trailing slashes
    let spec = json!({ "id": "my-tool", "loginUrl": "https://example.com/login" });
    d.server.routes(&[("POST /auth/start", MockReply::json(200, json!({ "sessionId": "s1", "tool": "my-tool", "status": "awaiting-login", "linkUrl": "https://link.example/abc" })))]);
    let inline = d.login(&["capture", "--spec-json", &spec.to_string()], &[]);
    let posted = d.server.requests()[0].clone();
    let file = d.dir.path().join("spec.json");
    std::fs::write(&file, spec.to_string()).unwrap();
    d.login(&["capture", "--spec", &file.display().to_string()], &[]);
    let from_file = serde_json::from_str::<Value>(&d.server.requests()[0].body).unwrap();
    d.login(&["capture", "--spec-json", "{}"], &[("VNC_AUTH_LISTENER_URL", Some(format!("http://127.0.0.1:{}///", d.server.port)))]);
    let slashed = d.server.requests()[0].url.clone();
    // Then the spec is posted with the trigger headers, the link and the next step are printed, and the path is never doubled
    assert_eq!(inline.exit_code, 0, "{}", inline.stderr);
    assert_eq!(inline.stdout, "Open this link on any device to complete the my-tool login:\nhttps://link.example/abc\n\nThis link is reusable across devices and expires in 12 hours.\n\nWhen the login is finished, complete the capture with:\n  wf secret login done s1\n");
    assert_eq!((posted.method.as_str(), posted.url.as_str(), serde_json::from_str::<Value>(&posted.body).unwrap()), ("POST", "/auth/start", json!({ "spec": spec })));
    assert_eq!((posted.headers["content-type"].as_str(), posted.headers["x-auth-trigger-secret"].as_str(), posted.headers["x-auth-owner-id"].as_str()), ("application/json", "trigger-secret", "4242"));
    assert_eq!((from_file, slashed.as_str()), (json!({ "spec": spec }), "/auth/start"));
    // and a rejected spec shows the listener's explanation, and missing credentials call nothing
    d.server.routes(&[("POST /auth/start", MockReply::text(400, "capture.urls is required"))]);
    let rejected = d.login(&["capture", "--spec-json", "{}"], &[]);
    let no_secret = d.login(&["capture", "--spec-json", "{}"], &[("VNC_AUTH_TRIGGER_SECRET", None)]);
    assert!(rejected.exit_code == 1 && rejected.stderr.contains("Failed to start the capture session: auth/start failed: 400 capture.urls is required"), "{}", rejected.stderr);
    assert!(no_secret.exit_code == 1 && no_secret.stderr.contains("VNC_AUTH_TRIGGER_SECRET and VNC_AUTH_OWNER_CHAT_ID must be set in this container") && d.server.requests().is_empty());

    // When the capture is completed, cannot capture, fails with an empty body, or has no credentials
    d.server.routes(&[("POST /auth/done", MockReply::json(200, json!({})))]);
    let done = d.login(&["done", "s1"], &[]);
    let done_body = serde_json::from_str::<Value>(&d.server.requests()[0].body).unwrap();
    d.server.routes(&[("POST /auth/done", MockReply::text(502, "no session cookie found in the login browser"))]);
    let cannot = d.login(&["done", "s1"], &[]);
    d.server.routes(&[("POST /auth/done", MockReply::text(500, ""))]);
    let empty = d.login(&["done", "s1"], &[]);
    let no_owner = d.login(&["done", "s1"], &[("VNC_AUTH_OWNER_CHAT_ID", None)]);
    // Then success is printed for the posted session, and each failure shows the reason or only the status
    assert_eq!((done.exit_code, done.stdout.as_str(), done_body), (0, "Credential captured and delivered.\n", json!({ "sessionId": "s1" })));
    assert!(cannot.exit_code == 1 && cannot.stderr.contains("Capture failed: auth/done failed: 502 no session cookie found in the login browser"), "{}", cannot.stderr);
    assert!(empty.stderr.contains("Capture failed: auth/done failed: 500\n"), "{}", empty.stderr);
    assert!(no_owner.exit_code == 1 && no_owner.stderr.contains("VNC_AUTH_TRIGGER_SECRET and VNC_AUTH_OWNER_CHAT_ID must be set in this container"), "{}", no_owner.stderr);

    // When the desktop's browser is sent to a magic link, a bare origin with a port, text that is no URL, and a refused origin
    d.server.routes(&[("POST /auth/launch-url", MockReply::json(200, json!({})))]);
    let magic = "https://claude.ai/magic-link?token=SECRET#frag";
    let launched = d.login(&["launch-url", magic], &[]);
    let launched_body = serde_json::from_str::<Value>(&d.server.requests()[0].body).unwrap();
    let bare = d.login(&["launch-url", "https://Chatgpt.com:443?x=1"], &[]);
    let text = d.login(&["launch-url", "not a url"], &[]);
    d.server.routes(&[("POST /auth/launch-url", MockReply::text(403, "origin not allowed"))]);
    let evil = d.login(&["launch-url", "https://evil.example/x"], &[]);
    // Then the full URL is sent but only its origin and path are echoed, and the refusal is shown
    assert_eq!((launched.exit_code, launched.stdout.as_str(), launched_body), (0, "Navigated the shared desktop's browser to https://claude.ai/magic-link\n", json!({ "url": magic })));
    assert!(bare.stdout.contains("to https://chatgpt.com/\n") && text.stdout.contains("to <unparseable>\n"), "{}{}", bare.stdout, text.stdout);
    assert!(evil.exit_code == 1 && evil.stderr.contains("Failed to launch url: auth/launch-url failed: 403: origin not allowed"), "{}", evil.stderr);
}

// ---- the listener ------------------------------------------------------------------------------------------------------------------

#[test]
fn the_listener_admits_only_the_owner_starts_and_reuses_sessions_and_gates_the_viewer_by_link_token() {
    // Given a listener started in a desktop container with the trigger secret and two owners
    let c = Container::new();
    let spawned = c.dir.path().join("spawned.log");
    c.stub("s6-setuidgid", &format!("echo \"$@\" >> \"{}\"", spawned.display()));
    let l = c.up(&[]);

    // When starts arrive without the secret, with a wrong one, and from someone outside the owners
    let no_secret = l.post("/auth/start", json!({ "tool": "claude" }), &[("x-auth-owner-id", OWNER)]);
    let wrong_secret = l.post("/auth/start", json!({ "tool": "claude" }), &[("x-auth-trigger-secret", "nope"), ("x-auth-owner-id", OWNER)]);
    let intruder = l.post("/auth/start", json!({ "tool": "claude" }), &[("x-auth-trigger-secret", SECRET), ("x-auth-owner-id", "intruder")]);
    // Then the first two are 401 and the third 403, each with a JSON error
    assert_eq!((no_secret.status, wrong_secret.status, intruder.status), (401, 401, 403));
    assert_eq!((wrong_secret.json(), intruder.json()), (json!({ "error": "unauthorized" }), json!({ "error": "forbidden" })));

    // When a GET, the removed /auth/persist, and a body declared over 1 MiB arrive
    let get = fetch(l.port, "GET", "/auth/start", &AUTHED, None);
    let persist = l.authed("/auth/persist", json!({ "tool": "claude-code-oauth-token", "credential": "tok" }));
    let oversized = fetch(l.port, "POST", "/auth/start", &[("content-type", "application/json"), ("expect", "100-continue"), ("x-auth-trigger-secret", SECRET), ("x-auth-owner-id", OWNER)], Some(&"x".repeat(2_000_000)));
    // Then 404 (with a JSON error) and 413 without waiting for the body
    assert_eq!((get.status, persist.status, oversized.status), (404, 404, 413));
    assert_eq!(persist.json(), json!({ "error": "not found" }));

    // When starts arrive with no tool, an unknown tool, and codex twice
    let missing = l.authed("/auth/start", json!({}));
    let unknown = l.authed("/auth/start", json!({ "tool": "nope" }));
    let first = l.authed("/auth/start", json!({ "tool": "codex" }));
    let again = l.authed("/auth/start", json!({ "tool": "codex" })).json();
    // Then 400, 404 naming the tool, and 202 with a tokened link that the second start reuses
    assert_eq!((missing.status, unknown.status, first.status), (400, 404, 202));
    assert_eq!(unknown.json(), json!({ "error": "unknown tool: nope" }));
    let first = first.json();
    assert_eq!((first["tool"].as_str(), first["status"].as_str()), (Some("codex"), Some("awaiting-login")));
    assert!(Regex::new(r"^https://vnc\.example\.ts\.net/\?t=[A-Za-z0-9_-]{40,}$").unwrap().is_match(first["linkUrl"].as_str().unwrap()), "{first}");
    assert_eq!((again["sessionId"].clone(), token_of(&again["linkUrl"])), (first["sessionId"].clone(), token_of(&first["linkUrl"])));

    // When the session is done, done again for nothing, and codex starts afresh
    let done = l.authed("/auth/done", json!({ "sessionId": first["sessionId"] }));
    let no_session = l.authed("/auth/done", json!({}));
    let unknown_session = l.authed("/auth/done", json!({ "sessionId": "never-started" }));
    let next = l.authed("/auth/start", json!({ "tool": "codex" })).json();
    // Then a file credential has nothing to harvest, a bad done is 400 or 404, and the next start is a new session
    assert_eq!((done.status, done.json()), (200, json!({ "ok": true, "persisted": false })));
    assert_eq!((no_session.status, unknown_session.status), (400, 404));
    assert_ne!(next["sessionId"], first["sessionId"]);

    // When a tool whose login runs a command starts, and a spec's command names a tool with path characters
    l.authed("/auth/start", json!({ "tool": "claude" }));
    let odd = l.authed("/auth/start", json!({ "spec": { "id": "../etc/pass wd", "loginCmd": "true", "authErrorPatterns": ["401"] } }));
    assert_eq!(odd.status, 202, "{}", odd.text());
    // Then each command runs in the background as the desktop user, its output in a log file with a safe name
    crate::support::wait_until(Duration::from_secs(10), "both commands to start", || std::fs::read_to_string(&spawned).is_ok_and(|c| c.contains("pass_wd")));
    let commands = std::fs::read_to_string(&spawned).unwrap();
    assert!(commands.contains("abc bash -lc claude setup-token >/tmp/vnc-auth-login-claude.log 2>&1"), "{commands}");
    assert!(commands.contains("/tmp/vnc-auth-login-___etc_pass_wd.log"), "{commands}");

    // When specs arrive: one for an unshipped tool, one that captures with nowhere to deliver, and ones with neither, both or no patterns
    let spec = json!({ "id": "some-vendor", "loginUrl": "http://127.0.0.1:9/login", "authErrorPatterns": ["401"], "capture": { "kind": "cookies", "urls": ["http://127.0.0.1:9"], "require": ["SID"] } });
    let mut good = spec.clone();
    good["deliver"] = json!({ "kind": "http", "url": "https://sink.example/store" });
    let mut half = spec;
    half["id"] = json!("half");
    let good = l.authed("/auth/start", json!({ "spec": good }));
    let half = l.authed("/auth/start", json!({ "spec": half }));
    let neither = l.authed("/auth/start", json!({ "spec": { "id": "x", "authErrorPatterns": ["401"] } }));
    let both = l.authed("/auth/start", json!({ "spec": { "id": "x", "authErrorPatterns": ["401"], "loginCmd": "x", "loginUrl": "https://x.example" } }));
    let no_patterns = l.authed("/auth/start", json!({ "spec": { "id": "x", "authErrorPatterns": [], "loginCmd": "x" } }));
    // Then the deliverable one starts, and each other is 400 saying why
    assert_eq!((good.status, good.json()["tool"].clone()), (202, json!("some-vendor")));
    assert_eq!((half.status, half.json()["error"].clone()), (400, json!("invalid spec")));
    assert!(half.json()["detail"].to_string().contains("capture and deliver"), "{}", half.text());
    assert_eq!((neither.status, both.status, no_patterns.status), (400, 400, 400));
    assert!(neither.json()["detail"].to_string().contains("exactly one of `loginCmd` or `loginUrl`"), "{}", neither.text());

    // When the desktop's browser is sent somewhere with a wrong secret, no URL, a foreign origin, or a look-alike
    let wrong = l.post("/auth/launch-url", json!({ "url": "https://claude.ai/x" }), &[("x-auth-trigger-secret", "wrong"), ("x-auth-owner-id", OWNER)]);
    let no_url = l.authed("/auth/launch-url", json!({}));
    let evil = l.authed("/auth/launch-url", json!({ "url": "https://evil.example.com/phish" }));
    let lookalike = l.authed("/auth/launch-url", json!({ "url": "https://claude.ai.evil.example/x" }));
    // Then 401, 400, and 400 for both origins
    assert_eq!((wrong.status, no_url.status, evil.status, lookalike.status), (401, 400, 400, 400));
    assert_eq!(evil.json(), json!({ "error": "url origin not allowlisted" }));

    // When the gate is hit without a token, with a wrong one, with a guessed cookie, and with the link token
    let token = token_of(&next["linkUrl"]);
    let refused = [l.gate("/", &[]).status, l.gate("/?t=wrong", &[]).status, l.gate("/frame", &[("cookie", "vnc_auth_t=guess")]).status, fetch(l.gate, "POST", "/input", &[], Some("{}")).status];
    let page = l.gate(&format!("/?t={token}"), &[]);
    let by_cookie = l.gate("/frame", &[("cookie", &format!("vnc_auth_t={token}"))]);
    let elsewhere = l.gate(&format!("/?t={token}"), &[]);
    // Then every route is 403 without it; with it the viewer is served with an HttpOnly cookie that alone opens it again
    assert_eq!(refused, [403, 403, 403, 403]);
    assert_eq!((page.status, page.header("content-type")), (200, Some("text/html; charset=utf-8")));
    assert!(page.text().contains("<canvas") && page.text().contains("id=\"paste\"") && page.text().contains("id=\"box\""), "the viewer needs its paste button and text box");
    assert_eq!(page.header("set-cookie"), Some(format!("vnc_auth_t={token}; HttpOnly; Path=/; SameSite=Lax").as_str()));
    assert_eq!(by_cookie.status, 503, "past the gate, and there is no browser yet");
    assert_eq!(elsewhere.status, 200);

    // When a frame, a tap, garbage, an unknown event and an unknown route are asked for with no browser running
    let input = |body: &str| fetch(l.gate, "POST", &format!("/input?t={token}"), &[], Some(body)).status;
    let statuses = [l.gate(&format!("/frame?t={token}"), &[]).status, input(r#"{"type":"tap","x":1,"y":2}"#), input("not json"), input(r#"{"type":"drag"}"#), l.gate(&format!("/etc/passwd?t={token}"), &[]).status];
    // Then 503 for what needs a browser, 400 for what is malformed, 404 for the unknown route
    assert_eq!(statuses, [503, 503, 400, 400, 404]);
}

#[test]
fn the_listener_refuses_to_start_half_configured_and_binds_its_tailnet_route_to_the_gate() {
    let c = Container::new();

    // When the trigger secret or the owner is missing
    let no_secret = c.start(&[("VNC_AUTH_TRIGGER_SECRET", Some(""))]).err().unwrap_or_default();
    let no_owner = c.start(&[("VNC_AUTH_OWNER_CHAT_ID", Some(""))]).err().unwrap_or_default();
    // Then it exits naming the variable
    assert!(no_secret.contains("Missing required env var: VNC_AUTH_TRIGGER_SECRET"), "{no_secret}");
    assert!(no_owner.contains("Missing required env var: VNC_AUTH_OWNER_CHAT_ID"), "{no_owner}");

    // When tailscale is down, or cannot bind the serve route, and no link base is set
    c.stub("tailscale", "exit 1");
    let down = c.start(&[("VNC_AUTH_LINK_BASE_URL", Some(""))]).err().unwrap_or_default();
    c.stub("tailscale", "echo \"serve is disabled on this node\" >&2; exit 1");
    let unbound = c.start(&[]).err().unwrap_or_default();
    // Then it exits saying how to fix it, rather than look healthy with a link that leads nowhere
    assert!(down.contains("Could not determine link base URL: set VNC_AUTH_LINK_BASE_URL or ensure tailscale is up"), "{down}");
    assert!(unbound.contains("tailscale serve bind failed: serve is disabled on this node"), "{unbound}");

    // When tailscale is up and knows this node's name
    c.stub("tailscale", &format!("echo \"$@\" >> \"{}/tailscale.log\"\ncase \"$*\" in *\"status --json\"*) echo '{{\"Self\":{{\"DNSName\":\"vnc-auth.tail1234.ts.net.\"}}}}';; esac", c.dir.path().display()));
    let l = c.up(&[("VNC_AUTH_LINK_BASE_URL", Some("")), ("TS_SOCKET", Some("/run/ts/tailscaled.sock"))]);
    let started = l.authed("/auth/start", json!({ "tool": "codex" })).json();
    // Then links use that name and the serve route goes to the gate, with --socket before the subcommand
    assert!(Regex::new(r"^https://vnc-auth\.tail1234\.ts\.net/\?t=").unwrap().is_match(started["linkUrl"].as_str().unwrap()), "{started}");
    assert_eq!(c.tailscale_calls(), ["--socket=/run/ts/tailscaled.sock status --json".to_string(), format!("--socket=/run/ts/tailscaled.sock serve --bg --https=443 http://127.0.0.1:{}", l.gate)]);
}

#[test]
fn the_listener_relays_a_file_credential_to_agent_shell_and_keeps_secrets_out_of_its_log() {
    // Given a listener with a vm-ops-api to relay to, and one without its token
    let c = Container::new();
    let written = answers::credentials("codex", ".codex/auth.json", "{\"token\":\"abc\"}".len()).to_string();
    let vm_ops = Recorder::start(move |_| ok(&written));
    let l = c.up(&[("VM_OPS_API_AUTH_TOKEN", Some("ops-token")), ("VNC_AUTH_VM_OPS_API_URL", Some(&format!("http://127.0.0.1:{}", vm_ops.port)))]);

    // When a codex login starts and its credential file appears
    l.authed("/auth/start", json!({ "tool": "codex" }));
    std::fs::create_dir_all(c.home.join(".codex")).unwrap();
    std::fs::write(c.home.join(".codex/auth.json"), "{\"token\":\"abc\"}").unwrap();
    l.wait_for("agent_shell_relay.succeeded", Duration::from_secs(15));
    // Then it is relayed once to agent-shell with the bearer token, and neither the token nor the content reach the log
    let seen = vm_ops.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].method.as_str(), seen[0].url.as_str(), seen[0].headers["authorization"].as_str()), ("POST", "/agent-shell/credentials", "Bearer ops-token"));
    assert_eq!(serde_json::from_str::<Value>(&seen[0].body).unwrap(), json!({ "tool": "codex", "content": base64::engine::general_purpose::STANDARD.encode("{\"token\":\"abc\"}") }));
    let everything = Value::Array(l.logs()).to_string();
    assert!(!everything.contains("ops-token") && !everything.contains("\\\"token\\\":\\\"abc\\\""));

    // When a file login starts with no vm-ops-api token available
    let other = Container::new();
    let untokened = other.up(&[("VM_OPS_API_AUTH_TOKEN", Some(""))]);
    untokened.authed("/auth/start", json!({ "tool": "codex" }));
    // Then the missing token is logged (after the start has answered: a loaded runner logs it later) and nothing is sent
    untokened.wait_for("agent_shell_relay.missing_token", Duration::from_secs(15));
}

#[test]
fn a_login_command_start_is_answered_and_run_while_a_navigation_is_stuck_on_the_browser() {
    // Given a listener whose browser endpoint accepts connections and never answers, so a navigation holds the browser lock
    let stuck = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://127.0.0.1:{}", stuck.local_addr().unwrap().port());
    let c = Container::new();
    let spawned = c.dir.path().join("spawned.log");
    c.stub("s6-setuidgid", &format!("echo \"$@\" >> \"{}\"", spawned.display()));
    let l = c.up(&[("VNC_AUTH_CDP_ENDPOINT", Some(endpoint.as_str()))]);
    let spec = json!({ "id": "some-vendor", "loginUrl": "http://127.0.0.1:9/login", "authErrorPatterns": ["401"] });

    // When a loginUrl start begins navigating and a login-command start follows
    let navigating = l.authed("/auth/start", json!({ "spec": spec }));
    let began = std::time::Instant::now();
    let command = l.authed("/auth/start", json!({ "tool": "claude" }));

    // Then both answer 202 and the command runs without waiting out the navigation
    assert_eq!((navigating.status, command.status), (202, 202), "{} {}", navigating.text(), command.text());
    crate::support::wait_until(Duration::from_secs(5), "the login command to start", || std::fs::read_to_string(&spawned).is_ok_and(|c| c.contains("claude setup-token")));
    assert!(began.elapsed() < Duration::from_secs(5), "the command waited {:?} behind the navigation", began.elapsed());
}

#[test]
fn login_with_copy_writes_the_declared_files_here_and_selects_the_gcloud_account() {
    // Given a machine with no credential, and a listener that holds the finished login's files
    let d = Desk::new();
    let content = base64::engine::general_purpose::STANDARD.encode(r#"{"token":"from-the-desktop"}"#);
    d.stub("gcloud", "if [ \"$1 $2\" = \"auth list\" ]; then echo me@example.com; exit 0; fi; echo \"$@\" >> \"$HOME/gcloud.calls\"; exit 0");
    d.server.routes(&[
        ("POST /auth/start", MockReply::json(200, session_for("gcloud"))),
        ("POST /auth/credential", MockReply::json(200, json!({ "ready": true, "files": [{ "path": "~/.config/gcloud/credentials.db", "content": content }] }))),
        ("POST /auth/done", MockReply::json(200, json!({}))),
    ]);
    // When the login waits with --copy
    let r = d.login(&["gcloud", "--copy"], &[]);
    // Then the file is written under this home, owner-only, the session is closed, and the account is selected
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("gcloud: authenticated ✓ (credential copied to this machine)") && r.stdout.contains("active account me@example.com"), "{}", r.stdout);
    let written = d.home.join(".config/gcloud/credentials.db");
    assert_eq!(std::fs::read_to_string(&written).unwrap(), r#"{"token":"from-the-desktop"}"#);
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&written).unwrap().permissions()) & 0o777, 0o600);
    assert_eq!(d.urls(), ["/auth/start", "/auth/credential", "/auth/done"]);
    assert_eq!(serde_json::from_str::<Value>(&d.server.requests()[1].body).unwrap(), json!({ "sessionId": "s1", "copy": true }));
    assert_eq!(std::fs::read_to_string(d.home.join("gcloud.calls")).unwrap().trim(), "config set account me@example.com");

    // When a login declares nothing to copy and --copy is asked for
    d.server.routes(&[("POST /auth/start", MockReply::json(200, session_for("leetcode"))), ("POST /auth/credential", MockReply::text(400, r#"{"error":"this login declares nothing to copy"}"#))]);
    let nothing = d.login(&["leetcode", "--copy"], &[]);
    // Then it says so and exits 1 without waiting out the link
    assert!(nothing.exit_code == 1 && nothing.stderr.contains("declares no credential to copy"), "{}", nothing.stderr);
}

/// Every file under `dir` with its bytes.
fn home_snapshot(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(path.strip_prefix(root).unwrap().display().to_string(), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn a_listener_started_on_a_restored_home_serves_a_login_check_and_leaves_every_credential_byte_identical() {
    // Given the credential volume a backup restored as the container's home: logins of three tools and a file of the old container's own
    let c = Container::new();
    c.stub("s6-setuidgid", "true");
    for (path, body) in [(".claude/.credentials.json", "{\"claudeAiOauth\":\"restored\"}"), (".codex/auth.json", "tok-restored"), (".config/gcloud/application_default_credentials.json", "{\"type\":\"authorized_user\"}"), ("notes.txt", "kept\n")] {
        std::fs::create_dir_all(c.home.join(path).parent().unwrap()).unwrap();
        std::fs::write(c.home.join(path), body).unwrap();
    }
    let before = home_snapshot(&c.home);

    // When the listener starts on it, a login for codex is started, and its credential state is asked for (with the copy of the declared file), and the listener stops
    let l = c.up(&[]);
    let id = l.authed("/auth/start", json!({ "tool": "codex" })).json()["sessionId"].clone();
    let state = l.authed("/auth/credential", json!({ "sessionId": id, "copy": true }));
    assert_eq!(state.status, 200, "{}", state.text());
    drop(l);

    // Then every restored file is byte-identical, none was created over, rewritten or removed, and the credential the login found was the restored one
    assert_eq!(state.json()["files"][0]["content"], base64::engine::general_purpose::STANDARD.encode("tok-restored"));
    assert_eq!(home_snapshot(&c.home), before);
}

#[test]
fn the_listener_reports_a_finished_login_and_hands_over_only_the_files_its_config_declares() {
    // Given a listener in a desktop container whose login for codex has not finished
    let c = Container::new();
    c.stub("s6-setuidgid", "true");
    let l = c.up(&[]);
    let id = l.authed("/auth/start", json!({ "tool": "codex" })).json()["sessionId"].clone();
    let ask = |copy: bool| l.authed("/auth/credential", json!({ "sessionId": id, "copy": copy }));
    // When it is asked before and after the file lands, with and without copy
    let before = ask(true);
    std::fs::create_dir_all(c.home.join(".codex")).unwrap();
    std::fs::write(c.home.join(".codex/auth.json"), "tok-1").unwrap();
    let after = ask(true);
    let ready_only = ask(false);
    // Then it is not ready, then ready with the declared file's base64, and ready alone when nothing is asked to be copied
    assert_eq!((before.status, before.json()), (200, json!({ "ready": false })));
    assert_eq!(after.json(), json!({ "ready": true, "files": [{ "path": "~/.codex/auth.json", "content": base64::engine::general_purpose::STANDARD.encode("tok-1") }] }));
    assert_eq!(ready_only.json(), json!({ "ready": true }));

    // When a spec declares nothing to copy, a stranger's session is named, or the secret is missing
    let spec = json!({ "id": "some-vendor", "loginCmd": "true", "credentialPaths": ["~/.codex/auth.json"], "authErrorPatterns": ["401"] });
    let bare = l.authed("/auth/start", json!({ "spec": spec })).json()["sessionId"].clone();
    let undeclared = l.authed("/auth/credential", json!({ "sessionId": bare, "copy": true }));
    let unknown = l.authed("/auth/credential", json!({ "sessionId": "nope", "copy": true }));
    let no_secret = l.post("/auth/credential", json!({ "sessionId": id, "copy": true }), &[("x-auth-owner-id", OWNER)]);
    // Then an undeclared path is never read (400), a stranger's session is 404, and a missing secret is 401
    assert_eq!((undeclared.status, unknown.status, no_secret.status), (400, 404, 401));
    assert_eq!(undeclared.json(), json!({ "error": "this login declares nothing to copy" }));
    // and a spec cannot declare a path outside the home
    let escape = l.authed("/auth/start", json!({ "spec": { "id": "x", "loginCmd": "true", "copy": ["~/../etc/passwd"], "authErrorPatterns": ["401"] } }));
    assert_eq!(escape.status, 400);
    // and a vault mirror must name one of the declared copy files
    let stray = l.authed("/auth/start", json!({ "spec": { "id": "y", "loginCmd": "true", "copy": ["~/a"], "vault": { "key": "K", "file": "~/b" }, "authErrorPatterns": ["401"] } }));
    assert_eq!(stray.status, 400);
    // and the credential never reaches the listener's log
    assert!(!l.logs().iter().any(|line| line.to_string().contains("tok-1")));
}

#[test]
fn login_with_clear_forgets_the_old_login_here_and_in_the_vm_before_it_starts_a_fresh_one() {
    // Given a machine that still holds claude's credential file, and a listener that clears and then finishes a new login
    let d = Desk::new();
    let credential = d.home.join(".claude/.credentials.json");
    std::fs::create_dir_all(credential.parent().unwrap()).unwrap();
    std::fs::write(&credential, "{}").unwrap();
    d.server.routes(&[
        ("POST /auth/logout", MockReply::json(200, json!({ "ok": true, "removed": ["~/.claude/.credentials.json"], "browser": true }))),
        ("POST /auth/start", MockReply::json(200, session_for("claude"))),
        ("POST /auth/credential", MockReply::json(200, json!({ "ready": true }))),
        ("POST /auth/done", MockReply::json(200, json!({}))),
    ]);
    // When it logs in with --clear
    let r = d.login(&["claude", "--clear"], &[]);
    // Then the local file is gone, the VM is asked to log out before anything starts, and the login still completes
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("claude: cleared 1 file(s) here and 1 on the VM, browser cookies cleared"), "{}", r.stdout);
    assert!(!credential.exists());
    assert_eq!(d.urls(), ["/auth/logout", "/auth/start", "/auth/credential", "/auth/done"]);
    assert_eq!(serde_json::from_str::<Value>(&d.server.requests()[0].body).unwrap(), json!({ "tool": "claude" }));

    // When the VM cannot be logged out
    d.server.routes(&[("POST /auth/logout", MockReply::text(502, "no"))]);
    let failed = d.login(&["claude", "--clear"], &[]);
    // Then it says so and does not start a login
    assert!(failed.exit_code == 1 && failed.stderr.contains("Failed to clear the login on the VM"), "{}", failed.stderr);
    assert_eq!(d.urls(), ["/auth/logout"]);
}

#[test]
fn the_listener_logs_a_tool_out_by_deleting_its_declared_files_revoking_and_ending_the_open_session() {
    // Given a listener whose codex login finished and whose gcloud is a stub that records its calls
    let c = Container::new();
    let users = c.dir.path().join("users.log");
    c.stub("s6-setuidgid", &format!("echo \"$1\" >> \"{}\"; shift; exec \"$@\"", users.display()));
    let revoked = c.dir.path().join("gcloud.calls");
    c.stub("gcloud", &format!("echo \"$@\" >> \"{}\"", revoked.display()));
    let l = c.up(&[]);
    std::fs::create_dir_all(c.home.join(".codex")).unwrap();
    std::fs::write(c.home.join(".codex/auth.json"), "tok-2").unwrap();
    std::fs::create_dir_all(c.home.join(".config/gcloud")).unwrap();
    std::fs::write(c.home.join(".config/gcloud/root-owned-leftover"), "x").unwrap();
    let first = l.authed("/auth/start", json!({ "tool": "codex" })).json()["sessionId"].clone();
    // When codex is logged out, gcloud is logged out, an unknown tool is named, and the secret is missing
    let out = l.authed("/auth/logout", json!({ "tool": "codex" }));
    let gcloud = l.authed("/auth/logout", json!({ "tool": "gcloud" }));
    let unknown = l.authed("/auth/logout", json!({ "tool": "nope" }));
    let no_secret = l.post("/auth/logout", json!({ "tool": "codex" }), &[("x-auth-owner-id", OWNER)]);
    // Then the declared file is deleted and reported by path, the next start is a new session, gcloud was revoked, and the gate held
    assert_eq!(out.status, 200);
    assert_eq!(out.json()["removed"], json!(["~/.codex/auth.json"]));
    assert!(!c.home.join(".codex/auth.json").exists());
    assert_ne!(l.authed("/auth/start", json!({ "tool": "codex" })).json()["sessionId"], first);
    assert_eq!(gcloud.status, 200);
    assert_eq!(std::fs::read_to_string(&revoked).unwrap().trim(), "auth revoke --all");
    assert!(std::fs::read_to_string(&users).unwrap().lines().all(|u| u == "abc") && !c.home.join(".config/gcloud").exists(), "gcloud ran as the login user and its config dir is gone");
    assert_eq!((unknown.status, no_secret.status), (404, 401));
    assert!(!l.logs().iter().any(|line| line.to_string().contains("tok-2")));
}

#[test]
fn the_listener_relays_gcloud_only_once_its_validity_check_passes_not_when_an_empty_file_appears() {
    // Given a listener with a vm-ops-api to relay to, and a gcloud whose account list is empty until a marker exists
    let c = Container::new();
    let users = c.dir.path().join("users.log");
    c.stub("s6-setuidgid", &format!("echo \"$1\" >> \"{}\"; shift; exec \"$@\"", users.display()));
    let marker = c.dir.path().join("logged-in");
    c.stub("gcloud", &format!("[ -f \"{}\" ] && echo me@example.com", marker.display()));
    let written = answers::credentials("gcloud", ".config/gcloud/credentials.db", 5).to_string();
    let vm_ops = Recorder::start(move |_| ok(&written));
    let l = c.up(&[("VM_OPS_API_AUTH_TOKEN", Some("ops-token")), ("VNC_AUTH_VM_OPS_API_URL", Some(&format!("http://127.0.0.1:{}", vm_ops.port)))]);
    // When a gcloud login starts and an empty credentials.db appears (any gcloud command creates one)
    l.authed("/auth/start", json!({ "tool": "gcloud" }));
    std::fs::create_dir_all(c.home.join(".config/gcloud")).unwrap();
    std::fs::write(c.home.join(".config/gcloud/credentials.db"), "EMPTY").unwrap();
    std::thread::sleep(Duration::from_secs(5));
    // Then nothing is relayed
    assert!(vm_ops.seen().is_empty(), "an empty credential must never overwrite agent-shell's login");
    // When the login really finishes
    std::fs::write(&marker, "").unwrap();
    l.wait_for("agent_shell_relay.succeeded", Duration::from_secs(15));
    // Then it is relayed once, and the validity check ran as the login user, never as the listener's own
    assert_eq!(vm_ops.seen().len(), 1);
    assert!(std::fs::read_to_string(&users).unwrap().lines().all(|u| u == "abc"));
}

const VAULT_STUB: &str = r#"echo "$*" >> "$HOME/bws.log"
if [ "$1 $2" = "secret list" ]; then echo '[{"id":"v1","key":"GCLOUD_DRIVE_CREDENTIALS_DB_B64","value":"old"},{"id":"v2","key":"VNC_AUTH_TRIGGER_SECRET","value":"trigger-secret"},{"id":"v3","key":"VNC_AUTH_OWNER_CHAT_ID","value":"4242"}]'; exit 0; fi
exit 0"#;

#[test]
fn a_gcloud_login_mirrors_its_credential_into_the_vault_and_clear_empties_that_secret() {
    // Given a vault that holds the gcloud secret, and a listener that finishes the login with its credential file
    let d = Desk::new();
    d.stub("bws", VAULT_STUB);
    let content = base64::engine::general_purpose::STANDARD.encode("sqlite-bytes");
    let edit = [("BWS_DEPLOY_SECRETS_EDIT_ACCESS_TOKEN", Some("edit".to_string()))];
    d.server.routes(&[
        ("POST /auth/start", MockReply::json(200, session_for("gcloud"))),
        ("POST /auth/credential", MockReply::json(200, json!({ "ready": true, "files": [{ "path": "~/.config/gcloud/credentials.db", "content": content }] }))),
        ("POST /auth/done", MockReply::json(200, json!({}))),
        ("POST /auth/logout", MockReply::json(200, json!({ "ok": true, "removed": [], "browser": false }))),
    ]);
    // When it logs in without --copy
    let r = d.login(&["gcloud"], &edit);
    // Then the vault secret is updated with the file's base64, nothing is copied here, and the files were asked for anyway
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("gcloud: vault secret GCLOUD_DRIVE_CREDENTIALS_DB_B64 updated"), "{}{}", r.stdout, r.stderr);
    let log = std::fs::read_to_string(d.home.join("bws.log")).unwrap();
    assert!(log.contains("secret edit") && log.contains(&format!("--value {content}")) && log.contains(" v1"), "{log}");
    assert!(!d.home.join(".config/gcloud/credentials.db").exists());
    assert_eq!(serde_json::from_str::<Value>(&d.server.requests()[1].body).unwrap(), json!({ "sessionId": "s1", "copy": true }));

    // When it logs in with --clear
    std::fs::remove_file(d.home.join("bws.log")).unwrap();
    let cleared = d.login(&["gcloud", "--clear"], &edit);
    // Then the secret is first emptied (a value no restore can use as a login), then set again by the new login
    assert_eq!(cleared.exit_code, 0, "{}", cleared.stderr);
    assert!(cleared.stdout.contains("gcloud: vault secret GCLOUD_DRIVE_CREDENTIALS_DB_B64 cleared"), "{}", cleared.stdout);
    let log = std::fs::read_to_string(d.home.join("bws.log")).unwrap();
    let edits: Vec<&str> = log.lines().filter(|l| l.contains("secret edit")).collect();
    assert!(edits.len() == 2 && edits[0].contains("--value bG9nZ2VkLW91dA==") && edits[1].contains(&format!("--value {content}")), "{log}");

    // When the vault cannot be written (no write token), Then the login still succeeds and says why
    let no_token = d.login(&["gcloud"], &[]);
    assert_eq!(no_token.exit_code, 0, "{}", no_token.stderr);
    assert!(no_token.stderr.contains("could not update the vault secret GCLOUD_DRIVE_CREDENTIALS_DB_B64"), "{}", no_token.stderr);
}
