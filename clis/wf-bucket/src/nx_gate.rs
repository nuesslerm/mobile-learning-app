//! `wf cache gate nx`: whether a job's Nx cache is saved after its test stage. A task that re-runs over an entry the restore already held rewrites the same
//! directory, so counting entries never saw it and the row Nx wrote was never uploaded; the next run restored the same state and ran the task again. The decision
//! is made from what the job did (a task that ran and passed) and from what changed in Nx's own store (the rows of `cache_outputs`, the newest `created_at`, the
//! entry listing), after the WAL is flushed into the database file the archive carries.
use crate::cache_events;
use crate::nx_store::{self, Flush, Snapshot};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use wf_kit::args::{Parsed, Positional, Spec};

pub const SPEC: Spec = Spec { values: &["max-mb", "key", "stage", "snapshot-file"], positional: Positional::Exactly(1), ..Spec::NONE };

const DEFAULT_MAX_MB: u64 = 3072;

/// What the decision rests on.
pub struct Facts<'a> {
    pub restored: &'a Snapshot,
    pub now: &'a Snapshot,
    /// Tasks of this job that ran (were not replayed) and passed.
    pub tasks_ran: usize,
    pub mb: u64,
    pub max_mb: u64,
}

#[derive(Debug, PartialEq)]
pub struct Decision {
    pub save: bool,
    pub reason: String,
}

fn added(restored: &Snapshot, now: &Snapshot) -> usize {
    now.entries.difference(&restored.entries).count()
}

fn removed(restored: &Snapshot, now: &Snapshot) -> usize {
    restored.entries.difference(&now.entries).count()
}

/// The signals that say there is something new to keep, each as the words of the reason.
fn signals(f: &Facts) -> Vec<String> {
    let mut out = Vec::new();
    if f.tasks_ran > 0 {
        out.push(format!("{} task(s) ran and passed", f.tasks_ran));
    }
    if f.restored.db_rows != f.now.db_rows {
        out.push(format!("cache_outputs rows {} -> {}", f.restored.db_rows.map_or("none".into(), |n| n.to_string()), f.now.db_rows.map_or("none".into(), |n| n.to_string())));
    }
    if f.restored.db_created != f.now.db_created {
        out.push("cache_outputs newest created_at moved".to_string());
    }
    if f.restored.entries != f.now.entries {
        out.push(format!("entries +{} -{}", added(f.restored, f.now), removed(f.restored, f.now)));
    }
    out
}

pub fn decide(f: &Facts) -> Decision {
    let why = signals(f);
    let over = f.mb > f.max_mb;
    match (why.is_empty(), over) {
        (_, true) => Decision { save: false, reason: format!("over the size limit ({} MB, limit {} MB)", f.mb, f.max_mb) },
        (true, false) => Decision {
            save: false,
            reason: format!("nothing new: no task ran, {} entries and {} cache_outputs rows as restored", f.now.entries.len(), f.now.db_rows.map_or("no".into(), |n| n.to_string())),
        },
        (false, false) => Decision { save: true, reason: why.join("; ") },
    }
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

fn append(path: &str, line: &str) {
    if path.is_empty() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn text(p: &Parsed, name: &str) -> Option<String> {
    p.val(name).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

fn restored_snapshot(p: &Parsed) -> Option<Snapshot> {
    let from_file = |path: PathBuf| std::fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str::<Value>(&raw).ok()).map(|v| Snapshot::from_json(&v));
    if let Some(file) = text(p, "snapshot-file") {
        return from_file(PathBuf::from(file));
    }
    nx_store::restored().or_else(|| Some(env("RUNNER_TEMP")).filter(|t| !t.is_empty()).and_then(|t| from_file(PathBuf::from(t).join("nx-cache.restore.json"))))
}

fn flush_json(flushed: &[Flush]) -> Value {
    Value::Array(flushed.iter().map(|f| json!({ "db": f.db, "frames": f.result.as_ref().ok(), "error": f.result.as_ref().err() })).collect())
}

fn usage(error: &str) -> Option<i32> {
    println!("{}", json!({ "ok": false, "error": error, "next": "wf cache gate nx [--max-mb N] [--key K] [--stage S]" }));
    Some(1)
}

pub fn run(args: &[String]) -> Option<i32> {
    let parsed = wf_kit::args::parse_spec(args, &SPEC)?;
    let layer = parsed.positional[0].as_str();
    if layer != "nx" {
        return usage(&format!("the layer is nx, not {layer}"));
    }
    let max_mb = text(&parsed, "max-mb").and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_MAX_MB);
    let (key, stage) = (text(&parsed, "key"), text(&parsed, "stage").unwrap_or_else(|| env(nx_store::STAGE_ENV)));
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();

    let flushed = nx_store::checkpoint(&home);
    let now = nx_store::take(&home);
    let restored = restored_snapshot(&parsed);
    let baseline = restored.clone().unwrap_or_default();
    let tasks_ran = cache_events::ran_tasks();
    let mb = now.bytes.div_ceil(1024 * 1024);
    let decision = decide(&Facts { restored: &baseline, now: &now, tasks_ran, mb, max_mb });

    let job = env("GITHUB_JOB");
    let before = baseline.entries.len();
    let line = format!("- **nx** ({job}) before save: {mb} MB, {} entries, {}", now.entries.len(), if decision.save { format!("saving ({})", decision.reason) } else { format!("not saving, {}", decision.reason) });
    append(&env("GITHUB_STEP_SUMMARY"), &line);
    println!("{line}");
    let output = env("GITHUB_OUTPUT");
    append(&output, &format!("save={}", decision.save));
    append(&output, &format!("why={}", decision.reason.replace(['\r', '\n'], " ")));
    if mb > max_mb {
        println!("::warning::Nx cache is {mb} MB, over {max_mb} MB: not saved");
    }

    let mut event = Value::Null;
    if !decision.save {
        let mut argv: Vec<String> = ["nx", "save", "--outcome", "skipped", "--reason"].iter().map(|s| s.to_string()).collect();
        argv.push(decision.reason.clone());
        argv.extend(["--entries".to_string(), now.entries.len().to_string(), "--entries-before".to_string(), before.to_string()]);
        argv.extend(["--stage".to_string(), stage]);
        if let Some(key) = key {
            argv.extend(["--key".to_string(), key]);
        }
        if let Ok(recorded) = cache_events::record(&argv) {
            event = recorded;
        }
    }

    println!(
        "{}",
        json!({
            "ok": true, "save": decision.save, "reason": decision.reason, "mb": mb, "maxMb": max_mb, "tasksRan": tasks_ran,
            "restoreSnapshot": restored.is_some(),
            "entries": { "before": before, "now": now.entries.len(), "added": added(&baseline, &now), "removed": removed(&baseline, &now) },
            "dbRows": { "before": baseline.db_rows, "now": now.db_rows },
            "dbCreated": { "before": baseline.db_created, "now": now.db_created },
            "checkpoint": flush_json(&flushed), "event": event,
        })
    );
    Some(0)
}
