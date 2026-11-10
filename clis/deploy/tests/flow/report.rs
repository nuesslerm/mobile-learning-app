//! The service health report of `wf check --health`: vm-ops-api behind a fake tailnet, ClickHouse and ORDS as fakes, and the
//! workspace's test tiers run through pnpm and npx stubs.
use crate::support::{create_sh_stub, ed25519_private_key_one_line, repo_root, run_wf, MockHttps, MockReply, MockServer, MockTailnet, RunOpts, RunResult, Rule, Upstream};
use wf_vmops::testing::answers;
use base64::Engine;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What a ClickHouse query asks for decides which canned reply answers it, in this order; the last kind is the catch-all.
const KINDS: [(&str, &[&str]); 5] = [("doctor", &["max(dt)"]), ("errcount", &["count() AS n"]), ("spans", &["AS duration_ms"]), ("lines", &["AS module"]), ("errsamples", &[])];
const USERS: [(&str, &str); 2] = [("backend", "reader"), ("mobile", "mreader")];

/// vm-ops-api behind a tailnet, ClickHouse answering BetterStack's queries by kind and reader, ORDS, and a tailnet where nothing answers.
struct World {
    dir: tempfile::TempDir,
    bin_dir: PathBuf,
    vm_ops: MockServer,
    ords: MockServer,
    clickhouse: MockHttps,
    tailnet: MockTailnet,
    down: MockTailnet,
    key: String,
    vm_routes: Vec<(String, MockReply)>,
    ch_replies: HashMap<String, String>,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("wf-flow-health-").tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();
        let vm_ops = MockServer::start(None);
        let tailnet = MockTailnet::start(Upstream::All(vm_ops.port));
        Self { dir, bin_dir, ords: MockServer::start(None), clickhouse: MockHttps::start(), down: MockTailnet::start(Upstream::Map(HashMap::new())), tailnet, vm_ops, key: ed25519_private_key_one_line(), vm_routes: vec![], ch_replies: HashMap::new() }
    }

    fn reply(&mut self, name: &str, body: &str) {
        if name == "health" || name.starts_with("ps-") {
            let path = match name {
                "health" => "/health".to_string(),
                _ => format!("/services/{}/ps", &name[3..]),
            };
            self.vm_routes.retain(|(key, _)| *key != format!("GET {path}"));
            self.vm_routes.push((format!("GET {path}"), MockReply::text(200, body)));
            let table: Vec<(&str, MockReply)> = self.vm_routes.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            self.vm_ops.routes(&table);
        } else {
            self.ch_replies.insert(name.to_string(), body.to_string());
            let mut rules = vec![];
            for (source, user) in USERS {
                for (kind, includes) in KINDS {
                    if let Some(body) = self.ch_replies.get(&format!("{kind}-{source}")) {
                        rules.push(Rule { user: Some(user.into()), includes: includes.iter().map(|s| s.to_string()).collect(), body: body.clone(), ..Default::default() });
                    }
                }
            }
            self.clickhouse.rules(rules);
        }
    }

    fn healthy(&mut self) {
        self.reply("health", &answers::health("abc1234").to_string());
        let backend = [("leetcode-backend", "running", "Up 2 hours", "ns/leetcode-backend:sha-abc1234"), ("leetcode-backend-tailscale", "running", "Up 2 hours", "tailscale/tailscale:v1")];
        self.reply("ps-leetcode-backend", &answers::ps("leetcode-backend", &backend).to_string());
        self.reply("ps-remote-chat-agent", &answers::ps("remote-chat-agent", &[]).to_string());
        let capture = [("leetcode-capture", "exited", "Exited (1) 5 minutes ago", "ns/leetcode-auth-cookie-capture:sha-abc1234")];
        self.reply("ps-leetcode-auth-cookie-capture", &answers::ps("leetcode-auth-cookie-capture", &capture).to_string());
        for src in ["backend", "mobile"] {
            let rows = if src == "backend" { 42 } else { 7 };
            self.reply(&format!("doctor-{src}"), &format!("newest\trow_count\n2026-10-06T09:59:00.123456Z\t{rows}\n"));
            self.reply(&format!("errcount-{src}"), &format!("n\n{}\n", if src == "backend" { 3 } else { 0 }));
            self.reply(&format!("errsamples-{src}"), if src == "backend" { "message\nboom one\nboom two\n" } else { "message\n" });
        }
        self.reply("lines-backend", &format!("dt\tlevel\tmodule\tmessage\n2026-10-06T09:59:00.000000Z\tinfo\tserver\tlistening on 8787\n2026-10-06T09:58:00.000000Z\terror\t\t{}\n", "x".repeat(250)));
        self.reply("spans-backend", "dt\tservice\tname\tduration_ms\tstatus_code\n2026-10-06T09:59:00.000000Z\trca\tGET /health\t12.5\t200\n2026-10-06T09:58:00.000000Z\trca\tjob\t3\t0\n");
    }

    fn report(&self, args: &[&str], env: &[(&str, Option<&str>)], cwd: Option<&Path>) -> RunResult {
        let ch_host = format!("127.0.0.1:{}", self.clickhouse.port);
        let mut merged: Vec<(String, String)> = [
            ("WF_TAILNET_SOCKS", self.tailnet.address()),
            ("SSL_CERT_FILE", self.clickhouse.ca.display().to_string()),
            ("VM_OPS_API_AUTH_TOKEN", "seed".to_string()),
            ("VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", self.key.clone()),
            ("CH_BACKEND_HOST", ch_host.clone()),
            ("CH_BACKEND_USER", "reader".to_string()),
            ("CH_BACKEND_PASSWORD", "pw".to_string()),
            ("CH_BACKEND_TABLE", "t.backend_logs".to_string()),
            ("CH_MOBILE_HOST", ch_host),
            ("CH_MOBILE_USER", "mreader".to_string()),
            ("CH_MOBILE_PASSWORD", "mpw".to_string()),
            ("CH_MOBILE_TABLE", "t.mobile_logs".to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        for (key, value) in env {
            merged.retain(|(k, _)| k != key);
            if let Some(v) = value {
                merged.push((key.to_string(), v.to_string()));
            }
        }
        let mut full = vec!["check", "--health"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().bin_dir(&self.bin_dir).cwd(cwd.unwrap_or(self.dir.path())).clean_env();
        for (key, value) in &merged {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }
}

fn untimed(s: &str) -> String {
    let ts = Regex::new(r"(?m)^ts=\d+").unwrap().replace_all(s, "ts=T").into_owned();
    Regex::new(r"(?m)^Generated: .*$").unwrap().replace_all(&ts, "Generated: T").into_owned()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

#[test]
fn the_health_report_renders_every_section_from_the_vm_and_betterstack_and_degrades_instead_of_failing() {
    let mut w = World::new();
    w.healthy();

    // Given every dependency answers, Then each section of the markdown snapshot is rendered
    let r = w.report(&[], &[], None);
    assert_eq!(r.exit_code, 0);

    // Given a quiet source whose newest rows BetterStack archived, Then liveness and the 24 h error count ask the archive too
    let asked: Vec<String> = w.clickhouse.requests().into_iter().map(|q| q.body).collect();
    for needle in ["max(dt)", "count() AS n", "AS message"] {
        let queries: Vec<&String> = asked.iter().filter(|b| b.contains(needle) && b.contains("t.mobile_logs")).collect();
        assert!(!queries.is_empty() && queries.iter().all(|b| b.contains("s3Cluster(primary, t.mobile_s3) WHERE _row_type = 1")), "{needle}: {queries:?}");
    }
    let expected = [
        "ts=T level=info source=logs.doctor msg=\"source live\" source=backend rows=42 newest=2026-10-06T09:59:00.123456Z window_min=30".to_string(),
        "ts=T level=info source=logs.doctor msg=\"source live\" source=mobile rows=7 newest=2026-10-06T09:59:00.123456Z window_min=30".to_string(),
        "# Service health report".into(),
        "".into(),
        "Generated: T".into(),
        "".into(),
        "## vm-ops-api".into(),
        format!("Reachable — host `main`, server time 2026-10-06T10:00:00.000Z, running `sha-abc1234` (server.js sha `{}`)", answers::SERVER_SCRIPT_SHA),
        "".into(),
        "## Deployed services".into(),
        "**leetcode-backend**".into(),
        "  - `leetcode-backend` running — Up 2 hours (ns/leetcode-backend:sha-abc1234)".into(),
        "  - `leetcode-backend-tailscale` running — Up 2 hours (tailscale/tailscale:v1)".into(),
        "**remote-chat-agent** — no containers reported".into(),
        "**leetcode-auth-cookie-capture**".into(),
        "  - `leetcode-capture` exited — Exited (1) 5 minutes ago (ns/leetcode-auth-cookie-capture:sha-abc1234)".into(),
        "".into(),
        "## BetterStack logging".into(),
        "Overall: HEALTHY (both sources receiving)".into(),
        "  - backend: live — 42 rows in the last window, newest 2026-10-06T09:59:00.123456Z".into(),
        "  - mobile: live — 7 rows in the last window, newest 2026-10-06T09:59:00.123456Z".into(),
        "".into(),
        "## Recent errors (last 24h)".into(),
        "  - backend: 3 error-level rows".into(),
        "      - boom one".into(),
        "      - boom two".into(),
        "  - mobile: 0 error-level rows".into(),
        "".into(),
        "## Recent log lines".into(),
        "  - [2026-10-06T09:59:00.000000Z] info (server): listening on 8787".into(),
        format!("  - [2026-10-06T09:58:00.000000Z] error (): {}", "x".repeat(200)),
        "".into(),
        "## Recent traces (spans)".into(),
        "  - [2026-10-06T09:59:00.000000Z] rca — GET /health (12.5ms, HTTP 200)".into(),
        "  - [2026-10-06T09:58:00.000000Z] rca — job (3ms)".into(),
        "".into(),
        "## e2e / runbook tiers".into(),
        "Not run by this command (session-driven against real Telegram + real PRs — see AGENTS.md test priority). Pass --e2e-note to record the last live result.".into(),
        "".into(),
    ];
    assert_eq!(untimed(&r.stdout), expected.join("\n"));

    // Given --json, Then stdout holds only the bare report object, the progress on stderr
    let r = w.report(&["--json"], &[], None);
    let out: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(out.as_object().unwrap().keys().collect::<Vec<_>>(), ["generatedAt", "vmOps", "services", "logs", "recentErrors", "recentLogLines", "recentSpans"]);
    assert_eq!(out["vmOps"], json!({ "reachable": true, "host": "main", "time": "2026-10-06T10:00:00.000Z", "serverScriptSha": answers::SERVER_SCRIPT_SHA, "imageTag": "sha-abc1234" }));
    assert_eq!(out["logs"]["healthy"], true);
    for (i, (source, rows)) in [("backend", 42.0), ("mobile", 7.0)].into_iter().enumerate() {
        let entry = &out["logs"]["sources"][i];
        assert_eq!((entry["source"].as_str().unwrap(), entry["newestRowIso"].as_str().unwrap(), entry["live"].clone(), num(&entry["rowCount"])), (source, "2026-10-06T09:59:00.123456Z", json!(true), rows));
    }
    let errors = out["recentErrors"].as_array().unwrap();
    assert_eq!((errors[0]["source"].as_str().unwrap(), num(&errors[0]["windowHours"]), num(&errors[0]["errorCount"]), errors[0]["sampleMessages"].clone()), ("backend", 24.0, 3.0, json!(["boom one", "boom two"])));
    assert_eq!((errors[1]["source"].as_str().unwrap(), num(&errors[1]["errorCount"]), errors[1]["sampleMessages"].clone()), ("mobile", 0.0, json!([])));
    let spans = out["recentSpans"].as_array().unwrap();
    assert_eq!((spans[0]["dt"].as_str().unwrap(), spans[0]["service"].as_str().unwrap(), spans[0]["name"].as_str().unwrap(), num(&spans[0]["durationMs"]), num(&spans[0]["statusCode"])), ("2026-10-06T09:59:00.000000Z", "rca", "GET /health", 12.5, 200.0));
    assert!(spans[1]["name"] == "job" && num(&spans[1]["durationMs"]) == 3.0 && spans[1].get("statusCode").is_none());
    assert_eq!(out["services"].as_array().unwrap().iter().map(|x| format!("{}:{}", x["app"].as_str().unwrap(), x["reachable"])).collect::<Vec<_>>(), ["leetcode-backend:true", "remote-chat-agent:true", "leetcode-auth-cookie-capture:true"]);
    assert!(r.stderr.contains("source=logs.doctor msg=\"source live\""));

    // Given --check-secrets-drift and no vault, Then the drift is clean
    assert!(w.report(&["--check-secrets-drift"], &[], Some(&repo_root())).stdout.contains("\nSecrets registry drift: clean\n"));

    // Given a source with no rows, a ClickHouse error body, or unresolved settings, Then that source says so
    w.reply("doctor-mobile", "newest\trow_count\n1970-01-01T00:00:00.000000Z\t0\n");
    let out = w.report(&[], &[], None).stdout;
    assert!(out.contains("Overall: UNHEALTHY") && out.contains("  - mobile: NOT LIVE — 0 rows in the last window\n") && Regex::new("source NOT live\" source=mobile rows=0 newest=none window_min=30").unwrap().is_match(&out), "{out}");
    w.reply("doctor-backend", "Code: 516. DB::Exception: Authentication failed");
    assert!(w.report(&[], &[], None).stdout.contains("  - backend: NOT LIVE — 0 rows in the last window (error: BetterStack query failed: Code: 516. DB::Exception: Authentication failed)"));
    w.healthy();
    let out = w.report(&[], &[("CH_MOBILE_HOST", None), ("CH_MOBILE_TABLE", Some("PLACEHOLDER_FILL_ME"))], None).stdout;
    assert!(out.contains("credentials unresolved: BetterStack config for source \"mobile\"") && out.contains("  - mobile: could not query (BetterStack config for source \"mobile\""), "{out}");

    // Given nothing is reachable, Then every section degrades instead of failing
    let down = w.down.address();
    let r = w.report(&[], &[("WF_TAILNET_SOCKS", Some(&down)), ("CH_BACKEND_HOST", Some("127.0.0.1:1")), ("CH_MOBILE_HOST", Some("127.0.0.1:1"))], None);
    assert_eq!(r.exit_code, 0);
    let out = untimed(&r.stdout);
    for part in ["## vm-ops-api\nUNREACHABLE — ", "Overall: UNHEALTHY", "## Recent log lines\nNo recent log activity.", "## Recent traces (spans)\nNo recent trace activity."] {
        assert!(out.contains(part), "{part}: {out}");
    }
    for re in [r"\*\*leetcode-backend\*\* — UNREACHABLE \(", r"  - backend: NOT LIVE — 0 rows in the last window \(error: ", r"  - backend: could not query \(", "level=info source=logs.doctor msg=\"source NOT live\""] {
        assert!(Regex::new(re).unwrap().is_match(&out), "{re}: {out}");
    }

    // Given --e2e-note and --out, Then the note is recorded and the report also written to the file
    let file = w.dir.path().join("report.md").display().to_string();
    let r = w.report(&["--e2e-note", "live e2e green at 10:00", "--out", &file], &[], None);
    assert!(Regex::new(r"## e2e / runbook tiers\nlive e2e green at 10:00\n$").unwrap().is_match(&r.stdout));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), r.stdout[r.stdout.find("# Service health report").unwrap()..]);
    assert!(r.stderr.contains(&format!("\nWritten to {file}\n")));

    // Given --publish and the ORDS settings, Then the report is posted with basic auth and stdout is its JSON
    let url = format!("http://127.0.0.1:{}/ords/health/health_reports/", w.ords.port);
    w.ords.routes(&[("POST /ords/health/health_reports/", MockReply::json(201, json!({})))]);
    let r = w.report(&["--publish"], &[("HEALTH_REPORTS_ORDS_BASE_URL", Some(&url)), ("HEALTH_REPORTS_ORDS_PASSWORD", Some("ordspw"))], None);
    assert!(r.exit_code == 0 && serde_json::from_str::<Value>(&r.stdout).unwrap().get("generatedAt").is_some() && r.stderr.contains(&format!("Published to {url}")), "{}", r.stderr);
    let requests = w.ords.requests();
    assert_eq!((requests.len(), requests[0].method.as_str()), (1, "POST"));
    assert_eq!((requests[0].headers["authorization"].clone(), requests[0].headers["content-type"].as_str()), (format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("health_rpt:ordspw")), "application/json"));
    let body: Value = serde_json::from_str(&requests[0].body).unwrap();
    assert!(Regex::new(r"^\d{4}-\d\d-\d\dT").unwrap().is_match(body["generated_at"].as_str().unwrap()));
    assert_eq!(serde_json::from_str::<Value>(body["report_json"].as_str().unwrap()).unwrap().as_object().unwrap().keys().next().unwrap(), "generatedAt");
    // and without the settings it fails naming them; a non-2xx answer fails with the status
    let r = w.report(&["--publish"], &[], None);
    assert!(r.exit_code == 1 && r.stderr.contains("Cannot publish to health-reports ORDS: missing HEALTH_REPORTS_ORDS_BASE_URL, HEALTH_REPORTS_ORDS_PASSWORD."), "{}", r.stderr);
    w.ords.routes(&[("POST /x/", MockReply::json(500, json!({ "message": "boom" })))]);
    let bad = format!("http://127.0.0.1:{}/x/", w.ords.port);
    let r = w.report(&["--publish"], &[("HEALTH_REPORTS_ORDS_BASE_URL", Some(&bad)), ("HEALTH_REPORTS_ORDS_PASSWORD", Some("ordspw"))], None);
    assert!(r.exit_code == 1 && r.stderr.contains("ORDS publish failed (HTTP 500)"), "{}", r.stderr);

    // Given --run-tests in a workspace with a package that has test:unit, Then its unit tier is run and parsed
    let repo = w.dir.path().join("repo");
    std::fs::create_dir_all(repo.join("libraries/a")).unwrap();
    std::fs::write(repo.join("pnpm-workspace.yaml"), "packages:\n  - 'libraries/*'\n").unwrap();
    std::fs::write(repo.join("libraries/a/package.json"), json!({ "name": "@lib/a", "scripts": { "test:unit": "vitest run" } }).to_string()).unwrap();
    assert!(Command::new("git").args(["init", "-q"]).current_dir(&repo).status().unwrap().success());
    create_sh_stub(&w.bin_dir.join("pnpm"), "echo \" Test Files  1 passed (1)\"; echo \"      Tests  3 passed (3)\"; exit 0");
    create_sh_stub(&w.bin_dir.join("npx"), "exit 1");
    let report: Value = serde_json::from_str(&w.report(&["--json", "--run-tests"], &[], Some(&repo)).stdout).unwrap();
    let unit = &report["tests"].as_array().unwrap().iter().find(|t| t["pkgName"] == "@lib/a").unwrap()["tiers"][0];
    assert_eq!((unit["tier"].as_str(), num(&unit["filesPassed"]), num(&unit["filesTotal"]), num(&unit["testsPassed"]), num(&unit["testsTotal"]), unit["ranOk"].clone()), (Some("unit"), 1.0, 1.0, 3.0, 3.0, json!(true)));
    let md = w.report(&["--run-tests"], &[], Some(&repo)).stdout;
    assert!(md.contains("**@lib/a**\n  - unit: **PASS** — 3/3 tests, 1/1 files") && md.contains("## Fast test tiers (unit / integration / contract — run live by this command)"), "{md}");

    // When the unit targets are listed, Then only packages with a non-empty test:unit appear, sorted per scanned directory, from any depth
    let root = tempfile::Builder::new().prefix("wf-flow-health-targets-").tempdir().unwrap();
    let ws = root.path().canonicalize().unwrap().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("pnpm-workspace.yaml"), "packages: []\n").unwrap();
    let targets = |cwd: &Path| run_wf(&["check", "--health", "--list-unit-targets"], &RunOpts::default().cwd(cwd));
    assert_eq!((targets(&ws).stdout.as_str(), targets(&ws).exit_code), ("[]\n", 0));
    let pkg = |rel: &str, text: String| {
        std::fs::create_dir_all(ws.join(rel)).unwrap();
        std::fs::write(ws.join(rel).join("package.json"), text).unwrap();
    };
    pkg("backend/zeta", json!({ "name": "@app/zeta", "scripts": { "test:unit": "vitest" } }).to_string());
    pkg("backend/alpha", json!({ "name": "@app/alpha", "scripts": { "test:unit": "vitest" } }).to_string());
    pkg("backend/none", json!({ "name": "@app/none", "scripts": { "test": "vitest" } }).to_string());
    pkg("backend/empty", json!({ "name": "@app/empty", "scripts": { "test:unit": "" } }).to_string());
    pkg("clis/tool", json!({ "scripts": { "test:unit": "x" } }).to_string());
    pkg("libraries/broken", "{not json".to_string());
    pkg("tooling/t", json!({ "name": "@t/t", "scripts": { "test:unit": "x" } }).to_string());
    pkg("frontend/ignored", json!({ "name": "ignored", "scripts": { "test:unit": "x" } }).to_string());
    std::fs::create_dir_all(ws.join("deep/er")).unwrap();
    let dir = |rel: &str| ws.join(rel).display().to_string();
    let listed = json!([
        { "pkgName": "@app/alpha", "dir": dir("backend/alpha"), "script": "test:unit" },
        { "pkgName": "@app/zeta", "dir": dir("backend/zeta"), "script": "test:unit" },
        { "pkgName": "clis/tool", "dir": dir("clis/tool"), "script": "test:unit" },
        { "pkgName": "@t/t", "dir": dir("tooling/t"), "script": "test:unit" },
    ]);
    let r = targets(&ws);
    assert_eq!(serde_json::from_str::<Value>(&r.stdout).unwrap(), listed);
    assert!(r.stdout.starts_with("[\n  {\n    \"pkgName\": \"@app/alpha\","));
    assert_eq!(serde_json::from_str::<Value>(&targets(&ws.join("deep/er")).stdout).unwrap(), listed);
}
