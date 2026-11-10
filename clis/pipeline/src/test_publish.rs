//! `wf test publish <dir>`: one run of any harness becomes a run bundle in the verification-artifacts bucket, the standard
//! evidence a reporting job picks up: `bundle.json` (schema 1) with every file's public URL, the report (HTML and JSON),
//! screenshots, recordings, the debugger hits, the timing lines and the metrics row, under `test-runs/<runId>/`, listed
//! newest first in `verification/test-runs/index.json`. The same call posts the run's `HEALTH_RPT.TEST_RUNS` row with the bundle's
//! report URL as `report_path`, so the dashboards and the reporter see one record per run. The bundle's `cache` block is what the
//! build-cache shim did since the last publish (`cache_runs`), `null` when no shim ran. Its `traces` list the method-hit traces
//! (`trace.json`) of an integration chain beside its run record, which `wf parity coverage --traces` reads.
use crate::test_metrics;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use wf_bucket::artifacts::{content_type_for, existing_index, par_url, public_base, put_par, TEST_RUNS_PREFIX};
use wf_bucket::cache_runs;

const SCHEMA: u64 = 1;
const INDEX_KEEP: usize = 500;
const FILE_CAP: u64 = 60 * 1024 * 1024;
const BUNDLE_CAP: u64 = 250 * 1024 * 1024;
const KEEP: [&str; 10] = ["html", "json", "jsonl", "png", "jpg", "jpeg", "webm", "mp4", "log", "txt"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// A harness run's test output dir (`wf android test`, `wf browser test`): `report.json` (schema 1) beside the recording and screenshots.
    Run,
    /// A flow reporter dir: `report.json` with `schema: "flow-report/1"`.
    Flow,
    /// A queue job's results dir: `result.json` with the run under `rec/`.
    Job,
}

/// What `dir` holds, from the files that are there.
fn kind_of(dir: &Path) -> Result<Kind, String> {
    if dir.join("result.json").is_file() {
        return Ok(Kind::Job);
    }
    let report = std::fs::read_to_string(dir.join("report.json")).map_err(|_| format!("{}: neither a harness run dir (report.json), a flow report dir, nor a queue job dir (result.json)", dir.display()))?;
    let v: Value = serde_json::from_str(&report).map_err(|e| format!("{}/report.json: {e}", dir.display()))?;
    match &v["schema"] {
        Value::String(s) if s.starts_with("flow-report/") => Ok(Kind::Flow),
        Value::Number(_) => Ok(Kind::Run),
        other => Err(format!("{}/report.json: unknown schema {other}", dir.display())),
    }
}

/// The files of a bundle: every kept type under `dir` (three levels deep), relative paths sorted, oversized ones left out
/// with a note.
fn files_of(dir: &Path) -> (Vec<(String, u64)>, Vec<String>) {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    let mut total = 0u64;
    walk(dir, dir, 0, &mut out);
    out.sort();
    let mut kept = Vec::new();
    for (rel, bytes) in out {
        if bytes > FILE_CAP {
            notes.push(format!("{rel} left out: {bytes} bytes is over the {FILE_CAP} byte file cap"));
            continue;
        }
        if total + bytes > BUNDLE_CAP {
            notes.push(format!("{rel} left out: the bundle is at its {BUNDLE_CAP} byte cap"));
            continue;
        }
        total += bytes;
        kept.push((rel, bytes));
    }
    (kept, notes)
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 3 && p.file_name().is_some_and(|n| n != "node_modules") {
                walk(root, &p, depth + 1, out);
            }
            continue;
        }
        let ext = p.extension().and_then(|x| x.to_str()).map(|x| x.to_ascii_lowercase()).unwrap_or_default();
        if !KEEP.contains(&ext.as_str()) {
            continue;
        }
        let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
        if let Ok(rel) = p.strip_prefix(root) {
            out.push((rel.to_string_lossy().replace('\\', "/"), bytes));
        }
    }
}

/// The metrics row of the run, from the same builders the harness hooks use; its `run_id` names the bundle.
fn row_of(dir: &Path, kind: &Kind, ran_on: &str) -> Result<Map<String, Value>, String> {
    let read = |p: PathBuf| std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display())).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| format!("{}: {e}", p.display())));
    match kind {
        Kind::Run => test_metrics::row_from_run_report(&read(dir.join("report.json"))?, ran_on),
        Kind::Flow => Ok(test_metrics::row_from_flow_report(&read(dir.join("report.json"))?, ran_on)),
        Kind::Job => {
            let result = read(dir.join("result.json"))?;
            let run = read(dir.join("rec/report.json")).ok();
            let attempt = result["attempt"].as_u64().unwrap_or(1);
            Ok(test_metrics::row_from_job_result(&result, run.as_ref(), attempt, result["retryReason"].as_str()))
        }
    }
}

fn pick(files: &[(String, u64)], pred: impl Fn(&str) -> bool) -> Vec<&str> {
    files.iter().map(|(r, _)| r.as_str()).filter(|r| pred(r)).collect()
}

/// The bundle's manifest, before upload: every file with the URL it will have.
fn bundle_json(run_id: &str, kind: &Kind, row: &Map<String, Value>, files: &[(String, u64)], notes: &[String], base: &str, published_at: &str) -> Value {
    let url = |rel: &str| format!("{base}/{rel}");
    let report_html = pick(files, |r| r == "report.html" || r == "rec/report.html" || r == "index.html").first().map(|r| url(r));
    let report_json = pick(files, |r| r == "report.json" || r == "rec/report.json").first().map(|r| url(r));
    let kind_name = match kind {
        Kind::Run => "run",
        Kind::Flow => "flow",
        Kind::Job => "job",
    };
    json!({
        "schema": SCHEMA,
        "runId": run_id,
        "kind": kind_name,
        "harness": row.get("harness").cloned().unwrap_or(Value::Null),
        "suiteOrRunbook": row.get("suite_or_runbook").cloned().unwrap_or(Value::Null),
        "sha": row.get("sha").cloned().unwrap_or(Value::Null),
        "verdict": row.get("verdict").cloned().unwrap_or(Value::Null),
        "durationS": row.get("duration_s").cloned().unwrap_or(Value::Null),
        "publishedAt": published_at,
        "base": base,
        "report": { "html": report_html, "json": report_json },
        "screenshots": pick(files, |r| r.ends_with(".png") || r.ends_with(".jpg") || r.ends_with(".jpeg")).iter().map(|r| url(r)).collect::<Vec<_>>(),
        "recordings": pick(files, |r| r.ends_with(".mp4") || r.ends_with(".webm")).iter().map(|r| url(r)).collect::<Vec<_>>(),
        "debug": pick(files, |r| r.ends_with("debug.jsonl")).first().map(|r| url(r)),
        // a chain run's method-hit traces (`method-trace/1`, `exec_testkit::trace`): which wire calls reached which agent methods and executor tools
        "traces": pick(files, |r| r == "trace.json" || r.ends_with("/trace.json")).iter().map(|r| url(r)).collect::<Vec<_>>(),
        "timing": pick(files, |r| r.ends_with("timing.jsonl")).first().map(|r| url(r)),
        "files": files.iter().map(|(rel, bytes)| json!({ "path": rel, "url": url(rel), "bytes": bytes, "type": content_type_for(rel) })).collect::<Vec<_>>(),
        "notes": notes,
        "metrics": row,
    })
}

/// The index entry a reporter lists runs by.
/// The facts a gate reads without opening the report: the screen (with its density on Android), the speed, the warnings and
/// the timing.
fn add_run_facts(bundle: &mut Value, report: &Value) {
    bundle["args"] = report["args"].clone();
    bundle["screen"] = report["screen"].clone();
    bundle["speed"] = report["speed"].clone();
    bundle["warnings"] = json!(report["warnings"].as_array().map_or(0, Vec::len));
    bundle["timingSummary"] = report["timing"].clone();
}

/// Each recording and the report page, fetched back from where the bundle says it lives.
fn read_back(bundle: &Value) -> Value {
    let mut urls: Vec<String> = bundle["recordings"].as_array().into_iter().flatten().filter_map(|u| u.as_str().map(str::to_string)).collect();
    urls.extend(bundle["report"]["html"].as_str().map(str::to_string));
    let failed: Vec<&String> = urls
        .iter()
        .filter(|u| {
            !cli_http::Request::get(u).header("Range", "bytes=0-1023").timeout(std::time::Duration::from_secs(30)).send().is_ok_and(|r| matches!(r.status, 200 | 206) && !r.body.is_empty())
        })
        .collect();
    json!({ "ok": failed.is_empty() && !bundle["recordings"].as_array().is_none_or(Vec::is_empty), "checked": urls.len(), "failed": failed })
}

/// What an index entry keeps of a run's timing for charts over time: the phases, sleep and Lua time, and the eight most
/// expensive verbs with their p95; the full summary stays in the bundle.
fn compact_timing(t: &Value) -> Value {
    if !t.is_object() {
        return Value::Null;
    }
    let verbs: Vec<Value> = t["byVerb"].as_array().map(|a| a.iter().take(8).map(|v| json!({ "verb": v["verb"], "calls": v["calls"], "s": v["s"], "p95Ms": v["p95Ms"] })).collect()).unwrap_or_default();
    json!({ "deviceS": t["deviceS"], "sleepS": t["sleepS"], "luaS": t["luaS"], "phases": t["phases"], "appStart": t["appStart"], "startupS": t["startupS"], "verbs": verbs })
}

fn index_entry(bundle: &Value) -> Value {
    json!({
        "runId": bundle["runId"], "kind": bundle["kind"], "harness": bundle["harness"], "suiteOrRunbook": bundle["suiteOrRunbook"], "sha": bundle["sha"],
        "verdict": bundle["verdict"], "durationS": bundle["durationS"], "publishedAt": bundle["publishedAt"],
        "bundle": format!("{}/bundle.json", bundle["base"].as_str().unwrap_or("")), "report": bundle["report"]["html"],
        "screenshots": bundle["screenshots"].as_array().map_or(0, Vec::len), "recordings": bundle["recordings"].as_array().map_or(0, Vec::len), "traces": bundle["traces"].as_array().map_or(0, Vec::len),
        "args": bundle["args"], "screen": bundle["screen"], "warnings": bundle["warnings"], "wallS": bundle["timingSummary"]["wallS"],
        "byVerb": bundle["timingSummary"]["byVerb"].as_array().map_or(0, Vec::len), "readBack": bundle["readBack"]["ok"],
        "timing": compact_timing(&bundle["timingSummary"]),
    })
}

/// `index.json` with `entry` first and any earlier entry of the same run gone; at most `INDEX_KEEP` runs.
fn merged_index(existing: Option<&str>, entry: Value) -> Value {
    let mut runs: Vec<Value> = existing
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .and_then(|v| v["runs"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r["runId"] != entry["runId"])
        .collect();
    runs.insert(0, entry);
    runs.truncate(INDEX_KEEP);
    json!({ "schema": SCHEMA, "runs": runs })
}

pub struct Published {
    pub(crate) run_id: String,
    pub base: String,
    pub(crate) bundle: Value,
    pub(crate) files: usize,
    bytes: u64,
    recorded: Value,
}

/// Uploads the run in `dir` and, unless `record` is false, posts its metrics row with the bundle's report as `report_path`.
pub fn publish(dir: &Path, ran_on: &str, record: bool) -> Result<Published, String> {
    let kind = kind_of(dir)?;
    let mut row = row_of(dir, &kind, ran_on)?;
    let run_id = row.get("run_id").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("the run has no run_id")?.to_string();
    let par = par_url()?;
    let (files, notes) = files_of(dir);
    let base = format!("{}/{TEST_RUNS_PREFIX}/{run_id}", public_base());
    let published_at = rca_core::time::iso(now_ms());
    // the row inside the bundle and the posted one both name the bucket's report, never the runner's local path
    if let Some(rel) = files.iter().map(|(r, _)| r.as_str()).find(|r| *r == "report.html" || *r == "rec/report.html" || *r == "index.html") {
        row.insert("report_path".into(), json!(format!("{base}/{rel}")));
    }
    let mut bundle = bundle_json(&run_id, &kind, &row, &files, &notes, &base, &published_at);
    let taken = cache_runs::pending(now_ms());
    bundle["cache"] = cache_runs::block(&taken);
    if let Some(rel) = files.iter().map(|(r, _)| r.as_str()).find(|r| *r == "report.json" || *r == "rec/report.json") {
        if let Some(report) = std::fs::read_to_string(dir.join(rel)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            add_run_facts(&mut bundle, &report);
        }
    }
    let mut bytes = 0u64;
    for (rel, n) in &files {
        let body = std::fs::read(dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        put_par(&format!("{par}{TEST_RUNS_PREFIX}/{run_id}/{rel}"), &body, content_type_for(rel))?;
        bytes += n;
    }
    bundle["readBack"] = read_back(&bundle);
    put_par(&format!("{par}{TEST_RUNS_PREFIX}/{run_id}/bundle.json"), serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())?.as_bytes(), "application/json")?;
    let index = merged_index(existing_index()?.as_deref(), index_entry(&bundle));
    put_par(&format!("{par}{TEST_RUNS_PREFIX}/index.json"), index.to_string().as_bytes(), "application/json")?;
    cache_runs::consume(&taken);
    row.insert("note".into(), json!(format!("bundle {base}/bundle.json{}", row.get("note").and_then(Value::as_str).filter(|n| !n.is_empty()).map(|n| format!("; {n}")).unwrap_or_default())));
    let recorded = if record { test_metrics::post_best_effort(&row) } else { json!({ "posted": false, "reason": "no-record" }) };
    Ok(Published { run_id, base, bundle, files: files.len(), bytes, recorded })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `wf test publish <dir> [--no-record] [--ran-on <host>]`: the JSON summary on stdout, 1 with `error` and `next` on failure.
pub(crate) fn run(dir: &str, record: bool, ran_on: Option<&str>) -> i32 {
    let ran_on = ran_on.map(str::to_string).unwrap_or_else(|| std::env::var("WF_SITE").unwrap_or_else(|_| wf_kit::proc::hostname()));
    match publish(Path::new(dir), &ran_on, record) {
        Ok(p) => {
            println!("{}", json!({ "runId": p.run_id, "base": p.base, "bundle": format!("{}/bundle.json", p.base), "report": p.bundle["report"]["html"], "cache": p.bundle["cache"], "files": p.files, "bytes": p.bytes, "index": format!("{}/{TEST_RUNS_PREFIX}/index.json", public_base()), "recorded": p.recorded }));
            0
        }
        Err(e) => {
            println!("{}", json!({ "error": e, "next": "wf test publish <dir> wants a harness run record dir (report.json), a flow report dir or a queue job dir (result.json), and VERIFICATION_ARTIFACTS_PAR_URL" }));
            1
        }
    }
}
