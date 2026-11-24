//! `wf ci cache-misses` and the "Cache problems" block of the CI comment: every Nx task of the run that ran instead of replaying gets one reason
//! (`wf_bucket::miss_reason`), decided in the report job from the CI database (this run's job files merged over the sealed history) and the job logs. The block
//! lists each cache restore or save that failed, then the reasons with the seconds they cost; the cache section shows, per stage over the last runs,
//! how often a hash that had already been executed replayed (the figure that must be near 100). `wf ci db merge` stores the reasons in `NX_TASKS.MISS_REASON`.
use crate::ci_cache_health::{fields, get};
use crate::ci_comment::{cell, log_lines, var};
use wf_bucket::ci_db::Connection;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use wf_bucket::ci_db::{self, CacheEvent, Working};
use wf_bucket::miss_reason::{self, Miss, StageRate};
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &["json", "send"], positional: Positional::None, ..Spec::NONE };

/// How many runs the hit-rate table looks back over.
pub(crate) const RATE_RUNS: usize = 20;

/// Below this share of replays among the hashes already seen, a stage is red; below `AMBER`, amber.
const RED: f64 = 90.0;
const AMBER: f64 = 98.0;

/// A cache restore or save that did not work, as a job reported it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Problem {
    pub job: String,
    pub layer: String,
    pub phase: String,
    /// `error`, `timeout`, or `skipped` for a save that left results behind.
    pub outcome: String,
    pub key: String,
    pub reason: String,
}

/// The problems the `cache.event` lines of the jobs' logs name: a restore or save that errored or timed out, and a save that was skipped while tasks ran.
pub(crate) fn problems_in_logs(logs: &[(String, String)]) -> Vec<Problem> {
    let mut out = Vec::new();
    for (job, log) in logs {
        for line in log_lines(log) {
            if !line.contains("source=ci-cache") {
                continue;
            }
            let f = fields(&line);
            if get(&f, "msg") != "cache.event" {
                continue;
            }
            let (outcome, tasks) = (get(&f, "outcome"), get(&f, "tasks_ran"));
            let skipped_with_tasks = outcome == "skipped" && tasks.parse::<u64>().unwrap_or(0) > 0;
            if matches!(outcome, "error" | "timeout") || skipped_with_tasks {
                let key = [get(&f, "key"), get(&f, "matched_key")].into_iter().find(|k| !k.is_empty()).unwrap_or_default().to_string();
                out.push(Problem { job: job.clone(), layer: get(&f, "layer").to_string(), phase: get(&f, "phase").to_string(), outcome: outcome.to_string(), key, reason: get(&f, "reason").to_string() });
            }
        }
    }
    out
}

/// The problems the database's events hold for the run.
pub(crate) fn problems_in_events(events: &[CacheEvent]) -> Vec<Problem> {
    events
        .iter()
        .filter(|e| matches!(e.outcome.as_str(), "error" | "timeout") || (e.outcome == "skipped" && e.reason.as_deref().is_some_and(|r| r.contains("task(s) ran and passed"))))
        .map(|e| Problem {
            job: if e.shard.is_empty() { e.job.clone() } else { format!("{} ({})", e.job, e.shard) },
            layer: e.layer.clone(),
            phase: e.phase.clone(),
            outcome: e.outcome.clone(),
            key: e.key_requested.clone().or_else(|| e.key_matched.clone()).unwrap_or_default(),
            reason: e.reason.clone().unwrap_or_default(),
        })
        .collect()
}

/// The CI database as a comment or the report job reads it: the sealed history with this run's job files merged over it, in a private working copy.
pub(crate) struct Analysis {
    work: Working,
    pub run: String,
}

impl Analysis {
    /// `WF_CI_SEALED_FILE` names a sealed file to read instead of the state ref (tests, offline replay).
    pub(crate) fn open(run: &str, files: &[PathBuf]) -> Result<Analysis, String> {
        let work = match var("WF_CI_SEALED_FILE") {
            Some(path) => Working::from_sealed(ci_db::key()?, std::fs::read(path).ok())?,
            None => Working::fetch()?,
        };
        let conn = work.connect()?;
        ci_db::ensure_ledger(&conn)?;
        for file in files.iter().filter(|f| f.is_file()) {
            ci_db::merge(&conn, file).map_err(|e| format!("{}: {e}", file.display()))?;
        }
        Ok(Analysis { work, run: run.to_string() })
    }

    pub(crate) fn conn(&self) -> Result<Connection, String> {
        self.work.connect()
    }
}

/// What the database says about the run's cache.
pub(crate) struct Findings {
    pub misses: Vec<Miss>,
    pub rates: Vec<StageRate>,
    pub events: Vec<CacheEvent>,
}

pub(crate) fn analyse(a: &Analysis) -> Result<Findings, String> {
    let conn = a.conn()?;
    Ok(Findings { misses: miss_reason::classify_db(&conn, Some(&a.run), false)?, rates: miss_reason::hit_rates(&conn, RATE_RUNS)?, events: ci_db::cache_events_in(&conn, Some(&a.run))? })
}

/// The job-local database files of the run that the report job downloaded (`WF_CI_LEDGERS`).
pub(crate) fn local_files() -> Vec<PathBuf> {
    crate::ci_telemetry::ledger_files().into_iter().map(|(_, f)| f).collect()
}

fn problem_what(p: &Problem) -> String {
    match (p.phase.as_str(), p.outcome.as_str()) {
        ("restore", "timeout") => "restore timed out".to_string(),
        (phase, "skipped") => format!("{phase} skipped, results left behind"),
        (phase, outcome) => format!("{phase} {}", if outcome == "error" { "failed" } else { "timed out" }),
    }
}

fn seconds(s: f64) -> String {
    if s >= 120.0 {
        format!("{:.0} min", s / 60.0)
    } else {
        format!("{s:.0} s")
    }
}

/// The reasons of the misses as table rows, with the first task of each as the example.
fn breakdown_table(misses: &[Miss]) -> String {
    let mut out = String::from("| Reason | Tasks | Seconds lost | Example |\n|---|---|---|---|\n");
    for (reason, tasks, secs) in miss_reason::breakdown(misses) {
        let example = misses.iter().find(|m| m.reason == reason).map(|m| format!("`{}`: {}", m.task, m.detail)).unwrap_or_default();
        let mark = if reason.legitimate() { "" } else { "🔴 " };
        out.push_str(&format!("| {mark}{} | {tasks} | {} | {} |\n", reason.as_str(), seconds(secs), cell(&example.chars().take(160).collect::<String>())));
    }
    out
}

/// The "Cache problems" section: empty (the slot then says `none`) when nothing failed and no task ran, else the failed restores and saves, then why tasks ran.
pub(crate) fn problems_section(problems: &[Problem], findings: Option<&Result<Findings, String>>, running: bool) -> String {
    let mut problems = problems.to_vec();
    problems.sort();
    problems.dedup();
    let misses = match findings {
        Some(Ok(f)) => f.misses.as_slice(),
        _ => &[],
    };
    let unavailable = match findings {
        Some(Err(why)) if !running => Some(why.as_str()),
        _ => None,
    };
    if problems.is_empty() && misses.is_empty() && unavailable.is_none() {
        return String::new();
    }
    let mut out = format!("\n{}\n\n", crate::ci_skeleton::CACHE_PROBLEMS);
    if problems.is_empty() {
        out.push_str(&format!("{}\n\n", crate::ci_skeleton::none("no cache restore failed")));
    } else {
        let failed = problems.iter().filter(|p| p.outcome != "skipped").count();
        let head = if failed > 0 { format!("🔴 **{failed} cache restore or save failed**") } else { "🟠 **A cache save was skipped while tasks ran**".to_string() };
        out.push_str(&format!("{head}\n\n| Layer | Job | What | Key | Reason |\n|---|---|---|---|---|\n"));
        for p in &problems {
            out.push_str(&format!("| {} | {} | {} | `{}` | {} |\n", cell(&p.layer), cell(&p.job), problem_what(p), cell(&p.key), cell(&p.reason)));
        }
        out.push('\n');
    }
    if !misses.is_empty() {
        let lost: f64 = misses.iter().filter(|m| !m.reason.legitimate()).map(|m| m.ms as f64 / 1000.0).sum();
        let avoidable = misses.iter().filter(|m| !m.reason.legitimate()).count();
        let head = if avoidable > 0 {
            format!("🔴 **{avoidable} of {} task(s) that ran had their hash executed before ({} lost)**", misses.len(), seconds(lost))
        } else {
            format!("🟢 {} task(s) ran, every one under a hash no run had executed", misses.len())
        };
        out.push_str(&format!("{head}\n\n{}\n", breakdown_table(misses)));
    }
    if let Some(why) = unavailable {
        out.push_str(&format!("_why tasks ran is unavailable: {}_\n\n", cell(why)));
    }
    out
}

fn percent(v: Option<f64>) -> String {
    v.map_or("n/a".to_string(), |p| format!("{p:.0}%"))
}

/// The per-stage replay table of the cache section: inputs changed against an exact hash seen before, over the last runs.
pub(crate) fn rates_block(rates: &[StageRate]) -> String {
    if rates.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "\n**Nx replay per stage over the last {RATE_RUNS} runs**: a miss under a new hash is the inputs changing; a miss under a hash that already ran is the cache failing, and its column must be near 100%\n\n| Stage | Tasks | Replayed | Inputs changed (ran, new hash) | Exact hash seen before: replayed |\n|---|---|---|---|---|\n"
    );
    for r in rates {
        let exact = r.exact_rate();
        let mark = match exact {
            Some(p) if p < RED => "🔴 ",
            Some(p) if p < AMBER => "🟠 ",
            _ => "",
        };
        out.push_str(&format!("| {} | {} | {} | {} | {mark}{} ({} of {}) |\n", cell(&r.stage), r.tasks, percent(r.hit_rate()), r.inputs_changed, percent(exact), r.replayed, r.replayed + r.seen_before));
    }
    out
}

fn miss_json(m: &Miss) -> Value {
    json!({ "run": m.run, "job": m.job, "cacheStage": m.cache_stage, "task": m.task, "hash": m.hash, "ms": m.ms, "reason": m.reason.as_str(), "detail": m.detail })
}

/// `wf ci cache-misses [--send] [--json]`: the reason of every task of this run that ran, the failed restores and saves, the per-stage replay rates; with `--send` the
/// gauges `ci.nx.miss_tasks`, `ci.nx.miss_s` and `ci.nx.exact_replay_ratio`, and a workflow warning when a hash that had run before ran again. Never fails the run.
pub fn run(args: &[String]) -> Option<i32> {
    let parsed = parse_spec(args, &SPEC)?;
    let Some(run) = var("GITHUB_RUN_ID") else {
        println!("{}", json!({ "ok": false, "error": "GITHUB_RUN_ID is not set", "next": "run it in the report job" }));
        return Some(0);
    };
    let files = local_files();
    let outcome = Analysis::open(&run, &files).and_then(|a| analyse(&a));
    match outcome {
        Ok(f) => {
            let breakdown: Vec<Value> = miss_reason::breakdown(&f.misses).into_iter().map(|(r, n, s)| json!({ "reason": r.as_str(), "tasks": n, "seconds": s, "avoidable": !r.legitimate() })).collect();
            let problems = problems_in_events(&f.events);
            let rates: Vec<Value> = f.rates.iter().map(|r| json!({ "stage": r.stage, "tasks": r.tasks, "replayed": r.replayed, "inputsChanged": r.inputs_changed, "seenBefore": r.seen_before, "exactReplayPercent": r.exact_rate() })).collect();
            let problem_rows: Vec<Value> = problems.iter().map(|p| json!({ "job": p.job, "layer": p.layer, "phase": p.phase, "outcome": p.outcome, "key": p.key, "reason": p.reason })).collect();
            println!("{}", json!({ "ok": true, "run": run, "ran": f.misses.len(), "breakdown": breakdown, "misses": f.misses.iter().map(miss_json).collect::<Vec<_>>(), "problems": problem_rows, "rates": rates }));
            let avoidable: Vec<&Miss> = f.misses.iter().filter(|m| !m.reason.legitimate()).collect();
            if !avoidable.is_empty() {
                let lost: f64 = avoidable.iter().map(|m| m.ms as f64 / 1000.0).sum();
                let by: Vec<String> = miss_reason::breakdown(&f.misses).into_iter().filter(|(r, _, _)| !r.legitimate()).map(|(r, n, s)| format!("{} x{n} ({})", r.as_str(), seconds(s))).collect();
                println!("::warning title=cache misses under a hash that ran before::{} task(s), {} lost: {}", avoidable.len(), seconds(lost), by.join(", "));
            }
            if parsed.flag("send") {
                send(&f);
            }
        }
        Err(why) => {
            wf_kit::wf_err!("wf ci cache-misses: the reasons could not be computed, not failing the run: {why}");
            println!("{}", json!({ "ok": null, "reason": why }));
        }
    }
    Some(0)
}

fn send(f: &Findings) {
    use wf_bucket::ci_metrics::{Gauges, ALL};
    let mut gauges = Gauges::new();
    let mut by: std::collections::BTreeMap<(String, &'static str), (f64, f64)> = std::collections::BTreeMap::new();
    for m in &f.misses {
        let entry = by.entry((m.job.clone(), m.reason.as_str())).or_default();
        entry.0 += 1.0;
        entry.1 += m.ms as f64 / 1000.0;
    }
    for ((stage, reason), (tasks, secs)) in by {
        gauges.set("ci.nx.miss_tasks", &stage, reason, tasks).set("ci.nx.miss_s", &stage, reason, secs);
    }
    for r in &f.rates {
        if let Some(p) = r.exact_rate() {
            gauges.set("ci.nx.exact_replay_ratio", &r.stage, ALL, p / 100.0);
        }
    }
    gauges.send();
}

/// The runs whose rows a job database holds, for the merge to classify only what it brought.
pub(crate) fn runs_of(file: &Path) -> Vec<String> {
    let mut runs: Vec<String> = ci_db::tasks_of(file).unwrap_or_default().into_iter().map(|t| t.run).collect();
    runs.sort();
    runs.dedup();
    runs
}
