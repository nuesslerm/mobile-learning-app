//! The hand-off from the `wf` that ran the build-cache shim to the `wf test publish` that publishes the run: the shim leaves its stats
//! here when it stops, and the next publish attaches what finished in the last quarter hour to its bundle as `cache`, once.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const WINDOW_MS: i64 = 15 * 60 * 1000;
const KEEP: usize = 50;

/// Beside the sealed secrets copy: `WF_CACHE_DIR`, else `$XDG_CACHE_HOME/wf`, else `~/.cache/wf`.
fn file() -> Option<PathBuf> {
    let base = std::env::var_os("WF_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(|d| PathBuf::from(d).join("wf")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/wf")))?;
    Some(base.join("build-cache-runs.jsonl"))
}

fn read(file: &Path) -> Vec<Value> {
    std::fs::read_to_string(file).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn write(file: &Path, runs: &[Value]) {
    let text: String = runs.iter().map(|r| format!("{r}\n")).collect();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, text);
}

/// A shim stopped with these stats (`blob_cache::Stats::to_json`). Best effort: a run never fails for want of a place to write.
pub(crate) fn record(stats: &Value) {
    let Some(file) = file() else { return };
    let mut runs = read(&file);
    runs.push(json!({ "finishedMs": wf_kit::time::now_ms(), "stats": stats }));
    write(&file, &runs[runs.len().saturating_sub(KEEP)..]);
}

/// The shim runs that finished in the quarter hour before `now_ms` and were not attached to a bundle yet.
pub fn pending(now_ms: i64) -> Vec<Value> {
    let Some(file) = file() else { return Vec::new() };
    read(&file).into_iter().filter(|r| r["finishedMs"].as_i64().is_some_and(|t| t > now_ms - WINDOW_MS && t <= now_ms)).collect()
}

/// The runs are in a published bundle: the next one does not carry them again.
pub fn consume(taken: &[Value]) {
    let Some(file) = file() else { return };
    if !taken.is_empty() {
        let mut runs = read(&file);
        runs.retain(|r| !taken.contains(r));
        write(&file, &runs);
    }
}

/// The `cache` block of a bundle: the runs' counters added up, `null` when no shim ran.
pub fn block(runs: &[Value]) -> Value {
    if runs.is_empty() {
        return Value::Null;
    }
    let sum = |key: &str| runs.iter().filter_map(|r| r["stats"][key].as_u64()).sum::<u64>();
    json!({
        "runs": runs.len(), "hits": sum("hits"), "misses": sum("misses"), "puts": sum("puts"), "skipped": sum("skipped"), "quotaExceeded": sum("quotaExceeded"),
        "requests": sum("requests"), "bytes": sum("bytes"), "degraded": runs.iter().find_map(|r| r["stats"]["degraded"].as_str()),
        "latencyMs": { "get": latency(runs, "get"), "put": latency(runs, "put"), "exists": latency(runs, "exists") },
    })
}

/// One operation's timings over the runs: how many calls, their total and mean, and the slowest.
fn latency(runs: &[Value], op: &str) -> Value {
    let field = |r: &Value, key: &str| r["stats"]["latencyMs"][op][key].as_f64().unwrap_or(0.0);
    let n: u64 = runs.iter().filter_map(|r| r["stats"]["latencyMs"][op]["n"].as_u64()).sum();
    let sum: f64 = runs.iter().map(|r| field(r, "sumMs")).sum();
    let max = runs.iter().map(|r| field(r, "maxMs")).fold(0.0, f64::max);
    let round = |ms: f64| (ms * 10.0).round() / 10.0;
    let avg = if n == 0 { 0.0 } else { sum / n as f64 };
    json!({ "n": n, "sumMs": round(sum), "avgMs": round(avg), "maxMs": round(max) })
}
