//! `wf vm ops` and `wf ship vm-ops-api`: vm-ops-api reached through the tailnet with signed requests. vm-ops-api is a fake behind a
//! fake tailnet and `bws` is a stub that logs its argv (the other `vm` commands are in `clis/deploy`, `vm exec` in `clis/wf-vmops`).
use crate::support::{create_sh_stub, run_wf, RunOpts};
use wf_vmops::testing::{answers, FakeVm, VM_SEED};
use serde_json::{json, Value};

/// The commit the fake vm-ops-api was built from.
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// The vault as `bws` serves it: the vm-ops-api credentials, unreachable for the first `fail_first` listings; every call is logged.
const STUB_BWS: &str = r#"echo "$*" >> "$STUB_DIR/bws.log"
n=$(cat "$STUB_DIR/bws.n" 2>/dev/null || echo 0); n=$((n + 1)); echo "$n" > "$STUB_DIR/bws.n"
[ "$n" -le "${STUB_FAIL_FIRST:-0}" ] && { echo "[503 Service Unavailable]" >&2; exit 1; }
cat "$STUB_DIR/vault.json""#;

const FAKE_LINK: &str = "orca://pair?code=FAKE-CREDENTIAL-123";

#[test]
fn wf_vm_ops_pair_prints_only_the_link_on_stdout_and_never_when_the_reply_lacks_it() {
    // Given a fake vm-ops-api behind a fake tailnet
    let vm = FakeVm::start();
    let run = |args: &[&str], reply: &str| {
        vm.replies(&[reply]);
        run_wf(args, &vm.opts())
    };

    // When orca-serve is paired and vm-ops-api answers with a link
    let r = run(&["vm", "ops", "pair", "orca-serve"], &answers::reply(200, &json!({ "ok": true, "url": FAKE_LINK })));

    // Then stdout is exactly the link and a newline, stderr is a human line without it, and one signed empty-body POST was sent
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, format!("{FAKE_LINK}\n").as_str()), "{}", r.stderr);
    assert!(!r.stderr.is_empty() && !r.stderr.contains("FAKE-CREDENTIAL"), "{}", r.stderr);
    let sent = vm.requests();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!((sent[0].method.as_str(), sent[0].url.as_str(), sent[0].body.as_str()), ("POST", "/services/orca-serve/pair", ""));
    vm.assert_signed(&sent[0], "POST", "/services/orca-serve/pair", "");

    // When the reply is a failure, a 200 without a link, a 200 with something that is no pairing link, or a non-JSON page
    let cases = [
        answers::reply(502, &json!({ "ok": false, "error": "orca-serve pair failed or printed no pairing link" })),
        answers::reply(200, &json!({ "ok": true })),
        answers::reply(200, &json!({ "ok": true, "url": "https://example.com/FAKE-CREDENTIAL-999" })),
        answers::reply(200, &json!({ "ok": false, "url": FAKE_LINK })),
        "<html>FAKE-CREDENTIAL-777</html>\n200".to_string(),
    ];
    for reply in &cases {
        let r = run(&["vm", "ops", "pair", "orca-serve"], reply);

        // Then nothing is on stdout, stderr is one JSON error with exit 1, and no reply text is echoed
        assert_eq!((r.exit_code, r.stdout.as_str()), (1, ""), "{reply}: {}", r.stderr);
        let error: Value = serde_json::from_str(r.stderr.trim()).unwrap_or_else(|e| panic!("{reply}: {e}: {}", r.stderr));
        assert_eq!(error["ok"], json!(false), "{reply}");
        assert!(!r.stderr.contains("FAKE-CREDENTIAL"), "{reply}: {}", r.stderr);
    }

    // When any other app is paired, Then it is refused before a request is made
    let r = run(&["vm", "ops", "pair", "leetcode-backend"], &answers::reply(200, &json!({ "ok": true, "url": FAKE_LINK })));
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, ""));
    assert!(r.stderr.contains(r#""ok":false"#), "{}", r.stderr);
    assert!(vm.requests().is_empty());
}

#[test]
fn vm_ops_api_is_reached_through_the_tailnet_with_requests_signed_by_the_vault_key() {
    let vm = FakeVm::start();
    let run = |args: &[&str], reply: &str, env: &[(&str, &str)]| {
        vm.replies(&[reply]);
        let mut opts = vm.opts();
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(args, &opts)
    };
    let only = || {
        let all = vm.requests();
        assert_eq!(all.len(), 1, "{all:?}");
        all[0].clone()
    };

    let pretty = |v: &Value| format!("{}\n", serde_json::to_string_pretty(v).unwrap());
    let health = answers::health(COMMIT);

    // When health is asked for, Then the reply is pretty-printed and one signed GET went to the tailnet host through the SOCKS proxy
    let r = run(&["vm", "ops", "health"], &answers::reply(200, &health), &[]);
    assert_eq!((r.exit_code, r.stdout.clone()), (0, pretty(&health)), "{}", r.stderr);
    let request = only();
    assert_eq!(vm.tailnet.destinations(), ["vm-ops-api-main:8080"]);
    assert_eq!((request.method.as_str(), request.url.as_str(), request.body.as_str(), request.headers["host"].as_str()), ("GET", "/health", "", "vm-ops-api-main:8080"));
    vm.assert_signed(&request, "GET", "/health", "");

    // When stats are asked for, Then the stats route answers the same way
    let stats = answers::stats(11927, 781, [0.52, 0.58, 0.59], &[("agent-shell", 2292, 0.1)]);
    let r = run(&["vm", "ops", "stats"], &answers::reply(200, &stats), &[]);
    assert_eq!(r.stdout, pretty(&stats));
    vm.assert_signed(&only(), "GET", "/stats", "");

    // When a service's logs are asked for, Then the tail is in the query and each container's text prints under its name
    let logs = answers::logs("remote-chat-agent", 50, &[("remote-chat-agent", "line1\nline2\n"), ("remote-chat-agent-tailscale", "up\n")]);
    let r = run(&["vm", "ops", "logs", "remote-chat-agent", "--tail", "50"], &answers::reply(200, &logs), &[]);
    assert_eq!(r.stdout, "==> remote-chat-agent <==\nline1\nline2\n\n==> remote-chat-agent-tailscale <==\nup\n");
    vm.assert_signed(&only(), "GET", "/services/remote-chat-agent/logs?tail=50", "");
    let logs = answers::logs("leetcode-backend", 200, &[("leetcode-backend", "listening\n")]);
    let r = run(&["vm", "ops", "logs", "leetcode-backend"], &answers::reply(200, &logs), &[]);
    assert_eq!((r.stdout.as_str(), only().url.as_str()), ("==> leetcode-backend <==\nlistening\n", "/services/leetcode-backend/logs?tail=200"));
    // and an app with no containers fails with the service's reason
    let r = run(&["vm", "ops", "logs", "agent-shell"], &answers::reply(404, &json!({ "error": "no containers found for this app (not running?)" })), &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("returned HTTP 404: {\"error\":\"no containers found for this app (not running?)\"}"), "{}", r.stderr);
    // and a 200 without the logs object (never Node's; a port that broke the shape) fails carrying the body
    let r = run(&["vm", "ops", "logs", "leetcode-backend"], "{\"logs\":\"listening\\n\"}\n200", &[]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, ""));
    assert!(r.stderr.contains("wf: vm-ops-api /services/leetcode-backend/logs?tail=200 answered without a logs object: {\"logs\":\"listening\\n\"}"), "{}", r.stderr);

    // When a ledger file is read, Then it prints verbatim with exactly one trailing newline
    let r = run(&["vm", "ledger", "cat", "pool-cooldowns.json"], "{\"a\":1}\n200", &[]);
    assert_eq!((r.stdout.as_str(), only().url.as_str()), ("{\"a\":1}\n", "/services/agent-shell/vm-ledger/pool-cooldowns.json"));
    assert_eq!(run(&["vm", "ledger", "cat", "opencode-dispatch-runs.jsonl"], "{\"a\":1}\n{\"b\":2}\n\n200", &[]).stdout, "{\"a\":1}\n{\"b\":2}\n");

    // Given a body that is not JSON (a page from something in front of vm-ops-api), or an HTTP error, Then wf exits 1 showing them
    let r = run(&["vm", "ops", "health"], "<html>gateway</html>\n200", &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("wf: vm-ops-api /health returned non-JSON body: <html>gateway</html>"), "{}", r.stderr);
    let r = run(&["vm", "ops", "health"], &answers::reply(401, &answers::unauthorized()), &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("wf: vm-ops-api /health returned HTTP 401: {\"error\":\"unauthorized\"}"), "{}", r.stderr);

    // Given no tailnet proxy (the environment's proxy settings apply), or one that cannot be reached, Then the request fails
    // without reaching anything: the proxies named are closed ports, so a machine on the tailnet does not reach the real VM
    let before = vm.tailnet.destinations().len();
    let closed = "http://127.0.0.1:1";
    for socks in ["", "127.0.0.1:1"] {
        let r = run(&["vm", "ops", "health"], &answers::reply(200, &health), &[("WF_TAILNET_SOCKS", socks), ("ALL_PROXY", closed), ("HTTP_PROXY", closed), ("http_proxy", closed), ("NO_PROXY", ""), ("no_proxy", "")]);
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("wf: vm-ops-api request to /health failed"), "{socks:?}: {}", r.stderr);
    }
    assert_eq!(vm.tailnet.destinations().len(), before);

    // Given a credential that does not resolve, Then it is named and nothing is sent
    for missing in ["VM_OPS_API_AUTH_TOKEN", "VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY"] {
        let r = run(&["vm", "ops", "health"], &answers::reply(200, &health), &[(missing, "")]);
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains(&format!("wf: {missing} could not be resolved.")), "{}", r.stderr);
        assert!(vm.requests().is_empty());
    }

    // Given the credentials come from the vault and its first answer is a 503, When one command makes several requests
    let dir = tempfile::Builder::new().prefix("deploy-vault-").tempdir().unwrap();
    std::fs::create_dir(dir.path().join("bin")).unwrap();
    create_sh_stub(&dir.path().join("bin/bws"), STUB_BWS);
    let vault = json!([{ "id": "1", "key": "VM_OPS_API_AUTH_TOKEN", "value": VM_SEED }, { "id": "2", "key": "VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", "value": vm.private_one_line }]);
    std::fs::write(dir.path().join("vault.json"), vault.to_string()).unwrap();
    vm.replies(&[answers::reply(200, &answers::ps("reporting-service", &[])), answers::reply(200, &health), answers::reply(200, &answers::redeploy_status("", json!([])))]);
    let opts = RunOpts::default()
        .clean_env()
        .bin_dir(&dir.path().join("bin"))
        .env("SECRETS_BACKEND", "bitwarden")
        .env("SECRETS_LOG_BETTERSTACK", "false")
        .env("BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN", "read")
        .env("WF_TAILNET_SOCKS", &vm.tailnet.address())
        .env("STUB_DIR", &dir.path().display().to_string())
        .env("STUB_FAIL_FIRST", "1");
    let r = run_wf(&["ship", "--status", "leetcode-backend", "reporting-service"], &opts);
    // Then the call made during the outage fails alone, the next one loads the key, and every later one reuses it
    let out: Value = serde_json::from_str(&r.stdout).unwrap_or_else(|_| panic!("{}{}", r.stdout, r.stderr));
    assert!(out["services"]["leetcode-backend"]["error"].as_str().is_some_and(|e| e.contains("could not be resolved")), "{out}");
    assert_eq!(out["services"]["reporting-service"], json!([]), "{out}");
    let listings = std::fs::read_to_string(dir.path().join("bws.log")).unwrap().lines().filter(|l| l.starts_with("secret list")).count();
    assert_eq!(listings, 2, "one failed listing, then one that is kept");
    for request in vm.requests() {
        vm.assert_signed(&request, &request.method, &request.url, "");
    }
}

