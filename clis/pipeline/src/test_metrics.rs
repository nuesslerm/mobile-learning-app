//! The test_runs row of a published run (`wf test publish`): one row per test-harness run (flow tests, `wf android test` and `wf browser test` in the
//! browser or on Android, device runs, integration suites) in HEALTH_RPT.TEST_RUNS, the ORDS auto-REST table. Posting is best effort: it logs
//! one line when it cannot post and never changes a run's exit code.
use wf_kit::logfmt;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::time::Duration;

const URL_KEY: &str = "TEST_RUNS_ORDS_BASE_URL";
const PASSWORD_KEY: &str = "HEALTH_REPORTS_ORDS_PASSWORD";
const SOURCE: &str = "test-metrics";

const COLUMNS: [&str; 27] = [
    "run_id", "ts", "harness", "suite_or_runbook", "job_id", "sha", "metro_commit", "verdict", "exit_code", "duration_s", "attempt", "retry_reason",
    "device_or_screen", "platform", "ran_on", "steps", "screenshots", "recordings", "goldens_total", "goldens_failed", "warnings", "debug_hits",
    "tests_total", "tests_failed", "report_path", "timing_json", "note",
];

/// Byte widths of the VARCHAR2 columns; longer text is cut to fit instead of failing the insert.
const WIDTHS: [(&str, usize); 13] = [
    ("run_id", 128), ("harness", 16), ("suite_or_runbook", 512), ("job_id", 64), ("sha", 64), ("metro_commit", 64), ("verdict", 8), ("retry_reason", 1000),
    ("device_or_screen", 128), ("platform", 32), ("ran_on", 128), ("report_path", 1024), ("note", 4000),
];

/* ---------- rows ---------- */

fn empty_row() -> Map<String, Value> {
    COLUMNS.iter().map(|c| (c.to_string(), Value::Null)).collect()
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

fn count(v: &Value) -> Value {
    v.as_array().map_or(Value::Null, |a| json!(a.len()))
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Cuts `s` to at most `bytes` bytes on a character boundary.
fn fit(s: &str, bytes: usize) -> String {
    if s.len() <= bytes {
        return s.to_string();
    }
    let mut end = bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn finish(mut row: Map<String, Value>) -> Map<String, Value> {
    for (col, width) in WIDTHS {
        if let Some(Value::String(s)) = row.get(col) {
            let cut = fit(s, width);
            row.insert(col.to_string(), json!(cut));
        }
    }
    // ORDS auto-REST binds an omitted column to NULL, not its default, so the NOT NULL columns always travel with the row.
    if row["ts"].is_null() {
        row.insert("ts".into(), json!(rca_core::time::iso(wf_kit::time::now_ms())));
    }
    if row["attempt"].is_null() {
        row.insert("attempt".into(), json!(1));
    }
    row
}

/// A harness run's report.json (the harness's `run_report`, schema 1).
/// A run without timing lines posts NULL: the text `null` fails the column's IS JSON check (ORDS 400).
fn timing_text(timing: &Value) -> Value {
    if timing.is_null() {
        Value::Null
    } else {
        // the timeline is for the report page; the row keeps the numbers charts read
        let mut t = timing.clone();
        if let Some(o) = t.as_object_mut() {
            o.remove("timeline");
        }
        json!(t.to_string())
    }
}

/// `integration` is a service's integration suite (backend/vm-ops-api/integration), its record in the same shape.
pub(crate) fn row_from_run_report(r: &Value, ran_on: &str) -> Result<Map<String, Value>, String> {
    let platform = text(&r["platform"]).unwrap_or_default();
    if !matches!(platform.as_str(), "browser" | "android" | "integration") {
        return Err(format!("not a test harness platform: '{platform}' (browser, android, integration)"));
    }
    let mut row = empty_row();
    let started = r["startedMs"].as_i64();
    let key = format!("{}|{}|{}", text(&r["recordDir"]).unwrap_or_default(), started.unwrap_or_default(), text(&r["script"]).unwrap_or_default());
    row.insert("run_id".into(), json!(format!("run-{:016x}", fnv(&key))));
    let ts = text(&r["finished"]).or_else(|| r["finishedMs"].as_i64().map(rca_core::time::iso));
    row.insert("ts".into(), json!(ts));
    // the table's harness check allows flow, browser, android and integration: the platform names the harness of every run posted here
    row.insert("harness".into(), json!(platform.as_str()));
    row.insert("platform".into(), json!(platform));
    row.insert("suite_or_runbook".into(), json!(text(&r["script"])));
    row.insert("sha".into(), json!(text(&r["sha"])));
    let verdict = text(&r["verdict"]).filter(|v| matches!(v.as_str(), "pass" | "fail" | "error")).unwrap_or_else(|| "error".into());
    row.insert("verdict".into(), json!(verdict));
    row.insert("exit_code".into(), r["exitCode"].clone());
    row.insert("duration_s".into(), r["durationS"].clone());
    let device = [text(&r["device"]), text(&r["screen"])].into_iter().flatten().collect::<Vec<_>>().join(" ");
    row.insert("device_or_screen".into(), if device.is_empty() { Value::Null } else { json!(device) });
    row.insert("ran_on".into(), json!(ran_on));
    for (col, key) in [("steps", "steps"), ("screenshots", "screenshots"), ("recordings", "recordings"), ("goldens_total", "goldens"), ("warnings", "warnings"), ("debug_hits", "debugHits")] {
        row.insert(col.into(), count(&r[key]));
    }
    if let Some(goldens) = r["goldens"].as_array() {
        row.insert("goldens_failed".into(), json!(goldens.iter().filter(|g| g["ok"] != true).count()));
    }
    row.insert("report_path".into(), json!(text(&r["recordDir"]).map(|d| format!("{d}/report.html"))));
    if r["timing"].is_object() {
        row.insert("timing_json".into(), timing_text(&r["timing"]));
    }
    let note = text(&r["message"]).or_else(|| r["notes"].as_array().and_then(|n| n.iter().find_map(text)));
    row.insert("note".into(), json!(note));
    Ok(finish(row))
}

/// A flow-test-harness report.json (`flow-report/1`, frontend/libraries/flow-test-harness/src/vitest/report.ts).
pub(crate) fn row_from_flow_report(r: &Value, ran_on: &str) -> Map<String, Value> {
    let mut row = empty_row();
    let root = text(&r["root"]).unwrap_or_default();
    row.insert("run_id".into(), json!(format!("flow-{:016x}", fnv(&format!("{root}|{}", text(&r["startedAt"]).unwrap_or_default())))));
    row.insert("ts".into(), json!(text(&r["finishedAt"])));
    row.insert("harness".into(), json!("flow"));
    row.insert("platform".into(), json!("jsdom"));
    let tests = r["tests"].as_array().map_or(&[][..], Vec::as_slice);
    let mut files: Vec<&str> = tests.iter().filter_map(|t| t["file"].as_str()).collect();
    files.sort_unstable();
    files.dedup();
    let suite = match files.as_slice() {
        [one] => Some(one.to_string()),
        _ => Path::new(&root).file_name().map(|n| n.to_string_lossy().to_string()),
    };
    row.insert("suite_or_runbook".into(), json!(suite));
    let verdict = match r["verdict"].as_str() {
        Some("passed") => "pass",
        Some("failed") => "fail",
        _ => "error",
    };
    row.insert("verdict".into(), json!(verdict));
    row.insert("duration_s".into(), r["durationMs"].as_f64().map_or(Value::Null, |ms| json!((ms / 100.0).round() / 10.0)));
    row.insert("ran_on".into(), json!(ran_on));
    row.insert("tests_total".into(), r["summary"]["total"].clone());
    row.insert("tests_failed".into(), r["summary"]["failed"].clone());
    if r["tests"].is_array() {
        let sum = |key: &str| tests.iter().map(|t| t[key].as_array().map_or(0, Vec::len)).sum::<usize>();
        row.insert("goldens_total".into(), json!(sum("goldens")));
        row.insert("debug_hits".into(), json!(sum("stops")));
        row.insert("warnings".into(), json!(sum("warnings") + r["warnings"].as_array().map_or(0, Vec::len)));
    }
    let html = text(&r["paths"]["html"]).map(|h| if root.is_empty() { h } else { format!("{}/{h}", root.trim_end_matches('/')) });
    row.insert("report_path".into(), json!(html));
    let first_error = r["errors"].as_array().into_iter().flatten().chain(tests.iter().flat_map(|t| t["errors"].as_array().into_iter().flatten())).find_map(|e| text(&e["message"]));
    row.insert("note".into(), json!(first_error));
    finish(row)
}

/// An e2e run's result.json (`wf test run`), with the run's own report.json when the runner wrote one.
pub(crate) fn row_from_job_result(result: &Value, run: Option<&Value>, attempt: u64, retry_reason: Option<&str>) -> Map<String, Value> {
    let ran_on = text(&result["ranOn"]).unwrap_or_default();
    let mut row = run.and_then(|r| row_from_run_report(r, &ran_on).ok()).unwrap_or_else(empty_row);
    let id = text(&result["id"]).unwrap_or_default();
    let platform = text(&result["platform"]).unwrap_or_else(|| "android".into());
    row.insert("run_id".into(), json!(format!("job-{id}")));
    row.insert("job_id".into(), json!(id));
    row.insert("harness".into(), json!(if platform == "browser" { "browser" } else { "android" }));
    row.insert("platform".into(), json!(platform));
    row.insert("suite_or_runbook".into(), json!(text(&result["runbook"])));
    row.insert("sha".into(), json!(text(&result["sha"])));
    row.insert("metro_commit".into(), json!(text(&result["metroCommit"])));
    row.insert("verdict".into(), json!(text(&result["verdict"]).filter(|v| matches!(v.as_str(), "pass" | "fail" | "error")).unwrap_or_else(|| "error".into())));
    row.insert("exit_code".into(), result["exitCode"].clone());
    row.insert("duration_s".into(), result["durationS"].clone());
    row.insert("attempt".into(), json!(attempt.max(1)));
    row.insert("retry_reason".into(), json!(retry_reason));
    row.insert("ran_on".into(), json!(ran_on));
    if let Some(ms) = result["finishedMs"].as_i64().filter(|m| *m > 0) {
        row.insert("ts".into(), json!(rca_core::time::iso(ms)));
    }
    if row["recordings"].is_null() {
        row.insert("recordings".into(), count(&result["recording"]));
    }
    if let Some(page) = text(&result["report"]) {
        row.insert("report_path".into(), json!(page));
    }
    if result["timing"].is_object() {
        row.insert("timing_json".into(), timing_text(&result["timing"]));
    }
    if let Some(note) = text(&result["note"]) {
        row.insert("note".into(), json!(note));
    }
    finish(row)
}

/* ---------- posting ---------- */

fn is_placeholder(v: &str) -> bool {
    let t = v.trim();
    t.is_empty() || t.starts_with("PLACEHOLDER_FILL_ME") || (t.starts_with('<') && t.ends_with('>'))
}

struct Ords {
    url: String,
    user: String,
    password: String,
}

fn ords_from(read: impl Fn(&str) -> Option<String>) -> Result<Ords, String> {
    let (url, password) = (read(URL_KEY).filter(|v| !is_placeholder(v)), read(PASSWORD_KEY).filter(|v| !is_placeholder(v)));
    let missing: Vec<&str> = [(URL_KEY, url.is_none()), (PASSWORD_KEY, password.is_none())].into_iter().filter(|(_, m)| *m).map(|(k, _)| k).collect();
    match (url, password) {
        (Some(url), Some(password)) => {
            let user = std::env::var("HEALTH_REPORTS_ORDS_USER").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "health_rpt".into());
            Ok(Ords { url, user, password })
        }
        _ => Err(format!("{} unset", missing.join(" and "))),
    }
}

/// The environment first; a key absent from it comes from the vault, once. An empty URL (the black-box harness sets it so)
/// disables the writer without a vault call, which costs about 2 s per run.
fn writer_ords() -> Result<Ords, String> {
    let disabled = std::env::var(URL_KEY).is_ok_and(|v| v.is_empty());
    let absent = !disabled && [URL_KEY, PASSWORD_KEY].into_iter().any(|k| std::env::var_os(k).is_none());
    let vault = if absent { secrets::load_values(&[URL_KEY, PASSWORD_KEY]) } else { Default::default() };
    ords_from(|k| match std::env::var(k) {
        Ok(v) => Some(v),
        Err(_) => vault.get(k).cloned().flatten(),
    })
}

/// Inserts the row; a run already recorded (RUN_ID is unique) is updated in place, so a republished run keeps one row.
fn post(cfg: &Ords, row: &Map<String, Value>) -> Result<(), String> {
    let body = Value::Object(row.clone());
    let response = cli_http::Request::post(&cfg.url).basic_auth(&cfg.user, &cfg.password).json(&body).timeout(Duration::from_secs(10)).send()?;
    if response.ok() {
        return Ok(());
    }
    let text = response.text();
    if response.status == 400 && text.contains("RUN_ID") && text.contains("ORA-00001") {
        return update(cfg, row).map_err(|e| format!("insert hit the unique run_id, and the update failed: {e}"));
    }
    Err(format!("HTTP {}: {}", response.status, text.chars().take(300).collect::<String>()))
}

/// ORDS auto-REST refuses a PUT on the identity row (500), so the earlier row goes (DELETE by `id`) and the new one is inserted.
fn update(cfg: &Ords, row: &Map<String, Value>) -> Result<(), String> {
    let run_id = row["run_id"].as_str().unwrap_or_default();
    let base = cfg.url.trim_end_matches('/');
    let q = json!({ "run_id": { "$eq": run_id } }).to_string();
    let found = cli_http::Request::get(&format!("{base}/?q={}", cli_http::encode_uri_component(&q))).basic_auth(&cfg.user, &cfg.password).timeout(Duration::from_secs(10)).send()?;
    if !found.ok() {
        return Err(format!("HTTP {} looking up {run_id}", found.status));
    }
    let id = serde_json::from_str::<Value>(&found.text()).ok().and_then(|v| v["items"][0]["id"].as_u64()).ok_or_else(|| format!("no row with run_id {run_id} to update"))?;
    let gone = cli_http::Request::new("DELETE", &format!("{base}/{id}")).basic_auth(&cfg.user, &cfg.password).timeout(Duration::from_secs(10)).send()?;
    if !gone.ok() {
        return Err(format!("HTTP {} deleting row {id}: {}", gone.status, gone.text().chars().take(200).collect::<String>()));
    }
    let again = cli_http::Request::post(&cfg.url).basic_auth(&cfg.user, &cfg.password).json(&Value::Object(row.clone())).timeout(Duration::from_secs(10)).send()?;
    if again.ok() {
        Ok(())
    } else {
        Err(format!("HTTP {}: {}", again.status, again.text().chars().take(300).collect::<String>()))
    }
}

/// The rows matching an ORDS filter (`q`, with `$orderby`), at most `limit`: what `wf test results` reads. Same credentials as
/// the writer; an empty URL (the black-box default) means the table is not read.
pub fn query_rows(q: &Value, limit: usize) -> Result<Vec<Value>, String> {
    let cfg = writer_ords()?;
    let base = cfg.url.trim_end_matches('/');
    let url = format!("{base}/?q={}&limit={limit}", cli_http::encode_uri_component(&q.to_string()));
    let r = cli_http::Request::get(&url).basic_auth(&cfg.user, &cfg.password).timeout(Duration::from_secs(20)).send()?;
    if !r.ok() {
        return Err(format!("HTTP {}: {}", r.status, r.text().chars().take(300).collect::<String>()));
    }
    let v: Value = serde_json::from_str(&r.text()).map_err(|e| format!("the table answered no JSON: {e}"))?;
    Ok(v["items"].as_array().cloned().unwrap_or_default())
}

/// Posts the row or logs one line saying why not; the outcome as JSON (`posted`, `runId`, `reason`).
pub(crate) fn post_best_effort(row: &Map<String, Value>) -> Value {
    let run_id = row["run_id"].as_str().unwrap_or_default().to_string();
    let outcome = writer_ords().map_err(|e| ("disabled", e)).and_then(|cfg| post(&cfg, row).map_err(|e| ("failed", e)));
    match outcome {
        Ok(()) => {
            logfmt::mcp("info", SOURCE, "test run row posted", &[("run_id", run_id.clone())]);
            json!({ "posted": true, "runId": run_id })
        }
        Err((reason, error)) => {
            logfmt::mcp("warn", SOURCE, "test run row not posted", &[("reason", reason.to_string()), ("run_id", run_id.clone()), ("error", error.clone())]);
            json!({ "posted": false, "runId": run_id, "reason": reason, "error": error })
        }
    }
}

/* ---------- verbs ---------- */
