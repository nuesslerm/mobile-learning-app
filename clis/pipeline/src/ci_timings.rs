//! `wf ci timings` and `wf ci progress`: the durations of a run, from the Actions API, as gauges and as one JSON line.
//!
//! `timings` reads the finished jobs of a run: how long each took, how long it waited for a runner, and how long each of its steps took, then the
//! spread of each stage's shards (the slowest shard less the fastest, the number the weighted split exists to shrink) and the run's wall time. They go
//! to Better Stack as gauges (`--send`; best effort, never a failure) and are printed, so a person or an agent reads the same rows. `progress` reads
//! a running run: which jobs are not done, which step each is in and for how long, against what the same job took in the last green run of the branch.
use crate::ci_comment::{epoch_of, json_of, list_jobs_of, send, var, Env};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use wf_bucket::ci_metrics::{Gauges, ALL};
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &["send"], values: &["run"], positional: Positional::None, ..Spec::NONE };
const PROGRESS: Spec = Spec { values: &["run"], positional: Positional::None, ..Spec::NONE };
/// A step shorter than this is not a gauge: the series would be many and say nothing.
const STEP_MIN_S: i64 = 5;

fn stamp(job: &Value, key: &str) -> Option<i64> {
    job[key].as_str().and_then(epoch_of)
}

/// `flow-tests (3/6)` is the stage `flow-tests` and the shard `3/6`; a job with no parentheses is its own stage.
fn split(name: &str) -> (String, String) {
    match name.split_once(" (") {
        Some((stage, rest)) => (stage.trim().to_string(), rest.trim_end_matches(')').to_string()),
        None => (name.to_string(), ALL.to_string()),
    }
}

/// The stage's name as a job names it: the part before ` / ` of a reusable workflow's `run / flow-tests (3/6)`, or its last part for a nested job.
fn stage_name(name: &str) -> String {
    let last = name.rsplit(" / ").next().unwrap_or(name);
    split(last).0
}

fn shard_of(name: &str) -> String {
    split(name.rsplit(" / ").next().unwrap_or(name)).1
}

fn api_env(run: Option<String>) -> Result<Env, String> {
    let token = var("GITHUB_TOKEN").or_else(gh_token).ok_or("GITHUB_TOKEN is not set and `gh auth token` gave none")?;
    let repo = var("GITHUB_REPOSITORY").or_else(origin_repo).ok_or("GITHUB_REPOSITORY is not set and the origin remote is not a GitHub repository")?;
    let run = run.or_else(|| var("GITHUB_RUN_ID")).ok_or("--run <id> is required outside a CI run")?;
    let base = |name: &str, default: &str| var(name).unwrap_or_else(|| default.to_string()).trim_end_matches('/').to_string();
    Ok(Env {
        token,
        repo,
        run,
        sha: var("GITHUB_SHA").unwrap_or_default(),
        api: base("GITHUB_API_URL", "https://api.github.com"),
        server: base("GITHUB_SERVER_URL", "https://github.com"),
        branch: var("GITHUB_REF_NAME").unwrap_or_else(|| "dev".to_string()),
        runner: String::new(),
        label: None,
        summary: None,
        results: None,
        vm_url: String::new(),
    })
}

fn gh_token() -> Option<String> {
    let out = std::process::Command::new("gh").args(["auth", "token"]).output().ok()?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !token.is_empty()).then_some(token)
}

fn origin_repo() -> Option<String> {
    let out = std::process::Command::new("git").args(["remote", "get-url", "origin"]).output().ok()?;
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let path = url.split_once("github.com").map(|(_, rest)| rest.trim_start_matches([':', '/']))?;
    Some(path.trim_end_matches(".git").trim_matches('/').rsplitn(3, '/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/"))
}

fn now() -> i64 {
    var("WF_CI_NOW").and_then(|n| epoch_of(&n)).unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64))
}

fn run_of(env: &Env) -> Result<Value, String> {
    json_of(send(env, "GET", &format!("{}/repos/{}/actions/runs/{}", env.api, env.repo, env.run), None)?, "reading the run")
}

/// The rows of the finished jobs of a run, the stages' spreads and the run's wall time.
pub(crate) fn rows(run: &Value, jobs: &[Value]) -> Value {
    let mut by_stage: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut steps_by: BTreeMap<(String, String), i64> = BTreeMap::new();
    let mut out = Vec::new();
    for job in jobs {
        let (Some(from), Some(to)) = (stamp(job, "started_at"), stamp(job, "completed_at")) else { continue };
        let name = job["name"].as_str().unwrap_or_default();
        let (stage, shard) = (stage_name(name), shard_of(name));
        let queue = stamp(job, "created_at").map(|c| (from - c).max(0));
        let steps: Vec<Value> = job["steps"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|s| {
                let d = stamp(s, "completed_at")? - stamp(s, "started_at")?;
                let step = s["name"].as_str()?.to_string();
                (d >= STEP_MIN_S).then(|| {
                    let slot = steps_by.entry((stage.clone(), step.clone())).or_insert(0);
                    *slot = (*slot).max(d);
                    json!({"name": step, "duration_s": d})
                })
            })
            .collect();
        by_stage.entry(stage.clone()).or_default().push(to - from);
        out.push(json!({"job": name, "stage": stage, "shard": shard, "conclusion": job["conclusion"], "duration_s": to - from, "queue_s": queue, "steps": steps}));
    }
    let spreads: Vec<Value> = by_stage.iter().filter(|(_, d)| d.len() > 1).map(|(stage, d)| json!({"stage": stage, "shards": d.len(), "spread_s": d.iter().max().unwrap_or(&0) - d.iter().min().unwrap_or(&0), "slowest_s": d.iter().max()})).collect();
    let created = stamp(run, "run_started_at").or_else(|| stamp(run, "created_at"));
    let ended = jobs.iter().filter_map(|j| stamp(j, "completed_at")).max();
    let wall = created.zip(ended).map(|(a, b)| (b - a).max(0));
    let slowest: Vec<Value> = steps_by.iter().map(|((stage, step), d)| json!({"stage": stage, "step": step, "slowest_s": d})).collect();
    json!({"jobs": out, "spreads": spreads, "steps": slowest, "wall_s": wall})
}

fn gauges(rows: &Value, run: &str) -> Gauges {
    let mut g = Gauges::new();
    for j in rows["jobs"].as_array().into_iter().flatten() {
        let (stage, shard) = (j["stage"].as_str().unwrap_or_default(), j["shard"].as_str().unwrap_or(ALL));
        g.set("ci.job.duration_s", stage, shard, j["duration_s"].as_f64().unwrap_or(0.0));
        if let Some(q) = j["queue_s"].as_f64() {
            g.set("ci.job.queue_s", stage, shard, q);
        }
    }
    for s in rows["spreads"].as_array().into_iter().flatten() {
        g.set("ci.stage.spread_s", s["stage"].as_str().unwrap_or_default(), ALL, s["spread_s"].as_f64().unwrap_or(0.0));
    }
    for s in rows["steps"].as_array().into_iter().flatten() {
        g.set("ci.step.duration_s", s["stage"].as_str().unwrap_or_default(), s["step"].as_str().unwrap_or_default(), s["slowest_s"].as_f64().unwrap_or(0.0));
    }
    if let Some(w) = rows["wall_s"].as_f64() {
        g.set("ci.run.wall_s", run, ALL, w);
    }
    g
}

fn fail(message: &str, next: &str) -> i32 {
    println!("{}", json!({"ok": false, "error": message, "next": next}));
    1
}

/// The concurrency group of a job when it is a shard with a key: `WF_CI_SHARD_KEYS` is the plan's JSON array of keys of the integration shards.
fn group_of(stage: &str, shard: &str) -> Option<String> {
    if stage != "integration-tests" {
        return None;
    }
    let keys: Vec<String> = serde_json::from_str(&var("WF_CI_SHARD_KEYS")?).ok()?;
    let index = shard.split('/').next()?.parse::<usize>().ok()?.checked_sub(1)?;
    keys.get(index).cloned()
}

/// Each finished job into the CI database's `CI_JOBS` (the job-local file the report job merges): how it ended, how long it ran, how long it waited
/// (a job cancelled while pending ran 0 s, never started, and waited until it was replaced), and the concurrency group it queued in. Best effort like the gauges.
fn keep_jobs(env: &Env, run: &Value, run_id: &str, jobs: &[Value]) {
    let sha = run["head_sha"].as_str().unwrap_or_default().to_string();
    let ts = now();
    let rows: Vec<wf_bucket::ci_db::JobRow> = jobs
        .iter()
        .filter_map(|j| {
            let conclusion = j["conclusion"].as_str()?.to_string();
            let name = j["name"].as_str()?;
            let (stage, shard) = (stage_name(name), shard_of(name));
            let (created, started, completed) = (stamp(j, "created_at"), stamp(j, "started_at").unwrap_or(0), stamp(j, "completed_at").unwrap_or(0));
            let secs = if started > 0 && completed > 0 { (completed - started).max(0) } else { 0 };
            let queued = match (created, started, completed) {
                (Some(c), s, _) if s > 0 => (s - c).max(0),
                (Some(c), _, e) if e > 0 => (e - c).max(0),
                _ => 0,
            };
            let red = matches!(conclusion.as_str(), "failure" | "timed_out");
            let (step, excerpt) = if red { crate::ci_comment::excerpt_of(env, j) } else { (String::new(), String::new()) };
            Some(wf_bucket::ci_db::JobRow {
                run: run_id.to_string(),
                sha: sha.clone(),
                ts,
                group_key: group_of(&stage, &shard),
                kind: stage,
                job: name.to_string(),
                secs,
                queued,
                conclusion,
                started,
                step: Some(step).filter(|s| !s.is_empty() && s != "-"),
                excerpt: Some(excerpt).filter(|e| !e.is_empty()),
            })
        })
        .collect();
    if let Err(why) = wf_bucket::ci_db::record_jobs_local(&rows) {
        wf_kit::wf_err!("wf: ci jobs not recorded: {why}");
    }
}

fn timings(rest: &[String]) -> Option<i32> {
    let parsed = parse_spec(rest, &SPEC)?;
    let env = match api_env(parsed.val("run").map(str::to_string)) {
        Ok(e) => e,
        Err(why) => return Some(fail(&why, "set GITHUB_TOKEN and GITHUB_REPOSITORY, or pass --run <id> with `gh` signed in")),
    };
    let read = run_of(&env).and_then(|run| list_jobs_of(&env, &env.run).map(|jobs| (run, jobs)));
    let (run, jobs) = match read {
        Ok(pair) => pair,
        Err(why) => return Some(fail(&why, "check the run id and the token's actions:read scope")),
    };
    let table = rows(&run, &jobs);
    keep_jobs(&env, &run, &env.run, &jobs);
    let mut sent = false;
    if parsed.flag("send") {
        gauges(&table, &env.run).send();
        sent = true;
    }
    println!("{}", json!({"ok": true, "run": env.run, "sent": sent, "timings": table}));
    Some(0)
}

/// What each job of the last green run of the branch took, by job name.
fn expected(env: &Env, run: &Value) -> BTreeMap<String, i64> {
    let branch = run["head_branch"].as_str().unwrap_or(&env.branch);
    let url = format!("{}/repos/{}/actions/workflows/{}/runs?branch={branch}&status=success&per_page=1", env.api, env.repo, run["workflow_id"]);
    let prior = send(env, "GET", &url, None).and_then(|r| json_of(r, "listing the green runs")).ok().and_then(|b| b["workflow_runs"].get(0).and_then(|r| r["id"].as_u64()));
    let Some(prior) = prior else { return BTreeMap::new() };
    let mut out = BTreeMap::new();
    for job in list_jobs_of(env, &prior.to_string()).unwrap_or_default() {
        if let (Some(a), Some(b)) = (stamp(&job, "started_at"), stamp(&job, "completed_at")) {
            out.insert(job["name"].as_str().unwrap_or_default().to_string(), b - a);
        }
    }
    out
}

/// The live view of a run: its jobs that are not done, with the step each is in.
pub(crate) fn live(jobs: &[Value], expected: &BTreeMap<String, i64>, at: i64) -> Value {
    let (mut done, mut running, mut waiting) = (0, 0, 0);
    let mut open = Vec::new();
    for job in jobs {
        match job["status"].as_str() {
            Some("completed") => done += 1,
            Some("in_progress") => running += 1,
            _ => waiting += 1,
        }
        if job["status"] == "completed" {
            continue;
        }
        let name = job["name"].as_str().unwrap_or_default();
        let started = stamp(job, "started_at");
        let step = job["steps"].as_array().and_then(|s| s.iter().find(|s| s["status"] == "in_progress"));
        open.push(json!({
            "job": name,
            "status": job["status"],
            "elapsed_s": started.map(|s| (at - s).max(0)),
            "waiting_s": if started.is_none() { stamp(job, "created_at").map(|c| (at - c).max(0)) } else { None },
            "step": step.and_then(|s| s["name"].as_str()),
            "step_elapsed_s": step.and_then(|s| stamp(s, "started_at")).map(|s| (at - s).max(0)),
            "expected_s": expected.get(name),
        }));
    }
    json!({"jobs_total": jobs.len(), "done": done, "running": running, "not_started": waiting, "open": open})
}

fn progress(rest: &[String]) -> Option<i32> {
    let parsed = parse_spec(rest, &PROGRESS)?;
    let env = match api_env(parsed.val("run").map(str::to_string)) {
        Ok(e) => e,
        Err(why) => return Some(fail(&why, "set GITHUB_TOKEN and GITHUB_REPOSITORY, or pass --run <id> with `gh` signed in")),
    };
    let read = run_of(&env).and_then(|run| list_jobs_of(&env, &env.run).map(|jobs| (run, jobs)));
    let (run, jobs) = match read {
        Ok(pair) => pair,
        Err(why) => return Some(fail(&why, "check the run id and the token's actions:read scope")),
    };
    let view = live(&jobs, &expected(&env, &run), now());
    println!("{}", json!({"ok": true, "run": env.run, "head": run["head_sha"], "status": run["status"], "conclusion": run["conclusion"], "progress": view}));
    Some(0)
}

const FAILED: Spec = Spec { values: &["run", "runner"], positional: Positional::None, ..Spec::NONE };

/// `wf ci failed`: the step of this job that went red, read live from the Actions API (a job's log text is not published until it ends, but its steps
/// are). The job is the one running on this runner. Always exits 0: it is a report from inside a failing job, never a second failure.
fn failed(rest: &[String]) -> Option<i32> {
    let parsed = parse_spec(rest, &FAILED)?;
    let env = match api_env(parsed.val("run").map(str::to_string)) {
        Ok(e) => e,
        Err(why) => {
            println!("{}", json!({"ok": false, "error": why, "next": "set GITHUB_TOKEN and GITHUB_REPOSITORY"}));
            return Some(0);
        }
    };
    let runner = parsed.val("runner").map(str::to_string).or_else(|| var("RUNNER_NAME")).unwrap_or_default();
    let jobs = match list_jobs_of(&env, &env.run) {
        Ok(j) => j,
        Err(why) => {
            println!("{}", json!({"ok": false, "error": why, "next": "check the token's actions:read scope"}));
            return Some(0);
        }
    };
    let mine = jobs.iter().find(|j| j["status"] == "in_progress" && !runner.is_empty() && j["runner_name"].as_str() == Some(runner.as_str()));
    let Some(job) = mine else {
        println!("{}", json!({"ok": false, "error": format!("no running job on runner '{runner}'"), "next": "pass --runner <name>"}));
        return Some(0);
    };
    let steps: Vec<String> = job["steps"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter(|s| s["conclusion"] == "failure").filter_map(|s| s["name"].as_str().map(str::to_string)).collect();
    println!("{}", json!({"ok": true, "run": env.run, "job": job["name"], "url": job["html_url"], "failed_steps": steps}));
    Some(0)
}

/// `wf ci timings [--run <id>] [--send]`, `wf ci progress [--run <id>]` and `wf ci failed [--runner <name>]`.
pub fn run(verb: &str, rest: &[String]) -> Option<i32> {
    match verb {
        "timings" => timings(rest),
        "progress" => progress(rest),
        "failed" => failed(rest),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_job_names_when_split_then_the_stage_and_the_shard_are_those_of_the_last_part() {
        assert_eq!((stage_name("run / flow-tests (3/6)"), shard_of("run / flow-tests (3/6)")), ("flow-tests".to_string(), "3/6".to_string()));
        assert_eq!((stage_name("run / wf-build"), shard_of("run / wf-build")), ("wf-build".to_string(), "all".to_string()));
    }
}
