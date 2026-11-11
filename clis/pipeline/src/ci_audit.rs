//! `wf ci audit`: the nightly audit of CI on `dev`. It never gates a push: it reads the runs of the last day and says, as one JSON verdict,
//! what wasted time or resources and what to do next. A run's wall time says little (a cold run and a replayed one differ by an hour), so
//! each unit of work is judged against itself in the ledger (`ci_ledger`): a task that ran although its input hash was already produced is
//! a miss that should have been a hit; a task whose hash moved while nothing it depends on changed has a non-deterministic input; the same
//! work under the same hash taking far longer is contention. Beside them: the VM's CPU-seconds per run against a threshold, the cache
//! layers' own health (`ci_cache_health`), a pnpm store packed by a job other than the one that owns the save, the hit ratio of each Nx
//! stage, runner waits not explained by another run, idle gaps between jobs, an I/O stall on the VM, and (for a forced cold run) a job
//! slower than the recent cold runs.
use crate::ci_cache_health::{assess, fields, get};
use crate::ci_comment::{cell, cut, epoch, json_of, list_jobs_of, log_lines, read_log, redact, send, text, var, Env};
use crate::ci_insights::span;
use crate::ci_ledger::{JobRow, Observed};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &["dry-run", "cold"], values: &["since", "run"], positional: Positional::None, ..Spec::NONE };
const LIMITS_FILE: &str = ".github/ci-audit.json";
const BRANCH: &str = "dev";
const SCHEMA: i64 = 1;
const DEFAULT_WINDOW_SECS: i64 = 24 * 3600;
/// A cached result older than this may have expired (the Nx layer keeps entries 14 days), so a miss is no proof of a fault.
const CACHE_TTL_MS: i64 = 13 * 86_400_000;
/// Cold runs kept for the baseline.
const COLD_RUNS: usize = 4;
/// Stages with fewer tasks over the window say nothing about a ratio.
const MIN_TASKS_FOR_RATIO: u64 = 10;

/// The thresholds, committed in `.github/ci-audit.json`; a missing key keeps the default. They are thresholds, not gates: the nightly
/// agent moves them with evidence from the ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct Limits {
    /// A task is slower than its own history under the same hash by this percent and at least `slower_secs`.
    pub slower_pct: i64,
    pub slower_secs: i64,
    /// A cold job may exceed the median of the recent cold runs by this percent plus `slack_secs`.
    pub slack_pct: i64,
    pub slack_secs: i64,
    /// Jobs shorter than this are never judged against a cold baseline.
    pub floor_secs: i64,
    /// Seconds a job may wait for a runner that no other run was holding.
    pub wait_secs: i64,
    /// Idle time between jobs, as a share of the wall time and at least `idle_secs`.
    pub idle_pct: i64,
    pub idle_secs: i64,
    /// The host's I/O wait peak (percent of time stalled) over the run.
    pub io_wait_peak: f64,
    /// The most the CPU may wait on disk (percent of its time, `cpu_iowait`), the measure of a build stalled on I/O rather than the kernel's pressure figure.
    pub cpu_iowait_peak: f64,
    /// Most CPU-seconds the VM may spend over a run (average busy share of its 2 cores times the wall time); 0 turns the check off.
    pub cpu_seconds: i64,
    /// What each deploy leg beyond the first adds to `cpu_seconds`: a run that builds two images does two images' work.
    pub cpu_seconds_per_extra_deploy: i64,
    /// A stage that replayed less than this percent of its tasks over the window (at least ten tasks) is named; 0 turns the check off.
    pub hit_ratio_floor_pct: i64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { slower_pct: 50, slower_secs: 30, slack_pct: 25, slack_secs: 60, floor_secs: 120, wait_secs: 600, idle_pct: 5, idle_secs: 120, io_wait_peak: 60.0, cpu_iowait_peak: 15.0, cpu_seconds: 0, cpu_seconds_per_extra_deploy: 0, hit_ratio_floor_pct: 0 }
    }
}

impl Limits {
    fn read(raw: &str) -> Result<Limits, String> {
        let v: Value = serde_json::from_str(raw).map_err(|e| format!("{LIMITS_FILE}: {e}"))?;
        let d = Limits::default();
        let int = |key: &str, default: i64| v[key].as_i64().unwrap_or(default);
        Ok(Limits {
            slower_pct: int("slowerPercent", d.slower_pct),
            slower_secs: int("slowerSeconds", d.slower_secs),
            slack_pct: int("slackPercent", d.slack_pct),
            slack_secs: int("slackSeconds", d.slack_secs),
            floor_secs: int("floorSeconds", d.floor_secs),
            wait_secs: int("waitSeconds", d.wait_secs),
            idle_pct: int("idlePercent", d.idle_pct),
            idle_secs: int("idleSeconds", d.idle_secs),
            io_wait_peak: v["ioWaitPeak"].as_f64().unwrap_or(d.io_wait_peak),
            cpu_iowait_peak: v["cpuIowaitPeak"].as_f64().unwrap_or(d.cpu_iowait_peak),
            cpu_seconds: int("cpuSecondsThreshold", d.cpu_seconds),
            cpu_seconds_per_extra_deploy: int("cpuSecondsPerExtraDeploy", d.cpu_seconds_per_extra_deploy),
            hit_ratio_floor_pct: int("hitRatioFloorPercent", d.hit_ratio_floor_pct),
        })
    }
}

/// One job of a run as the audit sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct JobSpan {
    pub name: String,
    pub started: i64,
    pub ended: i64,
    /// Seconds waited for a runner that no other run was holding.
    pub queued: i64,
}

impl JobSpan {
    fn secs(&self) -> i64 {
        self.ended - self.started
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Breach {
    /// A correctness fault (the cache refused to write; a task reran what was already cached), as against a cost over a threshold.
    pub hard: bool,
    pub kind: &'static str,
    pub subject: String,
    pub message: String,
    /// Seconds the breach cost, where it can be said.
    pub waste_secs: i64,
}

fn breach(hard: bool, kind: &'static str, subject: &str, message: String, waste_ms: i64) -> Breach {
    Breach { hard, kind, subject: subject.to_string(), message, waste_secs: waste_ms / 1000 }
}

pub fn median(values: &[i64]) -> i64 {
    let mut v = values.to_vec();
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

/// What the task checks need besides this run's tasks.
pub struct Evidence<'a> {
    pub priors: &'a [Observed],
    /// Tasks seen replayed from a cache at least once: proof they can be cached.
    pub cacheable: &'a BTreeSet<String>,
    /// The newest earlier run's hash per task.
    pub previous: Option<&'a BTreeMap<String, String>>,
    /// Projects the commits since that run changed or depend on; `None` when it could not be asked.
    pub affected: Option<&'a BTreeSet<String>>,
    pub now_ms: i64,
}

fn project_of(task: &str) -> &str {
    task.rsplit_once(':').map_or(task, |(project, _)| project)
}

/// The task-level decisions: pure.
pub fn judge_tasks(now: &[Observed], ev: &Evidence, limits: &Limits) -> Vec<Breach> {
    let mut out = Vec::new();
    for o in now.iter().filter(|o| o.outcome == "ran" && o.passed) {
        let same: Vec<&Observed> = ev.priors.iter().filter(|p| p.hash == o.hash && p.task == o.task).collect();
        let produced = same.iter().find(|p| p.passed && ev.now_ms - p.ts <= CACHE_TTL_MS);
        if let (Some(prior), true) = (produced, ev.cacheable.contains(&o.task)) {
            out.push(breach(true, "should-have-hit", &o.task, format!("{} ran ({}) in {} although run {} already produced its result for the same inputs: the cache did not serve it", o.task, span(o.ms / 1000), o.job, prior.run), o.ms));
            continue;
        }
        let ran: Vec<i64> = same.iter().filter(|p| p.outcome == "ran" && p.passed).map(|p| p.ms).collect();
        if ran.len() >= 2 {
            let typical = median(&ran);
            if o.ms * 100 >= typical * (100 + limits.slower_pct) && o.ms - typical >= limits.slower_secs * 1000 {
                out.push(breach(false, "slower-same-work", &o.task, format!("{} took {} where its median is {} for the same inputs ({} earlier runs): the machine was slower, not the work", o.task, span(o.ms / 1000), span(typical / 1000), ran.len()), o.ms - typical));
            }
        }
    }
    if let (Some(previous), Some(affected)) = (ev.previous, ev.affected) {
        for o in now {
            if previous.get(&o.task).is_some_and(|before| *before != o.hash) && !affected.contains(project_of(&o.task)) {
                out.push(breach(false, "key-drift", &o.task, format!("the input hash of {} changed although nothing in {} or what it depends on did: something non-deterministic is in its key", o.task, project_of(&o.task)), if o.outcome == "ran" { o.ms } else { 0 }));
            }
        }
    }
    out
}

/// The time-level decisions: pure.
pub fn judge_time(jobs: &[JobSpan], refused_cache: &[String], io_wait_peak: Option<f64>, cpu_iowait_peak: Option<f64>, cpu_secs: Option<f64>, cold: Option<&BTreeMap<String, Vec<i64>>>, limits: &Limits) -> Vec<Breach> {
    let mut out = Vec::new();
    let deploys = jobs.iter().filter(|j| wf_bucket::job_name::job_name(&j.name).starts_with("deploy (")).count() as i64;
    let ceiling = limits.cpu_seconds + (deploys - 1).max(0) * limits.cpu_seconds_per_extra_deploy;
    if let Some(used) = cpu_secs.filter(|s| limits.cpu_seconds > 0 && *s > ceiling as f64) {
        out.push(breach(false, "cpu-seconds", "vm", format!("the VM spent {used:.0} CPU-seconds on this run (limit {ceiling} for {deploys} deploy legs)"), 0));
    }
    for job in jobs.iter().filter(|j| j.queued > limits.wait_secs) {
        out.push(breach(false, "runner-wait", &job.name, format!("{} waited {} for a runner that no other run was holding (limit {})", job.name, span(job.queued), span(limits.wait_secs)), job.queued * 1000));
    }
    for job in refused_cache {
        out.push(breach(true, "cache-read-only", job, format!("{job} could not write the build cache (its guard refused), so nothing it built is reused"), 0));
    }
    // judged on the whole percent the report shows, so "15% (limit 15%)" is never a breach
    if let Some(peak) = cpu_iowait_peak.filter(|p| p.round() > limits.cpu_iowait_peak) {
        out.push(breach(false, "cpu-iowait", "vm", format!("the CPU waited on disk up to {peak:.0}% of its time (limit {:.0}%)", limits.cpu_iowait_peak), 0));
    }
    if let Some(peak) = io_wait_peak.filter(|p| *p > limits.io_wait_peak) {
        out.push(breach(false, "io-wait", "vm", format!("the VM's I/O wait peaked at {peak:.0}% (limit {:.0}%)", limits.io_wait_peak), 0));
    }
    let spans: Vec<(i64, i64)> = jobs.iter().filter(|j| j.ended >= j.started).map(|j| (j.started, j.ended)).collect();
    if let Some((wall, _busy, idle)) = busy_and_idle(&spans) {
        if wall > 0 && idle >= limits.idle_secs && idle * 100 >= wall * limits.idle_pct {
            out.push(breach(false, "idle", "run", format!("{} of the {} wall time was spent between jobs, with nothing running", span(idle), span(wall)), idle * 1000));
        }
    }
    if let Some(cold) = cold {
        for job in jobs.iter().filter(|j| j.secs() >= limits.floor_secs) {
            let history = cold.get(&job.name).map(Vec::as_slice).unwrap_or_default();
            if history.len() >= 2 {
                let ceiling = median(history) * (100 + limits.slack_pct) / 100 + limits.slack_secs;
                if job.secs() > ceiling {
                    out.push(breach(false, "cold-slower", &job.name, format!("{} took {} on a cold run; the median of the last {} cold runs is {} (limit {})", job.name, span(job.secs()), history.len(), span(median(history)), span(ceiling)), (job.secs() - median(history)) * 1000));
                }
            }
        }
    }
    out
}

/// The wall time of the spans, the part of it any span was running, and the rest.
fn busy_and_idle(spans: &[(i64, i64)]) -> Option<(i64, i64, i64)> {
    let mut v = spans.to_vec();
    v.sort_unstable();
    let (first, last) = (v.first()?.0, v.iter().map(|s| s.1).max()?);
    let (mut busy, mut reach) = (0, first);
    for (from, to) in v {
        busy += (to - from.max(reach)).max(0);
        reach = reach.max(to);
    }
    Some((last - first, busy, last - first - busy))
}

fn successful(jobs: &[Value]) -> impl Iterator<Item = &Value> {
    jobs.iter().filter(|j| j["conclusion"] == "success" && !matches!(wf_bucket::job_name::job_name(&text(j, "name")), "report" | "budget"))
}

/// Seconds of `[from, to]` that some other run held the runner for.
fn covered(from: i64, to: i64, others: &[(i64, i64)]) -> i64 {
    let mut cut: Vec<(i64, i64)> = others.iter().map(|(a, b)| ((*a).max(from), (*b).min(to))).filter(|(a, b)| b > a).collect();
    cut.sort_unstable();
    let (mut total, mut reach) = (0, from);
    for (a, b) in cut {
        total += (b - a.max(reach)).max(0);
        reach = reach.max(b);
    }
    total
}

fn job_spans(jobs: &[Value], others: &[(i64, i64)]) -> Vec<JobSpan> {
    successful(jobs)
        .filter_map(|j| {
            let (started, ended) = (epoch(&j["started_at"])?, epoch(&j["completed_at"])?);
            let created = epoch(&j["created_at"]).unwrap_or(started);
            let waited = (started - created).max(0);
            let queued = (waited - covered(created, started, others)).max(0);
            (ended >= started).then(|| JobSpan { name: text(j, "name"), started, ended, queued })
        })
        .collect()
}

/// The kernel's I/O pressure and the CPU's own I/O wait, each the peak of the run.
fn io_wait_peaks(env: &Env, from_ms: i64, to_ms: i64) -> (Option<f64>, Option<f64>, Option<f64>) {
    let Ok(series) = wf_vmops::host_summary::fetch_scoped(&env.vm_url, "24h", Some("host")) else { return (None, None, None) };
    let points = series["points"].as_array().cloned().unwrap_or_default();
    let summary = wf_vmops::host_summary::summarize(&points, from_ms, to_ms, 180_000);
    let cpu_secs = summary["signals"]["cpu"]["avg"].as_f64().map(|avg| avg / 100.0 * 2.0 * ((to_ms - from_ms - 60_000) as f64 / 1000.0));
    (summary["signals"]["psi_io"]["peak"].as_f64(), summary["signals"]["cpu_iowait"]["peak"].as_f64(), cpu_secs)
}

/// When the other runs of the workflow held the runner, from their start to their last update.
fn other_runs(env: &Env) -> Vec<(i64, i64)> {
    let url = format!("{}/repos/{}/actions/workflows/deploy.yml/runs?branch={}&per_page=30", env.api, env.repo, env.branch);
    let Ok(reply) = send(env, "GET", &url, None).and_then(|r| json_of(r, "listing the runs")) else { return Vec::new() };
    reply["workflow_runs"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter(|r| r["id"].to_string().trim_matches('"') != env.run).filter_map(|r| Some((epoch(&r["run_started_at"])?, epoch(&r["updated_at"])?))).collect()
}

/// Projects `nx` says the commits `base..head` changed or that depend on what changed.
fn affected_projects(base: &str, head: &str) -> Option<BTreeSet<String>> {
    let root = crate::e2e_tests::repo();
    let nx = crate::nx::bin(&root).ok()?;
    let out = std::process::Command::new(nx).args(["show", "projects", "--affected", &format!("--base={base}"), &format!("--head={head}")]).current_dir(&root).env("NX_DAEMON", "false").env("NX_NO_CLOUD", "true").env("NX_TUI", "false").stdin(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

/// What to do about a finding, by kind: every finding of the verdict carries it.
fn next_step(kind: &str) -> &'static str {
    match kind {
        "should-have-hit" => "Find why the cache did not serve the task: compare its input hash with the earlier run's (`wf test drift`), read the same run's cache findings, then fix the key or the store. A correctness fault.",
        "cache-read-only" => "Read the job's `wf: build cache reads only for this run` line for the reason (a dirty tree, a refused guard) and remove it. A correctness fault.",
        "key-drift" => "Find the non-deterministic input of the task's hash (`wf test drift`: a timestamp, an absolute path, an unpinned version) and make it stable.",
        "slower-same-work" => "Check the host's CPU and I/O in that window (`wf vm ops host --window 24h`); fix a noisy neighbour on the runner, not the task.",
        "runner-wait" => "Check what held the runner (`wf vm ops host --window 24h`); more than one run at a time or a stuck job is the cause to remove.",
        "idle" => "Find the gap between the jobs (a `needs` chain that waits on a slow job, a runner pick-up delay) and shorten it.",
        "io-wait" | "cpu-iowait" => "Find what stalled the disk in that window (an image build, a store pack, a cache restore) and spread or shrink it.",
        "cpu-seconds" => "Rank the run's jobs by CPU-seconds in the ledger and cut the largest avoidable one; raise the threshold in .github/ci-audit.json only with evidence that the work is needed.",
        "cold-slower" => "Compare the job's cold time with the recent cold runs in the ledger and find what grew.",
        "hit-ratio" => "List the stage's tasks that ran instead of replaying (the ledger's `ran` rows) and fix their keys or inputs; lower hitRatioFloorPercent only with evidence that the stage legitimately changes every push.",
        "pnpm-packed-outside-flow-tests" => "Set `WF_PNPM_SAVE_JOB` correctly or add the missing guard so only flow-tests packs and uploads the pnpm store; the other jobs skip the pack.",
        "degraded" | "rejected-write" | "seeded-lost" => "Read the cache layer's event in the job log (`wf cache health`, docs/solutions/ci/cache-layers.md): a degraded store, a full bucket or a lost entry; fix the store or its guard. A correctness fault.",
        "pnpm-save-slow" => "The store is packed for nothing: find what grew it (`wf cache save pnpm` bytes) and trim it.",
        "drift" => "Read the drift report of the job (which task hashed differently and the cause it names) and make that input stable.",
        "replayed-nothing" => "The push gate seeded the stage and CI replayed none of it: compare the gate's hashes with CI's (`wf test drift`) and fix the one that differs.",
        _ => "Read the finding's message and the run's job log, fix the cause through the normal gate, or record it in the findings inbox issue.",
    }
}

/// One thing the audit found, in one run or over the window.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub kind: String,
    pub run: Option<String>,
    pub subject: String,
    pub hard: bool,
    pub waste_secs: i64,
    pub message: String,
}

impl Finding {
    fn of(run: &str, b: &Breach) -> Finding {
        Finding { kind: b.kind.to_string(), run: Some(run.to_string()), subject: b.subject.clone(), hard: b.hard, waste_secs: b.waste_secs, message: b.message.clone() }
    }

    fn json(&self) -> Value {
        json!({ "kind": self.kind, "run": self.run, "subject": self.subject, "hard": self.hard, "wasteSeconds": self.waste_secs, "message": cut(&self.message, 300), "next": next_step(&self.kind) })
    }
}

/// The jobs whose log carries a pnpm store pack that is not `save_job`'s.
fn pnpm_packed_outside(logs: &[(String, String)], save_job: &str) -> Vec<(String, f64)> {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    for (job, log) in logs.iter().filter(|(job, _)| wf_bucket::job_name::stage(job) != wf_bucket::job_name::stage(save_job)) {
        for line in log_lines(log).iter().filter(|l| l.contains("source=ci-cache")) {
            let f = fields(line);
            if get(&f, "msg") == "cache.save" && get(&f, "kind") == "pnpm" && get(&f, "verb") == "save" && get(&f, "done") == "true" {
                *out.entry(job.clone()).or_default() += get(&f, "seconds").parse::<f64>().unwrap_or(0.0);
            }
        }
    }
    out.into_iter().collect()
}

/// The ledger rows of this run's logs, per job.
fn observed(env: &Env, logs: &[(String, String)], ts: i64) -> Vec<Observed> {
    logs.iter()
        .flat_map(|(job, log)| crate::nx_ledger::from_log(log).into_iter().map(move |t| (job.clone(), t)))
        .map(|(job, t)| Observed { run: env.run.clone(), sha: env.sha.clone(), ts, job, task: t.task, hash: t.hash, outcome: t.outcome.to_string(), ms: t.ms, passed: t.passed })
        .collect()
}

/// What the audit made of one run.
struct RunVerdict {
    run: String,
    sha: String,
    cold: bool,
    hosted: bool,
    judged: bool,
    reason: Option<String>,
    tasks: usize,
    cpu_secs: Option<f64>,
    findings: Vec<Finding>,
    stages: Vec<(String, u64, u64)>,
    notes: Vec<String>,
}

type LedgerOpen = Result<Box<dyn crate::ci_ledger::Ledger>, String>;

fn judge_run(env: &Env, push_to_dev: bool, cold: bool, hosted: bool, limits: &Limits, ledger: &LedgerOpen, dry_run: bool) -> RunVerdict {
    let mut v = RunVerdict { run: env.run.clone(), sha: env.sha.clone(), cold, hosted, judged: false, reason: None, tasks: 0, cpu_secs: None, findings: Vec::new(), stages: Vec::new(), notes: Vec::new() };
    let jobs = match list_jobs_of(env, &env.run) {
        Ok(jobs) => jobs,
        Err(reason) => {
            v.reason = Some(format!("the jobs could not be listed: {}", cut(&reason, 160)));
            return v;
        }
    };
    let finished = |j: &&Value| matches!(j["conclusion"].as_str(), Some("success" | "failure" | "timed_out")) && !crate::ci_comment::is_meta(j);
    let logs: Vec<(String, String)> = jobs.iter().filter(finished).filter_map(|j| Some((text(j, "name"), read_log(env, j).ok()?))).collect();
    let health = assess(&logs, push_to_dev);
    v.findings.extend(health.findings.iter().map(|f| Finding { kind: f.kind.to_string(), run: Some(env.run.clone()), subject: f.job.clone(), hard: matches!(f.kind, "degraded" | "rejected-write" | "seeded-lost"), waste_secs: 0, message: f.detail.clone() }));
    v.stages = health.stages.iter().map(|(_, stage, replayed, total)| (stage.clone(), *replayed, *total)).collect();
    let save_job = var("WF_PNPM_SAVE_JOB").unwrap_or_else(|| "flow-tests".to_string());
    for (job, seconds) in pnpm_packed_outside(&logs, &save_job) {
        v.findings.push(Finding { kind: "pnpm-packed-outside-flow-tests".to_string(), run: Some(env.run.clone()), subject: job.clone(), hard: false, waste_secs: seconds as i64, message: format!("{job} packed and uploaded the pnpm store ({seconds:.0} s); only {save_job} should") });
    }
    if let Some(bad) = jobs.iter().find(|j| matches!(j["conclusion"].as_str(), Some("failure" | "timed_out" | "cancelled"))) {
        v.reason = Some(format!("{} did not pass, so the timings are not comparable", text(bad, "name")));
        return v;
    }
    v.judged = true;
    let spans = job_spans(&jobs, &other_runs(env));
    let ok_logs: Vec<(String, String)> = logs.iter().filter(|(job, _)| jobs.iter().any(|j| text(j, "name") == *job && j["conclusion"] == "success")).cloned().collect();
    let refused: Vec<String> = ok_logs.iter().filter(|(_, log)| log.contains("wf: build cache reads only for this run")).map(|(job, _)| job.clone()).collect();
    let started = spans.iter().map(|s| s.started).min().unwrap_or(0);
    let ended = spans.iter().map(|s| s.ended).max().unwrap_or(started);
    let now_obs = observed(env, &ok_logs, started * 1000);
    v.tasks = now_obs.len();
    let mut breaches = Vec::new();
    let mut cold_history = None;
    match ledger {
        Err(reason) => v.notes.push(format!("the ledger is unavailable ({}), so tasks were not compared with earlier runs", cut(reason, 160))),
        Ok(ledger) => {
            if cold {
                cold_history = if hosted { None } else { ledger.cold_jobs(COLD_RUNS).ok() };
            } else {
                let hashes: Vec<String> = now_obs.iter().map(|o| o.hash.clone()).collect::<BTreeSet<_>>().into_iter().collect();
                let tasks: Vec<String> = now_obs.iter().map(|o| o.task.clone()).collect::<BTreeSet<_>>().into_iter().collect();
                match (ledger.priors(&hashes, &env.run), ledger.ever_hit(&tasks), ledger.previous(&env.run)) {
                    (Ok(priors), Ok(cacheable), Ok(previous)) => {
                        let affected = previous.as_ref().and_then(|(sha, _)| affected_projects(sha, &env.sha));
                        if previous.is_some() && affected.is_none() {
                            v.notes.push("nx could not say which projects changed since the previous run, so changed input hashes were not judged".to_string());
                        }
                        let ev = Evidence { priors: &priors, cacheable: &cacheable, previous: previous.as_ref().map(|(_, h)| h), affected: affected.as_ref(), now_ms: ended * 1000 };
                        breaches.extend(judge_tasks(&now_obs, &ev, limits));
                    }
                    (priors, hit, previous) => {
                        let reason = [priors.err(), hit.err(), previous.err()].into_iter().flatten().next().unwrap_or_default();
                        v.notes.push(format!("the ledger could not be read ({}), so tasks were not compared with earlier runs", cut(&reason, 160)));
                    }
                }
            }
        }
    }
    // The VM's samples, waits behind its one runner and its cold baseline say nothing about a GitHub-hosted machine
    if hosted {
        v.notes.push("a GitHub-hosted run: its tasks and caches were judged, its times and the VM's load were not".to_string());
    } else {
        let (psi_io, cpu_iowait, cpu_secs) = io_wait_peaks(env, started * 1000, ended * 1000 + 60_000);
        v.cpu_secs = cpu_secs;
        breaches.extend(judge_time(&spans, &refused, psi_io, cpu_iowait, cpu_secs, cold_history.as_ref(), limits));
    }
    v.findings.extend(breaches.iter().map(|b| Finding::of(&env.run, b)));
    if !dry_run {
        if let Ok(ledger) = ledger {
            let kind = if cold { "cold" } else { "push" };
            let rows: Vec<JobRow> = spans.iter().map(|s| JobRow { run: env.run.clone(), sha: env.sha.clone(), ts: started * 1000, kind: kind.to_string(), job: s.name.clone(), secs: s.secs(), queued: s.queued }).collect();
            let rows = if hosted { Vec::new() } else { rows };
            if let Err(reason) = ledger.record_tasks(&now_obs).and_then(|()| ledger.record_jobs(&rows)) {
                v.notes.push(format!("the ledger could not be written: {}", cut(&reason, 160)));
            }
        }
    }
    v
}

/// A run is cold when it was dispatched by hand, started by a schedule, or its head commit asks for no cache (`[cache:no]`): nothing in it replays.
pub fn is_cold(event: &str, head_message: &str) -> bool {
    matches!(event, "workflow_dispatch" | "schedule") || head_message.contains("[cache:no]")
}

/// The workflows whose runs are judged, as (file, runs on a GitHub-hosted machine). The VM's own is the one every push to `dev` runs.
const WORKFLOWS: [(&str, bool); 2] = [("deploy.yml", false), ("ci-hosted.yml", true)];

/// A completed run to judge: its id, head sha, whether it is cold, and whether it ran on GitHub-hosted machines.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Picked {
    started: i64,
    id: String,
    sha: String,
    cold: bool,
    hosted: bool,
}

/// The runs of one workflow's listing that started at or after `since`: pushes, hand dispatches and schedules, never a pull request's.
fn pick_runs(reply: &Value, since: i64, hosted: bool) -> Vec<Picked> {
    reply["workflow_runs"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|r| matches!(r["event"].as_str(), Some("push" | "workflow_dispatch" | "schedule")))
        .filter_map(|r| Some(Picked { started: epoch(&r["run_started_at"])?, id: r["id"].to_string().trim_matches('"').to_string(), sha: text(r, "head_sha"), cold: is_cold(&text(r, "event"), r["head_commit"]["message"].as_str().unwrap_or_default()), hosted }))
        .filter(|p| p.started >= since)
        .collect()
}

/// The completed runs on `dev` of the workflows in `WORKFLOWS` that started at or after `since` (epoch seconds), oldest first. The VM's
/// listing failing fails the audit; the hosted one failing (the workflow switched off, or not there) is a note, not a failure.
fn runs_since(env: &Env, since: i64) -> Result<(Vec<Picked>, Vec<String>), String> {
    let (mut runs, mut notes) = (Vec::new(), Vec::new());
    for (file, hosted) in WORKFLOWS {
        let url = format!("{}/repos/{}/actions/workflows/{file}/runs?branch={BRANCH}&status=completed&per_page=50", env.api, env.repo);
        match send(env, "GET", &url, None).and_then(|r| json_of(r, "listing the runs")) {
            Ok(reply) => runs.extend(pick_runs(&reply, since, hosted)),
            Err(reason) if hosted => notes.push(format!("the runs of {file} could not be listed ({}), so only the other workflow was audited", cut(&reason, 120))),
            Err(reason) => return Err(reason),
        }
    }
    runs.sort();
    Ok((runs, notes))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn report_block(findings: &[Finding], judged: &str) -> String {
    if findings.is_empty() {
        return format!("\n### CI audit: nothing found\n\n{judged}\n");
    }
    let waste: i64 = findings.iter().map(|f| f.waste_secs).sum();
    let rows: String = findings.iter().map(|f| format!("| {}{} | {} | {} |\n", if f.hard { "correctness: " } else { "" }, f.kind, cell(&f.message), cell(next_step(&f.kind))) ).collect();
    format!("\n### CI audit: **{} finding(s)**, about {} avoidable\n\n{judged}\n\n| What | Numbers | Next step |\n|---|---|---|\n{rows}", findings.len(), span(waste))
}

fn attempt(args: &Args) -> Result<Value, String> {
    let env = Env::read()?;
    let env = Env { branch: BRANCH.to_string(), ..env };
    let limits = match std::fs::read_to_string(var("WF_CI_AUDIT_FILE").unwrap_or_else(|| LIMITS_FILE.to_string())) {
        Ok(raw) => Limits::read(&raw)?,
        Err(_) => Limits::default(),
    };
    let since = args.since.unwrap_or_else(|| now_secs() - DEFAULT_WINDOW_SECS);
    let mut listing_notes = Vec::new();
    let targets: Vec<Picked> = match &args.run {
        Some(run) => vec![Picked { started: 0, id: run.clone(), sha: env.sha.clone(), cold: args.cold, hosted: false }],
        None => {
            let (runs, notes) = runs_since(&env, since)?;
            listing_notes = notes;
            runs
        }
    };
    let ledger = crate::ci_ledger::open().and_then(|l| l.ensure().map(|()| l));
    let verdicts: Vec<RunVerdict> = targets.iter().map(|t| judge_run(&Env { run: t.id.clone(), sha: t.sha.clone(), ..env.clone() }, true, t.cold, t.hosted, &limits, &ledger, args.dry_run)).collect();

    let mut findings: Vec<Finding> = verdicts.iter().flat_map(|v| v.findings.clone()).collect();
    let mut by_stage: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for (stage, replayed, total) in verdicts.iter().flat_map(|v| v.stages.iter()) {
        let e = by_stage.entry(stage.clone()).or_default();
        (e.0, e.1) = (e.0 + replayed, e.1 + total);
    }
    let ratio = |replayed: u64, total: u64| (replayed * 100).checked_div(total).unwrap_or(0) as i64;
    for (stage, (replayed, total)) in &by_stage {
        if limits.hit_ratio_floor_pct > 0 && *total >= MIN_TASKS_FOR_RATIO && ratio(*replayed, *total) < limits.hit_ratio_floor_pct {
            findings.push(Finding { kind: "hit-ratio".to_string(), run: None, subject: stage.clone(), hard: false, waste_secs: 0, message: format!("{stage} replayed {replayed} of {total} task(s) over the window ({}%, floor {}%)", ratio(*replayed, *total), limits.hit_ratio_floor_pct) });
        }
    }
    let judged = verdicts.iter().filter(|v| v.judged).count();
    let mut notes: Vec<String> = verdicts.iter().flat_map(|v| v.notes.iter().map(|n| format!("run {}: {n}", v.run))).collect::<BTreeSet<_>>().into_iter().collect();
    notes.extend(listing_notes);
    if verdicts.is_empty() {
        notes.push("no completed CI run on dev in the window".to_string());
    }
    let summary = format!("Audited {} run(s) of CI on dev since {since} ({judged} judged for time and tasks).{}", verdicts.len(), notes.iter().map(|n| format!(" {n}.")).collect::<String>());
    let report = redact(&report_block(&findings, &summary), &env.token);
    if !args.dry_run {
        if let Some(path) = &env.summary {
            use std::io::Write;
            let _ = std::fs::OpenOptions::new().append(true).create(true).open(path).and_then(|mut f| writeln!(f, "{report}"));
        }
    }
    let runs: Vec<Value> = verdicts.iter().map(|v| json!({ "run": v.run, "sha": v.sha, "cold": v.cold, "hosted": v.hosted, "judged": v.judged, "reason": v.reason, "tasks": v.tasks, "cpuSeconds": v.cpu_secs.map(|s| s.round() as i64), "findings": v.findings.len() })).collect();
    let stages: Vec<Value> = by_stage.iter().map(|(stage, (replayed, total))| json!({ "stage": stage, "replayed": replayed, "tasks": total, "hitRatioPercent": ratio(*replayed, *total) })).collect();
    let threshold = json!({ "cpuSecondsThreshold": limits.cpu_seconds, "cpuSecondsPerExtraDeploy": limits.cpu_seconds_per_extra_deploy, "hitRatioFloorPercent": limits.hit_ratio_floor_pct, "file": LIMITS_FILE });
    Ok(json!({ "schema": SCHEMA, "ok": findings.is_empty(), "since": since, "runs": runs, "findings": findings.iter().map(Finding::json).collect::<Vec<_>>(), "stages": stages, "threshold": threshold, "notes": notes, "report": report }))
}

/// The parsed options.
pub struct Args {
    since: Option<i64>,
    run: Option<String>,
    cold: bool,
    dry_run: bool,
}

/// `wf ci audit [--since <epoch>] [--run <id> [--cold]] [--dry-run]`: always exit 0 with one JSON verdict; the audit never fails a push.
pub fn run(args: &[String]) -> Option<i32> {
    let parsed = parse_spec(args, &SPEC)?;
    let since = match parsed.val("since").map(str::parse::<i64>) {
        Some(Ok(secs)) => Some(secs),
        Some(Err(_)) => {
            wf_kit::wf_err!("wf ci audit: --since takes epoch seconds");
            return Some(2);
        }
        None => None,
    };
    let args = Args { since, run: parsed.val("run").map(str::to_string), cold: parsed.flag("cold"), dry_run: parsed.flag("dry-run") };
    match attempt(&args) {
        Ok(out) => {
            println!("{out}");
            Some(0)
        }
        Err(reason) => {
            let reason = redact(&reason, &var("GITHUB_TOKEN").unwrap_or_default());
            eprintln!("wf ci audit: {reason}");
            println!("{}", json!({ "schema": SCHEMA, "ok": null, "reason": reason, "findings": [] }));
            Some(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(run: &str, name: &str, hash: &str, outcome: &str, ms: i64, ts: i64) -> Observed {
        Observed { run: run.into(), sha: "s".into(), ts, job: "flow".into(), task: name.into(), hash: hash.into(), outcome: outcome.into(), ms, passed: true }
    }

    fn kinds(b: &[Breach]) -> Vec<&'static str> {
        b.iter().map(|b| b.kind).collect()
    }

    #[test]
    fn given_a_task_that_ran_on_inputs_already_cached_when_judged_then_it_is_waste_but_only_for_a_task_known_to_be_cacheable() {
        let priors = [task("1", "a:lint", "h", "ran", 9000, 1000)];
        let cacheable: BTreeSet<String> = ["a:lint".to_string()].into();
        fn ev<'a>(priors: &'a [Observed], c: &'a BTreeSet<String>) -> Evidence<'a> {
            Evidence { priors, cacheable: c, previous: None, affected: None, now_ms: 2000 }
        }
        let now = [task("2", "a:lint", "h", "ran", 8000, 2000)];
        let b = judge_tasks(&now, &ev(&priors, &cacheable), &Limits::default());
        assert_eq!((kinds(&b), b[0].hard, b[0].waste_secs), (vec!["should-have-hit"], true, 8));
        assert!(judge_tasks(&now, &ev(&priors, &BTreeSet::new()), &Limits::default()).is_empty(), "never replayed, so maybe not cacheable");
        let old = Evidence { priors: &priors, cacheable: &cacheable, previous: None, affected: None, now_ms: 1000 + CACHE_TTL_MS + 1 };
        assert!(judge_tasks(&now, &old, &Limits::default()).is_empty(), "the entry may have expired");
    }

    #[test]
    fn given_a_hash_that_moved_when_judged_then_it_is_drift_only_if_nothing_the_task_depends_on_changed() {
        let previous: BTreeMap<String, String> = [("a:lint".to_string(), "old".to_string())].into();
        let now = [task("2", "a:lint", "new", "ran", 1000, 1)];
        let judge = |affected: &BTreeSet<String>| judge_tasks(&now, &Evidence { priors: &[], cacheable: &BTreeSet::new(), previous: Some(&previous), affected: Some(affected), now_ms: 1 }, &Limits::default());
        assert_eq!(kinds(&judge(&BTreeSet::new())), ["key-drift"]);
        assert!(judge(&["a".to_string()].into_iter().collect()).is_empty());
    }

    #[test]
    fn given_the_same_work_much_slower_than_before_when_judged_then_it_is_contention_and_a_small_difference_is_not() {
        let priors = [task("1", "a:t", "h", "ran", 10_000, 1), task("2", "a:t", "h", "ran", 11_000, 2)];
        let ev = Evidence { priors: &priors, cacheable: &BTreeSet::new(), previous: None, affected: None, now_ms: 3 };
        assert_eq!(kinds(&judge_tasks(&[task("3", "a:t", "h", "ran", 60_000, 3)], &ev, &Limits::default())), ["slower-same-work"]);
        assert!(judge_tasks(&[task("3", "a:t", "h", "ran", 12_000, 3)], &ev, &Limits::default()).is_empty());
    }

    #[test]
    fn given_waits_a_refused_cache_an_io_stall_and_gaps_when_judged_then_each_is_named_and_the_run_is_cold_judged_against_cold_runs() {
        let job = |name: &str, started, ended, queued| JobSpan { name: name.into(), started, ended, queued };
        let jobs = [job("a", 0, 100, 0), job("b", 400, 1000, 900)];
        let b = judge_time(&jobs, &["a".into()], Some(85.0), Some(40.0), None, None, &Limits::default());
        assert_eq!(kinds(&b), ["runner-wait", "cache-read-only", "cpu-iowait", "io-wait", "idle"], "{b:?}");
        let cold: BTreeMap<String, Vec<i64>> = [("b".to_string(), vec![300, 320, 310])].into();
        assert!(kinds(&judge_time(&[job("b", 0, 900, 0)], &[], None, None, None, Some(&cold), &Limits::default())).contains(&"cold-slower"));
    }

    #[test]
    fn given_a_dispatched_run_or_a_push_tagged_cache_no_when_classified_then_it_is_cold_and_any_other_push_is_not() {
        assert!(is_cold("workflow_dispatch", "any message"));
        assert!(is_cold("push", "ci: a full cold run [services:all] [cache:no]"));
        assert!(!is_cold("push", "ci: an ordinary push [services:all] [cache:yes]"));
        assert!(!is_cold("push", ""));
    }

    #[test]
    fn given_a_scheduled_run_when_classified_then_it_is_cold_like_a_dispatched_one() {
        assert!(is_cold("schedule", ""));
        assert!(!is_cold("pull_request", "a message"));
    }

    #[test]
    fn given_a_listing_of_runs_when_picked_then_pushes_dispatches_and_schedules_in_the_window_remain_and_a_pull_request_does_not() {
        let run = |id: u64, event: &str, at: &str| json!({ "id": id, "event": event, "head_sha": "abc", "run_started_at": at });
        let listing = json!({ "workflow_runs": [run(1, "push", "2026-11-10T10:00:00Z"), run(2, "schedule", "2026-11-10T11:00:00Z"), run(3, "pull_request", "2026-11-10T12:00:00Z"), run(4, "workflow_dispatch", "2026-11-01T00:00:00Z")] });
        let since = epoch(&json!("2026-11-10T00:00:00Z")).unwrap();
        let picked: Vec<(String, bool, bool)> = pick_runs(&listing, since, true).into_iter().map(|p| (p.id, p.cold, p.hosted)).collect();
        assert_eq!(picked, [("1".to_string(), false, true), ("2".to_string(), true, true)]);
    }

    #[test]
    fn given_a_cpu_iowait_peak_at_the_limit_when_judged_then_only_a_whole_percent_over_it_is_a_breach() {
        let limits = Limits::default();
        assert!(judge_time(&[], &[], None, Some(15.4), None, None, &limits).is_empty());
        assert_eq!(kinds(&judge_time(&[], &[], None, Some(15.6), None, None, &limits)), ["cpu-iowait"]);
    }

    #[test]
    fn given_a_cpu_seconds_ceiling_when_a_run_spends_more_then_it_is_named_and_with_no_ceiling_it_is_not() {
        let limits = Limits { cpu_seconds: 1000, ..Limits::default() };
        assert_eq!(kinds(&judge_time(&[], &[], None, None, Some(1200.0), None, &limits)), ["cpu-seconds"]);
        assert!(judge_time(&[], &[], None, None, Some(900.0), None, &limits).is_empty());
        assert!(judge_time(&[], &[], None, None, Some(5000.0), None, &Limits::default()).is_empty());
    }

    #[test]
    fn given_a_run_with_two_deploy_legs_when_judged_then_the_second_leg_adds_its_allowance_and_more_work_still_breaches() {
        let limits = Limits { cpu_seconds: 1000, cpu_seconds_per_extra_deploy: 500, ..Limits::default() };
        let leg = |name: &str| JobSpan { name: name.into(), started: 0, ended: 10, queued: 0 };
        let two = [leg("deploy (reporting-service)"), leg("deploy (leetcode-auth-cookie-capture)")];
        assert!(judge_time(&two, &[], None, None, Some(1048.0), None, &limits).is_empty());
        assert_eq!(kinds(&judge_time(&two, &[], None, None, Some(1600.0), None, &limits)), ["cpu-seconds"]);
        assert_eq!(kinds(&judge_time(&[leg("deploy (reporting-service)")], &[], None, None, Some(1048.0), None, &limits)), ["cpu-seconds"]);
    }

    #[test]
    fn given_deploy_legs_named_behind_the_callers_job_when_judged_then_they_count_as_deploy_legs() {
        let limits = Limits { cpu_seconds: 1000, cpu_seconds_per_extra_deploy: 500, ..Limits::default() };
        let leg = |name: &str| JobSpan { name: name.into(), started: 0, ended: 10, queued: 0 };
        let two = [leg("run / deploy (reporting-service)"), leg("run / deploy (leetcode-auth-cookie-capture)")];
        assert!(judge_time(&two, &[], None, None, Some(1048.0), None, &limits).is_empty());
        assert_eq!(kinds(&judge_time(&two[..1], &[], None, None, Some(1048.0), None, &limits)), ["cpu-seconds"]);
    }

    #[test]
    fn given_the_saver_named_by_stage_when_a_sharded_job_of_that_stage_packs_then_it_is_not_outside() {
        let pack = "source=ci-cache msg=cache.save kind=pnpm verb=save done=true seconds=5".to_string();
        let logs = [("flow-tests (2/4)".to_string(), pack.clone()), ("integration-tests (1/2)".to_string(), pack)];
        let outside = pnpm_packed_outside(&logs, "run / flow-tests (1/4)");
        assert_eq!(outside.iter().map(|(job, _)| job.as_str()).collect::<Vec<_>>(), ["integration-tests (1/2)"]);
    }

    #[test]
    fn given_another_runs_interval_when_a_wait_is_measured_then_only_the_unexplained_part_counts() {
        assert_eq!(covered(0, 1000, &[(0, 600), (500, 700), (900, 2000)]), 800);
        assert_eq!(median(&[5, 1, 3]), 3);
    }

    #[test]
    fn given_a_limits_file_when_read_then_its_keys_win_and_the_rest_keep_their_defaults() {
        let l = Limits::read(r#"{"waitSeconds": 60, "ioWaitPeak": 40.5}"#).unwrap();
        assert_eq!((l.wait_secs, l.io_wait_peak, l.slower_pct), (60, 40.5, 50));
    }
}
