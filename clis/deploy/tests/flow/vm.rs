//! `wf vm`: BetterStack log queries, the recovery of an unreachable VM, the Oracle credentials and Terraform runs, and the probe of
//! apps on the tailnet (the signed vm-ops-api requests are in `clis/wf/tests/flow/vm.rs`, `vm exec` in `clis/wf-vmops`). vm-ops-api is
//! a fake behind a fake tailnet; ClickHouse is an HTTPS fake; `bws`, `oci` and `terraform` are stubs that log their argv.
use crate::support::{iso_from_ms, node_path, now_ms, run_wf, Aux, HttpsRequest, MockHttps, MockReply, MockServer, MockTailnet, RunOpts, RunResult, Upstream};
use wf_vmops::testing::{answers, FakeVm};
use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The commit the fake vm-ops-api was built from.
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

const APP_SQL: &str = "coalesce(nullIf(JSONExtractString(raw,'app'), ''), nullIf(JSONExtractString(raw,'resource','attributes','service.name'), ''), '')";
const MODULE_SQL: &str = "coalesce(nullIf(JSONExtractString(raw,'module'), ''), nullIf(JSONExtractString(raw,'attributes','source'), ''), nullIf(JSONExtractString(raw,'source'), ''), nullIf(JSONExtractString(raw,'scope','name'), ''), nullIf(JSONExtractString(raw,'resource','attributes','service.name'), ''), '')";

/// BetterStack keeps the newest rows in `<source>_logs` and archives older ones to `<source>_s3`; a read is both.
fn base_sql(table: &str, filter: &str, limit: &str) -> String {
    let cold = table.strip_suffix("_logs").map(|base| format!("{base}_s3")).expect("a _logs table");
    format!("SELECT formatDateTime(dt, '%Y-%m-%dT%H:%i:%S.%fZ') AS ts, raw FROM (SELECT dt, raw FROM remote({table}) WHERE {filter} UNION ALL SELECT dt, raw FROM s3Cluster(primary, {cold}) WHERE _row_type = 1 AND {filter}) ORDER BY dt DESC LIMIT {limit} FORMAT TabSeparatedWithNames")
}

#[test]
fn vm_logs_queries_betterstack_over_https_and_prints_one_line_per_row() {
    let ch = MockHttps::start();
    ch.status(200);
    let dir = tempfile::Builder::new().prefix("deploy-logs-").tempdir().unwrap();
    let host = format!("127.0.0.1:{}", ch.port);
    let logs = |args: &[&str], reply: &str, env: &[(&str, &str)]| {
        ch.clear();
        ch.reply(reply);
        let mut full = vec!["vm", "logs"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().cwd(dir.path()).clean_env();
        let ca = ch.ca.display().to_string();
        for (key, value) in [
            ("SECRETS_BACKEND", "macos"),
            ("SECRETS_LOG_BETTERSTACK", "false"),
            ("SSL_CERT_FILE", ca.as_str()),
            ("CH_BACKEND_HOST", host.as_str()),
            ("CH_BACKEND_USER", "reader"),
            ("CH_BACKEND_PASSWORD", "pw"),
            ("CH_BACKEND_TABLE", "t.backend_logs"),
            ("CH_MOBILE_HOST", host.as_str()),
            ("CH_MOBILE_USER", "mreader"),
            ("CH_MOBILE_PASSWORD", "mpw"),
            ("CH_MOBILE_TABLE", "t.mobile_logs"),
        ]
        .into_iter()
        .chain(env.iter().copied())
        {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    };
    let sent = || -> Vec<HttpsRequest> { ch.requests() };
    let basic = |user: &str, password: &str| format!("Basic {}", b64(&format!("{user}:{password}")));

    // Given no options, Then the backend source is asked for 10 minutes and 200 rows, with basic auth, over HTTPS
    let r = logs(&[], "ts\traw\n", &[]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, ""), "{}", r.stderr);
    let request = &sent()[0];
    assert_eq!((sent().len(), request.method.as_str(), request.url.as_str()), (1, "POST", "/"));
    assert_eq!((request.headers["authorization"].clone(), request.headers["content-type"].as_str()), (basic("reader", "pw"), "plain/text"));
    assert_eq!(request.body, base_sql("t.backend_logs", "dt > now() - INTERVAL 10 MINUTE", "200"));

    // Given every filter, Then each joins the where clause in order with quotes and backslashes escaped
    logs(&["--service", "my-app", "--since", "2h", "--level", "warn", "--module", "it's", "--grep", "a\\b'c", "--limit", "5"], "ts\traw\n", &[]);
    let filter = format!(
        "dt > now() - INTERVAL 2 HOUR AND {APP_SQL} = 'my-app' AND lower(JSONExtractString(raw,'level')) IN ('warn', 'error') AND {MODULE_SQL} = 'it\\'s' AND lowerUTF8(CAST(JSONExtractString(raw,'message') AS String)) LIKE lowerUTF8('%a\\\\b\\'c%')"
    );
    assert_eq!(sent()[0].body, base_sql("t.backend_logs", &filter, "5"));
    for (since, interval) in [("30s", "INTERVAL 30 SECOND"), ("5m", "INTERVAL 5 MINUTE"), ("1d", "INTERVAL 1 DAY")] {
        logs(&["--since", since], "ts\traw\n", &[]);
        assert_eq!(sent()[0].body, base_sql("t.backend_logs", &format!("dt > now() - {interval}"), "200"), "{since}");
    }

    // Given the mobile source, Then its credentials and table are used
    logs(&["--source", "mobile"], "ts\traw\n", &[]);
    assert_eq!(sent()[0].headers["authorization"], basic("mreader", "mpw"));
    assert!(sent()[0].body.contains("remote(t.mobile_logs)"));

    // Given the mobile source whose recent table is empty because BetterStack archived its rows, When app.boot is read back over
    // 7 days, Then the archived table is read too, with the grep and the window in both halves, and its row prints as one line
    let filter = "dt > now() - INTERVAL 7 DAY AND lowerUTF8(CAST(JSONExtractString(raw,'message') AS String)) LIKE lowerUTF8('%app.boot%')";
    let boot = "{\"level\":\"info\",\"message\":\"app.boot\",\"app\":\"mla-tamagui\",\"platform\":\"android\",\"source\":\"AppShell\",\"updateId\":\"01a16509\"}";
    let r = logs(&["--source", "mobile", "--since", "7d", "--grep", "app.boot"], &format!("ts\traw\n2026-10-22T19:54:47.000000Z\t{boot}\n"), &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let body = sent()[0].body.clone();
    assert_eq!(body, base_sql("t.mobile_logs", filter, "200"));
    assert!(body.contains("s3Cluster(primary, t.mobile_s3) WHERE _row_type = 1 AND dt > now() - INTERVAL 7 DAY AND"), "{body}");
    assert_eq!(body.matches("LIKE lowerUTF8('%app.boot%')").count(), 2, "{body}");
    assert_eq!(r.stdout, format!("2026-10-22 19:54:47.000000  {:<5} {:<24} app.boot  source=AppShell updateId=01a16509\n", "INFO", "AppShell"));

    // Given a table name without the _logs suffix, Then only that table is read: no archive name can be derived from it
    logs(&[], "ts\traw\n", &[("CH_BACKEND_TABLE", "t.custom")]);
    assert_eq!(sent()[0].body, "SELECT formatDateTime(dt, '%Y-%m-%dT%H:%i:%S.%fZ') AS ts, raw FROM (SELECT dt, raw FROM remote(t.custom) WHERE dt > now() - INTERVAL 10 MINUTE) ORDER BY dt DESC LIMIT 200 FORMAT TabSeparatedWithNames");

    // Given a JSON row, a row with OTLP attributes and a row that is not JSON, Then each is one formatted line
    let reply = ["ts\traw", "2026-10-06T10:00:00.123456Z\t{\"level\":\"info\",\"module\":\"api\",\"message\":\"started\",\"requestId\":\"r1\",\"n\":2}", "2026-10-06T09:59:00.000000Z\t{\"level\":\"error\",\"message\":\"boom\",\"attributes\":{\"source\":\"svc\",\"code\":7,\"detail\":{\"a\":1}},\"resource\":{\"x\":1}}", "2026-10-06T09:58:00.000000Z\tnot json", ""].join("\n");
    let r = logs(&[], &reply, &[]);
    assert_eq!(
        r.stdout.split('\n').collect::<Vec<_>>(),
        [
            format!("2026-10-06 10:00:00.123456  {:<5} {:<24} started  requestId=r1 n=2", "INFO", "api"),
            format!("2026-10-06 09:59:00.000000  {:<5} {:<24} boom  source=svc code=7 detail={{\"a\":1}}", "ERROR", "svc"),
            format!("2026-10-06 09:58:00.000000  {:<5} {:<24} not json", "?", "?"),
            String::new()
        ]
    );

    // Given a ClickHouse error answered with 200, Then the exception is the failure
    let r = logs(&[], "{\"exception\":\"Code: 60. DB::Exception:   Table   missing\"}", &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: BetterStack query failed: Code: 60. DB::Exception: Table missing"), "{}", r.stderr);
    let r = logs(&[], "Code: 47. DB::Exception: Unknown identifier x\nmore", &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: BetterStack query failed: Code: 47. DB::Exception: Unknown identifier x"), "{}", r.stderr);

    // Given a bad option or setting, an unreachable host or an untrusted certificate, Then it fails and no credentials are sent
    type Refusal<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)], &'a str);
    let refused: [Refusal; 7] = [
        (&["--source", "nope"], &[], "wf: Unknown --source \"nope\". Valid sources: backend, mobile."),
        (&[], &[("CH_BACKEND_PASSWORD", "")], "wf: BetterStack config for source \"backend\" missing: CH_BACKEND_PASSWORD."),
        (&[], &[("CH_BACKEND_HOST", "<ch-host>")], "wf: BetterStack config for source \"backend\" still holds PLACEHOLDER values: CH_BACKEND_HOST."),
        (&["--since", "5x"], &[], "Invalid --since \"5x\" (expected a duration like \"5m\", \"1h\", \"30s\", \"2d\")"),
        (&["--level", "loud"], &[], "Unknown --level \"loud\" (expected debug|info|warn|error)"),
        (&[], &[("CH_BACKEND_HOST", "127.0.0.1:1")], "wf: BetterStack query failed (exit 1):"),
        (&[], &[("SSL_CERT_FILE", "/nonexistent")], "wf: BetterStack query failed (exit 1):"),
    ];
    for (args, env, message) in refused {
        let r = logs(args, "", env);
        assert_eq!(r.exit_code, 1, "{args:?} {env:?}");
        assert!(r.stderr.contains(message), "{args:?} {env:?}: {}", r.stderr);
        assert!(sent().is_empty(), "{args:?} {env:?}");
    }
    let r = logs(&[], "", &[("CH_BACKEND_PASSWORD", "")]);
    assert!(r.stderr.contains("See `wf secret list` for where each value comes from."), "{}", r.stderr);
}

const APPS: [&str; 9] = ["leetcode-backend", "remote-chat-agent", "leetcode-auth-cookie-capture", "agent-shell", "ci-runner", "vnc-auth-session", "reporting-service", "t3-serve", "orca-serve"];

/// vm-ops-api for `wf vm recover`: routes by method and path, a queue per route, entries that answer only once `oci` dropped a
/// flag file for an action, and entries that drop the connection as a host that is down does.
fn recover_api(dir: &str) -> String {
    format!(
        "const http = require('http');
const fs = require('fs');
const dir = {dir:?};
http.createServer((req, res) => {{
  req.resume();
  req.on('end', () => {{
    const key = req.method + ' ' + req.url;
    fs.appendFileSync(dir + '/api.log', key + '\\n');
    let counts = {{}};
    try {{ counts = JSON.parse(fs.readFileSync(dir + '/counts.json', 'utf8')); }} catch {{}}
    const n = counts[key] || 0;
    counts[key] = n + 1;
    fs.writeFileSync(dir + '/counts.json', JSON.stringify(counts));
    const routes = JSON.parse(fs.readFileSync(dir + '/routes.json', 'utf8'));
    const list = routes[key] || routes['*'] || [{{ body: JSON.stringify({{ error: 'not found' }}), status: 404 }}];
    const flagged = list.find((e) => e.flag && fs.existsSync(dir + '/flag.' + e.flag));
    const plain = list.filter((e) => !e.flag);
    const entry = flagged || plain[Math.min(n, plain.length - 1)];
    if (entry.down) {{ req.socket.destroy(); return; }}
    res.writeHead(entry.status || 200, {{ 'Content-Type': 'application/json' }});
    res.end(entry.body);
  }});
}}).listen(0, '127.0.0.1', function () {{ console.log(this.address().port); }});
"
    )
}

/// The Oracle CLI: logs its argv, answers from `oci.json` by its first three words, and an `action` drops `flag.<ACTION>`.
const OCI_RECOVER: &str = "const fs = require('fs');
const dir = process.env.STUB_DIR;
const args = process.argv.slice(2);
fs.appendFileSync(dir + '/oci.log', args.join(' ') + '\\n');
const routes = JSON.parse(fs.readFileSync(dir + '/oci.json', 'utf8'));
const entry = routes[args.slice(0, 3).join(' ')] || { stdout: '{}' };
if (args[2] === 'action') fs.writeFileSync(dir + '/flag.' + args[args.length - 1], '');
if (entry.exit) { process.stderr.write(entry.stderr || ''); process.exit(entry.exit); }
process.stdout.write(entry.stdout);
";

struct Recovery {
    dir: tempfile::TempDir,
    _api: Aux,
    tailnet: MockTailnet,
    key: String,
}

impl Recovery {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("wf-recover-").tempdir().unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        let oci = dir.path().join("bin/oci");
        std::fs::write(&oci, format!("#!{}\n{}", node_path().expect("node on PATH").display(), OCI_RECOVER)).unwrap();
        std::fs::set_permissions(&oci, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.path().join("routes.json"), "{}").unwrap();
        let api = Aux::spawn(&recover_api(&dir.path().display().to_string()));
        let tailnet = MockTailnet::start(Upstream::All(api.port));
        Self { dir, _api: api, tailnet, key: crate::support::ed25519_private_key_one_line() }
    }

    /// A fresh world: every app healthy and `/health` answering, plus `overrides`; the oci instance RUNNING unless `oci` says otherwise.
    fn world(&self, overrides: Vec<(&str, Value)>, oci: Option<Value>) {
        for name in ["api.log", "oci.log", "counts.json", "flag.SOFTRESET", "flag.RESET"] {
            let _ = std::fs::remove_file(self.dir.path().join(name));
        }
        let mut routes = Map::new();
        routes.insert("GET /health".into(), json!([answer(200, &answers::health(COMMIT))]));
        for app in APPS {
            routes.insert(format!("GET /services/{app}/ps"), json!([healthy(app)]));
        }
        for (key, entries) in overrides {
            routes.insert(key.to_string(), entries);
        }
        std::fs::write(self.dir.path().join("routes.json"), Value::Object(routes).to_string()).unwrap();
        let oci = oci.unwrap_or_else(|| json!({ "compute instance action": { "stdout": "{}" }, "compute instance get": { "stdout": "{\"data\":{\"id\":\"ocid1.x\",\"shape\":\"VM.Standard.A1.Flex\",\"lifecycle-state\":\"RUNNING\",\"shape-config\":null}}" } }));
        std::fs::write(self.dir.path().join("oci.json"), oci.to_string()).unwrap();
    }

    fn recover(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut full = vec!["vm", "recover"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default()
            .bin_dir(&self.dir.path().join("bin"))
            .cwd(self.dir.path())
            .clean_env()
            .env("SECRETS_BACKEND", "macos")
            .env("SECRETS_LOG_BETTERSTACK", "false")
            .env("STUB_DIR", &self.dir.path().display().to_string())
            .env("WF_TAILNET_SOCKS", &self.tailnet.address())
            .env("VM_OPS_API_AUTH_TOKEN", "seed")
            .env("VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", &self.key)
            .env("WF_RECOVER_POLL_MS", "10")
            .env("WF_RECOVER_GRACE_MS", "60")
            .env("WF_OCI_POLL_MS", "5");
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    fn lines(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join(name)).map(|t| t.lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
    }
}

/// A route entry of `recover_api` answering `body` with `status`.
fn answer(status: u16, body: &Value) -> Value {
    json!({ "body": body.to_string(), "status": status })
}

fn app_status(app: &str, status: &str) -> Value {
    answer(200, &answers::ps(app, &[(answers::container_name(app), "running", status, &format!("ns/{app}:latest"))]))
}

fn healthy(app: &str) -> Value {
    app_status(app, "Up 1 hour")
}

fn unhealthy(app: &str) -> Value {
    app_status(app, "Up 1 hour (unhealthy)")
}

/// The answer of an app redeploy whose sibling exited 0.
fn redeployed(app: &str, git_ref: &str) -> Value {
    answer(200, &answers::redeploy(app, "latest", git_ref, 0, "[redeploy] up\n", ""))
}

fn recovery_plan(id: &str, max_wait: &str, git_ref: &str) -> String {
    [
        format!("plan 1. probe vm-ops-api /health; if unreachable poll every 30s for up to {max_wait}s"),
        format!("plan 2. still unreachable: OCI soft reboot {id}, wait for RUNNING, then up to 180s for /health"),
        format!("plan 3. still unreachable: OCI hard reset {id}, wait for RUNNING, then up to 180s for /health"),
        format!("plan 4. sweep {} apps ({}); redeploy each unhealthy one with recreate=all from {git_ref}", APPS.len(), APPS.join(", ")),
        format!("plan 5. re-verify every app until healthy or {max_wait}s elapse"),
        String::new(),
    ]
    .join("\n")
}

#[test]
fn vm_recover_brings_an_unreachable_vm_and_its_unhealthy_apps_back() {
    let s = Recovery::new();
    let untimed = [("WF_RECOVER_POLL_MS", ""), ("WF_RECOVER_GRACE_MS", "")];

    // Given --dry-run, Then the five-step plan prints with the instance, ref, window and apps it would use, and nothing is called
    s.world(vec![], None);
    let r = s.recover(&["--dry-run"], &untimed);
    assert_eq!((r.exit_code, r.stdout.clone()), (0, recovery_plan("<--instance-id required>", "600", "dev")));
    assert_eq!(s.recover(&["--dry-run", "--instance-id", "ocid1.abc", "--ref", "main", "--max-wait", "120"], &untimed).stdout, recovery_plan("ocid1.abc", "120", "main"));
    assert_eq!(s.recover(&["--dry-run"], &[("OCI_INSTANCE_ID", "ocid1.env"), untimed[0], untimed[1]]).stdout, recovery_plan("ocid1.env", "600", "dev"));
    let r = s.recover(&["--dry-run", "--only", "agent-shell", "reporting-service", "--ref", "dev"], &untimed);
    assert!(r.stdout.contains("sweep 2 apps (agent-shell, reporting-service); redeploy each unhealthy one with recreate=all from dev"), "{}", r.stdout);
    assert!(s.lines("api.log").is_empty() && s.lines("oci.log").is_empty());
    for value in ["abc", "0", "-5"] {
        let r = s.recover(&["--dry-run", "--max-wait", value], &[]);
        assert_eq!(r.exit_code, 1, "{value}");
        assert!(r.stderr.contains(&format!("wf: invalid --max-wait {value}")), "{value}: {}", r.stderr);
    }

    // Given every app healthy, Then it sweeps once, redeploys nothing and exits 0, and the retired Metro container is no app of the sweep
    let r = s.recover(&["--max-wait", "1"], &[]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "all apps healthy\n"), "{}", r.stderr);
    let mut expected = vec!["GET /health".to_string()];
    expected.extend(APPS.iter().map(|a| format!("GET /services/{a}/ps")));
    assert_eq!(s.lines("api.log"), expected);
    assert!(!s.lines("api.log").iter().any(|l| l.contains("metro-runner")), "{:?}", s.lines("api.log"));

    // Given an unhealthy container, an app with no containers and one whose status call fails, Then each is redeployed with recreate=all
    s.world(
        vec![
            ("GET /services/vnc-auth-session/ps", json!([unhealthy("vnc-auth-session"), healthy("vnc-auth-session")])),
            ("GET /services/agent-shell/ps", json!([answer(200, &answers::ps("agent-shell", &[])), healthy("agent-shell")])),
            ("GET /services/reporting-service/ps", json!([answer(502, &answers::engine_down()), healthy("reporting-service")])),
            ("POST /services/agent-shell/redeploy?tag=latest&ref=main&recreate=all", json!([redeployed("agent-shell", "main")])),
            ("POST /services/vnc-auth-session/redeploy?tag=latest&ref=main&recreate=all", json!([redeployed("vnc-auth-session", "main")])),
            ("POST /services/reporting-service/redeploy?tag=latest&ref=main&recreate=all", json!([redeployed("reporting-service", "main")])),
        ],
        None,
    );
    let r = s.recover(&["--max-wait", "1", "--ref", "main"], &[]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "redeploy agent-shell recreate=all ref=main\nredeploy vnc-auth-session recreate=all ref=main\nredeploy reporting-service recreate=all ref=main\nall apps healthy\n"), "{}", r.stderr);

    // Given an app that never recovers, Then the window ends naming it; Given a redeploy that fails, Then recovery stops with its error
    s.world(vec![("GET /services/vnc-auth-session/ps", json!([unhealthy("vnc-auth-session")])), ("POST /services/vnc-auth-session/redeploy?tag=latest&ref=dev&recreate=all", json!([redeployed("vnc-auth-session", "dev")]))], None);
    let r = s.recover(&["--max-wait", "0.1"], &[]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, "redeploy vnc-auth-session recreate=all ref=dev\nstill unhealthy: vnc-auth-session\n"));
    let failed = answers::redeploy("agent-shell", "latest", "dev", 1, "", "Error response from daemon: manifest for ns/agent-shell:latest not found\n");
    s.world(vec![("GET /services/agent-shell/ps", json!([unhealthy("agent-shell")])), ("POST /services/agent-shell/redeploy?tag=latest&ref=dev&recreate=all", json!([answer(502, &failed)]))], None);
    let r = s.recover(&["--max-wait", "1"], &[]);
    assert_eq!(r.exit_code, 1);
    let reason = "wf: vm-ops-api /services/agent-shell/redeploy?tag=latest&ref=dev&recreate=all returned HTTP 502: exitCode=1\n--- script stderr ---\nError response from daemon: manifest for ns/agent-shell:latest not found";
    assert!(r.stderr.contains(reason), "{}", r.stderr);

    // Given vm-ops-api stays down and no instance id, Then it says it cannot reboot and touches nothing
    let down = || ("GET /health", json!([{ "down": true }]));
    s.world(vec![down()], None);
    let r = s.recover(&["--max-wait", "0.05"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("wf: vm-ops-api unreachable and no --instance-id given -- cannot reboot the VM"), "{}", r.stderr);
    assert!(s.lines("oci.log").is_empty());

    // Given a soft reboot brings it back, Then only the soft reset is issued and every app is swept twice
    s.world(vec![("GET /health", json!([{ "down": true }, { "flag": "SOFTRESET", "body": answers::health(COMMIT).to_string() }]))], None);
    let r = s.recover(&["--max-wait", "0.05", "--instance-id", "ocid1.x"], &[]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "vm-ops-api unreachable -- OCI soft reboot of ocid1.x\nreboot soft: finalState=RUNNING\nall apps healthy\n"), "{}", r.stderr);
    assert_eq!(s.lines("oci.log"), ["compute instance action --instance-id ocid1.x --action SOFTRESET", "compute instance get --instance-id ocid1.x"]);
    assert_eq!(s.lines("api.log").iter().filter(|l| l.contains("/ps")).count(), 2 * APPS.len());

    // Given only a hard reset helps, Then it escalates from soft to hard
    s.world(vec![("GET /health", json!([{ "down": true }, { "flag": "RESET", "body": answers::health(COMMIT).to_string() }]))], None);
    let r = s.recover(&["--max-wait", "0.05", "--instance-id", "ocid1.x"], &[]);
    assert_eq!(
        (r.exit_code, r.stdout.as_str()),
        (0, "vm-ops-api unreachable -- OCI soft reboot of ocid1.x\nreboot soft: finalState=RUNNING\nvm-ops-api unreachable -- OCI hard reboot of ocid1.x\nreboot hard: finalState=RUNNING\nall apps healthy\n"),
        "{}",
        r.stderr
    );
    let actions: Vec<String> = s.lines("oci.log").into_iter().filter(|l| l.contains(" action ")).collect();
    assert_eq!(actions, ["compute instance action --instance-id ocid1.x --action SOFTRESET", "compute instance action --instance-id ocid1.x --action RESET"]);

    // Given nothing brings it back, Then it gives up after the hard reset
    s.world(vec![down()], None);
    let r = s.recover(&["--max-wait", "0.05", "--instance-id", "ocid1.x"], &[]);
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stdout.trim_end().lines().last(), Some("vm-ops-api still unreachable after hard reset"));

    // Given the instance never reports RUNNING, Then each final state shows and the next step is tried
    s.world(vec![down()], Some(json!({ "compute instance action": { "stdout": "{}" }, "compute instance get": { "stdout": "{\"data\":{\"id\":\"ocid1.x\",\"shape\":\"s\",\"lifecycle-state\":\"STOPPING\",\"shape-config\":null}}" } })));
    let r = s.recover(&["--max-wait", "0.05", "--instance-id", "ocid1.x"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stdout.contains("reboot soft: finalState=STOPPING") && r.stdout.contains("reboot hard: finalState=STOPPING"), "{}", r.stdout);

    // Given the Oracle CLI fails, Then recovery fails with its error
    s.world(vec![down()], Some(json!({ "compute instance action": { "exit": 1, "stderr": "NotAuthorizedOrNotFound" } })));
    let r = s.recover(&["--max-wait", "0.05", "--instance-id", "ocid1.x"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("oci compute instance failed: NotAuthorizedOrNotFound"), "{}", r.stderr);
}

const INFRA_BWS: &str = "#!NODE\nprocess.stdout.write(require('fs').readFileSync(process.env.BWS_FIXTURE, 'utf8'));\n";

/// Logs argv, cwd and the injected AWS_* env, answering from a per-subcommand queue in `terraform.json`.
const TERRAFORM: &str = r#"#!NODE
const fs = require('fs');
const dir = process.env.STUB_DIR;
const args = process.argv.slice(2);
const key = args[0] === 'state' ? 'state ' + args[1] : args[0];
const file = args[args.length - 1];
const line = { args: args.map((a) => (a.startsWith('-out=') ? '-out=PLAN' : a === file && args[0] === 'show' ? 'PLAN' : a)), cwd: process.cwd(),
  env: { id: process.env.AWS_ACCESS_KEY_ID, secret: process.env.AWS_SECRET_ACCESS_KEY, region: process.env.AWS_REGION,
    calc: process.env.AWS_REQUEST_CHECKSUM_CALCULATION, valid: process.env.AWS_RESPONSE_CHECKSUM_VALIDATION } };
if (key === 'state push') line.pushed = fs.readFileSync(file, 'utf8');
fs.appendFileSync(dir + '/terraform.log', JSON.stringify(line) + '\n');
let counts = {};
try { counts = JSON.parse(fs.readFileSync(dir + '/counts.json', 'utf8')); } catch {}
const n = counts[key] || 0;
counts[key] = n + 1;
fs.writeFileSync(dir + '/counts.json', JSON.stringify(counts));
const table = JSON.parse(fs.readFileSync(dir + '/terraform.json', 'utf8'));
const raw = table[key] || {};
const entry = Array.isArray(raw) ? raw[Math.min(n, raw.length - 1)] : raw;
if (entry.exit) { process.stderr.write(entry.stderr || ''); process.exit(entry.exit); }
process.stdout.write(entry.stdout || '');
"#;

/// Answers `gh api repos/{owner}/{repo}/actions/runs?status=<s>` from `gh.json` (status -> runs), or fails with its `exit` and `stderr`.
const INFRA_GH: &str = r#"#!NODE
const fs = require('fs');
const m = /status=([a-z_]+)/.exec(process.argv[3] || '');
let t = {};
try { t = JSON.parse(fs.readFileSync(process.env.STUB_DIR + '/gh.json', 'utf8')); } catch {}
if (t.exit) { process.stderr.write(t.stderr || ''); process.exit(t.exit); }
process.stdout.write(JSON.stringify({ workflow_runs: (m && t[m[1]]) || [] }));
"#;

const INFRA_OCI: &str = "#!NODE\nprocess.stdout.write(require('fs').readFileSync(process.env.STUB_DIR + '/oci.json', 'utf8'));\n";

/// A temp HOME and repo, a vault, and terraform and oci stubs.
struct Oracle {
    dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
}

impl Oracle {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("wf-infra-").tempdir().unwrap();
        let (bin, home, repo) = (dir.path().join("bin"), dir.path().join("home"), dir.path().join("repo"));
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir_all(repo.join("infra/oracle-cloud")).unwrap();
        assert!(Command::new("git").args(["init", "-q"]).current_dir(&repo).status().unwrap().success());
        let node = node_path().expect("node on PATH").display().to_string();
        for (name, source) in [("bws", INFRA_BWS), ("terraform", TERRAFORM), ("oci", INFRA_OCI), ("gh", INFRA_GH)] {
            let path = bin.join(name);
            std::fs::write(&path, source.replace("NODE", &node)).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { dir, home, repo }
    }

    /// A fresh state: the vault holds `secrets`, terraform answers from `table`, oci lists `instances`; old logs and files are gone.
    fn world(&self, secrets: Value, table: Value, instances: Value) {
        for name in ["terraform.log", "counts.json"] {
            let _ = std::fs::remove_file(self.dir.path().join(name));
        }
        let _ = std::fs::remove_dir_all(self.home.join(".oci"));
        let _ = std::fs::remove_file(self.tfvars());
        std::fs::write(self.dir.path().join("vault.base.json"), secrets.to_string()).unwrap();
        std::fs::write(self.dir.path().join("terraform.json"), table.to_string()).unwrap();
        std::fs::write(self.dir.path().join("oci.json"), json!({ "data": instances }).to_string()).unwrap();
    }

    /// The vault serves what the world holds plus the OCI credentials the call names: a vault-backed `wf` reads them from there, never from its environment.
    fn wf(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let (creds, env): (Vec<_>, Vec<_>) = env.iter().copied().partition(|(k, _)| CREDS.iter().any(|(c, _)| c == k) || OPTIONAL_CREDS.contains(k));
        let mut vault: Vec<Value> = std::fs::read_to_string(self.dir.path().join("vault.base.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        vault.extend(creds.iter().map(|(k, v)| json!({ "id": k, "key": k, "value": v })));
        std::fs::write(self.dir.path().join("vault.json"), Value::Array(vault).to_string()).unwrap();
        let mut opts = RunOpts::default()
            .bin_dir(&self.dir.path().join("bin"))
            .cwd(&self.repo)
            .clean_env()
            .env("HOME", &self.home.display().to_string())
            .env("SECRETS_BACKEND", "bitwarden")
            .env("SECRETS_LOG_BETTERSTACK", "false")
            .env("BWS_ADMIN_ACCESS_TOKEN", "read")
            .env("BWS_ADMIN_ACCESS_TOKEN", "edit")
            .env("TF_VAR_adb_admin_password", "pw")
            .env("TF_VAR_ssh_public_key_path", "/k.pub")
            .env("BWS_FIXTURE", &self.dir.path().join("vault.json").display().to_string())
            .env("STUB_DIR", &self.dir.path().display().to_string());
        for (key, value) in &env {
            opts = opts.env(key, value);
        }
        run_wf(args, &opts)
    }

    fn apply(&self, env: &[(&str, &str)]) -> RunResult {
        self.wf(&["vm", "terraform", "apply", &self.dir.path().display().to_string(), "ocid1.c"], env)
    }

    fn tf_log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.path().join("terraform.log")).map(|log| log.trim().lines().map(|l| serde_json::from_str(l).unwrap()).collect()).unwrap_or_default()
    }

    fn verbs(&self) -> Vec<String> {
        self.tf_log().iter().map(|l| l["args"][0].as_str().unwrap().to_string()).collect()
    }

    fn tfvars(&self) -> PathBuf {
        self.repo.join("infra/oracle-cloud/terraform.tfvars")
    }
}

/// The other keys a call reads from the vault: the optional OCI and PagerDuty ones, and the tailnet's.
const OPTIONAL_CREDS: [&str; 5] = ["OCI_API_PRIVATE_KEY_PASSPHRASE", "PAGERDUTY_INTEGRATION_KEY", "PAGERDUTY_EVENTS_DOMAIN", "TS_TAILNET_ID", "TS_VM_OAUTH_CLIENT_SECRET"];

const CREDS: [(&str, &str); 6] = [
    ("OCI_TENANCY_OCID", "ocid1.tenancy.x"),
    ("OCI_USER_OCID", "ocid1.user.x"),
    ("OCI_API_KEY_FINGERPRINT", "aa:bb"),
    ("OCI_API_PRIVATE_KEY", "  -----BEGIN KEY-----\nabc\n-----END KEY-----  \n"),
    ("OCI_COMPARTMENT_OCID", "ocid1.compartment.x"),
    ("OCI_REGION", "eu-frankfurt-1"),
];

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().mode() & 0o777
}

fn state_keys() -> Value {
    json!([{ "id": "1", "key": "OCI_TFSTATE_S3_ACCESS_KEY_ID", "value": "AKID" }, { "id": "2", "key": "OCI_TFSTATE_S3_SECRET_ACCESS_KEY", "value": "SECRET" }])
}

fn show(resources: Value) -> Value {
    json!({ "stdout": json!({ "resource_changes": resources }).to_string() })
}

fn create(shape: &str, ocpus: u32, mem: u32) -> Value {
    json!({ "address": format!("oci_core_instance.{shape}"), "change": { "actions": ["create"], "after": { "shape": shape, "shape_config": [{ "ocpus": ocpus, "memory_in_gbs": mem }] } } })
}

fn no_change() -> Value {
    json!({ "address": "oci_core_instance.a", "change": { "actions": ["no-op"], "after": null } })
}

fn verdict(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", r.stdout, r.stderr))
}

#[test]
fn the_oracle_credentials_are_synced_from_the_vault_and_terraform_keeps_the_vm_within_the_free_tier() {
    let o = Oracle::new();

    // Given all six OCI secrets, When they are synced, Then the key, the tfvars block and the OCI config are written privately
    o.world(json!([]), json!({}), json!([]));
    assert_eq!(o.wf(&["vm", "creds", "sync"], &CREDS).exit_code, 0);
    let pem = o.home.join(".oci/oci_api_key.pem");
    assert_eq!(std::fs::read_to_string(&pem).unwrap(), "-----BEGIN KEY-----\nabc\n-----END KEY-----\n");
    assert_eq!((mode(&pem), mode(&o.home.join(".oci"))), (0o600, 0o700));
    let block = [
        "# --- BEGIN wf deploy sync-creds (managed) ---".to_string(),
        "tenancy_ocid = \"ocid1.tenancy.x\"".into(),
        "user_ocid = \"ocid1.user.x\"".into(),
        "api_key_fingerprint = \"aa:bb\"".into(),
        format!("api_private_key_path = \"{}\"", pem.display()),
        "api_private_key_passphrase = \"\"".into(),
        "region = \"eu-frankfurt-1\"".into(),
        "compartment_ocid = \"ocid1.compartment.x\"".into(),
        "# --- END wf deploy sync-creds (managed) ---".into(),
    ];
    let mut whole = block.to_vec();
    whole.extend(["".into(), "# Everything else (ssh_public_key_path, sizing overrides,".into(), "# bws_read_access_token, ...) is NOT managed by".into(), "# sync-creds — see terraform.tfvars.example.".into(), "".into()]);
    assert_eq!(std::fs::read_to_string(o.tfvars()).unwrap(), whole.join("\n"));
    let cfg = o.home.join(".oci/config");
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), format!("[DEFAULT]\nuser=ocid1.user.x\nfingerprint=aa:bb\ntenancy=ocid1.tenancy.x\nregion=eu-frankfurt-1\nkey_file={}\n", pem.display()));
    assert_eq!(mode(&cfg), 0o600);

    // Given a passphrase and PagerDuty settings, Then they are rendered too
    let mut env = CREDS.to_vec();
    env.extend([("OCI_API_PRIVATE_KEY_PASSPHRASE", "hunter2"), ("PAGERDUTY_INTEGRATION_KEY", "pd-key"), ("PAGERDUTY_EVENTS_DOMAIN", "events.eu.pagerduty.com")]);
    o.wf(&["vm", "creds", "sync"], &env);
    let tfvars = std::fs::read_to_string(o.tfvars()).unwrap();
    for line in ["api_private_key_passphrase = \"hunter2\"", "pagerduty_integration_key = \"pd-key\"", "pagerduty_events_domain = \"events.eu.pagerduty.com\""] {
        assert!(tfvars.contains(line), "{line}");
    }
    assert!(std::fs::read_to_string(&cfg).unwrap().contains("pass_phrase=hunter2\n"));

    // Given a tfvars with a managed block, Then only the block changes; without one, the block is appended
    std::fs::write(o.tfvars(), "budget_alert_email = \"a@b.c\"\n\n# --- BEGIN wf deploy sync-creds (managed) ---\nold\n# --- END wf deploy sync-creds (managed) ---\n\nssh_public_key_path = \"k.pub\"\n").unwrap();
    o.wf(&["vm", "creds", "sync"], &CREDS);
    let tfvars = std::fs::read_to_string(o.tfvars()).unwrap();
    assert!(tfvars.starts_with("budget_alert_email = \"a@b.c\"\n\n# --- BEGIN") && !tfvars.contains("\nold\n"));
    assert!(tfvars.ends_with("# --- END wf deploy sync-creds (managed) ---\n\nssh_public_key_path = \"k.pub\"\n"));
    std::fs::write(o.tfvars(), "budget_alert_email = \"a@b.c\"\n\n\n").unwrap();
    o.wf(&["vm", "creds", "sync"], &CREDS);
    let tfvars = std::fs::read_to_string(o.tfvars()).unwrap();
    assert!(tfvars.starts_with("budget_alert_email = \"a@b.c\"\n\n# --- BEGIN") && tfvars.ends_with("# --- END wf deploy sync-creds (managed) ---\n"));

    // Given missing secrets, Then exactly those are named and nothing is written
    o.world(json!([]), json!({}), json!([]));
    let partial: Vec<(&str, &str)> = CREDS.iter().copied().filter(|(k, _)| *k != "OCI_REGION" && *k != "OCI_USER_OCID").collect();
    let r = o.wf(&["vm", "creds", "sync"], &partial);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("wf: Missing OCI credentials in the vault: OCI_USER_OCID, OCI_REGION."), "{}", r.stderr);
    assert!(!o.home.join(".oci").exists() && !o.tfvars().exists());

    // Given the state credentials, When terraform runs, Then its args, cwd and the S3 environment reach it and its exit passes through
    o.world(state_keys(), json!({ "output": { "stdout": "ok" }, "plan": { "exit": 3 } }), json!([]));
    let work = o.dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    assert_eq!(o.wf(&["vm", "terraform", "exec", &work.display().to_string(), "output", "-json"], &[]).exit_code, 0);
    let call = &o.tf_log()[0];
    assert_eq!((call["args"].clone(), call["cwd"].clone()), (json!(["output", "-json"]), json!(work.display().to_string())));
    assert_eq!(call["env"], json!({ "id": "AKID", "secret": "SECRET", "region": "eu-frankfurt-1", "calc": "when_required", "valid": "when_required" }));
    assert_eq!(o.wf(&["vm", "terraform", "exec", &work.display().to_string(), "plan"], &[]).exit_code, 3);
    // and with no terraform arguments, or a state secret absent, terraform is never called
    o.world(state_keys(), json!({}), json!([]));
    assert_eq!(o.wf(&["vm", "terraform", "exec", &work.display().to_string()], &[]).exit_code, 2);
    o.world(json!([state_keys()[0].clone()]), json!({}), json!([]));
    let r = o.wf(&["vm", "terraform", "exec", &work.display().to_string(), "plan"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Cannot reach Terraform remote state: OCI_TFSTATE_S3_SECRET_ACCESS_KEY did not resolve from the vault."), "{}", r.stderr);
    assert!(o.tf_log().is_empty());

    // When the VM is replaced through exec, Then it refuses before terraform runs and names the recreate path
    o.world(state_keys(), json!({}), json!([]));
    let r = o.wf(&["vm", "terraform", "exec", &work.display().to_string(), "apply", "-replace=oci_core_instance.main"], &[]);
    assert_eq!(r.exit_code, 2);
    assert_eq!(serde_json::from_str::<Value>(r.stdout.trim()).unwrap()["next"], "wf vm terraform apply --recreate", "{}", r.stdout);
    assert!(o.tf_log().is_empty());
    // When a recreate finds no VM in the remote state, Then it destroys nothing: the recreate would be stranded
    let vars = [("OCI_COMPARTMENT_OCID", "ocid1.c")];
    o.world(state_keys(), json!({ "state list": { "stdout": "oci_objectstorage_bucket.tfstate\n" } }), json!([]));
    let r = o.wf(&["vm", "terraform", "apply", "--recreate", &work.display().to_string()], &vars);
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("does not hold oci_core_instance.main"), "{}", r.stdout);
    assert_eq!(o.verbs(), ["state"]);

    // Given a plan that converges after apply, Then it plans, shows, saves the state, applies and verifies
    o.world(state_keys(), json!({ "show": [show(json!([create("VM.Standard.A1.Flex", 2, 12)])), show(json!([no_change()]))], "state pull": { "stdout": "{\"serial\":1}" }, "apply": { "stdout": "applied\n" } }), json!([]));
    let r = o.apply(&[]);
    assert_eq!((r.exit_code, r.stdout.trim()), (0, "{\"ok\":true,\"stdout\":\"applied\\n\"}"));
    assert_eq!(o.verbs(), ["plan", "show", "state", "apply", "plan", "show"]);
    assert_eq!(o.tf_log()[0]["args"].as_array().unwrap()[..2], [json!("plan"), json!("-input=false")]);

    // Given existing A1 usage a create would push past the free pool, a billable shape or an oversized micro, Then the guard blocks first
    let instances = json!([
        { "id": "i1", "shape": "VM.Standard.A1.Flex", "lifecycle-state": "RUNNING", "shape-config": { "ocpus": 3, "memory-in-gbs": 18 } },
        { "id": "i2", "shape": "VM.Standard.A1.Flex", "lifecycle-state": "TERMINATED", "shape-config": { "ocpus": 4, "memory-in-gbs": 24 } },
        { "id": "i3", "shape": "VM.Standard.E2.1.Micro", "lifecycle-state": "RUNNING", "shape-config": null },
    ]);
    o.world(state_keys(), json!({ "show": show(json!([create("VM.Standard.A1.Flex", 2, 12)])) }), instances);
    let r = o.apply(&[]);
    assert_eq!(r.exit_code, 1);
    assert_eq!(verdict(&r), json!({ "ok": false, "reason": "free-tier guard blocked oci_core_instance.VM.Standard.A1.Flex: VM.Standard.A1.Flex OCPU pool would be 5, exceeding the free-tier limit of 2", "rolledBack": false }));
    assert!(!o.verbs().iter().any(|v| v == "state" || v == "apply"));
    o.world(state_keys(), json!({ "show": show(json!([create("VM.Standard.E4.Flex", 1, 8)])) }), json!([]));
    assert!(verdict(&o.apply(&[]))["reason"].as_str().unwrap().contains("Shape \"VM.Standard.E4.Flex\" is not a recognized Always Free shape"));
    assert!(!o.verbs().iter().any(|v| v == "apply"));
    o.world(state_keys(), json!({ "show": show(json!([create("VM.Standard.E2.1.Micro", 2, 1)])) }), json!([]));
    assert!(verdict(&o.apply(&[]))["reason"].as_str().unwrap().contains("VM.Standard.E2.1.Micro requested with 2 OCPU/1GB, exceeding its fixed free spec"));

    // Given terraform plan or apply fails, Then the failure is reported and nothing is rolled back
    o.world(state_keys(), json!({ "plan": { "exit": 1, "stderr": "no config\n" } }), json!([]));
    let r = o.apply(&[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("Error: terraform plan failed: no config"), "{}", r.stderr);
    o.world(state_keys(), json!({ "show": show(json!([create("VM.Standard.A1.Flex", 1, 6)])), "state pull": { "stdout": "{\"serial\":1}" }, "apply": { "exit": 1, "stderr": "boom\n" } }), json!([]));
    let r = o.apply(&[]);
    assert_eq!((r.exit_code, verdict(&r)), (1, json!({ "ok": false, "reason": "terraform apply failed", "rolledBack": false, "stdout": "", "stderr": "boom" })));
    assert_eq!(o.verbs().iter().filter(|v| *v == "state").count(), 1);

    // Given the plan after apply still has changes, Then the state from before is pushed back
    o.world(state_keys(), json!({ "show": [show(json!([create("VM.Standard.A1.Flex", 1, 6)])), show(json!([create("VM.Standard.A1.Flex", 1, 6)]))], "state pull": { "stdout": "{\"serial\":7}" }, "apply": { "stdout": "applied\n" } }), json!([]));
    let r = o.apply(&[]);
    assert_eq!(
        (r.exit_code, verdict(&r)),
        (1, json!({ "ok": false, "reason": "post-apply plan still shows 1 pending change(s) -- rolled back to the pre-apply state", "rolledBack": true, "stdout": "applied\n", "stderr": "" }))
    );
    let log = o.tf_log();
    let push = log.iter().find(|l| l.get("pushed").is_some()).expect("a state push");
    assert_eq!((push["pushed"].as_str(), push["args"].as_array().unwrap()[..3].to_vec()), (Some("{\"serial\":7}"), vec![json!("state"), json!("push"), json!("-force")]));

    // Given a plan that replaces the VM, Then it is destroyed, its stale tailnet record deleted, and then applied
    let tailscale = MockServer::start(None);
    let seen = iso_from_ms(now_ms());
    tailscale.routes(&[
        ("POST /oauth/token", MockReply::json(200, json!({ "access_token": "tok" }))),
        (
            "GET /tailnet/example.com/devices",
            MockReply::json(200, json!({ "devices": [
                { "id": "d1", "name": "vm-ops-api-main.tail.ts.net", "lastSeen": seen, "tags": ["tag:mobile-learning"] },
                { "id": "d2", "name": "sandbox-agent.tail.ts.net", "lastSeen": seen, "tags": ["tag:mobile-learning"] },
                { "id": "d3", "name": "phone.tail.ts.net", "lastSeen": seen, "tags": [] },
            ] })),
        ),
        ("DELETE /device/d1", MockReply::json(200, json!({}))),
    ]);
    // and the pool the old VM holds counts as freed, so replacing a VM that fills the pool is allowed
    let a1 = json!({ "shape": "VM.Standard.A1.Flex", "shape_config": [{ "ocpus": 2, "memory_in_gbs": 12 }] });
    let replace = json!({ "address": "oci_core_instance.main", "change": { "actions": ["delete", "create"], "before": a1, "after": a1 } });
    let running = json!([{ "id": "i1", "shape": "VM.Standard.A1.Flex", "lifecycle-state": "RUNNING", "shape-config": { "ocpus": 2, "memory-in-gbs": 12 } }]);
    o.world(state_keys(), json!({ "show": [show(json!([replace])), show(json!([no_change()]))], "state pull": { "stdout": "{\"serial\":1}" }, "destroy": { "stdout": "destroyed\n" }, "apply": { "stdout": "applied\n" } }), running);
    let api = format!("http://127.0.0.1:{}", tailscale.port);
    let r = o.apply(&[("WF_TAILSCALE_API", api.as_str()), ("TS_TAILNET_ID", "example.com"), ("TS_VM_OAUTH_CLIENT_SECRET", "secret")]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(o.verbs(), ["plan", "show", "state", "destroy", "apply", "plan", "show"]);
    let deletes: Vec<String> = tailscale.requests().iter().filter(|q| q.method == "DELETE").map(|q| q.url.clone()).collect();
    assert_eq!(deletes, ["/device/d1"]);

    // When the VM is recreated, Then it is destroyed, its tailnet records deleted, and the guarded apply creates it again
    let create_main = json!({ "address": "oci_core_instance.main", "change": { "actions": ["create"], "before": null, "after": a1 } });
    o.world(state_keys(), json!({ "state list": { "stdout": "oci_core_instance.main\n" }, "show": [show(json!([create_main])), show(json!([no_change()]))], "state pull": { "stdout": "{\"serial\":2}" }, "destroy": { "stdout": "destroyed\n" }, "apply": { "stdout": "applied\n" } }), json!([]));
    let recreate_env = [("WF_TAILSCALE_API", api.as_str()), ("TS_TAILNET_ID", "example.com"), ("TS_VM_OAUTH_CLIENT_SECRET", "secret"), ("OCI_COMPARTMENT_OCID", "ocid1.c")];
    let r = o.wf(&["vm", "terraform", "apply", "--recreate", &o.dir.path().display().to_string()], &recreate_env);
    assert_eq!((r.exit_code, r.stdout.trim()), (0, "{\"ok\":true,\"stdout\":\"applied\\n\"}"), "{}", r.stderr);
    assert_eq!(o.verbs(), ["state", "destroy", "plan", "show", "state", "apply", "plan", "show"]);
    assert_eq!(tailscale.requests().iter().filter(|q| q.method == "DELETE").count(), 2);
}

fn tailnet_api(devices: Value) -> MockServer {
    let server = MockServer::start(None);
    server.routes(&[
        ("POST /oauth/token", MockReply::json(200, json!({ "access_token": "tok" }))),
        ("GET /tailnet/example.com/devices", MockReply::json(200, json!({ "devices": devices }))),
        ("DELETE /device/wf-preflight-no-such-device", MockReply::json(404, json!({ "message": "device not found" }))),
    ]);
    server
}

#[test]
fn a_failed_tailnet_cleanup_retries_then_stops_the_recreate_before_the_vm_is_created() {
    let o = Oracle::new();
    let a1 = json!({ "shape": "VM.Standard.A1.Flex", "shape_config": [{ "ocpus": 2, "memory_in_gbs": 12 }] });
    let create_main = json!({ "address": "oci_core_instance.main", "change": { "actions": ["create"], "before": null, "after": a1 } });
    let replace = json!({ "address": "oci_core_instance.main", "change": { "actions": ["delete", "create"], "before": a1, "after": a1 } });
    let dir = o.dir.path().display().to_string();
    let tailscale = MockServer::start(None);
    let api = format!("http://127.0.0.1:{}", tailscale.port);
    let env = [("WF_TAILSCALE_API", api.as_str()), ("TS_TAILNET_ID", "example.com"), ("TS_VM_OAUTH_CLIENT_SECRET", "secret"), ("OCI_COMPARTMENT_OCID", "ocid1.c"), ("WF_TAILNET_RETIRE_BACKOFF_MS", "1")];
    let token = || MockReply::json(200, json!({ "access_token": "tok" }));
    let recreate = |table: Value| {
        o.world(state_keys(), table, json!([]));
        o.wf(&["vm", "terraform", "apply", "--recreate", &dir], &env)
    };
    let recreate_table = |extra: Value| {
        let mut t = json!({ "state list": { "stdout": "oci_core_instance.main\n" }, "show": [show(json!([create_main.clone()])), show(json!([no_change()]))], "state pull": { "stdout": "{\"serial\":2}" }, "destroy": { "stdout": "destroyed\n" }, "apply": { "stdout": "applied\n" } });
        t.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        t
    };

    // Given the tailnet API fails once and then answers, When the VM is recreated, Then the cleanup is retried and the create goes ahead
    tailscale.queue(
        vec![token(), MockReply::json(500, json!({})), token(), MockReply::json(200, json!({ "devices": [{ "id": "d1", "name": "vm-ops-api-main.tail.ts.net", "lastSeen": "2026-11-07T00:00:00Z", "tags": ["tag:mobile-learning"] }] })), MockReply::json(200, json!({}))],
        false,
    );
    let r = recreate(recreate_table(json!({})));
    assert_eq!((r.exit_code, r.stdout.trim()), (0, "{\"ok\":true,\"stdout\":\"applied\\n\"}"), "{}", r.stderr);
    assert!(r.stderr.contains("tailnet cleanup attempt 1/4 failed"), "{}", r.stderr);
    assert_eq!(tailscale.requests().iter().filter(|q| q.method == "DELETE").map(|q| q.url.clone()).collect::<Vec<_>>(), ["/device/d1"]);
    assert!(o.verbs().iter().any(|v| v == "apply"));

    // Given the tailnet API never answers, When the VM is recreated, Then it exits 1 with one verdict that names the evidence and the commands to continue, and the VM is never created
    tailscale.clear();
    tailscale.queue(vec![MockReply::json(500, json!({}))], true);
    let r = recreate(recreate_table(json!({})));
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let v = verdict(&r);
    assert_eq!((v["ok"].clone(), v["vmDestroyed"].clone(), v["created"].clone()), (json!(false), json!(true), json!(false)), "{v}");
    assert_eq!(v["evidence"].as_array().unwrap().len(), 4, "{v}");
    assert!(v["evidence"][3]["error"].as_str().unwrap().contains("OAuth exchange failed"), "{v}");
    assert_eq!(v["next"], json!(["wf vm net device retire --replaced-vm --apply", format!("wf vm terraform apply {dir} ocid1.c")]));
    assert_eq!(o.verbs(), ["state", "destroy"], "the create must not start");
    assert_eq!(tailscale.requests().len(), 4);

    // and the same holds when a plain apply replaces the VM
    tailscale.clear();
    tailscale.queue(vec![MockReply::json(500, json!({}))], true);
    o.world(state_keys(), json!({ "show": show(json!([replace])), "state pull": { "stdout": "{\"serial\":1}" }, "destroy": { "stdout": "destroyed\n" } }), json!([]));
    let r = o.wf(&["vm", "terraform", "apply", &dir, "ocid1.c"], &env);
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(verdict(&r)["created"], json!(false));
    assert_eq!(o.verbs(), ["plan", "show", "state", "destroy"]);
}

/// A vault holding every secret the recreate preflight reads for an app list of `t3-serve` (it has no live check), less `without`.
fn preflight_vault(without: &[&str]) -> Value {
    let fingerprint = ["aa"; 16].join(":");
    let password = "p".repeat(43);
    let mut vault = state_keys().as_array().unwrap().clone();
    for (key, value) in [
        ("TS_VM_AUTH_KEY", "tskey-abc"),
        ("T3_RCA_BEARER", "bearer"),
        ("OCI_TENANCY_OCID", "ocid1.tenancy.x"),
        ("OCI_USER_OCID", "ocid1.user.x"),
        ("OCI_API_KEY_FINGERPRINT", fingerprint.as_str()),
        ("OCI_API_PRIVATE_KEY", "-----BEGIN KEY-----\nabc\n-----END KEY-----"),
        ("OCI_COMPARTMENT_OCID", "ocid1.compartment.x"),
        ("OCI_REGION", "eu-frankfurt-1"),
        ("BWS_ADMIN_ACCESS_TOKEN", "read"),
        ("ADB_ADMIN_PASSWORD", "pw"),
        ("TS_VM_OAUTH_CLIENT_SECRET", "secret"),
        ("TS_TAILNET_ID", "example.com"),
        ("VM_STATE_RESTIC_PASSWORD", password.as_str()),
    ] {
        if !without.contains(&key) {
            vault.push(json!({ "id": key, "key": key, "value": value }));
        }
    }
    Value::Array(vault)
}

fn change(address: &str, actions: Value) -> Value {
    json!({ "address": address, "change": { "actions": actions, "before": {}, "after": {} } })
}

#[test]
fn the_recreate_preflight_passes_only_when_state_secrets_ci_and_plan_all_hold() {
    let o = Oracle::new();
    let work = o.dir.path().display().to_string();
    let backups = o.dir.path().join("backups");
    let backups_arg = backups.display().to_string();
    std::fs::write(
        o.repo.join("infra/oracle-cloud/vm-apps.json"),
        json!([{ "name": "t3-serve", "repoPath": "y", "composeFile": "c.yml" }]).to_string(),
    )
    .unwrap();
    let tailscale = tailnet_api(json!([{ "id": "d1", "name": "vm-ops-api-main.tail.ts.net", "tags": ["tag:mobile-learning"] }]));
    let tailnet = format!("http://127.0.0.1:{}", tailscale.port);
    let holds_vm = json!({ "serial": 7, "resources": [{ "mode": "managed", "type": "oci_core_instance", "name": "main" }, { "mode": "managed", "type": "oci_identity_dynamic_group", "name": "budget_guard_fn", "instances": [{}] }] }).to_string();
    let a1 = json!({ "shape": "VM.Standard.A1.Flex", "shape_config": [{ "ocpus": 2, "memory_in_gbs": 12 }] });
    let replace_vm = json!({ "address": "oci_core_instance.main", "change": { "actions": ["delete", "create"], "before": a1, "after": a1 } });
    let table = |plan: Vec<Value>, state: &str| json!({ "state pull": { "stdout": state }, "show": show(Value::Array(plan)) });
    let set_runs = |runs: Value| std::fs::write(o.dir.path().join("gh.json"), runs.to_string()).unwrap();
    let preflight = |extra: &[&str]| {
        let mut args = vec!["vm", "terraform", "preflight", work.as_str()];
        args.extend_from_slice(extra);
        o.wf(&args, &[("WF_TFSTATE_BACKUP_DIR", backups_arg.as_str()), ("WF_TAILSCALE_API", tailnet.as_str())])
    };
    let failed = |v: &Value| -> Vec<String> { v["checks"].as_array().unwrap().iter().filter(|c| c["ok"] == false).map(|c| c["name"].as_str().unwrap().to_string()).collect() };
    let detail = |v: &Value, name: &str| -> String { v["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap()["detail"].as_str().unwrap().to_string() };

    // Given a state that holds the VM, every secret, no run in flight and a plan that replaces only the VM
    set_runs(json!({}));
    o.world(preflight_vault(&[]), table(vec![replace_vm.clone(), no_change()], &holds_vm), json!([]));
    let r = preflight(&[]);
    // Then every check passes, the plan lists the actions per resource, and nothing needs minting after the apply
    let v = verdict(&r);
    assert_eq!((r.exit_code, v["ok"].clone(), failed(&v)), (0, json!(true), Vec::<String>::new()), "{}{}", r.stdout, r.stderr);
    assert_eq!(v["plan"]["destroys"], json!(["oci_core_instance.main"]));
    assert_eq!(v["plan"]["afterApply"], json!([]));
    assert_eq!(v["next"], "wf vm terraform apply --recreate");
    // and the plan was made with the VM replaced, the state backed up byte for byte, and nothing destroyed or applied
    let log = o.tf_log();
    assert!(log.iter().any(|l| l["args"].as_array().unwrap().contains(&json!("-replace=oci_core_instance.main"))));
    assert!(!o.verbs().iter().any(|v| v == "destroy" || v == "apply"), "{:?}", o.verbs());
    let saved: Vec<_> = std::fs::read_dir(&backups).unwrap().flatten().collect();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].file_name().to_string_lossy().ends_with("-serial7.json"));
    assert_eq!(std::fs::read_to_string(saved[0].path()).unwrap(), holds_vm);
    // When it runs again on the same state, Then that backup is the current one and no second file is made
    let again = verdict(&preflight(&[]));
    assert!(detail(&again, "tfstate").ends_with("(already current)"), "{again}");
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1);

    // Given a run in flight, Then only the ci check fails and it names the run
    set_runs(json!({ "in_progress": [{ "id": 42, "name": "CI", "head_sha": "abcdef1234567" }] }));
    let r = preflight(&[]);
    let v = verdict(&r);
    assert_eq!((r.exit_code, failed(&v)), (1, vec!["ci".to_string()]));
    assert!(detail(&v, "ci").contains("CI #42 (in_progress, abcdef123)"), "{v}");
    assert!(v["next"].as_str().unwrap().starts_with("fix the failed checks"));
    // Given a run only queued, Then the ci check passes and says it waits for the new runner
    set_runs(json!({ "queued": [{ "id": 43, "name": "CI", "head_sha": "abcdef1234567" }] }));
    let q = verdict(&preflight(&[]));
    assert!(failed(&q).is_empty(), "{q}");
    assert!(detail(&q, "ci").contains("only queued") && detail(&q, "ci").contains("CI #43"), "{q}");
    // and the run list the API cannot give is a failure too, never a pass
    set_runs(json!({ "exit": 1, "stderr": "HTTP 403" }));
    assert!(detail(&verdict(&preflight(&[])), "ci").contains("HTTP 403"));
    set_runs(json!({}));

    // Given a secret a deploy of one app needs is missing from the vault, Then the secrets check names it
    o.world(preflight_vault(&["T3_RCA_BEARER"]), table(vec![replace_vm.clone()], &holds_vm), json!([]));
    let v = verdict(&preflight(&[]));
    assert_eq!(failed(&v), vec!["secrets".to_string()]);
    assert!(detail(&v, "secrets").contains("T3_RCA_BEARER"), "{v}");

    // Given the restic password of the state backup is missing from the vault, Then the secrets check fails naming it and how to mint it (the first boot would start no stateful app)
    o.world(preflight_vault(&["VM_STATE_RESTIC_PASSWORD"]), table(vec![replace_vm.clone()], &holds_vm), json!([]));
    let v = verdict(&preflight(&[]));
    assert_eq!(failed(&v), vec!["secrets".to_string()], "{v}");
    assert!(detail(&v, "secrets").contains("VM_STATE_RESTIC_PASSWORD"), "{v}");
    let next = v["checks"].as_array().unwrap().iter().find(|c| c["name"] == "secrets").unwrap()["next"].as_str().unwrap().to_string();
    assert!(next.contains("openssl rand -base64 32") && next.contains("wf secret create VM_STATE_RESTIC_PASSWORD"), "{next}");

    // Given a state that holds the VM but no tenancy-scoped dynamic group at all, Then only the iam check fails, naming the rights the Terraform user needs
    let bare = json!({ "serial": 7, "resources": [{ "mode": "managed", "type": "oci_core_instance", "name": "main" }] }).to_string();
    o.world(preflight_vault(&[]), table(vec![replace_vm.clone()], &bare), json!([]));
    let v = verdict(&preflight(&[]));
    assert_eq!(failed(&v), vec!["iam".to_string()], "{v}");
    assert!(detail(&v, "iam").contains("oci_identity_dynamic_group.vm_state") && detail(&v, "iam").contains("tenancy"), "{v}");
    let next = v["checks"].as_array().unwrap().iter().find(|c| c["name"] == "iam").unwrap()["next"].as_str().unwrap().to_string();
    assert!(next.contains("manage dynamic-groups in tenancy"), "{next}");
    // and a state that already holds the VM state group and its policy needs no proof
    let created = json!({ "serial": 7, "resources": [{ "mode": "managed", "type": "oci_core_instance", "name": "main" }, { "mode": "managed", "type": "oci_identity_dynamic_group", "name": "vm_state", "instances": [{}] }, { "mode": "managed", "type": "oci_identity_policy", "name": "vm_state", "instances": [{}] }] }).to_string();
    o.world(preflight_vault(&[]), table(vec![replace_vm.clone()], &created), json!([]));
    let v = verdict(&preflight(&[]));
    assert_eq!(failed(&v), Vec::<String>::new(), "{v}");

    // Given a plan that would also destroy a subnet, Then the plan check fails naming it, unless that destroy is expected
    let replace_subnet = change("oci_core_subnet.public", json!(["delete", "create"]));
    o.world(preflight_vault(&[]), table(vec![replace_vm.clone(), replace_subnet], &holds_vm), json!([]));
    let v = verdict(&preflight(&[]));
    assert_eq!(failed(&v), vec!["plan".to_string()]);
    assert!(detail(&v, "plan").contains("oci_core_subnet.public"), "{v}");
    assert_eq!(v["plan"]["unexpectedDestroys"], json!(["oci_core_subnet.public"]));
    assert_eq!(verdict(&preflight(&["--expect", "oci_core_subnet.public"]))["ok"], json!(true));

    // Given the tailnet API refuses the credentials, Then only the tailnet check fails, before any VM is destroyed
    tailscale.routes(&[("POST /oauth/token", MockReply::json(401, json!({})))]);
    o.world(preflight_vault(&[]), table(vec![replace_vm.clone()], &holds_vm), json!([]));
    let r = preflight(&[]);
    let v = verdict(&r);
    assert_eq!((r.exit_code, failed(&v)), (1, vec!["tailnet".to_string()]), "{v}");
    assert!(detail(&v, "tailnet").contains("OAuth exchange failed"), "{v}");
    tailscale.routes(&[("POST /oauth/token", MockReply::json(200, json!({ "access_token": "tok" }))), ("GET /tailnet/example.com/devices", MockReply::json(200, json!({ "devices": [] }))), ("DELETE /device/wf-preflight-no-such-device", MockReply::json(404, json!({})))]);

    // Given a read-only OAuth client (it lists devices but the delete is refused), Then the tailnet check fails naming the missing write scope, and deletes nothing real
    let probe_answers = |reply: MockReply| tailscale.routes(&[("POST /oauth/token", MockReply::json(200, json!({ "access_token": "tok" }))), ("GET /tailnet/example.com/devices", MockReply::json(200, json!({ "devices": [] }))), ("DELETE /device/wf-preflight-no-such-device", reply)]);
    probe_answers(MockReply::json(403, json!({ "message": "calling actor does not have enough permissions" })));
    o.world(preflight_vault(&[]), table(vec![replace_vm.clone()], &holds_vm), json!([]));
    let v = verdict(&preflight(&[]));
    assert_eq!(failed(&v), vec!["tailnet".to_string()], "{v}");
    assert!(detail(&v, "tailnet").contains("lacks the write scope `devices:core`"), "{v}");
    assert!(tailscale.requests().iter().filter(|q| q.method == "DELETE").all(|q| q.url == "/device/wf-preflight-no-such-device"));
    // and a probe answered with anything but not-found leaves the write scope unproven
    probe_answers(MockReply::json(200, json!({})));
    let v = verdict(&preflight(&[]));
    assert!(detail(&v, "tailnet").contains("answered 200, not 404"), "{v}");
    probe_answers(MockReply::json(404, json!({})));

    // Given a state that does not hold the VM, Then the state check fails and says why destroying would strand the recreate
    let empty = json!({ "serial": 3, "resources": [] }).to_string();
    o.world(preflight_vault(&[]), table(vec![replace_vm.clone()], &empty), json!([]));
    let v = verdict(&preflight(&[]));
    assert_eq!(failed(&v), vec!["tfstate".to_string()]);
    assert!(detail(&v, "tfstate").contains("does not hold oci_core_instance.main"), "{v}");
}

type Pair = (String, String);

#[test]
fn the_recreate_preflight_refuses_state_whose_backup_is_stale_damaged_or_unreachable() {
    let o = Oracle::new();
    let work = o.dir.path().display().to_string();
    let backups = o.dir.path().join("backups").display().to_string();
    std::fs::write(
        o.repo.join("infra/oracle-cloud/vm-apps.json"),
        json!([{ "name": "orca-serve", "repoPath": "y", "composeFile": "c.yml", "stateVolumes": [{ "name": "orca-state", "maxBytes": 1000 }] }]).to_string(),
    )
    .unwrap();
    let holds_vm = json!({ "serial": 7, "resources": [{ "mode": "managed", "type": "oci_core_instance", "name": "main" }, { "mode": "managed", "type": "oci_identity_dynamic_group", "name": "budget_guard_fn", "instances": [{}] }] }).to_string();
    let a1 = json!({ "shape": "VM.Standard.A1.Flex", "shape_config": [{ "ocpus": 2, "memory_in_gbs": 12 }] });
    let replace_vm = json!({ "address": "oci_core_instance.main", "change": { "actions": ["delete", "create"], "before": a1, "after": a1 } });
    std::fs::write(o.dir.path().join("gh.json"), "{}").unwrap();
    let tailscale = tailnet_api(json!([]));
    let tailnet = format!("http://127.0.0.1:{}", tailscale.port);
    let vm = FakeVm::start();
    // the vault of this harness answers for the vm-ops-api credentials; the proxy and the rest stay environment
    let (vm_vault, vm_env): (Vec<Pair>, Vec<Pair>) = vm.opts().env.into_iter().filter(|(k, _)| !k.starts_with("SECRETS_")).partition(|(k, _)| k.starts_with("VM_"));
    let vault = || {
        let mut v = preflight_vault(&[]);
        v.as_array_mut().unwrap().push(json!({ "id": "orca", "key": "ORCA_RCA_BEARER", "value": "bearer" }));
        v.as_array_mut().unwrap().extend(vm_vault.iter().map(|(k, val)| json!({ "id": k, "key": k, "value": val })));
        v
    };
    // agent-shell answers the verify with `row` for orca-serve
    let preflight = |row: Value| {
        let job = answers::job_id(1);
        let printed = json!({ "event": "vm-state", "verb": "verify", "ok": row["ok"], "apps": [row] });
        vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(0), &format!("{printed}\n")))]);
        o.world(vault(), json!({ "state pull": { "stdout": holds_vm }, "show": show(json!([replace_vm.clone()])) }), json!([]));
        let mut env: Vec<(&str, &str)> = vm_env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        env.push(("WF_TFSTATE_BACKUP_DIR", backups.as_str()));
        env.push(("WF_TAILSCALE_API", tailnet.as_str()));
        verdict(&o.wf(&["vm", "terraform", "preflight", work.as_str()], &env))
    };
    let check = |v: &Value| v["checks"].as_array().unwrap().iter().find(|c| c["name"] == "backups").unwrap().clone();

    // Given a backup of every app with state that is an hour old and verifies, Then the backups check passes
    let v = preflight(json!({ "app": "orca-serve", "ok": true, "stamp": "20261107033000Z", "ageSeconds": 3600 }));
    assert_eq!(check(&v)["ok"], json!(true), "{v}");
    assert_eq!(v["ok"], json!(true), "{v}");
    let request = &vm.requests()[0];
    assert!(request.body.contains("vm state verify --local --plan "), "{}", request.body);

    // Given a latest backup over 24 hours old, Then the check fails naming the app and the age, and the verdict is not ok
    let v = preflight(json!({ "app": "orca-serve", "ok": true, "stamp": "20261105033000Z", "ageSeconds": 2 * 86_400 + 5 }));
    let c = check(&v);
    assert_eq!((c["ok"].clone(), v["ok"].clone()), (json!(false), json!(false)));
    assert!(c["detail"].as_str().unwrap().contains("orca-serve: the latest backup is 48 h old") && c["next"].as_str().unwrap().contains("wf vm state backup"), "{c}");

    // Given a latest backup that does not verify (its object was altered), Then the check fails with the cause
    let v = preflight(json!({ "app": "orca-serve", "ok": false, "error": "orca-state: restic restore abc123 exited 1: ciphertext verification failed", "ageSeconds": 60 }));
    assert_eq!(check(&v)["ok"], json!(false));
    assert!(check(&v)["detail"].as_str().unwrap().contains("ciphertext verification failed"), "{v}");

    // Given agent-shell that cannot be reached, Then the check fails: no answer is never a pass
    vm.replies(&vec![answers::reply(503, &answers::agent_shell_not_running()); 4]);
    o.world(vault(), json!({ "state pull": { "stdout": holds_vm }, "show": show(json!([replace_vm.clone()])) }), json!([]));
    let mut env: Vec<(&str, &str)> = vm_env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    env.push(("WF_TFSTATE_BACKUP_DIR", backups.as_str()));
    env.push(("WF_TAILSCALE_API", tailnet.as_str()));
    env.push(("WF_VM_STATE_RELAY_BACKOFF_MS", "1"));
    let v = verdict(&o.wf(&["vm", "terraform", "preflight", work.as_str()], &env));
    assert_eq!(check(&v)["ok"], json!(false));
    assert!(check(&v)["detail"].as_str().unwrap().contains("not running"), "{v}");
}

/// The services `wf vm ops probe` asks, each behind the tailnet name the host answers to (`names` stands in for MagicDNS); the
/// vm-ops-api that lists containers is `vm`'s.
struct Probing {
    vm: FakeVm,
    tailnet: MockTailnet,
    names: String,
}

impl Probing {
    fn new(names: &str, hosts: &[(&str, &MockServer)]) -> Self {
        let vm = FakeVm::start();
        let mut upstream: HashMap<String, u16> = hosts.iter().map(|(host, server)| (host.to_string(), server.port)).collect();
        upstream.insert("vm-ops-api-main:8080".into(), vm.server.port);
        Self { tailnet: MockTailnet::start(Upstream::Map(upstream)), vm, names: names.into() }
    }

    fn probe(&self, args: &[&str]) -> RunResult {
        let opts = self.vm.opts().env("WF_TAILNET_SOCKS", &self.tailnet.address()).env("WF_TEST_TAILNET_NAMES", &self.names);
        run_wf(&[&["vm", "ops", "probe"], args].concat(), &opts)
    }
}

fn serving() -> MockServer {
    let server = MockServer::start(None);
    server.routes(&[("GET /health", MockReply::json(200, answers::health(COMMIT)))]);
    server
}

#[test]
fn an_app_that_does_not_answer_its_health_is_named_with_its_container_states_and_the_exit_is_1() {
    // Given leetcode-backend serving, and remote-chat-agent's stack left created, so its host is down
    let leetcode = serving();
    let probing = Probing::new("leetcode-backend-prod,remote-chat-agent-prod", &[("leetcode-backend-prod:8789", &leetcode)]);
    let created = answers::ps("remote-chat-agent", &[("remote-chat-agent-tailscale", "created", "Created", "ns/app:sha-1"), ("remote-chat-agent", "created", "Created", "ns/app:sha-1")]);
    probing.vm.replies(&[answers::reply(200, &created)]);

    // When both are probed
    let r = probing.probe(&["remote-chat-agent", "leetcode-backend"]);

    // Then the exit is 1, the line names the app that is down with its containers and the one that serves is ok
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", r.stdout, r.stderr));
    assert_eq!((report["ok"].clone(), report["unhealthy"].clone(), report["apps"][1]["ok"].clone()), (json!(false), json!(["remote-chat-agent"]), json!(true)), "{report}");
    assert_eq!(report["apps"][0]["containers"], "remote-chat-agent-tailscale=created, remote-chat-agent=created");
    assert!(report["apps"][0]["error"].as_str().is_some_and(|e| e.contains("http://remote-chat-agent-prod:8787/health")), "{report}");
    assert!(report["next"].as_str().is_some_and(|n| n.contains("wf ship --status remote-chat-agent")), "{report}");
    assert!(r.stderr.contains("wf: not answering /health: remote-chat-agent (") && r.stderr.contains("containers: remote-chat-agent-tailscale=created, remote-chat-agent=created"), "{}", r.stderr);
    // and both hosts were asked, the serving one got its GET, and vm-ops-api was asked only for the down app's containers, signed
    assert_eq!(probing.tailnet.destinations(), ["remote-chat-agent-prod:8787", "leetcode-backend-prod:8789", "vm-ops-api-main:8080"]);
    assert_eq!(leetcode.requests().iter().map(|q| (q.method.as_str(), q.url.as_str())).collect::<Vec<_>>(), [("GET", "/health")]);
    let asked = probing.vm.requests();
    assert_eq!(asked.iter().map(|q| q.url.as_str()).collect::<Vec<_>>(), ["/services/remote-chat-agent/ps"]);
    probing.vm.assert_signed(&asked[0], "GET", "/services/remote-chat-agent/ps", "");

    // When only the serving one is probed, Then the exit is 0 and vm-ops-api is not asked
    probing.vm.replies::<&str>(&[]);
    let r = probing.probe(&["leetcode-backend"]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(serde_json::from_str::<Value>(r.stdout.trim()).unwrap()["ok"], true);
    assert!(probing.vm.requests().is_empty());

    // When an app with no tailnet /health is named, Then it is refused before any host is asked
    let before = probing.tailnet.destinations().len();
    let r = probing.probe(&["vnc-auth-session"]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, ""));
    assert!(r.stderr.contains("wf: vnc-auth-session has no /health on the tailnet to probe"), "{}", r.stderr);
    assert_eq!(probing.tailnet.destinations().len(), before);
}

#[test]
fn an_app_that_answers_within_the_wait_at_the_name_its_host_drifted_to_passes() {
    // Given remote-chat-agent's host drifted to a suffixed name and it answers 503 first, then 200
    let agent = MockServer::start(None);
    agent.queue(vec![MockReply::json(503, json!({ "ok": false })), MockReply::json(200, answers::health(COMMIT))], false);
    let probing = Probing::new("remote-chat-agent-prod-1", &[("remote-chat-agent-prod-1:8787", &agent)]);

    // When it is probed with a wait of 10 s
    let r = probing.probe(&["--wait", "10", "remote-chat-agent"]);

    // Then the second probe passes, both went to the drifted name, and no container states were read
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(serde_json::from_str::<Value>(r.stdout.trim()).unwrap()["apps"][0]["ok"], true);
    assert_eq!(probing.tailnet.destinations(), ["remote-chat-agent-prod-1:8787"; 2]);
    assert_eq!(agent.requests().len(), 2);
    assert!(probing.vm.requests().is_empty());
}
