//! The shard runner's per-unit rows (`CI_UNITS`): a row when a test unit of a weighted stage starts and its span when it ends, so a long step shows which
//! unit it is in and the series of spans regenerates `data/shard-weights.json` (`wf ci plan shard-weights`). The rows go to the job-local database; the report
//! job's merge folds them into the sealed one, and nothing pushes the state ref from a job.
use crate::nx_ledger::TaskRun;
use std::collections::BTreeMap;
use wf_bucket::ci_db::{self, UnitSpan};
use wf_bucket::ci_metrics::Gauges;

/// A unit shorter than this is not a gauge: the series would be many and say nothing.
const GAUGE_MIN_S: f64 = 2.0;

fn var(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// The job a stage's rows belong to: a `core-tests` job runs two stages, so the stage names it (as `NX_TASKS.JOB` does).
pub(crate) fn job_of(stage: &str) -> String {
    var("GITHUB_JOB").map(|j| if j == "core-tests" { stage.to_string() } else { j }).unwrap_or_default()
}

/// A row per project the stage's Nx run is about to run, `running` until the run ends.
pub(crate) fn started(stage: &str, target: &str, projects: &[String]) {
    let Some(run) = var("GITHUB_RUN_ID").filter(|_| crate::shard_weights::weighted(stage)) else { return };
    let (job, now) = (job_of(stage), wf_kit::time::now_ms() as i64);
    let first = target.split(',').next().unwrap_or_default();
    let spans: Vec<UnitSpan> = projects
        .iter()
        .filter_map(|p| crate::shard_weights::unit_of(stage, &format!("{p}:{first}")))
        .map(|unit| UnitSpan { run: run.clone(), job: job.clone(), stage: stage.to_string(), unit, started_ms: now, ended_ms: None, ms: None, outcome: "running".to_string() })
        .collect();
    if let Err(why) = ci_db::record_units_local(&spans) {
        wf_kit::wf_err!("wf: ci units not recorded: {why}");
    }
}

/// The spans of the units the tasks of one Nx run belong to: the first start to the last end, the milliseconds of the tasks that ran, `ran` when any did,
/// `failed` when any failed and `hit` when all were replayed.
pub(crate) fn spans(run: &str, stage: &str, tasks: &[TaskRun]) -> Vec<UnitSpan> {
    if !crate::shard_weights::weighted(stage) {
        return Vec::new();
    }
    let job = job_of(stage);
    let mut by_unit: BTreeMap<String, Vec<&TaskRun>> = BTreeMap::new();
    for t in tasks {
        if let Some(unit) = crate::shard_weights::unit_of(stage, &t.task) {
            by_unit.entry(unit).or_default().push(t);
        }
    }
    by_unit
        .into_iter()
        .map(|(unit, ts)| {
            let ran = ts.iter().any(|t| t.outcome == "ran");
            let started = ts.iter().map(|t| t.started_ms).filter(|m| *m > 0).min().unwrap_or(0);
            let ended = ts.iter().map(|t| t.ended_ms).max().filter(|m| *m > 0);
            let outcome = if ts.iter().any(|t| !t.passed) { "failed" } else if ran { "ran" } else { "hit" };
            UnitSpan { run: run.to_string(), job: job.clone(), stage: stage.to_string(), unit, started_ms: started, ended_ms: ended, ms: Some(ts.iter().filter(|t| t.outcome == "ran").map(|t| t.ms).sum()), outcome: outcome.to_string() }
        })
        .collect()
}

pub(crate) fn record(spans: &[UnitSpan]) {
    if spans.is_empty() {
        return;
    }
    if let Err(why) = ci_db::record_units_local(spans) {
        wf_kit::wf_err!("wf: ci units not recorded: {why}");
    }
}

/// `ci.unit.duration_s{stage,unit}` of each unit that ran for 2 s or more.
pub(crate) fn gauges(into: &mut Gauges, spans: &[UnitSpan]) {
    for u in spans.iter().filter(|u| u.outcome != "hit") {
        let seconds = u.ms.unwrap_or(0) as f64 / 1000.0;
        if seconds >= GAUGE_MIN_S {
            into.set("ci.unit.duration_s", &u.stage, &u.unit, seconds);
        }
    }
}

/// The run ended: whatever it left `running` is dropped.
pub(crate) fn clear_running(stage: &str) {
    if let (Some(run), true) = (var("GITHUB_RUN_ID"), crate::shard_weights::weighted(stage)) {
        if let Err(why) = ci_db::clear_running_units(&run, &job_of(stage), stage) {
            wf_kit::wf_err!("wf: ci units not cleared: {why}");
        }
    }
}
