//! `wf ci timings` and `wf ci progress`: the durations of a run, from the Actions API, as gauges and as one JSON line.
//!
//! `timings` reads the finished jobs of a run: how long each took, how long it waited for a runner, and how long each of its steps took, then the
//! spread of each stage's shards (the slowest shard less the fastest, the number the weighted split exists to shrink) and the run's wall time. They go
//! to Better Stack as gauges (`--send`; best effort, never a failure) and are printed, so a person or an agent reads the same rows. `progress` reads
//! a running run: which jobs are not done, which step each is in and for how long, against what the same job took in the last green run of the branch.
use crate::ci_comment::{epoch_of, excerpt_of, json_of, list_jobs_of, redact, send, text, var, Env};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use wf_bucket::ci_metrics::{Gauges, ALL};
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &["send"], values: &["run"], positional: Positional::None, ..Spec::NONE };
const PROGRESS: Spec = Spec { values: &["run"], positional: Positional::None, ..Spec::NONE };
/// A step shorter than this is not a gauge: the series would be many and say nothing.
const STEP_MIN_S: i64 = 5;

pub(crate) fn stamp(job: &Value, key: &str) -> Option<i64> {
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
pub(crate) fn stage_name(name: &str) -> String {
    let last = name.rsplit(" / ").next().unwrap_or(name);
    split(last).0
}

pub(crate) fn shard_of(name: &str) -> String {
    split(name.rsplit(" / ").next().unwrap_or(name)).1
}

pub(crate) fn api_env(run: Option<String>) -> Result<Env, String> {
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

pub(crate) fn now() -> i64 {
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

/// The concurrency group of a job when it is a shard with a key, as `pipeline.yml` spells it (caller workflow and `-fresh` included, since equal keys of
/// two workflows are two groups): `WF_CI_SHARD_KEYS` is the plan's JSON array of keys of the integration shards, `WF_CI_CORE_SHARD_KEYS` the same for core-tests.
pub(crate) fn group_of(stage: &str, shard: &str) -> Option<String> {
    let (var_name, prefix) = match stage {
        "integration-tests" => ("WF_CI_SHARD_KEYS", "hosted-integration"),
        "core-tests" => ("WF_CI_CORE_SHARD_KEYS", "hosted-core-tests"),
        _ => return None,
    };
    let keys: Vec<String> = serde_json::from_str(&var(var_name)?).ok()?;
    let index = shard.split('/').next()?.parse::<usize>().ok()?.checked_sub(1)?;
    let fresh = if stage == "core-tests" && var("WF_CI_FRESH").as_deref() == Some("true") { "-fresh" } else { "" };
    Some(format!("{prefix}-{}-{}{fresh}", var("GITHUB_WORKFLOW").unwrap_or_default(), keys.get(index)?))
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
                queued_s: created.filter(|_| started > 0).map(|c| (started - c).max(0)),
                runner: j["runner_name"].as_str().filter(|r| !r.is_empty()).map(str::to_string),
            })
        })
        .collect();
    if let Err(why) = wf_bucket::ci_db::record_jobs_local(&rows) {
        wf_kit::wf_err!("wf: ci jobs not recorded: {why}");
    }
    if let Err(why) = wf_bucket::ci_db::record_steps_local(&step_rows(run_id, jobs)) {
        wf_kit::wf_err!("wf: ci steps not recorded: {why}");
    }
}

/// Every step of every finished job (`CI_STEPS`), so the fixed per-runner overhead (checkout, toolchain, cache restore) is a query.
pub(crate) fn step_rows(run_id: &str, jobs: &[Value]) -> Vec<wf_bucket::ci_db::StepRow> {
    let mut out = Vec::new();
    for j in jobs.iter().filter(|j| j["conclusion"].is_string()) {
        let Some(job) = j["name"].as_str() else { continue };
        for (index, s) in j["steps"].as_array().map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
            let (started, completed) = (stamp(s, "started_at"), stamp(s, "completed_at"));
            out.push(wf_bucket::ci_db::StepRow {
                run: run_id.to_string(),
                job: job.to_string(),
                step_no: s["number"].as_i64().unwrap_or(index as i64 + 1),
                step: s["name"].as_str().unwrap_or_default().to_string(),
                started,
                completed,
                duration_s: started.zip(completed).map(|(a, b)| (b - a).max(0)),
                conclusion: s["conclusion"].as_str().map(str::to_string),
            });
        }
    }
    out
}

/// Jobs closer than this are one hand-over: a job starts within a couple of seconds of the job it waited for ending.
const HANDOVER_S: i64 = 2;

/// The run's parallelism from its jobs' intervals: runner seconds, the most jobs at once, the average, the waits for a runner and the critical path (the chain
/// of jobs from the last to finish back through whichever finished last before each started; the API does not say which job needs which).
pub(crate) fn profile(run: &Value, run_id: &str, jobs: &[Value]) -> wf_bucket::ci_db::RunProfile {
    struct Span {
        name: String,
        from: i64,
        to: i64,
        queue: i64,
    }
    let spans: Vec<Span> = jobs
        .iter()
        .filter_map(|j| {
            let (from, to) = (stamp(j, "started_at")?, stamp(j, "completed_at")?);
            (to > from && j["conclusion"] != "skipped").then(|| Span { name: j["name"].as_str().unwrap_or_default().to_string(), from, to, queue: stamp(j, "created_at").map_or(0, |c| (from - c).max(0)) })
        })
        .collect();
    let mut events: Vec<(i64, i64)> = spans.iter().flat_map(|s| [(s.from, 1), (s.to, -1)]).collect();
    events.sort();
    let (mut now, mut peak) = (0, 0);
    for (_, delta) in events {
        now += delta;
        peak = peak.max(now);
    }
    let runner_s: i64 = spans.iter().map(|s| s.to - s.from).sum();
    let created = stamp(run, "run_started_at").or_else(|| stamp(run, "created_at")).or_else(|| spans.iter().map(|s| s.from).min());
    let wall = created.zip(spans.iter().map(|s| s.to).max()).map_or(0, |(a, b)| (b - a).max(0));
    let mut chain: Vec<&Span> = spans.iter().max_by_key(|s| s.to).into_iter().collect();
    while let Some(current) = chain.last().copied() {
        let before = spans.iter().filter(|s| s.from < current.from && s.to <= current.from + HANDOVER_S).max_by_key(|s| s.to);
        match before {
            Some(s) => chain.push(s),
            None => break,
        }
    }
    chain.reverse();
    let ratio = |a: f64, b: f64| if b > 0.0 { a / b } else { 0.0 };
    wf_bucket::ci_db::RunProfile {
        run: run_id.to_string(),
        workflow: run["name"].as_str().unwrap_or_default().to_string(),
        wall_s: wall as f64,
        runner_s: runner_s as f64,
        peak_jobs: peak,
        avg_jobs: ratio(runner_s as f64, wall as f64),
        queue_s_total: spans.iter().map(|s| s.queue).sum::<i64>() as f64,
        queue_s_max: spans.iter().map(|s| s.queue).max().unwrap_or(0) as f64,
        jobs: spans.len() as i64,
        critical_path_job: chain.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(" > "),
        critical_path_s: chain.first().zip(chain.last()).map_or(0.0, |(a, b)| (b.to - a.from) as f64),
        parallel_efficiency: ratio(runner_s as f64, wall as f64 * peak as f64),
    }
}

fn profile_json(p: &wf_bucket::ci_db::RunProfile) -> Value {
    json!({
        "workflow": p.workflow, "wall_s": p.wall_s, "runner_s": p.runner_s, "peak_jobs": p.peak_jobs, "avg_jobs": p.avg_jobs, "queue_s_total": p.queue_s_total, "queue_s_max": p.queue_s_max,
        "jobs": p.jobs, "critical_path_job": p.critical_path_job, "critical_path_s": p.critical_path_s, "parallel_efficiency": p.parallel_efficiency,
    })
}

/// `ci.run.*{run=<id>, workflow}`: the run's parallelism as gauges.
fn profile_gauges(g: &mut Gauges, p: &wf_bucket::ci_db::RunProfile) {
    for (metric, value) in [
        ("ci.run.runner_s", p.runner_s),
        ("ci.run.peak_jobs", p.peak_jobs as f64),
        ("ci.run.avg_jobs", p.avg_jobs),
        ("ci.run.queue_s_total", p.queue_s_total),
        ("ci.run.queue_s_max", p.queue_s_max),
        ("ci.run.critical_path_s", p.critical_path_s),
        ("ci.run.parallel_efficiency", p.parallel_efficiency),
    ] {
        g.set(metric, &p.run, &p.workflow, value);
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
    let shape = profile(&run, &env.run, &jobs);
    if let Err(why) = wf_bucket::ci_db::record_run_profile_local(&shape) {
        wf_kit::wf_err!("wf: ci run profile not recorded: {why}");
    }
    let mut sent = false;
    if parsed.flag("send") {
        let mut g = gauges(&table, &env.run);
        profile_gauges(&mut g, &shape);
        g.send();
        sent = true;
    }
    println!("{}", json!({"ok": true, "run": env.run, "sent": sent, "timings": table, "profile": profile_json(&shape)}));
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

/// The units the job-local database says are running: the rows exist only on the runner of the job that wrote them, so this is the answer on that runner and empty elsewhere.
fn running_units() -> Vec<wf_bucket::ci_db::UnitSpan> {
    wf_bucket::ci_db::local_path().filter(|p| p.exists()).and_then(|p| wf_bucket::ci_db::running_units_of(&p).ok()).unwrap_or_default()
}

/// `running_units` as a field of the view, and as `unit` on the open job of that stage, with how long the unit has run.
fn attach_units(view: &mut Value, units: &[wf_bucket::ci_db::UnitSpan], at: i64) {
    view["running_units"] = json!(units.iter().map(|u| json!({"stage": u.stage, "unit": u.unit, "running_s": (at - u.started_ms / 1000).max(0)})).collect::<Vec<_>>());
    for open in view["open"].as_array_mut().into_iter().flatten() {
        let stage = stage_name(open["job"].as_str().unwrap_or_default());
        let names: Vec<&str> = units.iter().filter(|u| u.stage == stage || u.job == stage).map(|u| u.unit.as_str()).collect();
        if !names.is_empty() {
            open["unit"] = json!(names.join(", "));
        }
    }
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
    let mut view = live(&jobs, &expected(&env, &run), now());
    attach_units(&mut view, &running_units(), now());
    println!("{}", json!({"ok": true, "run": env.run, "head": run["head_sha"], "status": run["status"], "conclusion": run["conclusion"], "progress": view}));
    Some(0)
}

const FAILED: Spec = Spec { flags: &["notify"], values: &["run", "runner"], positional: Positional::None, ..Spec::NONE };

/// The first Nx task of this job that failed, from the job-local database the stage's ledger wrote (empty outside a hosted job or before any task finished red).
fn failing_task() -> String {
    let Some(path) = wf_bucket::ci_db::local_path().filter(|p| p.exists()) else { return String::new() };
    wf_bucket::ci_db::tasks_of(&path).unwrap_or_default().into_iter().find(|t| !t.passed).map(|t| t.task).unwrap_or_default()
}

/// The URL the failure hook posts to: `CI_FAILURE_HOOK_URL` from the environment, else the vault; none is a no-op.
fn hook_url() -> Option<String> {
    var("CI_FAILURE_HOOK_URL").or_else(|| secrets::load_values(&["CI_FAILURE_HOOK_URL"]).get("CI_FAILURE_HOOK_URL").cloned().flatten().filter(|v| !v.is_empty()))
}

/// One JSON post of the failure to the hook. `Null` when there is no URL; otherwise `{status}` or `{error}`.
fn notify(body: &Value, token: &str) -> Value {
    let Some(url) = hook_url() else { return Value::Null };
    let sent = cli_http::Request::new("POST", &url).header("User-Agent", "wf-ci-failed").timeout(std::time::Duration::from_secs(15)).json(body).send();
    match sent {
        Ok(reply) => json!({"status": reply.status}),
        Err(why) => json!({"error": crate::ci_comment::redact(&why.to_string(), token).replace(&url, "[hook]")}),
    }
}

/// `wf ci failed [--notify]`: the step of this job that went red, read live from the Actions API (a job's log text is not published until it ends, but its steps
/// are). The job is the one running on this runner. When a step is red it also sends the `ci.job.failed` line (run, job, step, task, excerpt, sha, scrubbed) to the
/// OTLP logs ingest, refreshes the run's comment (`wf ci live`) so it shows the job as failing now, and with `--notify` posts the same fields to `CI_FAILURE_HOOK_URL`.
/// Always exits 0: it is a report from inside a failing job, never a second failure.
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
    // whether the API shows a running job's error annotations at all: counted here and sent with the report, so the telemetry answers it over time
    let annotations = send(&env, "GET", &format!("{}/repos/{}/check-runs/{}/annotations", env.api, env.repo, job["id"].as_u64().unwrap_or(0)), None)
        .ok()
        .filter(|r| r.ok())
        .and_then(|r| serde_json::from_str::<Value>(&r.text()).ok())
        .and_then(|v| v.as_array().map(Vec::len));
    let mut out = json!({"ok": true, "run": env.run, "job": job["name"], "url": job["html_url"], "failed_steps": steps, "annotations": annotations});
    if let Some(step) = steps.first() {
        let sha = if env.sha.is_empty() { var("GITHUB_SHA").unwrap_or_default() } else { env.sha.clone() };
        let (job_name, task) = (text(job, "name"), failing_task());
        let (_, excerpt) = excerpt_of(&env, job);
        let fields = [
            ("run", env.run.clone()),
            ("job", job_name.clone()),
            ("step", step.clone()),
            ("task", task.clone()),
            ("excerpt", redact(&excerpt, &env.token)),
            ("sha", sha.clone()),
            ("scrubbed", "env-secrets".to_string()),
            ("annotations", annotations.map_or_else(|| "unreadable".to_string(), |n| n.to_string())),
        ];
        out["task"] = json!(task);
        out["logged"] = if var("WF_CI_LOGS").as_deref() == Some("off") {
            json!({"skipped": "WF_CI_LOGS=off"})
        } else {
            wf_bucket::ci_metrics::log("error", "ci.job.failed", &fields).map_or_else(|why| json!({"error": redact(&why, &env.token)}), |()| json!({"sent": true}))
        };
        out["live"] = crate::ci_live::refresh();
        if parsed.flag("notify") {
            let body = json!({"event": "ci.job.failed", "run": env.run, "job": job_name, "step": step, "task": task, "sha": sha, "url": job["html_url"], "annotations": annotations, "excerpt": wf_bucket::ci_metrics::scrub(&redact(&excerpt, &env.token))});
            out["notified"] = notify(&body, &env.token);
        }
    }
    println!("{out}");
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
