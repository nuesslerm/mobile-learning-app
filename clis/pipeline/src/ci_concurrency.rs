//! `wf ci concurrency [--phase start|end]`: how many of the account's runner slots are taken right now, from the Actions API: the jobs in progress ('busy') and
//! queued of the repository's in-progress and queued runs, per workflow and in all, against the cap of concurrent jobs (20). One row per workflow to `CI_CONCURRENCY`
//! (the job's local database, merged by the report job) and the gauges `ci.runners.busy|queued|headroom`. Best effort: it always exits 0.
use crate::ci_comment::{json_of, list_jobs_of, send, var, Env};
use crate::ci_timings::{api_env, now};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use wf_bucket::ci_db::ConcurrencyRow;
use wf_bucket::ci_metrics::Gauges;
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { values: &["phase", "run"], positional: Positional::None, ..Spec::NONE };
/// The account's concurrent-job limit on GitHub-hosted runners; `WF_CI_RUNNER_CAP` overrides it when the plan changes.
const CAP: i64 = 20;
/// The row that sums every workflow.
const TOTAL: &str = "all";
/// A listing of this many runs is already more than the cap can be running; the rest would cost API calls for nothing.
const MAX_RUNS: usize = 50;

/// (busy, queued, runs active) per workflow, the sum under `all`.
pub(crate) fn count(runs: &[(Value, Vec<Value>)]) -> BTreeMap<String, (i64, i64, i64)> {
    let mut out: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    for (run, jobs) in runs {
        let name = run["name"].as_str().unwrap_or("unknown").to_string();
        let busy = jobs.iter().filter(|j| j["status"] == "in_progress").count() as i64;
        let queued = jobs.iter().filter(|j| j["status"] == "queued").count() as i64;
        for key in [name, TOTAL.to_string()] {
            let slot = out.entry(key).or_default();
            *slot = (slot.0 + busy, slot.1 + queued, slot.2 + 1);
        }
    }
    out.entry(TOTAL.to_string()).or_default();
    out
}

/// The rows of one reading; every row's headroom is the account's (the cap less all busy jobs), not the workflow's.
pub(crate) fn rows(counts: &BTreeMap<String, (i64, i64, i64)>, run: &str, phase: &str, ts: i64, cap: i64) -> Vec<ConcurrencyRow> {
    let headroom = (cap - counts.get(TOTAL).map_or(0, |c| c.0)).max(0);
    counts.iter().map(|(workflow, (busy, queued, runs_active))| ConcurrencyRow { ts, run: run.to_string(), phase: phase.to_string(), workflow: workflow.clone(), busy: *busy, queued: *queued, runs_active: *runs_active, cap, headroom }).collect()
}

fn listing(env: &Env, status: &str) -> Result<Vec<Value>, String> {
    let url = format!("{}/repos/{}/actions/runs?status={status}&per_page=100", env.api, env.repo);
    let body = json_of(send(env, "GET", &url, None)?, "listing the active runs")?;
    Ok(body["workflow_runs"].as_array().cloned().unwrap_or_default())
}

fn read(env: &Env) -> Result<Vec<(Value, Vec<Value>)>, String> {
    let mut runs = listing(env, "in_progress")?;
    for run in listing(env, "queued")? {
        if !runs.iter().any(|r| r["id"] == run["id"]) {
            runs.push(run);
        }
    }
    runs.truncate(MAX_RUNS);
    runs.into_iter().map(|run| list_jobs_of(env, &run["id"].as_u64().unwrap_or(0).to_string()).map(|jobs| (run, jobs))).collect()
}

fn concurrency(rest: &[String]) -> Option<i32> {
    let parsed = parse_spec(rest, &SPEC)?;
    let phase = parsed.val("phase").unwrap_or("start").to_string();
    let say = |ok: bool, error: &str, next: &str| {
        println!("{}", json!({"ok": ok, "error": error, "next": next}));
        Some(0)
    };
    if !matches!(phase.as_str(), "start" | "end") {
        return say(false, "--phase is start or end", "wf ci concurrency --phase start");
    }
    let run = parsed.val("run").map(str::to_string).or_else(|| var("GITHUB_RUN_ID")).unwrap_or_default();
    let env = match api_env(Some(run.clone())) {
        Ok(e) => e,
        Err(why) => return say(false, &why, "set GITHUB_TOKEN and GITHUB_REPOSITORY"),
    };
    let cap = var("WF_CI_RUNNER_CAP").and_then(|c| c.parse().ok()).unwrap_or(CAP);
    let active = match read(&env) {
        Ok(a) => a,
        Err(why) => return say(false, &why, "check the token's actions:read scope"),
    };
    let counts = count(&active);
    let table = rows(&counts, &run, &phase, now(), cap);
    if let Err(why) = wf_bucket::ci_db::record_concurrency_local(&table) {
        wf_kit::wf_err!("wf: ci concurrency not recorded: {why}");
    }
    let mut g = Gauges::new();
    for r in &table {
        g.set("ci.runners.busy", &r.workflow, &phase, r.busy as f64).set("ci.runners.queued", &r.workflow, &phase, r.queued as f64).set("ci.runners.headroom", &r.workflow, &phase, r.headroom as f64);
    }
    g.send();
    let (busy, queued, runs_active) = counts.get(TOTAL).copied().unwrap_or_default();
    let workflows: Vec<Value> = counts.iter().filter(|(w, _)| w.as_str() != TOTAL).map(|(w, (b, q, r))| json!({"workflow": w, "busy": b, "queued": q, "runs_active": r})).collect();
    println!("{}", json!({"ok": true, "run": run, "phase": phase, "cap": cap, "busy": busy, "queued": queued, "runs_active": runs_active, "headroom": table.first().map_or(cap, |r| r.headroom), "workflows": workflows}));
    Some(0)
}

/// `wf ci concurrency [--phase start|end] [--run <id>]`.
pub fn run(verb: &str, rest: &[String]) -> Option<i32> {
    (verb == "concurrency").then(|| concurrency(rest)).flatten()
}
