//! `wf ci cache-health` and the "Cache health" section of the CI comment: what the cache layers said about themselves in the run's
//! job logs (the `ci-cache` events and the `wf: build cache {..}` lines), judged. A degraded cache, a refused required write, writes the
//! store acknowledged but does not hold, a stage the push gate seeded that replayed nothing, drift and a slow pnpm save are problems;
//! the command exits 1 on any, so the report job that runs it goes red. Alert rules for the same signals: `docs/runbooks/cache-health-alerts.md`.
use crate::ci_comment::{cell, list_jobs, log_lines, cached_log, is_meta, text, var, Env};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &["json"], positional: Positional::None, ..Spec::NONE };

/// A pnpm save takes seconds when the store is small and minutes when it is packed for nothing; past this it is a problem.
const SLOW_PNPM_SAVE_SECONDS: f64 = 60.0;

/// The stages the pre-push gate runs and seeds for a push to dev (`pushed.rs`): CI should replay every task of them.
const GATE_STAGES: &[&str] = &["typecheck-lint-unit-tests", "flow-tests", "integration-tests", "browser-e2e-tests", "cli-e2e-tests"];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Finding {
    pub kind: &'static str,
    pub job: String,
    pub detail: String,
}

#[derive(Debug, Default)]
pub(crate) struct Health {
    pub findings: Vec<Finding>,
    pub notes: Vec<String>,
    pub stages: Vec<(String, String, u64, u64)>,
    pub events: usize,
    pub unreadable: usize,
}

/// The `key=value` pairs of one logfmt line, a quoted value unescaped.
pub(crate) fn fields(line: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut rest = line;
    while let Some((key, after)) = rest.split_once('=') {
        let key = key.rsplit(' ').next().unwrap_or(key);
        let (value, tail) = if let Some(quoted) = after.strip_prefix('"') {
            let (mut value, mut escaped, mut end) = (String::new(), false, quoted.len());
            for (i, c) in quoted.char_indices() {
                match (escaped, c) {
                    (true, c) => {
                        value.push(c);
                        escaped = false;
                    }
                    (false, '\\') => escaped = true,
                    (false, '"') => {
                        end = i + 1;
                        break;
                    }
                    (false, c) => value.push(c),
                }
            }
            (value, &quoted[end.min(quoted.len())..])
        } else {
            let (value, tail) = after.split_once(' ').unwrap_or((after, ""));
            (value.to_string(), tail)
        };
        out.insert(key.to_string(), value);
        rest = tail;
    }
    out
}

fn number(f: &BTreeMap<String, String>, key: &str) -> f64 {
    f.get(key).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

pub(crate) fn get<'a>(f: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    f.get(key).map_or("", String::as_str)
}

/// Judges the run: `logs` is (job name, log text) of every finished job; `gate` is whether this run is a push to dev, whose stages the
/// pre-push gate has already seeded.
pub(crate) fn assess(logs: &[(String, String)], gate: bool) -> Health {
    let mut health = Health::default();
    let mut found: BTreeMap<(&'static str, String), String> = BTreeMap::new();
    let mut flag = |kind: &'static str, job: &str, detail: String| {
        found.entry((kind, job.to_string())).or_insert(detail);
    };
    let mut notes = BTreeSet::new();
    let mut stages: BTreeMap<(String, String), (u64, u64)> = BTreeMap::new();
    // The workspace root's tasks hash every npm package under the name the runner's node_modules hoists it by, so a gate on another install
    // never seeds the hash CI computes; they are no evidence that the gate's seeding failed.
    let mut root_ran: BTreeMap<&str, u64> = BTreeMap::new();
    for (job, log) in logs {
        let root_tasks = crate::nx_ledger::from_log(log).iter().filter(|t| t.outcome == "ran" && t.task.starts_with(&format!("{}:", crate::test_plan::ROOT))).count() as u64;
        root_ran.insert(job, root_tasks);
        for line in log_lines(log) {
            if let Some((_, json)) = line.split_once("wf: build cache {") {
                let Ok(v) = serde_json::from_str::<Value>(&format!("{{{json}")) else { continue };
                health.events += 1;
                if let Some(why) = v["degraded"].as_str() {
                    flag("degraded", job, format!("the build cache went cold ({why})"));
                }
                let (dropped, quota) = (v["droppedPuts"].as_u64().unwrap_or(0), v["quotaExceeded"].as_u64().unwrap_or(0));
                if quota > 0 {
                    flag("rejected-write", job, format!("{quota} cache write(s) refused: the tenancy's Object Storage is full"));
                } else if dropped > 0 {
                    flag("rejected-write", job, format!("{dropped} cache write(s) wanted and not stored"));
                }
                continue;
            }
            if !line.contains("source=ci-cache") {
                continue;
            }
            health.events += 1;
            let f = fields(&line);
            let kind = get(&f, "kind");
            match get(&f, "msg") {
                "cache.run" => {
                    if get(&f, "degraded") != "no" && !get(&f, "degraded").is_empty() {
                        flag("degraded", job, format!("{kind} went cold ({})", get(&f, "degraded")));
                    }
                    let (checked, present) = (number(&f, "seeded_checked"), number(&f, "seeded_present"));
                    if present < checked {
                        flag("seeded-lost", job, format!("{kind} seeded {present} of {checked}: the store acknowledged writes it does not hold"));
                    }
                }
                "cache.put.dropped" => {
                    let class = get(&f, "error_class");
                    let status = get(&f, "error_status");
                    flag("rejected-write", job, format!("{} {kind} write(s) wanted and not stored{}", get(&f, "dropped"), if class.is_empty() { String::new() } else { format!(" ({class}{})", if status.is_empty() { String::new() } else { format!(" HTTP {status}") }) }));
                }
                "cache.save" | "cache.restore" => {
                    let class = get(&f, "error_class");
                    if matches!(class, "unreachable" | "credential") {
                        flag("degraded", job, format!("{kind} {} met a store that is {class}", get(&f, "verb")));
                    }
                    if get(&f, "verb") == "save" && get(&f, "done") == "false" && (matches!(class, "rejected" | "quota-exceeded") || get(&f, "reason").contains("Object Storage is full")) {
                        flag("rejected-write", job, format!("the {kind} save was refused ({})", if class.is_empty() { "storage full" } else { class }));
                    }
                    let seconds = number(&f, "seconds");
                    if kind == "pnpm" && get(&f, "verb") == "save" && seconds > SLOW_PNPM_SAVE_SECONDS {
                        flag("pnpm-save-slow", job, format!("the pnpm save took {seconds:.0} s (limit {SLOW_PNPM_SAVE_SECONDS:.0} s)"));
                    }
                }
                "nx.stage.summary" => {
                    let (ran, replayed) = (number(&f, "ran") as u64, number(&f, "replayed") as u64);
                    let total = stages.entry((job.clone(), get(&f, "stage").to_string())).or_default();
                    total.0 += ran;
                    total.1 += replayed;
                }
                "drift.report" if number(&f, "drifted") > 0.0 => {
                    flag("drift", job, format!("{} of {} task(s) hashed differently ({})", get(&f, "drifted"), get(&f, "compared"), get(&f, "causes")));
                }
                "cache.shim.disabled" => {
                    notes.insert(format!("The shared build cache was disabled in {job}: {}; every layer ran cold.", get(&f, "reason")));
                }
                _ => {}
            }
        }
    }
    for ((job, stage), (ran, replayed)) in stages {
        health.stages.push((job.clone(), stage.clone(), replayed, ran + replayed));
        if gate && replayed == 0 && ran > root_ran.get(job.as_str()).copied().unwrap_or(0) && GATE_STAGES.contains(&stage.as_str()) {
            flag("replayed-nothing", &job, format!("{stage} ran {ran} task(s) and replayed none, though the push gate seeded it"));
        }
    }
    health.findings = found.into_iter().map(|((kind, job), detail)| Finding { kind, job, detail }).collect();
    health.notes = notes.into_iter().collect();
    health
}

/// The comment's section; nothing when the logs held no cache event and nothing is wrong.
pub(crate) fn section(health: &Health) -> String {
    if health.events == 0 && health.findings.is_empty() && health.notes.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n### 🗄️ Cache health\n\n");
    if health.findings.is_empty() {
        let replayed: u64 = health.stages.iter().map(|s| s.2).sum();
        let total: u64 = health.stages.iter().map(|s| s.3).sum();
        out.push_str(&format!("🟢 Healthy: no degraded layer, no refused write, no lost entry{}.\n", if total > 0 { format!("; Nx replayed {replayed} of {total} task(s)") } else { String::new() }));
    } else {
        out.push_str(&format!("🔴 **{} problem(s)**, the report job turns red\n\n| Problem | Job | Detail |\n|---|---|---|\n", health.findings.len()));
        for f in &health.findings {
            out.push_str(&format!("| {} | {} | {} |\n", f.kind, cell(&f.job), cell(&f.detail)));
        }
    }
    for note in &health.notes {
        out.push_str(&format!("\n⚠️ {}\n", cell(note)));
    }
    out
}

pub(crate) fn is_push_to_dev(env: &Env) -> bool {
    var("GITHUB_EVENT_NAME").is_some_and(|e| e == "push") && env.branch == "dev"
}

/// `wf ci cache-health [--json]`: prints the verdict as one JSON object and exits 1 when a problem was found and 0 (`ok` null, with the reason) when the run could not be read.
pub fn run(args: &[String]) -> Option<i32> {
    parse_spec(args, &SPEC)?;
    let verdict = Env::read().and_then(|env| {
        let jobs = list_jobs(&env)?;
        let judged: Vec<&Value> = jobs.iter().filter(|j| matches!(j["conclusion"].as_str(), Some("success" | "failure" | "timed_out")) && !is_meta(j)).collect();
        let logs: Vec<(String, String)> = judged.iter().filter_map(|j| Some((text(j, "name"), cached_log(&env, j).ok()?))).collect();
        let gate = is_push_to_dev(&env);
        // `WF_CI_PUSH_GATE=off`: the repo's pre-push hook runs no stage (lefthook.yml), so nothing was seeded for CI to replay
        let seeded = gate && var("WF_CI_PUSH_GATE").is_none_or(|v| v != "off");
        let mut health = assess(&logs, seeded);
        health.unreadable = judged.len() - logs.len();
        if health.unreadable > 0 {
            health.notes.push(format!("{} of {} job log(s) could not be read; their cache events are not judged.", health.unreadable, judged.len()));
        }
        if gate && health.events == 0 {
            health.notes.push("No cache event in any readable job log of a push to dev; the cache health could not be judged.".to_string());
        }
        Ok(health)
    });
    match verdict {
        Ok(health) => {
            let findings: Vec<Value> = health.findings.iter().map(|f| json!({ "kind": f.kind, "job": f.job, "detail": f.detail })).collect();
            let stages: Vec<Value> = health.stages.iter().map(|(job, stage, replayed, total)| json!({ "job": job, "stage": stage, "replayed": replayed, "tasks": total })).collect();
            println!("{}", json!({ "ok": health.findings.is_empty(), "events": health.events, "unreadable": health.unreadable, "findings": findings, "notes": health.notes, "stages": stages }));
            for f in &health.findings {
                wf_kit::wf_err!("wf ci cache-health: {} in {}: {}", f.kind, f.job, f.detail);
            }
            Some(i32::from(!health.findings.is_empty()))
        }
        Err(reason) => {
            wf_kit::wf_err!("wf ci cache-health: the verdict could not be computed, not failing the run: {reason}");
            println!("{}", json!({ "ok": null, "reason": reason }));
            Some(0)
        }
    }
}
