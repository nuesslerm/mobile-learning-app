//! `wf cache event <layer> <restore|save>`: one restore or save of a cache layer of a CI job, recorded in the CI database (`CI_CACHE_EVENTS`), logged
//! (`cache.event`), counted (`ci.cache.events`, `ci.cache.problems`) and, when it failed, shouted: an `::error title=cache restore failed::` annotation and a
//! red line in the step summary. The actions run it after `actions/cache`, whose step is `continue-on-error` so a failed restore never fails the job; this
//! is where the failure stops being silent. It exits 0 whatever it finds.
use crate::ci_db::{self, CacheEvent};
use crate::ci_metrics::{self, Gauges};
use crate::nx_store::{self, Snapshot};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use wf_kit::args::{Parsed, Positional, Spec};

pub const SPEC: Spec = Spec {
    values: &["outcome", "key", "matched-key", "hit", "match-kind", "size-bytes", "duration-ms", "started-ms", "reason", "stage", "shard", "entries", "entries-before", "snapshot-file"],
    variadic: &["path"],
    positional: Positional::Exactly(2),
    ..Spec::NONE
};

const OUTCOMES: [&str; 5] = ["ok", "miss", "timeout", "error", "skipped"];
const LAYERS: [&str; 6] = ["nx", "sccache", "pnpm", "metro", "playwright", "references"];

/// The layer a caller's name belongs to (`sccache-flow-tests` is the `sccache` layer, its `flow-tests` the stage), `other` for a name no layer owns.
fn split_layer(name: &str) -> (String, String) {
    for layer in LAYERS {
        if name == layer {
            return (layer.to_string(), String::new());
        }
        if let Some(rest) = name.strip_prefix(&format!("{layer}-")) {
            return (layer.to_string(), rest.to_string());
        }
    }
    ("other".to_string(), name.to_string())
}

/// `core-tests-1of3` is the stage `core-tests` and the shard `1of3`; a name with no such tail is a stage with no shard.
fn split_stage(stage: &str) -> (String, String) {
    if let Some((head, tail)) = stage.rsplit_once('-') {
        if let Some((n, m)) = tail.split_once("of") {
            if !n.is_empty() && !m.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && m.bytes().all(|b| b.is_ascii_digit()) {
                return (head.to_string(), tail.to_string());
            }
        }
    }
    (stage.to_string(), String::new())
}

/// A workflow command's message: `%`, CR and LF escaped the way the runner reads them.
fn escaped(text: &str) -> String {
    text.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

fn annotation(level: &str, title: &str, layer: &str, key: &str, reason: &str) -> String {
    format!("::{level} title={title}::{}", escaped(&format!("layer={layer} key={} reason={reason}", if key.is_empty() { "none" } else { key })))
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

fn number(p: &Parsed, name: &str) -> Option<i64> {
    p.val(name).and_then(|v| v.trim().parse().ok())
}

fn nonempty(p: &Parsed, name: &str) -> Option<String> {
    p.val(name).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

fn usage(error: &str) -> Option<i32> {
    println!("{}", json!({ "ok": false, "error": error, "next": "wf cache event <layer> <restore|save> --outcome <ok|miss|timeout|error|skipped> [--key K] [--matched-key M] [--reason TEXT]" }));
    Some(1)
}

pub fn run(args: &[String]) -> Option<i32> {
    let parsed = wf_kit::args::parse_spec(args, &SPEC)?;
    let (name, phase) = (parsed.positional[0].as_str(), parsed.positional[1].as_str());
    if !["restore", "save"].contains(&phase) {
        return usage(&format!("the phase is restore or save, not {phase}"));
    }
    let Some(outcome) = nonempty(&parsed, "outcome") else { return usage("--outcome is required") };
    if !OUTCOMES.contains(&outcome.as_str()) {
        return usage(&format!("the outcome is one of {}, not {outcome}", OUTCOMES.join(", ")));
    }
    let (layer, tail) = split_layer(name);
    let stage_name = nonempty(&parsed, "stage").unwrap_or(tail);
    let (stage, mut shard) = split_stage(&stage_name);
    if let Some(given) = nonempty(&parsed, "shard") {
        shard = given;
    }
    let (key, matched) = (nonempty(&parsed, "key"), nonempty(&parsed, "matched-key"));
    let hit = parsed.val("hit").is_some_and(|h| h == "true");
    let kind = nonempty(&parsed, "match-kind").unwrap_or_else(|| if phase == "save" { "none" } else if hit { "exact" } else if matched.is_some() { "restore-key" } else { "none" }.to_string());
    let duration = number(&parsed, "duration-ms").or_else(|| number(&parsed, "started-ms").map(|t| (wf_kit::time::now_ms() as i64 - t).max(0)));
    let mut reason = nonempty(&parsed, "reason");
    let mut outcome = outcome;

    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut snapshot: Option<Snapshot> = None;
    if layer == "nx" {
        snapshot = home.as_deref().map(nx_store::take);
    }
    let (mut entries, db_rows, wal_bytes) = (number(&parsed, "entries"), snapshot.as_ref().and_then(|s| s.db_rows), snapshot.as_ref().map(|s| s.wal_bytes as i64));
    if let Some(s) = &snapshot {
        entries = entries.or(Some(s.entries.len() as i64));
    }
    let paths: Vec<PathBuf> = parsed.values("path").into_iter().map(PathBuf::from).filter(|p| p.exists()).collect();
    let size = number(&parsed, "size-bytes").or_else(|| snapshot.as_ref().map(|s| s.bytes as i64)).or_else(|| (!paths.is_empty()).then(|| nx_store::bytes_of(&paths) as i64)).filter(|b| *b > 0);

    // a restore that matched an entry and put nothing in the cache directory is a restore that did not work
    let mut empty = false;
    if layer == "nx" && phase == "restore" && outcome == "ok" && entries == Some(0) {
        outcome = "error".to_string();
        empty = true;
        reason = Some(format!("matched {} and restored 0 entries", matched.as_deref().unwrap_or("an entry")));
    }
    if outcome == "timeout" && reason.is_none() {
        reason = Some("the cache step was cancelled before it finished (its time limit)".to_string());
    }
    if outcome == "error" && reason.is_none() {
        reason = Some("the cache step failed and wrote no message (see its log)".to_string());
    }

    let tasks_ran = if layer == "nx" && phase == "save" { ran_tasks() } else { 0 };
    if outcome == "skipped" && tasks_ran > 0 {
        reason = Some(format!("{}; {tasks_ran} task(s) ran and passed, and their results are not saved", reason.as_deref().unwrap_or("no reason recorded")));
    }

    let event = CacheEvent {
        run: env("GITHUB_RUN_ID"),
        ts: wf_kit::time::now_ms() as i64,
        attempt: env("GITHUB_RUN_ATTEMPT"),
        workflow: env("GITHUB_WORKFLOW"),
        job: env("GITHUB_JOB"),
        stage,
        shard,
        layer: layer.clone(),
        phase: phase.to_string(),
        key_requested: key.clone(),
        key_matched: matched.clone(),
        match_kind: kind,
        size_bytes: size,
        duration_ms: duration,
        outcome: outcome.clone(),
        reason: reason.clone(),
        entries,
        db_rows,
        wal_bytes,
    };
    let recorded = match ci_db::record_cache_events_local(std::slice::from_ref(&event)) {
        Ok(recorded) => recorded,
        Err(why) => {
            eprintln!("wf: cache event not recorded: {why}");
            false
        }
    };

    if layer == "nx" && phase == "restore" {
        if let Some(mut s) = snapshot.take() {
            s.outcome = outcome.clone();
            s.matched_key = matched.clone();
            s.reason = reason.clone();
            let file = nonempty(&parsed, "snapshot-file").or_else(|| Some(env("RUNNER_TEMP")).filter(|t| !t.is_empty()).map(|t| format!("{t}/nx-cache.restore.json")));
            if let Some(file) = file {
                match s.write(std::path::Path::new(&file)) {
                    Ok(()) => {
                        let github_env = env("GITHUB_ENV");
                        append(&github_env, &format!("{}={file}", nx_store::FILE_ENV));
                        append(&github_env, &format!("{}={stage_name}", nx_store::STAGE_ENV));
                        append(&github_env, &format!("{}={}", nx_store::KEY_ENV, matched.as_deref().unwrap_or_default()));
                    }
                    Err(why) => eprintln!("wf: nx cache snapshot not kept: {why}"),
                }
            }
        }
    }

    let failed = matches!(outcome.as_str(), "timeout" | "error");
    let problem = match (phase, outcome.as_str(), empty) {
        ("restore", "timeout", _) => Some("restore-timeout"),
        ("restore", "error", true) => Some("restore-empty"),
        ("restore", "error", _) => Some("restore-error"),
        ("save", "timeout", _) => Some("save-timeout"),
        ("save", "error", _) => Some("save-error"),
        ("save", "skipped", _) if tasks_ran > 0 => Some("save-skipped-tasks-ran"),
        _ => None,
    };
    let why = reason.clone().unwrap_or_default();
    let shown_key = key.clone().or_else(|| matched.clone()).unwrap_or_default();
    let note = match (failed, problem) {
        (true, _) => Some(annotation("error", &format!("cache {phase} failed"), &layer, &shown_key, &why)),
        (false, Some(_)) => Some(annotation("warning", &format!("cache {phase} skipped"), &layer, &shown_key, &why)),
        _ => None,
    };
    if let Some(note) = &note {
        println!("{note}");
        let verdict = if failed { format!("🔴 {phase} failed") } else { format!("🟠 {phase} skipped") };
        append(&env("GITHUB_STEP_SUMMARY"), &format!("- {verdict}: **{layer}** ({}) {why}; key `{}`", env("GITHUB_JOB"), if shown_key.is_empty() { "none" } else { &shown_key }));
    }

    let mut fields = vec![
        ("layer", layer.clone()),
        ("phase", phase.to_string()),
        ("outcome", outcome.clone()),
        ("match_kind", event.match_kind.clone()),
        ("key", key.clone().unwrap_or_default()),
        ("matched_key", matched.clone().unwrap_or_default()),
        ("stage", event.stage.clone()),
        ("shard", event.shard.clone()),
        ("entries", entries.map_or(String::new(), |n| n.to_string())),
        ("db_rows", db_rows.map_or(String::new(), |n| n.to_string())),
        ("size_bytes", size.map_or(String::new(), |n| n.to_string())),
        ("duration_ms", duration.map_or(String::new(), |n| n.to_string())),
        ("reason", why.clone()),
    ];
    if tasks_ran > 0 {
        fields.push(("tasks_ran", tasks_ran.to_string()));
    }
    ci_metrics::event(if problem.is_some() { "warn" } else { "info" }, "cache.event", &fields);

    let mut gauges = Gauges::new();
    gauges.set("ci.cache.events", &layer, &format!("{phase}-{outcome}"), 1.0);
    if let Some(problem) = problem {
        gauges.set("ci.cache.problems", &layer, problem, 1.0);
    }
    if let Some(ms) = duration {
        gauges.set("ci.cache.step_seconds", &layer, phase, ms as f64 / 1000.0);
    }
    gauges.send();

    println!("{}", json!({ "ok": true, "recorded": recorded, "layer": layer, "phase": phase, "outcome": outcome, "reason": reason, "problem": problem, "annotation": note, "event": event_json(&event) }));
    Some(0)
}

fn event_json(e: &CacheEvent) -> Value {
    json!({
        "run": e.run, "attempt": e.attempt, "workflow": e.workflow, "job": e.job, "stage": e.stage, "shard": e.shard, "layer": e.layer, "phase": e.phase,
        "keyRequested": e.key_requested, "keyMatched": e.key_matched, "matchKind": e.match_kind, "sizeBytes": e.size_bytes, "durationMs": e.duration_ms,
        "outcome": e.outcome, "reason": e.reason, "entries": e.entries, "dbRows": e.db_rows, "walBytes": e.wal_bytes,
    })
}

/// Tasks this job ran and passed, from the job's own database: the results a skipped save leaves behind.
fn ran_tasks() -> usize {
    let Some(path) = ci_db::local_path().filter(|p| p.exists()) else { return 0 };
    ci_db::tasks_of(&path).map_or(0, |tasks| tasks.iter().filter(|t| t.outcome == "ran" && t.passed).count())
}
