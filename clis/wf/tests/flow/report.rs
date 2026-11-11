//! What `wf` reports outward: one structured line to the BetterStack OTLP ingest, and the repository's knowledge (gaps and learnings, and
//! the Gemini context docs). The ingest is an HTTPS fake with a throwaway CA; ORDS and the Google Docs and Drive APIs are fakes; pnpm,
//! npx and gcloud are stubs (the health report is in `clis/deploy`).
use crate::support::{create_sh_stub, now_ms, run_wf, MockHttps, MockReply, MockServer, RunOpts};
use base64::Engine;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

const TOKEN: &str = "otlp-token-12345678";
const NO_DEST: &str = "no OTLP destination: BETTERSTACK_OTLP_TOKEN and BETTERSTACK_OTLP_HOST must both be set, the host a bare host name, and SECRETS_LOG_BETTERSTACK not false";

type Otlp = (HashMap<String, String>, String, Value, Vec<(String, String)>);

/// The one OTLP record of the `n`-th request: its resource attributes, scope, record and string attributes.
fn otlp(ingest: &MockHttps, n: usize) -> Otlp {
    let parsed: Value = serde_json::from_str(&ingest.requests()[n].body).unwrap();
    let rl = &parsed["resourceLogs"][0];
    let strings = |list: &Value| -> Vec<(String, String)> {
        list.as_array().unwrap().iter().map(|a| (a["key"].as_str().unwrap().to_string(), a["value"]["stringValue"].as_str().unwrap_or_default().to_string())).collect()
    };
    let record = rl["scopeLogs"][0]["logRecords"][0].clone();
    let attrs = strings(&record["attributes"]);
    (strings(&rl["resource"]["attributes"]).into_iter().collect(), rl["scopeLogs"][0]["scope"]["name"].as_str().unwrap().to_string(), record, attrs)
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// The console line `wf log emit` echoed on `out`, parsed.
fn echoed(out: &str) -> Value {
    out.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v.get("msg").is_some()).unwrap_or_else(|| panic!("no echoed line in {out:?}"))
}

/// The `{"sent":...}` line, the last on stdout.
fn sent(out: &str) -> Value {
    serde_json::from_str(out.lines().last().unwrap_or_default()).unwrap_or_else(|_| panic!("no result line in {out:?}"))
}

#[test]
fn log_emit_prints_and_sends_one_scrubbed_otlp_line_and_fails_loudly_when_it_cannot_send() {
    let ingest = MockHttps::start();
    let emit = |args: &[&str], env: &[(&str, &str)]| {
        ingest.clear();
        let mut full = vec!["log", "emit"];
        full.extend_from_slice(args);
        let (ca, host) = (ingest.ca.display().to_string(), format!("127.0.0.1:{}", ingest.port));
        let mut all = vec![("BETTERSTACK_OTLP_TOKEN", TOKEN.to_string()), ("BETTERSTACK_OTLP_HOST", host), ("SSL_CERT_FILE", ca), ("SECRETS_LOG_BETTERSTACK", String::new())];
        for (key, value) in env {
            all.retain(|(k, _)| k != key);
            all.push((key, value.to_string()));
        }
        let mut opts = RunOpts::default().clean_env().timeout(Duration::from_secs(90));
        for (key, value) in &all {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    };

    // Given no destination, the kill switch or an unusable host, Then nothing is sent and it fails with the reason
    for env in [vec![("BETTERSTACK_OTLP_TOKEN", ""), ("BETTERSTACK_OTLP_HOST", "")], vec![("SECRETS_LOG_BETTERSTACK", "false")], vec![("BETTERSTACK_OTLP_HOST", "<betterstack-otlp-host>.betterstackdata.com")]] {
        let r = emit(&["--service", "s", "--message", "m"], &env);
        assert_eq!((r.exit_code, sent(&r.stdout)), (1, json!({ "sent": false, "reason": NO_DEST })), "{env:?}");
        assert!(r.stderr.contains(&format!("[logger] emit failed: {NO_DEST}")), "{env:?}: {}", r.stderr);
        assert!(ingest.requests().is_empty(), "{env:?}");
    }

    // Given a destination, When a line is emitted, Then one OTLP record is posted with the bearer token and the line is echoed
    let r = emit(&["--service", "svc", "--message", "evt.name", "--host-label", "light", "--attr", "a=b", "--attr", "k=v=w", "--attr", "a=c"], &[]);
    assert_eq!((r.exit_code, sent(&r.stdout)), (0, json!({ "sent": true })));
    let line = echoed(&r.stdout);
    assert_eq!((line["level"].as_str(), line["msg"].as_str(), line["source"].as_str(), line["hostLabel"].as_str(), line["a"].as_str(), line["k"].as_str()), (Some("info"), Some("evt.name"), Some("svc"), Some("light"), Some("c"), Some("v=w")));
    assert!(line["ts"].as_u64().is_some_and(|ts| ts.abs_diff(now_ms() as u64) < 60_000) && line.get("filename").is_none(), "{line}");
    let request = &ingest.requests()[0];
    assert_eq!((request.method.as_str(), request.url.as_str(), request.headers["authorization"].clone(), request.headers["content-type"].as_str()), ("POST", "/v1/logs", format!("Bearer {TOKEN}"), "application/json"));
    let (resource, scope, record, attrs) = otlp(&ingest, 0);
    assert_eq!((resource["service.name"].as_str(), resource["os.type"].as_str(), scope.as_str(), record["body"]["stringValue"].as_str()), ("svc", "linux", "svc", Some("evt.name")));
    // and a shell script's line claims no version and no call site of wf's own
    assert!(!resource.contains_key("service.version") && !attrs.iter().any(|(k, _)| k.starts_with("code.")), "{resource:?} {attrs:?}");
    assert_eq!((record["severityNumber"].as_f64(), record["severityText"].as_str()), (Some(9.0), Some("INFO")));
    assert!(Regex::new(r"^\d{19}$").unwrap().is_match(record["timeUnixNano"].as_str().unwrap()) && record["observedTimeUnixNano"] == record["timeUnixNano"]);
    assert_eq!(attrs, pairs(&[("source", "svc"), ("hostLabel", "light"), ("a", "c"), ("k", "v=w")]));

    // Given each level, Then the severity matches, and warnings and errors echo on stderr
    for (level, number, text) in [("debug", 5.0, "DEBUG"), ("info", 9.0, "INFO"), ("warn", 13.0, "WARN"), ("error", 17.0, "ERROR")] {
        let r = emit(&["--service", "s", "--message", "m", "--level", level], &[]);
        assert_eq!(sent(&r.stdout), json!({ "sent": true }), "{level}");
        let (_, _, record, _) = otlp(&ingest, 0);
        assert_eq!((record["severityNumber"].as_f64(), record["severityText"].as_str()), (Some(number), Some(text)), "{level}");
        let line = echoed(if level == "warn" || level == "error" { &r.stderr } else { &r.stdout });
        assert_eq!((line["level"].as_str(), line["msg"].as_str()), (Some(level), Some("m")));
    }
    // and an invalid level fails before anything is sent
    let r = emit(&["--service", "s", "--message", "m", "--level", "loud"], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("[logger] invalid --level \"loud\", must be one of: info, debug, warn, error") && ingest.requests().is_empty(), "{}", r.stderr);

    // Given secrets and personal data, Then they are redacted in the record and the echo, a secret-named variable's value wherever it appears
    let message = format!("leak {TOKEN} and mail me@example.com");
    let r = emit(&["--service", "s", "--message", &message, "--attr", "password=hunter2", "--attr", "note=a.b@c.org eyJhbGciOi.eyJzdWIiOiIx.c2lnbmF0dXJl card 4111 1111 1111 1111 ok", "--attr", "API_TOKEN=abc", "--attr", "x=before shh-this-is-secret after"], &[("MY_SERVICE_SECRET", "shh-this-is-secret")]);
    assert_eq!(r.exit_code, 0);
    let (_, _, record, attrs) = otlp(&ingest, 0);
    assert_eq!(record["body"]["stringValue"], "leak [REDACTED] and mail [REDACTED]");
    let expected = pairs(&[("source", "s"), ("password", "[REDACTED]"), ("note", "[REDACTED] [REDACTED] card [REDACTED] ok"), ("API_TOKEN", "[REDACTED]"), ("x", "before [REDACTED] after")]);
    assert_eq!(attrs, expected);
    let line = echoed(&r.stdout);
    assert_eq!(line["msg"], record["body"]["stringValue"]);
    assert!(expected.iter().all(|(k, v)| line[k] == *v), "{line}");

    // Given an ingest that rejects the line, Then it fails with the status
    ingest.status(401);
    let r = emit(&["--service", "s", "--message", "m"], &[]);
    assert_eq!((r.exit_code, sent(&r.stdout)), (1, json!({ "sent": false, "reason": "export failed: HTTP 401" })));
    assert!(r.stderr.contains("[logger] emit failed: export failed: HTTP 401"), "{}", r.stderr);
    ingest.status(200);
}

#[test]
fn every_command_posts_one_anonymised_usage_row_and_says_so_when_it_cannot() {
    let ords = MockServer::start(None);
    let home = tempfile::Builder::new().prefix("wf-flow-usage-").tempdir().unwrap();
    let url = format!("http://127.0.0.1:{}/ords/health/wf_usage/", ords.port);
    let token = "tok-abcdef-1234567890";
    let wf = |args: &[&str], env: &[(&str, &str)]| {
        let mut opts = RunOpts::default()
            .clean_env()
            .env("HOME", &home.path().display().to_string())
            .env("WF_USAGE", "")
            .env("WF_USAGE_ORDS_BASE_URL", &url)
            .env("HEALTH_REPORTS_ORDS_PASSWORD", "ords-pw-12345678")
            .env("MY_API_TOKEN", token);
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(args, &opts)
    };
    // The row is posted by a detached copy of wf, so it lands after the command has returned.
    let posted = |n: usize| -> Vec<Value> {
        let started = std::time::Instant::now();
        while ords.requests().len() < n && started.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(100));
        }
        ords.requests().iter().map(|q| serde_json::from_str(&q.body).unwrap()).collect()
    };
    let failure = home.path().join(".cache/wf/usage.error");
    ords.routes(&[("POST /ords/health/wf_usage/", MockReply::json(201, json!({})))]);

    // Given a command that fails naming a secret value, When it ends, Then one row names the command, never the value
    let r = wf(&["vm", "logs", "--source", token], &[]);
    assert_eq!(r.exit_code, 1);
    let rows = posted(1);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    let keys: BTreeSet<&str> = row.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["command", "duration_ms", "error", "exit_code", "os", "ts", "wf_version"]));
    assert_eq!((row["command"].as_str(), row["exit_code"].as_i64(), row["os"].as_str()), (Some("vm logs"), Some(1), Some("linux")));
    assert_eq!(row["error"], "wf: Unknown --source \"[REDACTED]\". Valid sources: backend, mobile.");
    assert!(!ords.requests()[0].body.contains(token));
    assert_eq!(ords.requests()[0].auth.as_deref(), Some(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("health_rpt:ords-pw-12345678")).as_str()));

    // Given a command that succeeds, Then its row has no error
    ords.clear();
    assert_eq!(wf(&["vm", "logs", "--help"], &[]).exit_code, 0);
    let rows = posted(1);
    assert_eq!((rows[0]["command"].as_str(), rows[0]["exit_code"].as_i64(), rows[0]["error"].is_null()), (Some("vm logs"), Some(0), true));

    // Given the keys only in ~/.config/wf/.env, When a command served by its help ends, Then its row is posted too
    ords.clear();
    let user_env = home.path().join(".config/wf/.env");
    std::fs::create_dir_all(user_env.parent().unwrap()).unwrap();
    std::fs::write(&user_env, format!("WF_USAGE_ORDS_BASE_URL={url}\nHEALTH_REPORTS_ORDS_PASSWORD=ords-pw-12345678\n")).unwrap();
    let r = run_wf(&["vm", "--help"], &RunOpts::default().clean_env().cwd(home.path()).env("HOME", &home.path().display().to_string()).env("WF_USAGE", ""));
    assert!(r.exit_code == 0 && !r.stderr.contains("usage row"), "{}", r.stderr);
    assert_eq!(posted(1)[0]["command"].as_str(), Some("vm"));
    std::fs::remove_file(&user_env).unwrap();

    // Given the keys only in the vault, as on the CI runner (a read token, no env file), When a command ends, Then its row is posted
    ords.clear();
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let listing = json!([
        { "id": "u1", "key": "WF_USAGE_ORDS_BASE_URL", "value": url },
        { "id": "u2", "key": "HEALTH_REPORTS_ORDS_PASSWORD", "value": "ords-pw-12345678" },
    ]);
    create_sh_stub(&bin.join("bws"), &format!("[ \"$1 $2\" = \"secret list\" ] && [ \"$BWS_ACCESS_TOKEN\" = read-token ] && echo '{listing}'"));
    let r = run_wf(
        &["vm", "--help"],
        &RunOpts::default()
            .clean_env()
            .bin_dir(&bin)
            .cwd(home.path())
            .env("HOME", &home.path().display().to_string())
            .env("WF_USAGE", "")
            .env("SECRETS_BACKEND", "bitwarden")
            .env("BWS_ADMIN_ACCESS_TOKEN", "read-token"),
    );
    assert!(r.exit_code == 0 && !r.stderr.contains("usage row") && !r.stderr.contains("[secrets]"), "{}", r.stderr);
    assert_eq!(posted(1)[0]["command"].as_str(), Some("vm"));

    // Given the URL is unset, Then the command says the row was not recorded and nothing is posted
    ords.clear();
    let r = wf(&["vm", "logs", "--help"], &[("WF_USAGE_ORDS_BASE_URL", "")]);
    assert!(r.stderr.contains("wf: usage row not recorded: WF_USAGE_ORDS_BASE_URL unset"), "{}", r.stderr);
    // and with WF_USAGE=off nothing is posted and nothing is said
    let r = wf(&["vm", "logs", "--help"], &[("WF_USAGE", "off")]);
    assert!(!r.stderr.contains("usage row"), "{}", r.stderr);
    std::thread::sleep(Duration::from_millis(500));
    assert!(ords.requests().is_empty());

    // Given ORDS rejects the row, Then the next command says the last row was not recorded, once
    ords.routes(&[("POST /ords/health/wf_usage/", MockReply::json(500, json!({ "code": "ServerError" })))]);
    wf(&["vm", "logs", "--help"], &[]);
    let started = std::time::Instant::now();
    while !failure.exists() && started.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(100));
    }
    ords.routes(&[("POST /ords/health/wf_usage/", MockReply::json(201, json!({})))]);
    let next = wf(&["vm", "logs", "--help"], &[]);
    assert!(next.stderr.contains("wf: the last usage row was not recorded: ORDS answered 500"), "{}", next.stderr);
    assert!(!wf(&["vm", "logs", "--help"], &[]).stderr.contains("the last usage row"));
}
