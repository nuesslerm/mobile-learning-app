//! `wf browser`: the CLI against a fake daemon, and the tunnel and tailnet forward through a fake SOCKS5 tailnet. Every external
//! dependency is doubled; the real Chromium and ffmpeg journeys are `tests/e2e/browser.rs`.
use crate::support::{http_get, node_path, run_wf, spawn_wf, wait_until, MockDaemon, MockReply, MockServer, MockTailnet, RunOpts, RunResult, Upstream};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SLOT: &str = "main/mla-tamagui/default";

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("json {text:?}: {e}"))
}

/// A verb's one JSON line with its timing zeroed.
fn line(r: &RunResult) -> Value {
    let mut v = parse(r.stdout.trim());
    if v.get("ms").is_some() {
        v["ms"] = json!(0);
    }
    v
}

/// A repo on `main` whose slot names the fake daemon in a throwaway registry.
struct Client {
    root: tempfile::TempDir,
    repo: PathBuf,
    reg_dir: PathBuf,
    bin_dir: PathBuf,
    daemon: MockDaemon,
}

impl Client {
    fn new() -> Client {
        let root = tempfile::Builder::new().prefix("wf-flow-browser-client-").tempdir().unwrap();
        let (repo, reg_dir, bin_dir) = (root.path().join("repo"), root.path().join("reg"), root.path().join("bin"));
        for dir in [&repo, &reg_dir, &bin_dir] {
            std::fs::create_dir(dir).unwrap();
        }
        assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(&repo).status().unwrap().success());
        assert!(Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"]).current_dir(&repo).status().unwrap().success());
        let c = Client { root, repo, reg_dir, bin_dir, daemon: MockDaemon::start() };
        c.register(Some(&c.daemon.url.clone()));
        c
    }

    fn register(&self, url: Option<&str>) {
        let mut slot = json!({ "createdAt": "2026-01-01T00:00:00Z", "port_claims": {}, "device_claims": {} });
        if let Some(url) = url {
            slot["browserMcp"] = json!({ "url": url, "startedAt": "2026-01-01T00:00:00Z" });
        }
        let registry = json!({ "version": 3, "slots": { SLOT: slot }, "devices": { "ios": {}, "android": {} }, "services": {} });
        std::fs::write(self.reg_dir.join("registry.json"), format!("{}\n", serde_json::to_string_pretty(&registry).unwrap())).unwrap();
    }

    fn registered(&self) -> Option<Value> {
        parse(&std::fs::read_to_string(self.reg_dir.join("registry.json")).unwrap())["slots"][SLOT].get("browserMcp").cloned()
    }

    fn browser(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        self.daemon.clear();
        let mut full = vec!["browser"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().bin_dir(&self.bin_dir).cwd(&self.repo).clean_env().env("WF_REGISTRY_DIR", &self.reg_dir.display().to_string());
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }
}

/// `(args, daemon method, the call it must post, the daemon's result, the envelope's device and result)`.
type ClientCase = (Vec<&'static str>, &'static str, Value, Value, Value, Value);

fn client_cases() -> Vec<ClientCase> {
    let opened = json!({ "name": "a", "renderMode": "device-frame" });
    vec![
        (vec!["start-device", "--device", "a"], "open", json!({ "name": "a", "devicePreset": "iphone-15-pro", "orientation": "portrait", "colorScheme": "system", "frame": "screen" }), opened.clone(), json!("a"), opened.clone()),
        (
            vec!["start-device", "--device", "a", "--screen-size", "pixel-8", "--frame", "bezel", "--color-scheme", "dark", "--tailnet", "--plain", "--headless", "--json"],
            "open",
            json!({ "name": "a", "devicePreset": "pixel-8", "orientation": "portrait", "colorScheme": "dark", "frame": "bezel", "tailnetProxy": true, "renderMode": "plain", "headless": true }),
            opened.clone(),
            json!("a"),
            opened,
        ),
        (vec!["stop-device", "--device", "a"], "close", json!({ "name": "a" }), json!({ "closed": "a" }), json!("a"), json!({ "closed": ["a"] })),
        (vec!["set-breakpoint", "src/math.ts:3:a > 1", "--device", "a"], "debugSet", json!({ "name": "a", "file": "src/math.ts", "line": 3, "pause": true, "condition": "a > 1" }), json!({ "id": 1, "resolved": true }), json!("a"), json!({ "id": 1, "resolved": true })),
        (vec!["debug-pause", "--device", "a", "--timeout", "2000"], "debugPause", json!({ "name": "a", "timeout": 2000 }), json!({ "paused": true }), json!("a"), json!({ "paused": true })),
        (vec!["debug-step", "--device", "a"], "debugStep", json!({ "name": "a", "kind": "over" }), json!({ "line": 4 }), json!("a"), json!({ "line": 4 })),
        (vec!["debug-step", "into", "--device", "a"], "debugStep", json!({ "name": "a", "kind": "into" }), json!({ "line": 9 }), json!("a"), json!({ "line": 9 })),
        (vec!["debug-eval", "x + 1", "--device", "a"], "debugEval", json!({ "name": "a", "expression": "x + 1" }), json!({ "value": 2 }), json!("a"), json!({ "value": 2 })),
        (vec!["debug-resume", "--device", "a"], "debugResume", json!({ "name": "a" }), json!({ "resumed": true, "paused": false }), json!("a"), json!({ "resumed": true, "paused": false })),
        (vec!["tailnet-forward", "--host", "h", "--port", "8080", "--local-port", "9000"], "tailnetForward", json!({ "host": "h", "port": 8080, "localPort": 9000 }), json!({ "url": "http://127.0.0.1:9000", "reused": true }), Value::Null, json!({ "url": "http://127.0.0.1:9000", "reused": true })),
    ]
}

#[test]
fn the_browser_cli_posts_each_verb_to_the_registered_daemon_and_answers_with_one_envelope() {
    // Given a fake daemon registered for this repo's slot
    let c = Client::new();

    // When each verb runs
    for (args, method, call, result, device, answer) in client_cases() {
        c.daemon.results(&[(method, result)]);
        let r = c.browser(&args, &[]);
        // Then it posts exactly that call to /call (after /health) and prints one envelope carrying the daemon's answer
        let verb = args[0];
        assert_eq!((r.exit_code, line(&r), r.stderr.as_str()), (0, json!({ "ok": true, "command": verb, "platform": "browser", "device": device, "ms": 0, "result": answer }), ""), "{args:?}");
        assert_eq!(c.daemon.calls(), [(method.to_string(), call)], "{args:?}");
        assert!(c.daemon.requests().iter().all(|q| q.url == "/health" || q.url == "/call"), "{args:?}");
    }

    // When the devices are listed, and two sessions are open and a session verb names none
    c.daemon.results(&[("list", json!([{ "name": "a" }, { "name": "b" }]))]);
    let listed = c.browser(&["list-devices"], &[]);
    let several = c.browser(&["hierarchy"], &[]);
    // Then the list carries the daemon, the sessions and the 11 screen presets, and the verb asks which device
    let devices = line(&listed)["result"].clone();
    assert_eq!((listed.exit_code, devices["daemon"].clone(), devices["devices"].clone()), (0, json!({ "running": true, "url": c.daemon.url }), json!([{ "name": "a" }, { "name": "b" }])));
    let screens: Vec<&str> = devices["screens"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!((screens.len(), screens[0], screens[10]), (11, "iphone-se", "desktop"));
    assert_eq!((several.exit_code, line(&several)["error"].clone()), (2, json!("several devices: pass --device, one of a, b")));

    // When the only open session belongs to a running test, and every session is closed
    c.daemon.results(&[("list", json!([{ "name": "run-1", "owner": 4242 }]))]);
    let owned = c.browser(&["debug-resume"], &[]);
    let owned_calls = c.daemon.calls();
    c.daemon.results(&[("list", json!([{ "name": "a" }, { "name": "b" }])), ("close", json!({ "closed": "x" }))]);
    let all = c.browser(&["stop-device", "--all"], &[]);
    // Then the verb refuses it unless --device names it, and --all closes each one
    assert!(owned.exit_code == 1 && line(&owned)["error"].as_str().unwrap().contains("belongs to a running test (pid 4242): pass --device run-1"), "{}", owned.stdout);
    assert_eq!(owned_calls.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(), ["list"]);
    assert_eq!(line(&all)["result"], json!({ "closed": ["a", "b"] }));
    assert_eq!(c.daemon.calls().into_iter().filter(|(m, _)| m == "close").map(|(_, a)| a).collect::<Vec<_>>(), [json!({ "name": "a" }), json!({ "name": "b" })]);

    // When the daemon refuses a call, a preset is given as the device, the old --name is given, or a step kind or a screen
    // size is unknown
    c.daemon.errors(&[("debugEval", "debug-eval: the page is not paused (debug-pause, or set-breakpoint, first)")]);
    let refused = c.browser(&["debug-eval", "x", "--device", "a"], &[]);
    c.daemon.errors(&[]);
    let preset = c.browser(&["start-device", "--device", "pixel-8"], &[]);
    let named = c.browser(&["hierarchy", "--name", "a"], &[]);
    let table_named = c.browser(&["tap-on", "Go", "--name", "a"], &[]);
    let sideways = c.browser(&["debug-step", "sideways", "--device", "a"], &[]);
    let unknown_size = c.browser(&["start-device", "--device", "a", "--screen-size", "nokia-3310"], &[]);
    let run_size = c.browser(&["test", "t.lua", "--no-record", "--screen-size", "nokia-3310"], &[]);
    let bad_calls = c.daemon.calls();
    // Then the refusal exits 1 with the daemon's words and a next step, and each bad line exits 2 without calling the daemon
    assert_eq!((refused.exit_code, line(&refused)["error"].clone(), line(&refused)["device"].clone()), (1, json!("debug-eval: the page is not paused (debug-pause, or set-breakpoint, first)"), json!("a")));
    assert!(line(&refused)["next"].is_string());
    for (r, why) in [(&preset, "the screen preset 'pixel-8' goes in --screen-size"), (&named, "unexpected argument '--name'"), (&table_named, "unknown option --name"), (&sideways, "debug-step takes over, into or out, got 'sideways'"), (&unknown_size, "unknown screen size 'nokia-3310': one of iphone-se"), (&run_size, "unknown screen size 'nokia-3310'")] {
        assert!(r.exit_code == 2 && line(r)["error"].as_str().unwrap().contains(why) && line(r)["usage"].is_string(), "{why}: {}", r.stdout);
    }
    assert!(bad_calls.is_empty(), "{bad_calls:?}");

    // When each verb from before the rename and each dropped verb runs
    for old in ["run", "tap", "type", "press", "navigate", "trace", "screenshot", "audio-inject", "snapshot", "video-check", "goldens", "open", "close", "list", "debug-set"] {
        let r = c.browser(&[old, "--name", "a"], &[]);
        // Then an old one is an unknown verb: exit 2 listing the verbs there are
        let v = line(&r);
        let verbs: Vec<&str> = v["verbs"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!((r.exit_code, v["ok"].clone(), v["error"].clone(), v["use"].clone()), (2, json!(false), json!(format!("unknown verb '{old}'")), Value::Null), "{old}: {}", r.stdout);
        assert!(verbs.contains(&"tap-on") && verbs.contains(&"start-device") && verbs.contains(&"status") && !verbs.contains(&old) && !verbs.contains(&"back"), "{old}: {v}");
    }
    for old in ["eval", "outer-eval", "shell-eval", "wait-observe", "network-stub", "network-stub-hits", "debug-hits", "debug-clear", "network-proxy", "cookies", "memory", "coverage", "check-clipping", "upload", "opened-urls", "preflight", "camera-inject", "clipboard", "drag", "scroll", "permission", "set-device", "stroke"] {
        let r = c.browser(&[old], &[]);
        // and a dropped one exits 2 saying why
        let v = line(&r);
        assert!(r.exit_code == 2 && v["error"] == "dropped" && v["why"].as_str().is_some_and(|w| !w.is_empty()), "{old}: {}", r.stdout);
    }
    assert!(c.daemon.requests().is_empty(), "an unknown or dropped verb reached the daemon");

    // When the daemon's status is asked for plainly and as the browser SDK asks, and the daemon is stopped
    c.daemon.status(json!({ "activeBrowserName": "a", "browserCount": 1, "browsers": ["a"] }));
    let status = c.browser(&["status"], &[]);
    let sdk = c.browser(&["--json", "status"], &[]);
    let stopped = c.browser(&["stop"], &[]);
    let stop_requests: Vec<String> = c.daemon.requests().iter().map(|q| format!("{} {}", q.method, q.url)).collect();
    // Then status names the daemon's URL and session (with --json in one pino line), and stop posts /stop and forgets the daemon
    assert_eq!((status.exit_code, parse(&status.stdout)), (0, json!({ "url": c.daemon.url, "activeBrowserName": "a", "browserCount": 1, "browsers": ["a"] })));
    let pino = parse(sdk.stdout.trim());
    assert_eq!((pino["level"].as_f64(), pino["msg"].as_str(), pino["result"]["url"].clone()), (Some(30.0), Some("ok"), json!(c.daemon.url)));
    assert!(pino["time"].is_number() && pino["pid"].is_number() && pino["hostname"].is_string());
    assert_eq!((stopped.exit_code, stopped.stdout.as_str(), stop_requests), (0, "Browser daemon stopped\n", vec!["POST /stop".to_string()]));
    assert!(c.registered().is_none());

    // When no daemon is registered
    for cmd in ["status", "stop"] {
        let r = c.browser(&[cmd], &[]);
        // Then status and stop say none is running
        assert_eq!((r.exit_code, r.stderr.as_str()), (1, "[wf browser] Error: No browser daemon is running\n"), "{cmd}");
    }
    let idle = c.browser(&["list-devices"], &[]);
    let nothing = c.browser(&["hierarchy"], &[]);
    // and list-devices says so without starting one, while a session verb fails naming how to start a device
    assert_eq!(line(&idle)["result"]["daemon"], json!({ "running": false, "url": null }));
    assert!(nothing.exit_code == 1 && line(&nothing)["error"].as_str().unwrap().contains("no browser daemon is running"), "{}", nothing.stdout);

    // When the registered daemon is gone and a device is started
    c.register(Some("http://127.0.0.1:1"));
    let stub = c.bin_dir.join("timeout");
    std::fs::write(
        &stub,
        format!(
            "#!{}\nconst fs = require('fs');\nfs.appendFileSync(process.env.STUB_LOG, JSON.stringify(process.argv.slice(2)) + '\\n');\nconst p = process.env.WF_REGISTRY_DIR + '/registry.json';\nconst r = JSON.parse(fs.readFileSync(p, 'utf8'));\nr.slots[{}].browserMcp = {{ url: process.env.STUB_URL, startedAt: 'x' }};\nfs.writeFileSync(p, JSON.stringify(r));\n",
            node_path().expect("node on PATH").display(),
            json!(SLOT)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let log = c.root.path().join("timeout.log");
    c.daemon.results(&[("open", json!({ "name": "a" }))]);
    let restarted = c.browser(&["start-device", "--device", "a"], &[("STUB_LOG", &log.display().to_string()), ("STUB_URL", &c.daemon.url)]);
    // Then the record is cleared, a daemon is started under a 12-hour timeout guard, and the verb runs against it
    assert_eq!((restarted.exit_code, line(&restarted)["result"].clone()), (0, json!({ "name": "a" })), "{}", restarted.stderr);
    let argv: Vec<String> = serde_json::from_str(std::fs::read_to_string(&log).unwrap().trim()).unwrap();
    assert!(argv[0] == "43200" && Path::new(&argv[1]).file_name().is_some_and(|n| n.to_string_lossy().starts_with("wf")) && argv[2] == "browser-daemon", "{argv:?}");
    assert!(Path::new("/tmp/wf-browser-daemon.log").exists());
}

#[test]
fn the_tunnel_publishes_the_fleet_on_localhost_through_the_tailnet_under_the_names_it_resolves() {
    // Given tailscale listing the fleet's peers, and a SOCKS5 listener that records the host each forwarded connection asks for
    let dir = tempfile::Builder::new().prefix("wf-flow-tunnel-").tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let peers = r#"{"Self":{"Online":true},"Peer":{"a":{"HostName":"vm-ops-api-main","DNSName":"vm-ops-api-main-2.tail.ts.net."},"b":{"HostName":"remote-chat-agent-prod","DNSName":"remote-chat-agent-prod.tail.ts.net."}}}"#;
    crate::support::create_sh_stub(&bin.join("tailscale"), &format!("if [ \"$1\" = \"status\" ] && [ \"$2\" = \"--json\" ]; then printf '%s' '{peers}'; exit 0; fi\nexit 0"));
    let dialled = Arc::new(Mutex::new(Vec::<String>::new()));
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let socks_port = listener.local_addr().unwrap().port();
    let seen = dialled.clone();
    std::thread::spawn(move || {
        for mut sock in listener.incoming().flatten() {
            let seen = seen.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 512];
                if sock.read(&mut buf).unwrap_or(0) == 0 || sock.write_all(&[5, 0]).is_err() {
                    return;
                }
                let n = sock.read(&mut buf).unwrap_or(0);
                if n >= 5 && n >= 5 + buf[4] as usize {
                    seen.lock().unwrap().push(String::from_utf8_lossy(&buf[5..5 + buf[4] as usize]).into_owned());
                }
            });
        }
    });
    let start = |args: &[&str]| {
        let mut full = vec!["browser", "tunnel"];
        full.extend_from_slice(args);
        spawn_wf(&full, &RunOpts::default().cwd(dir.path()).bin_dir(&bin).env("HOME", &dir.path().display().to_string()).env("WF_SOCKS_PORT", &socks_port.to_string()))
    };

    // When the tunnel starts for vm-ops-api and a client connects to its local port
    let mut t = start(&["vm-ops-api"]);
    wait_until(Duration::from_secs(8), "the tunnel to forward", || t.out().contains("Forwarding. Press Ctrl-C to stop."));
    if let Ok(mut client) = TcpStream::connect(("127.0.0.1", 18080)) {
        let _ = client.write_all(b"hello");
        let mut sink = Vec::new();
        let _ = client.read_to_end(&mut sink);
    }
    wait_until(Duration::from_secs(8), "a dial", || !dialled.lock().unwrap().is_empty());
    // Then it prints the mapping, the SOCKS5 dial asks for the name the tailnet resolves, and SIGINT stops it with 130
    assert_eq!(t.out(), "  localhost:18080  ->  vm-ops-api-main:8080  (vm-ops-api)\n\nForwarding. Press Ctrl-C to stop.\n");
    assert_eq!(*dialled.lock().unwrap(), ["vm-ops-api-main-2"]);
    t.signal("INT");
    assert_eq!(t.wait(Duration::from_secs(8)), Some(130));

    // When the agent and the backend are tunnelled
    let both = start(&["remote-chat-agent", "leetcode-backend"]);
    wait_until(Duration::from_secs(8), "the tunnel to forward", || both.out().contains("Forwarding."));
    // Then the env for a deployed-service suite is printed
    let out = both.out();
    assert!(out.contains("  localhost:18787  ->  remote-chat-agent-prod:8787  (remote-chat-agent)\n") && out.contains("  localhost:18789  ->  leetcode-backend-prod:8789  (leetcode-backend)\n"), "{out}");
    assert!(out.contains("\nRun a suite against the DEPLOYED services with:\n  REMOTE_CHAT_AGENT_BASE_URL=http://localhost:18787\n  LEETCODE_BACKEND_BASE_URL=http://localhost:18789\n"), "{out}");
    drop(both);

    // When the port is already taken
    let holder = TcpListener::bind(("127.0.0.1", 18792)).expect("port 18792 is free");
    let mut taken = start(&["leetcode-capture"]);
    // Then it reports already forwarding and exits 0 without blocking
    assert_eq!(taken.wait(Duration::from_secs(8)), Some(0));
    assert_eq!(taken.out(), "  localhost:18792  ->  leetcode-capture:8792  (leetcode-capture) — already forwarding\n\nAlready forwarding — nothing to start.\n");
    drop(holder);

    // When an unknown service is asked for
    let unknown = run_wf(&["browser", "tunnel", "nope"], &RunOpts::default().bin_dir(&bin).cwd(dir.path()).clean_env());
    // Then it fails naming the known ones
    assert_eq!(unknown.exit_code, 1);
    assert!(unknown.stderr.contains("unknown service(s): nope. Known: vm-ops-api, remote-chat-agent, leetcode-backend, leetcode-capture"), "{}", unknown.stderr);
}

#[test]
fn a_tailnet_forward_reaches_a_tailnet_host_from_a_local_port_and_is_reused() {
    // Given a tailnet whose proxy joins every connection to a service answering /hello, and the Rust daemon
    let mock = MockServer::start(None);
    mock.routes(&[("GET /hello", MockReply::json(200, json!({ "hi": "there" })))]);
    let socks = MockTailnet::start(Upstream::All(mock.port));
    let dir = tempfile::Builder::new().prefix("wf-flow-forward-").tempdir().unwrap();
    let wf = |args: &[&str]| {
        let mut full = vec!["browser"];
        full.extend_from_slice(args);
        run_wf(&full, &RunOpts::default().cwd(dir.path()).timeout(Duration::from_secs(60)).env("WF_REGISTRY_DIR", &dir.path().join("registry").display().to_string()).env("WF_BROWSER_DAEMON", "rust").env("WF_SOCKS_PORT", &socks.port.to_string()))
    };

    // When a forward to vm-ops-api is asked for, then the same target twice
    let first = wf(&["tailnet-forward", "--host", "vm-ops-api-main", "--port", "8080", "--json"]);
    let out = parse(&first.stdout)["result"].clone();
    let url = out["url"].as_str().unwrap().to_string();
    let (_, answer) = http_get(url.trim_start_matches("http://"), "/hello");
    let a = parse(&wf(&["tailnet-forward", "--host", "h", "--port", "1", "--json"]).stdout)["result"].clone();
    let b = parse(&wf(&["tailnet-forward", "--host", "h", "--port", "1", "--json"]).stdout)["result"].clone();
    wf(&["stop"]);
    // Then a local port reaches the target through the proxy, and the second forward to a target reuses the first
    assert_eq!((first.exit_code, out["reused"].clone()), (0, json!(false)));
    assert_eq!(url, format!("http://127.0.0.1:{}", out["localPort"]));
    assert_eq!(parse(&answer), json!({ "hi": "there" }));
    assert!(socks.destinations().iter().any(|h| h.contains("vm-ops-api-main")), "{:?}", socks.destinations());
    assert_eq!((b["localPort"].clone(), b["reused"].clone()), (a["localPort"].clone(), json!(true)));
}
