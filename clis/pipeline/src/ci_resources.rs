//! `wf ci resources`: what each job's runner did while it ran, from the sampler's `stats-<job>.jsonl` files, as rows of `CI_JOB_RESOURCES`; and, joined with
//! the Nx task intervals each job kept in its own database (`ci-db-<job>/rows.sqlite`), what each task that ran saw of the machine, as `CI_TASK_RESOURCES`.
//! Laid over the steps the job kept in `CI_STEPS`, the same samples give `CI_STEP_RESOURCES`: what the machine did during each step.
//! The job is the artifact's name, taken from the data. Rows go to the report job's local database (`wf ci db merge` carries them on); `--send` makes them gauges.
use crate::ci_comment::var;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use wf_bucket::ci_db::{JobResources, LedgerTask, StepResources, StepRow, TaskResources};
use wf_bucket::ci_metrics::Gauges;
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &["send"], values: &["run", "stats", "ledgers"], positional: Positional::None, ..Spec::NONE };
/// A sample with the CPU under this percent is idle time; over `SATURATED_CPU`, or with CPU pressure over `SATURATED_PSI`, the machine was the limit.
const IDLE_CPU: f64 = 25.0;
const SATURATED_CPU: f64 = 90.0;
const SATURATED_PSI: f64 = 20.0;
const MB: f64 = 1_048_576.0;

fn num(sample: &Value, key: &str) -> Option<f64> {
    sample[key].as_f64()
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn max(v: impl Iterator<Item = f64>) -> f64 {
    v.fold(0.0, f64::max)
}

/// The nearest-rank 95th percentile.
fn p95(v: &[f64]) -> f64 {
    let mut sorted = v.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)).copied().unwrap_or(0.0)
}

/// The seconds between two samples: the median gap, 5 when there are fewer than two.
pub(crate) fn interval(samples: &[Value]) -> i64 {
    let mut gaps: Vec<i64> = samples.windows(2).filter_map(|w| Some(w[1]["t"].as_i64()? - w[0]["t"].as_i64()?)).filter(|g| *g > 0).collect();
    gaps.sort_unstable();
    gaps.get(gaps.len() / 2).copied().unwrap_or(5)
}

/// One job's samples boiled down; `None` when there are none.
pub(crate) fn job_resources(run: &str, job: &str, workflow: &str, samples: &[Value]) -> Option<JobResources> {
    let (first, last) = (samples.first()?, samples.last()?);
    let dt = interval(samples);
    let series = |key: &str| samples.iter().filter_map(|s| num(s, key)).collect::<Vec<f64>>();
    let cpu = series("cpu");
    let mem = series("memUsedMb");
    let at = |s: &&Value| s["t"].as_i64().unwrap_or(0);
    let seconds = |hit: &dyn Fn(&Value) -> bool| samples.iter().filter(|s| hit(s)).count() as i64 * dt;
    Some(JobResources {
        run: run.to_string(),
        job: job.to_string(),
        workflow: workflow.to_string(),
        runner: samples.iter().filter_map(|s| s["runner"].as_str()).find(|r| !r.is_empty()).unwrap_or_default().to_string(),
        cores: max(series("cores").into_iter()) as i64,
        samples: samples.len() as i64,
        cpu_avg: mean(&cpu),
        cpu_p95: p95(&cpu),
        cpu_max: max(cpu.iter().copied()),
        iowait_avg: mean(&series("iowaitPct")),
        mem_avg_mb: mean(&mem),
        mem_peak_mb: max(mem.iter().copied()),
        mem_total_mb: max(series("memTotalMb").into_iter()),
        swap_peak_mb: max(series("swapUsedMb").into_iter()),
        load1_max: max(series("load1").into_iter()),
        psi_cpu_max: max(series("psiCpu").into_iter()),
        psi_mem_max: max(series("psiMem").into_iter()),
        psi_io_max: max(series("psiIo").into_iter()),
        read_mb: series("readBytes").iter().sum::<f64>() / MB,
        write_mb: series("writeBytes").iter().sum::<f64>() / MB,
        duration_s: (at(&last) - at(&first) + dt) as f64,
        idle_s: seconds(&|s| num(s, "cpu").is_some_and(|c| c < IDLE_CPU)) as f64,
        saturated_s: seconds(&|s| num(s, "cpu").is_some_and(|c| c > SATURATED_CPU) || num(s, "psiCpu").is_some_and(|p| p > SATURATED_PSI)) as f64,
    })
}

/// The tasks that ran, each with the CPU the job's samples show over its interval (a sample at `t` covers the `dt` seconds before it) and how many tasks of
/// the job ran at the same time; the sample columns are empty when no sample overlaps the task.
pub(crate) fn task_resources(run: &str, job: &str, tasks: &[LedgerTask], samples: &[Value]) -> Vec<TaskResources> {
    let dt_ms = interval(samples) * 1000;
    let ran: Vec<&LedgerTask> = tasks.iter().filter(|t| t.outcome == "ran" && t.started_ms > 0 && t.ended_ms >= t.started_ms).collect();
    ran.iter()
        .map(|t| {
            let seen: Vec<&Value> = samples.iter().filter(|s| s["t"].as_i64().is_some_and(|at| at * 1000 > t.started_ms && at * 1000 - dt_ms < t.ended_ms)).collect();
            let seconds = (t.ended_ms - t.started_ms) as f64 / 1000.0;
            TaskResources {
                run: run.to_string(),
                job: job.to_string(),
                task: t.task.clone(),
                started: t.started_ms / 1000,
                ended: t.ended_ms / 1000,
                seconds,
                cpu_s: t.cpu_s,
                cores_used: if seconds > 0.0 { t.cpu_s / seconds } else { 0.0 },
                concurrent: ran.iter().filter(|o| o.started_ms < t.ended_ms && t.started_ms < o.ended_ms).count() as i64,
                cpu_avg: Some(seen.iter().filter_map(|s| num(s, "cpu")).collect::<Vec<_>>()).filter(|v| !v.is_empty()).map(|v| mean(&v)),
                mem_peak_mb: Some(seen.iter().filter_map(|s| num(s, "memUsedMb")).collect::<Vec<_>>()).filter(|v| !v.is_empty()).map(|v| max(v.into_iter())),
            }
        })
        .collect()
}

/// The sampler file's job name for a job as the API names it: `core-tests (1/3)` is `core-tests-1`, `build (app)` is `build-app`, the arm `wf-build` is `clis-arm`.
pub(crate) fn stats_key(job: &str) -> String {
    let name = wf_bucket::job_name::job_name(job);
    let Some((stage, param)) = name.strip_suffix(')').and_then(|n| n.rsplit_once(" (")) else { return name.to_string() };
    match (stage, param) {
        ("wf-build", "arm64") => "clis-arm".to_string(),
        (_, p) => format!("{stage}-{}", p.split('/').next().unwrap_or(p)),
    }
}

/// What each step saw of the machine: the job's samples (a sample at `t` covers the seconds since the sample before it, the first the median gap) laid over the step's interval. A step with no sample
/// over it has no row. The job's samples are the file `stats_key` names.
pub(crate) fn step_resources(steps: &[StepRow], samples: &[(String, Vec<Value>)]) -> Vec<StepResources> {
    let mut out = Vec::new();
    for step in steps {
        let (Some(from), Some(to)) = (step.started, step.completed) else { continue };
        let Some((_, job_samples)) = samples.iter().find(|(name, _)| *name == stats_key(&step.job)) else { continue };
        let dt = interval(job_samples);
        let (mut covered, mut idle, mut saturated, mut wait) = (0.0, 0.0, 0.0, 0.0);
        let mut previous: Option<i64> = None;
        for s in job_samples {
            let Some(t) = s["t"].as_i64() else { continue };
            let start = previous.map_or(t - dt, |p| p.min(t));
            previous = Some(t);
            let overlap = (t.min(to) - start.max(from)).max(0) as f64;
            if overlap == 0.0 {
                continue;
            }
            covered += overlap;
            idle += overlap * f64::from(u8::from(num(s, "cpu").is_some_and(|c| c < IDLE_CPU)));
            saturated += overlap * f64::from(u8::from(num(s, "cpu").is_some_and(|c| c > SATURATED_CPU) || num(s, "psiCpu").is_some_and(|p| p > SATURATED_PSI)));
            wait += overlap * num(s, "iowaitPct").unwrap_or(0.0);
        }
        if covered == 0.0 {
            continue;
        }
        let duration = step.duration_s.unwrap_or(to - from) as f64;
        out.push(StepResources { run: step.run.clone(), job: step.job.clone(), step_no: step.step_no, step: step.step.clone(), duration_s: duration, idle_s: idle.min(duration), saturated_s: saturated.min(duration), iowait_avg: wait / covered });
    }
    out
}

pub(crate) fn samples_of(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn job_json(r: &JobResources) -> Value {
    json!({
        "job": r.job, "workflow": r.workflow, "runner": r.runner, "cores": r.cores, "samples": r.samples, "cpu_avg": r.cpu_avg, "cpu_p95": r.cpu_p95, "cpu_max": r.cpu_max,
        "iowait_avg": r.iowait_avg, "mem_avg_mb": r.mem_avg_mb, "mem_peak_mb": r.mem_peak_mb, "mem_total_mb": r.mem_total_mb, "swap_peak_mb": r.swap_peak_mb, "load1_max": r.load1_max,
        "psi_cpu_max": r.psi_cpu_max, "psi_mem_max": r.psi_mem_max, "psi_io_max": r.psi_io_max, "read_mb": r.read_mb, "write_mb": r.write_mb, "duration_s": r.duration_s,
        "idle_s": r.idle_s, "saturated_s": r.saturated_s,
    })
}

/// Only a step this long is a series of its own.
const STEP_GAUGE_MIN_S: f64 = 30.0;

/// `ci.job.*{job, workflow}`: the job's machine as gauges, and `ci.step.idle_s{job, step}` for the steps over `STEP_GAUGE_MIN_S` (steps of one name in a job add up).
fn gauges(rows: &[JobResources], steps: &[StepResources]) -> Gauges {
    let mut g = Gauges::new();
    let mut idle: std::collections::BTreeMap<(&str, &str), f64> = std::collections::BTreeMap::new();
    for r in steps.iter().filter(|r| r.duration_s > STEP_GAUGE_MIN_S) {
        *idle.entry((&r.job, &r.step)).or_default() += r.idle_s;
    }
    for ((job, step), value) in idle {
        g.set("ci.step.idle_s", job, step, value);
    }
    for r in rows {
        for (metric, value) in [
            ("ci.job.cpu_avg", r.cpu_avg),
            ("ci.job.cpu_p95", r.cpu_p95),
            ("ci.job.cpu_max", r.cpu_max),
            ("ci.job.iowait_avg", r.iowait_avg),
            ("ci.job.mem_peak_mb", r.mem_peak_mb),
            ("ci.job.swap_peak_mb", r.swap_peak_mb),
            ("ci.job.psi_cpu_max", r.psi_cpu_max),
            ("ci.job.psi_mem_max", r.psi_mem_max),
            ("ci.job.psi_io_max", r.psi_io_max),
            ("ci.job.idle_s", r.idle_s),
            ("ci.job.saturated_s", r.saturated_s),
        ] {
            g.set(metric, &r.job, &r.workflow, value);
        }
    }
    g
}

fn resources(rest: &[String]) -> Option<i32> {
    let parsed = parse_spec(rest, &SPEC)?;
    let fail = |error: &str, next: &str| {
        println!("{}", json!({"ok": false, "error": error, "next": next}));
        Some(1)
    };
    let Some(stats) = parsed.val("stats").map(str::to_string).or_else(|| var("WF_CI_RUNNER_STATS_DIR")) else {
        return fail("no stats directory: WF_CI_RUNNER_STATS_DIR is not set", "pass --stats <dir> holding the stats-<job>.jsonl files");
    };
    let run = parsed.val("run").map(str::to_string).or_else(|| var("GITHUB_RUN_ID")).unwrap_or_default();
    let ledgers: Option<PathBuf> = parsed.val("ledgers").map(PathBuf::from).or_else(|| var("RUNNER_TEMP").map(|t| Path::new(&t).join("ci-db")));
    let workflow = var("GITHUB_WORKFLOW").unwrap_or_default();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&stats).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect()).unwrap_or_default();
    files.sort();
    let (mut jobs, mut tasks, mut sampled) = (Vec::new(), Vec::new(), Vec::new());
    for path in files {
        let job = path.file_stem().and_then(|n| n.to_str()).unwrap_or("job").trim_start_matches("stats-").to_string();
        let samples = samples_of(&path);
        let Some(row) = job_resources(&run, &job, &workflow, &samples) else { continue };
        let ledger = ledgers.as_ref().map(|d| d.join(format!("ci-db-{job}")).join("rows.sqlite")).filter(|p| p.is_file());
        match ledger.map(|p| wf_bucket::ci_db::tasks_of(&p)) {
            Some(Ok(ledger_tasks)) => tasks.extend(task_resources(&run, &job, &ledger_tasks, &samples)),
            Some(Err(why)) => wf_kit::wf_err!("wf: ci task resources of {job} not read: {why}"),
            None => {}
        }
        jobs.push(row);
        sampled.push((job, samples));
    }
    let steps = match wf_bucket::ci_db::steps_local(&run) {
        Ok(rows) => step_resources(&rows, &sampled),
        Err(why) => {
            wf_kit::wf_err!("wf: ci steps not read: {why}");
            Vec::new()
        }
    };
    if let Err(why) = wf_bucket::ci_db::record_step_resources_local(&steps) {
        wf_kit::wf_err!("wf: ci step resources not recorded: {why}");
    }
    if let Err(why) = wf_bucket::ci_db::record_job_resources_local(&jobs) {
        wf_kit::wf_err!("wf: ci job resources not recorded: {why}");
    }
    if let Err(why) = wf_bucket::ci_db::record_task_resources_local(&tasks) {
        wf_kit::wf_err!("wf: ci task resources not recorded: {why}");
    }
    let sent = parsed.flag("send") && !jobs.is_empty();
    if sent {
        gauges(&jobs, &steps).send();
    }
    println!("{}", json!({"ok": true, "run": run, "sent": sent, "jobs": jobs.iter().map(job_json).collect::<Vec<_>>(), "tasks": tasks.len(), "steps": steps.len()}));
    Some(0)
}

/// `wf ci resources [--stats <dir>] [--ledgers <dir>] [--run <id>] [--send]`.
pub fn run(verb: &str, rest: &[String]) -> Option<i32> {
    (verb == "resources").then(|| resources(rest)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(t: i64, cpu: f64) -> Value {
        json!({"t": t, "cpu": cpu})
    }

    #[test]
    fn given_irregular_sample_gaps_when_a_step_is_laid_over_them_then_each_sample_covers_the_time_since_the_one_before() {
        // Given samples 5 s apart but for a 20 s stall, the first two busy and the sample after the stall idle
        let samples = vec![("core-tests-1".to_string(), vec![sample(105, 90.0), sample(110, 90.0), sample(130, 5.0), sample(135, 5.0)])];
        let step = StepRow { run: "1".into(), job: "core-tests (1/3)".into(), step_no: 1, step: "Flow tests".into(), started: Some(100), completed: Some(135), duration_s: Some(35), conclusion: None };
        // When the step is laid over them
        let rows = step_resources(&[step], &samples);
        // Then the idle seconds are the 20 s stall plus 5 s, with no window overlapping another
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].idle_s, 25.0);
    }
}
