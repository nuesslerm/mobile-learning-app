//! `wf check --health [--json] [--publish] [--run-tests] [--run-expensive-e2e] [--e2e-note <text>] [--out <path>]
//! [--check-secrets-drift]`: the snapshot of the deployed services (vm-ops-api), BetterStack liveness, recent errors, log
//! lines and spans, as markdown (or the bare report object with `--json`), optionally with every test tier run live and
//! the report POSTed to the health-reports ORDS endpoint.
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_kit::js::js_num;
use wf_kit::logfmt;
use crate::vm_logs::{load_config, query, rows_source};
use wf_vmops::vmops::{encode_uri_component, request_json, Opts};
use serde_json::{json, Map, Value};
use std::process::{Command, Stdio};

const VM_APPS: [&str; 3] = ["leetcode-backend", "remote-chat-agent", "leetcode-auth-cookie-capture"];
const SOURCES: [&str; 2] = ["backend", "mobile"];

/// A JSON scalar as a template literal prints it; a missing key prints `undefined`.
fn text(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), js_num),
        other => other.to_string(),
    }
}

fn field(v: &Value, key: &str) -> String {
    v.get(key).map_or_else(|| "undefined".to_string(), text)
}

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn col(row: &[String], i: usize, default: &str) -> String {
    row.get(i).cloned().unwrap_or_else(|| default.to_string())
}

/// A JS number: whole values without a fraction.
fn num(n: f64) -> Value {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 9e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn check_source(source: &'static str) -> Value {
    let dead = |error: String| json!({ "source": source, "newestRowIso": null, "rowCount": 0, "live": false, "error": error });
    let cfg = match load_config(source) {
        Ok(c) => c,
        Err(f) => return dead(format!("credentials unresolved: {}", f.0)),
    };
    let sql = format!(
        "SELECT formatDateTime(max(dt), '%Y-%m-%dT%H:%i:%S.%fZ') AS newest, count() AS row_count FROM {} FORMAT TabSeparatedWithNames",
        rows_source(&cfg.table, "dt", "dt > now() - INTERVAL 30 MINUTE")
    );
    let rows = match query(&cfg, &sql) {
        Ok(r) => r,
        Err(f) => return dead(f.0),
    };
    let first = rows.first().cloned().unwrap_or_default();
    let count = col(&first, 1, "0").trim().parse::<f64>().ok().map_or(0, |n| n as u64);
    json!({ "source": source, "newestRowIso": if count > 0 { json!(col(&first, 0, "")) } else { Value::Null }, "rowCount": count, "live": count > 0 })
}

fn recent_errors(source: &'static str) -> Value {
    let run = || -> Result<(u64, Vec<String>), String> {
        let cfg = load_config(source).map_err(|f| f.0)?;
        let rows = rows_source(&cfg.table, "dt, raw", "dt > now() - INTERVAL 24 HOUR AND JSONExtractString(raw, 'level') = 'error'");
        let samples = query(&cfg, &format!("SELECT JSONExtractString(raw, 'message') AS message FROM {rows} ORDER BY dt DESC LIMIT 5 FORMAT TabSeparatedWithNames")).map_err(|f| f.0)?;
        let count = query(&cfg, &format!("SELECT count() AS n FROM {rows} FORMAT TabSeparatedWithNames")).map_err(|f| f.0)?;
        let n = count.first().map_or("0".to_string(), |r| col(r, 0, "0")).trim().parse::<f64>().ok().map_or(0, |n| n as u64);
        Ok((n, samples.iter().map(|r| col(r, 0, "")).filter(|m| !m.is_empty()).collect()))
    };
    match run() {
        Ok((count, samples)) => json!({ "source": source, "windowHours": 24, "errorCount": count, "sampleMessages": samples }),
        Err(e) => json!({ "source": source, "windowHours": 24, "errorCount": 0, "sampleMessages": [], "error": e }),
    }
}

fn window_sql(select: &str, table: &str) -> String {
    let rows = rows_source(table, "dt, raw", "dt > now() - INTERVAL 60 MINUTE");
    format!("SELECT {select} FROM {rows} ORDER BY dt DESC LIMIT 20 FORMAT TabSeparatedWithNames")
}

fn recent_log_lines() -> Vec<Value> {
    let run = || -> Result<Vec<Value>, ()> {
        let cfg = load_config("backend").map_err(|_| ())?;
        let sql = window_sql(
            "formatDateTime(dt, '%Y-%m-%dT%H:%i:%S.%fZ') AS dt, JSONExtractString(raw, 'level') AS level, JSONExtractString(raw, 'module') AS module, JSONExtractString(raw, 'message') AS message",
            &cfg.table,
        );
        let rows = query(&cfg, &sql).map_err(|_| ())?;
        Ok(rows.iter().map(|r| json!({ "dt": col(r, 0, ""), "level": col(r, 1, "?"), "module": col(r, 2, "?"), "message": col(r, 3, "") })).collect())
    };
    run().unwrap_or_default()
}

fn recent_spans() -> Vec<Value> {
    let run = || -> Result<Vec<Value>, ()> {
        let cfg = load_config("backend").map_err(|_| ())?;
        let table = match cfg.table.strip_suffix("_logs") {
            Some(base) => format!("{base}_spans"),
            None => format!("{}_spans", cfg.table),
        };
        let sql = window_sql(
            "formatDateTime(dt, '%Y-%m-%dT%H:%i:%S.%fZ') AS dt, JSONExtractString(raw, 'resource', 'attributes', 'service.name') AS service, JSONExtractString(raw, 'span', 'name') AS name, round(JSONExtractFloat(raw, 'span', 'duration') * 1000, 2) AS duration_ms, JSONExtractInt(raw, 'span', 'attributes', 'http.status_code') AS status_code",
            &table,
        );
        let rows = query(&cfg, &sql).map_err(|_| ())?;
        Ok(rows
            .iter()
            .map(|r| {
                let duration = col(r, 3, "0").trim().parse::<f64>().ok().filter(|n| *n != 0.0 && n.is_finite()).unwrap_or(0.0);
                let mut m = Map::new();
                m.insert("dt".into(), json!(col(r, 0, "")));
                m.insert("service".into(), json!(col(r, 1, "?")));
                m.insert("name".into(), json!(col(r, 2, "?")));
                m.insert("durationMs".into(), num(duration));
                if let Some(c) = r.get(4).filter(|s| !s.is_empty() && *s != "0").and_then(|s| s.trim().parse::<i64>().ok()) {
                    m.insert("statusCode".into(), json!(c));
                }
                Value::Object(m)
            })
            .collect())
    };
    run().unwrap_or_default()
}

fn service_entry(app: &str) -> Value {
    match request_json(&format!("/services/{}/ps", encode_uri_component(app)), &Opts::get()) {
        Err(e) => json!({ "app": app, "reachable": false, "error": e }),
        Ok(v) => {
            let mut m = Map::new();
            m.insert("app".into(), json!(app));
            m.insert("reachable".into(), json!(true));
            if let Some(c) = v.get("containers") {
                m.insert("containers".into(), c.clone());
            }
            Value::Object(m)
        }
    }
}

fn vm_ops_entry() -> Value {
    match request_json("/health", &Opts::get()) {
        Err(e) => json!({ "reachable": false, "error": e }),
        Ok(h) => {
            let mut m = Map::new();
            m.insert("reachable".into(), json!(true));
            m.insert("host".into(), h.get("host").cloned().unwrap_or(Value::Null));
            m.insert("time".into(), h.get("time").cloned().unwrap_or(Value::Null));
            for key in ["serverScriptSha", "imageTag"] {
                if let Some(v) = h.get(key).filter(|v| !v.is_null()) {
                    m.insert(key.into(), v.clone());
                }
            }
            Value::Object(m)
        }
    }
}

fn secrets_drift() -> Value {
    let out = std::env::current_exe().ok().and_then(|exe| Command::new(exe).args(["secret", "doctor", "--backend", "bitwarden", "--if-available"]).stdin(Stdio::null()).output().ok());
    match out {
        Some(o) => {
            let all = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            json!({ "checked": true, "clean": o.status.success(), "output": all.trim() })
        }
        None => json!({ "checked": true, "clean": false, "output": "" }),
    }
}

/* ---------- test tiers ---------- */

fn tail(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_in(program: &str, args: &[String], cwd: &std::path::Path, env: &[(&str, String)], timeout: std::time::Duration) -> Ran {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let Ok(mut child) = cmd.spawn() else { return Ran { code: 1, stdout: String::new(), stderr: String::new() } };
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let t_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(p) = out.as_mut() {
            let _ = std::io::Read::read_to_end(p, &mut b);
        }
        b
    });
    let t_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(p) = err.as_mut() {
            let _ = std::io::Read::read_to_end(p, &mut b);
        }
        b
    });
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(1),
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break 1;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Err(_) => break 1,
        }
    };
    Ran { code: status, stdout: String::from_utf8_lossy(&t_out.join().unwrap_or_default()).into_owned(), stderr: String::from_utf8_lossy(&t_err.join().unwrap_or_default()).into_owned() }
}

/// vitest's summary lines: `Test Files  1 failed | 3 passed (4)`.
fn summary_line(output: &str, label: &str) -> Option<(i64, i64)> {
    let re = regex::Regex::new(&format!(r"{label}\s+([^\n]*)\((\d+)\)")).ok()?;
    let m = re.captures(output)?;
    let total: i64 = m[2].parse().ok()?;
    let passed = regex::Regex::new(r"(\d+) passed").ok()?.captures(&m[1]).and_then(|p| p[1].parse().ok()).unwrap_or(0);
    Some((passed, total))
}

fn unit_tier(pkg: &Value, dir: &std::path::Path) -> Value {
    let r = run_in("pnpm", &["run".into(), "test:unit".into()], dir, &[], std::time::Duration::from_secs(300));
    let combined = format!("{}\n{}", r.stdout, r.stderr);
    let mut m = Map::new();
    m.insert("tier".into(), json!("unit"));
    let _ = pkg;
    if let Some((p, t)) = summary_line(&combined, "Test Files") {
        m.insert("filesPassed".into(), json!(p));
        m.insert("filesTotal".into(), json!(t));
    }
    if let Some((p, t)) = summary_line(&combined, "Tests") {
        m.insert("testsPassed".into(), json!(p));
        m.insert("testsTotal".into(), json!(t));
    }
    // Key order of the TS spread: files before tests.
    let mut ordered = Map::new();
    for k in ["tier", "filesPassed", "filesTotal", "testsPassed", "testsTotal"] {
        if let Some(v) = m.get(k) {
            ordered.insert(k.into(), v.clone());
        }
    }
    ordered.insert("ranOk".into(), json!(r.code == 0));
    ordered.insert("raw".into(), json!(tail(&combined, 2000)));
    Value::Object(ordered)
}

fn tmp_json(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.json", std::process::id()))
}

fn tier_from_cases(tier: &str, ran: &Ran, parsed: Option<(Vec<Value>, Map<String, Value>)>) -> Value {
    let mut m = Map::new();
    m.insert("tier".into(), json!(tier));
    let (cases, counts) = parsed.unwrap_or_default();
    for k in ["filesPassed", "filesTotal", "testsPassed", "testsTotal"] {
        if let Some(v) = counts.get(k) {
            m.insert(k.into(), v.clone());
        }
    }
    m.insert("ranOk".into(), json!(ran.code == 0));
    m.insert("raw".into(), json!(tail(&format!("{}\n{}", ran.stdout, ran.stderr), 2000)));
    m.insert("cases".into(), Value::Array(cases));
    Value::Object(m)
}

fn parse_vitest_json(text: &str) -> Option<(Vec<Value>, Map<String, Value>)> {
    let data: Value = serde_json::from_str(text).ok()?;
    let mut cases = vec![];
    for file in data["testResults"].as_array().into_iter().flatten() {
        for a in file["assertionResults"].as_array().into_iter().flatten() {
            let status = match a["status"].as_str() {
                Some("passed") => "passed",
                Some("failed") => "failed",
                _ => "skipped",
            };
            let mut suite: Vec<String> = vec![file["name"].as_str().unwrap_or("").to_string()];
            suite.extend(a["ancestorTitles"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string));
            let mut c = Map::new();
            c.insert("suite".into(), json!(suite.join(" > ")));
            c.insert("name".into(), json!(a["title"].as_str().unwrap_or("")));
            c.insert("status".into(), json!(status));
            if let Some(d) = a["duration"].as_f64() {
                c.insert("durationMs".into(), num(d));
            }
            if let Some(f) = a["failureMessages"][0].as_str().filter(|m| !m.is_empty()) {
                c.insert("failureMessage".into(), json!(f));
            }
            cases.push(Value::Object(c));
        }
    }
    let mut counts = Map::new();
    for (key, out) in [("numTotalTestSuites", "filesTotal"), ("numPassedTestSuites", "filesPassed"), ("numTotalTests", "testsTotal"), ("numPassedTests", "testsPassed")] {
        if let Some(n) = data[key].as_i64() {
            counts.insert(out.into(), json!(n));
        }
    }
    Some((cases, counts))
}

fn vitest_tier(tier: &str, args: &[&str], cwd: &std::path::Path, timeout_s: u64) -> Value {
    let out_file = tmp_json("vitest-json");
    let mut a: Vec<String> = vec!["vitest".into(), "run".into()];
    a.extend(args.iter().map(|s| s.to_string()));
    a.extend(["--reporter=json".into(), format!("--outputFile={}", out_file.display())]);
    let ran = run_in("npx", &a, cwd, &[], std::time::Duration::from_secs(timeout_s));
    let text = std::fs::read_to_string(&out_file).ok();
    let _ = std::fs::remove_file(&out_file);
    tier_from_cases(tier, &ran, text.and_then(|t| parse_vitest_json(&t)))
}

/// Every tier run live: the `test:unit` script of each workspace package that has one, then the fixed non-unit tiers whose
/// package is in this checkout. Nothing here is "expensive" today, so `--run-expensive-e2e` adds no tier.
fn run_tests(root: &std::path::Path) -> Value {
    let mut by_pkg: Vec<(String, Vec<Value>)> = vec![];
    let mut push = |pkg: &str, tier: Value| match by_pkg.iter_mut().find(|(p, _)| p == pkg) {
        Some((_, tiers)) => tiers.push(tier),
        None => by_pkg.push((pkg.to_string(), vec![tier])),
    };
    for t in crate::report_health::targets(root, &["backend", "clis", "features", "libraries", "tooling"]).unwrap_or_default() {
        let dir = std::path::PathBuf::from(t["dir"].as_str().unwrap_or(""));
        push(t["pkgName"].as_str().unwrap_or(""), unit_tier(&t, &dir));
    }
    let at = |rel: &str| root.join(rel);
    let vitest: [(&str, &str, &str, &[&str], u64); 1] =
        [("@app/remote-chat-agent", "e2e", "backend/remote-chat-agent", &["tests/e2e", "--config", "vitest.e2e.config.ts"], 300)];
    for (pkg, tier, rel, args, timeout) in vitest {
        if at(rel).is_dir() {
            push(pkg, vitest_tier(tier, args, &at(rel), timeout));
        }
    }
    Value::Array(by_pkg.into_iter().map(|(pkg_name, tiers)| json!({ "pkgName": pkg_name, "tiers": tiers })).collect())
}

/* ---------- the report ---------- */

struct Opts2 {
    run_tests: bool,
    e2e_note: Option<String>,
    drift: bool,
    to_stderr: bool,
}

fn build_report(o: &Opts2) -> Value {
    let say = |line: String| {
        if o.to_stderr {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    };
    let vm_ops = vm_ops_entry();
    let services: Vec<Value> = VM_APPS.iter().map(|a| service_entry(a)).collect();
    let sources: Vec<Value> = SOURCES
        .iter()
        .map(|s| {
            let h = check_source(s);
            let mut fields = vec![
                ("source", h["source"].as_str().unwrap_or("").to_string()),
                ("rows", h["rowCount"].to_string()),
                ("newest", h["newestRowIso"].as_str().unwrap_or("none").to_string()),
                ("window_min", "30".to_string()),
            ];
            if let Some(e) = h["error"].as_str() {
                fields.push(("err", e.to_string()));
            }
            say(logfmt::line("info", "logs.doctor", if h["live"] == json!(true) { "source live" } else { "source NOT live" }, &fields));
            h
        })
        .collect();
    let healthy = !sources.is_empty() && sources.iter().all(|s| s["live"] == json!(true));
    let errors: Vec<Value> = SOURCES.iter().map(|s| recent_errors(s)).collect();
    let log_lines = recent_log_lines();
    let spans = recent_spans();
    let tests = o.run_tests.then(|| run_tests(&crate::report_health::repo_root()));
    let mut m = Map::new();
    m.insert("generatedAt".into(), json!(rca_core::time::iso(wf_kit::time::now_ms())));
    m.insert("vmOps".into(), vm_ops);
    m.insert("services".into(), Value::Array(services));
    m.insert("logs".into(), json!({ "healthy": healthy, "sources": sources }));
    m.insert("recentErrors".into(), Value::Array(errors));
    m.insert("recentLogLines".into(), Value::Array(log_lines));
    m.insert("recentSpans".into(), Value::Array(spans));
    if let Some(t) = tests {
        m.insert("tests".into(), t);
    }
    if let Some(n) = o.e2e_note.as_ref().filter(|n| !n.is_empty()) {
        m.insert("e2eNote".into(), json!(n));
    }
    if o.drift {
        m.insert("secretsDrift".into(), secrets_drift());
    }
    Value::Object(m)
}

fn s(v: &Value) -> String {
    text(v)
}

fn format_container_block(entry: &Value) -> String {
    let app = s(&entry["app"]);
    if entry["reachable"] != json!(true) {
        return format!("**{app}** — UNREACHABLE ({})", s(&entry["error"]));
    }
    match entry["containers"].as_array() {
        Some(c) if !c.is_empty() => {
            let lines: Vec<String> = c.iter().map(|c| format!("  - `{}` {} — {} ({})", field(c, "name"), field(c, "state"), field(c, "status"), field(c, "image"))).collect();
            format!("**{app}**\n{}", lines.join("\n"))
        }
        _ => format!("**{app}** — no containers reported"),
    }
}

fn format_test_tier(t: &Value) -> String {
    let tier = s(&t["tier"]);
    if let Some(reason) = t["unavailableReason"].as_str() {
        return format!("  - {tier}: **UNAVAILABLE** — {reason}");
    }
    let status = if t["ranOk"] == json!(true) { "PASS" } else { "FAIL" };
    let counts = if t["testsPassed"].is_null() {
        "(summary not parsed — see raw log)".to_string()
    } else {
        format!("{}/{} tests, {}/{} files", s(&t["testsPassed"]), s(&t["testsTotal"]), s(&t["filesPassed"]), s(&t["filesTotal"]))
    };
    let mut lines = vec![format!("  - {tier}: **{status}** — {counts}")];
    for c in t["cases"].as_array().into_iter().flatten().filter(|c| c["status"] == json!("failed")) {
        let msg = c["failureMessage"].as_str().filter(|m| !m.is_empty()).map_or(String::new(), |m| format!(" — {}", cut(m, 200)));
        lines.push(format!("      - FAILED: {} > {}{msg}", s(&c["suite"]), s(&c["name"])));
    }
    lines.join("\n")
}

fn format_markdown(r: &Value) -> String {
    let mut l: Vec<String> = vec!["# Service health report".into(), String::new(), format!("Generated: {}", s(&r["generatedAt"])), String::new(), "## vm-ops-api".into()];
    let vm = &r["vmOps"];
    l.push(if vm["reachable"] == json!(true) {
        let tag = vm["imageTag"].as_str().filter(|v| !v.is_empty()).map_or(String::new(), |v| format!(", running `{v}`"));
        let sha = vm["serverScriptSha"].as_str().filter(|v| !v.is_empty()).map_or(String::new(), |v| format!(" (server.js sha `{v}`)"));
        format!("Reachable — host `{}`, server time {}{tag}{sha}", s(&vm["host"]), s(&vm["time"]))
    } else {
        format!("UNREACHABLE — {}", s(&vm["error"]))
    });
    let drift = &r["secretsDrift"];
    if !drift.is_null() {
        l.push(if drift["checked"] == json!(true) {
            if drift["clean"] == json!(true) { "Secrets registry drift: clean".to_string() } else { format!("Secrets registry drift: DRIFT DETECTED — {}", cut(drift["output"].as_str().unwrap_or(""), 500)) }
        } else {
            "Secrets registry drift: not checked".to_string()
        });
    }
    l.extend([String::new(), "## Deployed services".into()]);
    l.extend(r["services"].as_array().into_iter().flatten().map(format_container_block));
    l.extend([String::new(), "## BetterStack logging".into()]);
    l.push(format!("Overall: {}", if r["logs"]["healthy"] == json!(true) { "HEALTHY (both sources receiving)" } else { "UNHEALTHY" }));
    for x in r["logs"]["sources"].as_array().into_iter().flatten() {
        l.push(format!(
            "  - {}: {} — {} rows in the last window{}{}",
            s(&x["source"]),
            if x["live"] == json!(true) { "live" } else { "NOT LIVE" },
            s(&x["rowCount"]),
            x["newestRowIso"].as_str().map_or(String::new(), |n| format!(", newest {n}")),
            x["error"].as_str().map_or(String::new(), |e| format!(" (error: {e})"))
        ));
    }
    l.extend([String::new(), "## Recent errors (last 24h)".into()]);
    for e in r["recentErrors"].as_array().into_iter().flatten() {
        if let Some(err) = e["error"].as_str() {
            l.push(format!("  - {}: could not query ({err})", s(&e["source"])));
            continue;
        }
        l.push(format!("  - {}: {} error-level rows", s(&e["source"]), s(&e["errorCount"])));
        l.extend(e["sampleMessages"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|m| format!("      - {}", cut(m, 200))));
    }
    l.push(String::new());
    if let Some(tests) = r["tests"].as_array() {
        l.push("## Fast test tiers (unit / integration / contract — run live by this command)".into());
        for pkg in tests {
            l.push(format!("**{}**", s(&pkg["pkgName"])));
            l.extend(pkg["tiers"].as_array().into_iter().flatten().map(format_test_tier));
        }
        l.push(String::new());
    }
    l.push("## Recent log lines".into());
    let lines = r["recentLogLines"].as_array().cloned().unwrap_or_default();
    if lines.is_empty() {
        l.push("No recent log activity.".into());
    } else {
        l.extend(lines.iter().map(|x| format!("  - [{}] {} ({}): {}", s(&x["dt"]), s(&x["level"]), s(&x["module"]), cut(x["message"].as_str().unwrap_or(""), 200))));
    }
    l.extend([String::new(), "## Recent traces (spans)".into()]);
    let spans = r["recentSpans"].as_array().cloned().unwrap_or_default();
    if spans.is_empty() {
        l.push("No recent trace activity.".into());
    } else {
        l.extend(spans.iter().map(|x| format!("  - [{}] {} — {} ({}ms{})", s(&x["dt"]), s(&x["service"]), s(&x["name"]), s(&x["durationMs"]), x["statusCode"].as_i64().map_or(String::new(), |c| format!(", HTTP {c}")))));
    }
    l.extend([String::new(), "## e2e / runbook tiers".into()]);
    l.push(r["e2eNote"].as_str().map_or_else(
        || "Not run by this command (session-driven against real Telegram + real PRs — see AGENTS.md test priority). Pass --e2e-note to record the last live result.".to_string(),
        str::to_string,
    ));
    l.join("\n")
}

/* ---------- publishing ---------- */

fn is_placeholder(v: &str) -> bool {
    let t = v.trim();
    t == "PLACEHOLDER_FILL_ME" || t.starts_with("PLACEHOLDER_FILL_ME_") || (t.starts_with('<') && t.ends_with('>') && t.len() > 2 && !t[1..t.len() - 1].contains(['<', '>']))
}

struct Ords {
    url: String,
    user: String,
    password: String,
}

fn load_ords() -> Result<Ords, String> {
    let loaded = secrets::load_values(&["HEALTH_REPORTS_ORDS_BASE_URL", "HEALTH_REPORTS_ORDS_PASSWORD"]);
    let (mut missing, mut unfilled) = (vec![], vec![]);
    let mut read = |name: &str| -> String {
        match loaded.get(name).cloned().flatten() {
            None => {
                missing.push(name.to_string());
                String::new()
            }
            Some(v) if is_placeholder(&v) => {
                unfilled.push(name.to_string());
                String::new()
            }
            Some(v) => v,
        }
    };
    let url = read("HEALTH_REPORTS_ORDS_BASE_URL");
    let password = read("HEALTH_REPORTS_ORDS_PASSWORD");
    let user = std::env::var("HEALTH_REPORTS_ORDS_USER").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "health_rpt".into());
    if !unfilled.is_empty() {
        return Err(format!("health-reports ORDS config still holds PLACEHOLDER values: {}.\nThe Autonomous DB (infra/oracle-cloud/autonomous-db.tf) must be applied and the one-time HEALTH_RPT schema setup run first. Then: terraform output health_reports_ords_base_url, and printf %s '<url>/health/health_reports/' | wf secret update HEALTH_REPORTS_ORDS_BASE_URL --value-stdin --apply", unfilled.join(", ")));
    }
    if !missing.is_empty() {
        return Err(format!("Cannot publish to health-reports ORDS: missing {}. Run `wf secret set <KEY>` for each, or check the vault registry.", missing.join(", ")));
    }
    Ok(Ords { url, user, password })
}

fn publish(cfg: &Ords, report: &Value) -> Result<(), String> {
    let body = json!({ "generated_at": rca_core::time::iso(wf_kit::time::now_ms()), "report_json": report.to_string() });
    let response = cli_http::Request::post(&cfg.url)
        .basic_auth(&cfg.user, &cfg.password)
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .map_err(|e| format!("ORDS publish failed: {e}"))?;
    if response.ok() {
        Ok(())
    } else {
        Err(format!("ORDS publish failed (HTTP {}): {}", response.status, cut(&response.text(), 500)))
    }
}

/// What `report-health` takes when it runs the report.
pub const SPEC: Spec = Spec {
    values: &["e2e-note", "out"],
    flags: &["check-secrets-drift", "run-tests", "run-expensive-e2e", "publish"],
    positional: Positional::None,
    ..Spec::NONE
};

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let json_flag = p.flag("json");
    let publish_flag = p.flags.iter().any(|f| f == "publish");
    let wants_json = json_flag || publish_flag;
    let report = build_report(&Opts2 {
        run_tests: p.flags.iter().any(|f| f == "run-tests"),
        e2e_note: p.opts.get("e2e-note").cloned(),
        drift: p.flags.iter().any(|f| f == "check-secrets-drift"),
        to_stderr: wants_json,
    });
    let rendered = if wants_json { serde_json::to_string_pretty(&report).unwrap_or_default() } else { format_markdown(&report) };
    println!("{rendered}");
    if let Some(out) = p.opts.get("out").filter(|o| !o.is_empty()) {
        if let Err(e) = std::fs::write(out, format!("{rendered}\n")) {
            wf_kit::wf_err!("wf: {e}");
            return Some(1);
        }
        eprint!("\nWritten to {out}\n");
    }
    if publish_flag {
        let outcome = load_ords().and_then(|cfg| publish(&cfg, &report).map(|()| cfg.url));
        match outcome {
            Ok(url) => eprint!("\nPublished to {url}\n"),
            Err(e) => {
                wf_kit::wf_err!("wf: {e}");
                return Some(1);
            }
        }
    }
    Some(0)
}
