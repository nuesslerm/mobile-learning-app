//! The telemetry lines of the CI comment: the run's parallelism, the queue waits, what each runner did, where the fixed cost went and what the repository's
//! runner pool looked like. The comment is built before `wf ci db merge` (the sealed database does not hold this run yet), so everything is recomputed here
//! from the same raw inputs the `wf ci timings|resources|concurrency` verbs read: the jobs, the sampler's `stats-<job>.jsonl` files and the job databases.
use crate::ci_comment::{cell, json_of, send, text, var, Env};
use crate::ci_insights::span;
use serde_json::Value;
use std::path::PathBuf;
use wf_bucket::ci_db::{JobResources, RunProfile, StepResources, StepRow, TaskResources};

/// The composite steps every job spends before its own work.
const FIXED_STEPS: [&str; 2] = ["Run ./.github/actions/setup-job", "Run ./.github/actions/install-test-deps"];
/// The jobs that are all fixed cost.
const FIXED_JOBS: [&str; 2] = ["wf-build", "plan"];
/// A wait for a runner shorter than this is nothing to name.
const WAIT_S: i64 = 30;

#[derive(Default)]
pub(crate) struct Telemetry {
    /// The workflow's name as the run says it, known from the first render; the profile only exists once the run is over.
    pub workflow: String,
    pub profile: Option<RunProfile>,
    pub resources: Vec<JobResources>,
    /// (what, total seconds, jobs)
    pub fixed: Vec<(String, i64, usize)>,
    pub waits: Vec<String>,
    /// (job, its tasks, its cores)
    pub tasks: Vec<(String, Vec<TaskResources>, i64)>,
    pub concurrency: Option<String>,
    /// The run's shard split against what the shards measured (`shard-weights --report`), from the jobs' unit spans.
    pub shards: Vec<crate::shard_weights::StageReport>,
    /// Every finished step of the run, and what the job's samples saw during each (`CI_STEP_RESOURCES`, recomputed here like the rows `wf ci resources` keeps).
    pub step_rows: Vec<StepRow>,
    pub steps: Vec<StepResources>,
    /// (job, tasks that ran, tasks a cache replayed) from the jobs' Nx ledgers.
    pub replay: Vec<(String, usize, usize)>,
    /// Sustained memory or I/O pressure per job from the sampler's files; the stored rows keep only peaks, so a previous run has none.
    pub pressure: Vec<crate::ci_resources::Pressure>,
}

pub(crate) struct Parts {
    pub headline: String,
    pub verdict: String,
    pub tables: String,
    pub details: String,
}

/// The sampler's files of the run: (job, samples).
pub(crate) fn runner_samples() -> Vec<(String, Vec<Value>)> {
    let Some(dir) = var("WF_CI_RUNNER_STATS_DIR") else { return Vec::new() };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect()).unwrap_or_default();
    files.sort();
    files
        .iter()
        .map(|p| (p.file_stem().and_then(|n| n.to_str()).unwrap_or("job").trim_start_matches("stats-").to_string(), crate::ci_resources::samples_of(p)))
        .filter(|(_, s)| !s.is_empty())
        .collect()
}

/// The cores the sampler saw and the memory in MB, per job; `None` for what it did not write.
pub(crate) fn machine(samples: &[Value]) -> (Option<u32>, Option<f64>) {
    let max = |key: &str| samples.iter().filter_map(|s| s[key].as_f64()).fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v)))).filter(|v| *v > 0.0);
    (max("cores").map(|c| c as u32), max("memTotalMb"))
}

/// The one cores figure every sampled job agrees on.
pub(crate) fn common_cores(samples: &[(String, Vec<Value>)]) -> Option<u32> {
    let mut cores = samples.iter().map(|(_, s)| machine(s).0);
    let first = cores.next()??;
    cores.all(|c| c == Some(first)).then_some(first)
}

pub(crate) fn ledger_files() -> Vec<(String, PathBuf)> {
    let Some(dir) = var("WF_CI_LEDGERS") else { return Vec::new() };
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().filter_map(Result::ok) {
        let file = if entry.path().is_dir() { entry.path().join("rows.sqlite") } else { continue };
        if file.exists() {
            out.push((entry.file_name().to_string_lossy().trim_start_matches("ci-db-").to_string(), file));
        }
    }
    out.sort();
    out
}

fn base(job: &Value) -> String {
    wf_bucket::job_name::job_name(&text(job, "name")).to_string()
}

/// Reads what the report needs; `finished` is whether the run is over (the profile and the end reading of the pool are about a finished run).
pub(crate) fn gather(env: &Env, jobs: &[&Value], finished: bool) -> Telemetry {
    let mut t = Telemetry::default();
    let samples = runner_samples();
    let workflow = env.label.clone().unwrap_or_default();
    t.resources = samples.iter().filter_map(|(job, s)| crate::ci_resources::job_resources(&env.run, job, &workflow, s)).collect();
    t.pressure = samples.iter().filter_map(|(job, s)| crate::ci_resources::sustained_pressure(job, s)).collect();
    let owned: Vec<Value> = jobs.iter().map(|j| (*j).clone()).collect();
    let run = send(env, "GET", &format!("{}/repos/{}/actions/runs/{}", env.api, env.repo, env.run), None).and_then(|r| json_of(r, "reading the run")).unwrap_or(Value::Null);
    t.workflow = run["name"].as_str().unwrap_or_default().to_string();
    if finished {
        t.profile = Some(crate::ci_timings::profile(&run, &env.run, &owned)).filter(|p| p.jobs > 0);
    }
    let steps = crate::ci_timings::step_rows(&env.run, &owned);
    for name in FIXED_STEPS {
        let rows: Vec<_> = steps.iter().filter(|r| r.step == name).collect();
        let total: i64 = rows.iter().filter_map(|r| r.duration_s).sum();
        if !rows.is_empty() {
            t.fixed.push((name.trim_start_matches("Run ./.github/actions/").to_string(), total, rows.len()));
        }
    }
    for name in FIXED_JOBS {
        let ran: Vec<i64> = jobs.iter().filter(|j| base(j) == name).filter_map(|j| Some(crate::ci_timings::stamp(j, "completed_at")? - crate::ci_timings::stamp(j, "started_at")?)).collect();
        if !ran.is_empty() {
            t.fixed.push((format!("job {name}"), ran.iter().sum(), ran.len()));
        }
    }
    for j in jobs {
        let queued = crate::ci_timings::stamp(j, "started_at").zip(crate::ci_timings::stamp(j, "created_at")).map_or(0, |(s, c)| (s - c).max(0));
        if queued < WAIT_S {
            continue;
        }
        let name = text(j, "name");
        let key = crate::ci_timings::group_of(&crate::ci_timings::stage_name(&name), &crate::ci_timings::shard_of(&name));
        t.waits.push(match key {
            Some(key) => format!("{} queued {}; its concurrency group is `{key}`, so it waits while the same shard with identical inputs of another run holds it", cell(&name), span(queued)),
            None => format!("{} waited {} for a runner", cell(&name), span(queued)),
        });
    }
    let mut start = None;
    let mut spans: Vec<wf_bucket::ci_db::UnitSpan> = Vec::new();
    for (job, file) in ledger_files() {
        spans.extend(wf_bucket::ci_db::units_of(&file, Some(&env.run)).unwrap_or_default().into_iter().filter(|u| u.outcome != "running"));
        if let Ok(tasks) = wf_bucket::ci_db::tasks_of(&file) {
            let (mine, other): (Vec<_>, Vec<_>) = tasks.into_iter().partition(|x| x.run == env.run);
            for x in &mine {
                let at = match t.replay.iter().position(|r| r.0 == x.job) {
                    Some(at) => at,
                    None => {
                        t.replay.push((x.job.clone(), 0, 0));
                        t.replay.len() - 1
                    }
                };
                if x.outcome == "hit" { t.replay[at].2 += 1 } else { t.replay[at].1 += 1 }
            }
            let used = if mine.is_empty() { other } else { mine };
            let sampled = samples.iter().find(|(n, _)| *n == job).map(|(_, s)| s.as_slice()).unwrap_or_default();
            let rows = crate::ci_resources::task_resources(&env.run, &job, &used, sampled);
            if !rows.is_empty() {
                t.tasks.push((job.clone(), rows, machine(sampled).0.map_or(0, i64::from)));
            }
        }
        if let Ok(rows) = wf_bucket::ci_db::concurrency_of(&file) {
            start = start.or_else(|| rows.into_iter().find(|r| r.run == env.run && r.phase == "start" && r.workflow == crate::ci_concurrency::TOTAL));
        }
    }
    t.steps = crate::ci_resources::step_resources(&steps, &samples);
    t.step_rows = steps;
    let names: Vec<String> = jobs.iter().map(|j| text(j, "name")).collect();
    t.shards = crate::shard_weights::report(&spans, &crate::shard_weights::shard_counts(names.iter().map(String::as_str)));
    let end = finished.then(|| crate::ci_concurrency::read(env).ok()).flatten().map(|runs| crate::ci_concurrency::count(&runs));
    let end = end.and_then(|c| c.get(crate::ci_concurrency::TOTAL).copied());
    t.concurrency = match (start, end) {
        (None, None) => None,
        (start, end) => {
            let cap = start.as_ref().map_or_else(crate::ci_concurrency::cap, |s| s.cap);
            let at = |b: i64, q: i64, h: i64| format!("repo busy {b}/{cap}, queued {q}, headroom {h}");
            let first = start.as_ref().map(|s| format!("{} at plan", at(s.busy, s.queued, s.headroom)));
            let last = end.map(|(b, q, _)| format!("{} at report", at(b, q, (cap - b).max(0))));
            Some([first, last].into_iter().flatten().collect::<Vec<_>>().join(" → "))
        }
    };
    t
}

fn pct(part: f64, whole: f64) -> f64 {
    if whole > 0.0 { part * 100.0 / whole } else { 0.0 }
}

/// The headline figures, the verdict paragraph, the compact tables and the details block.
pub(crate) fn section(t: &Telemetry, failed: &[String]) -> Parts {
    let mut parts = Parts { headline: String::new(), verdict: String::new(), tables: String::new(), details: String::new() };
    let fixed_total: i64 = t.fixed.iter().map(|f| f.1).sum();
    if let Some(p) = &t.profile {
        parts.headline = format!(
            "⏱️ wall {} · runner time {} · peak {} / avg {:.1} jobs at once · parallel efficiency {:.0}% · {} job{} ran",
            span(p.wall_s as i64),
            span(p.runner_s as i64),
            p.peak_jobs,
            p.avg_jobs,
            p.parallel_efficiency * 100.0,
            p.jobs,
            if p.jobs == 1 { "" } else { "s" }
        );
        let mut say: Vec<String> = Vec::new();
        if !p.critical_path_job.is_empty() {
            say.push(format!("The critical path is {} ({}, {:.0}% of the wall time).", cell(&p.critical_path_job), span(p.critical_path_s as i64), pct(p.critical_path_s, p.wall_s)));
        }
        if fixed_total > 0 && p.runner_s > 0.0 {
            say.push(format!("Fixed cost (setup, dependency install, wf build, plan) was {} of {} runner time ({:.0}%).", span(fixed_total), span(p.runner_s as i64), pct(fixed_total as f64, p.runner_s)));
        }
        if p.queue_s_total > 0.0 {
            say.push(format!("Jobs waited {} for a runner in total, {} at worst.", span(p.queue_s_total as i64), span(p.queue_s_max as i64)));
        }
        let loosest = t.resources.iter().filter(|r| r.duration_s >= 60.0).max_by(|a, b| a.idle_s.total_cmp(&b.idle_s));
        if let Some(r) = loosest.filter(|r| r.idle_s > 0.0) {
            let used = if r.cores > 0 { format!(" and used {:.1} of {} cores", r.cpu_avg / 100.0 * r.cores as f64, r.cores) } else { String::new() };
            say.push(format!("{} had its CPU idle {:.0}% of the time{used}.", cell(&r.job), pct(r.idle_s, r.duration_s)));
        }
        if !failed.is_empty() {
            say.push(format!("Failed: {}.", failed.iter().take(5).map(|n| cell(n)).collect::<Vec<_>>().join(", ")));
        }
        parts.verdict = say.join(" ");
    }
    if !t.fixed.is_empty() {
        parts.tables.push_str("\n| Fixed cost | Total | Jobs | Average |\n|---|---|---|---|\n");
        for (what, total, n) in &t.fixed {
            parts.tables.push_str(&format!("| {} | {} | {n} | {} |\n", cell(what), span(*total), span(*total / (*n).max(1) as i64)));
        }
    }
    if let Some(line) = &t.concurrency {
        parts.tables.push_str(&format!("\n🏃 Runner pool: {line}.\n"));
    }
    if !t.waits.is_empty() {
        parts.tables.push_str(&format!("\n{}\n", t.waits.iter().take(8).map(|w| format!("⏳ {w}")).collect::<Vec<_>>().join("<br>")));
    }
    if !t.tasks.is_empty() {
        let mut rows = String::new();
        for (job, tasks, cores) in &t.tasks {
            let concurrent = tasks.iter().map(|x| x.concurrent).max().unwrap_or(1);
            let cores_used: f64 = tasks.iter().filter(|x| x.concurrent == concurrent).map(|x| x.cores_used).sum();
            let names: Vec<String> = tasks.iter().filter(|x| x.concurrent > 1).map(|x| cell(x.task.split(':').next_back().unwrap_or(&x.task))).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            let of = if *cores > 0 { format!(" of {cores}") } else { String::new() };
            rows.push_str(&format!("| {} | {concurrent} | {} | {cores_used:.1}{of} |\n", cell(job), if names.is_empty() { "-".to_string() } else { names.join(", ") }));
        }
        parts.details.push_str(&format!("\n<details><summary>In-runner parallelism (Nx tasks at once per job, cores used)</summary>\n\n| Job | Tasks at once | Stages in parallel | Cores used |\n|---|---|---|---|\n{rows}\n</details>\n"));
    }
    if !t.shards.is_empty() {
        parts.details.push_str(&format!("\n<details><summary>Shard split: predicted vs actual seconds per shard</summary>\n\n{}\n</details>\n", crate::shard_weights::table(&t.shards)));
    }
    parts
}

/// The sampler's work as the extra columns of a runner row: CPU p95, iowait, swap, pressure, and the seconds idle and saturated.
pub(crate) fn extra_columns(samples: &[Value], r: Option<&JobResources>) -> String {
    extra_columns_of(&|key| samples.iter().any(|s| s[key].is_number()), r)
}

/// The same columns where `has` says which signals the job sampled (a stored row holds them all).
pub(crate) fn extra_columns_of(has: &dyn Fn(&str) -> bool, r: Option<&JobResources>) -> String {
    let Some(r) = r else { return " - | - | - | - | - | - |".to_string() };
    let show = |present: bool, v: String| if present { v } else { "-".to_string() };
    let psi = if has("psiCpu") || has("psiMem") || has("psiIo") { format!("{:.0}/{:.0}/{:.0}%", r.psi_cpu_max, r.psi_mem_max, r.psi_io_max) } else { "-".to_string() };
    format!(
        " {} | {} | {} | {psi} | {} | {} |",
        show(has("cpu"), format!("{:.0}%", r.cpu_p95)),
        show(has("iowaitPct"), format!("{:.0}%", r.iowait_avg)),
        show(has("swapUsedMb"), format!("{:.1} GB", r.swap_peak_mb / 1024.0)),
        show(has("cpu"), format!("{:.0}", r.idle_s)),
        show(has("cpu"), format!("{:.0}", r.saturated_s)),
    )
}
