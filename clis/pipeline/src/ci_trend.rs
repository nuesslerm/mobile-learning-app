//! The "Trend" section of the CI comment: the last successful runs of each hosted workflow from the sealed CI database, this run against their median,
//! and the job and step that made this run long. The section is left out whenever the database cannot be read.
use crate::ci_comment::{cell, line_chart, slowest_of, span_secs, text, var, Env};
use crate::ci_insights::span;
use serde_json::Value;
use wf_bucket::ci_db::{self, RunProfile, Working};

const RUNS: usize = 20;
/// Strictly fewer charts than six, so the comment stays light.
const MAX_CHARTS: usize = 5;
/// Fewer past runs than this make a line, not a trend.
const MIN_CHART: usize = 3;
/// Within this share of the median a run is neither slower nor faster.
const BAND: f64 = 0.10;
const WORKFLOWS: [&str; 6] = ["cli_core", "mobile_core", "backend_core", "cli_build", "mobile_build", "backend_build"];

/// A successful run: no job failed, was cancelled or has no conclusion. The table keeps no branch, and only dev's pipeline merges its rows.
const SQL: &str = "SELECT p.RUN_ID AS run, p.WORKFLOW AS workflow, p.WALL_S AS wall, p.CRITICAL_PATH_S AS critical, p.QUEUE_S_TOTAL AS queue, MIN(j.TS) AS ts \
    FROM CI_RUN_PROFILE p JOIN CI_JOBS j ON j.RUN_ID = p.RUN_ID WHERE p.WORKFLOW LIKE '%+%' GROUP BY p.RUN_ID \
    HAVING SUM(CASE WHEN j.CONCLUSION IS NULL OR j.CONCLUSION IN ('failure', 'cancelled') THEN 1 ELSE 0 END) = 0 ORDER BY ts DESC, p.RUN_ID DESC LIMIT 2000";

struct Past {
    key: &'static str,
    wall: f64,
    critical: f64,
    queue: f64,
}

/// `WF_CI_SEALED_FILE` names a sealed file to read instead of the state ref (tests, offline replay).
fn history(run: &str) -> Result<Vec<Past>, String> {
    let work = match var("WF_CI_SEALED_FILE") {
        Some(path) => Working::from_sealed(ci_db::key()?, std::fs::read(path).ok())?,
        None => Working::fetch()?,
    };
    let conn = work.connect()?;
    ci_db::ensure_ledger(&conn)?;
    let rows = ci_db::query(&conn, SQL)?;
    Ok(rows
        .iter()
        .filter(|r| text(r, "run") != run)
        .filter_map(|r| {
            let key = key_of(&text(r, "workflow"))?;
            Some(Past { key, wall: r["wall"].as_f64()?, critical: r["critical"].as_f64().unwrap_or(0.0), queue: r["queue"].as_f64().unwrap_or(0.0) })
        })
        .collect())
}

fn key_of(workflow: &str) -> Option<&'static str> {
    let head = workflow.split('+').next()?;
    WORKFLOWS.iter().copied().find(|w| *w == head)
}

/// The newest `RUNS` of a workflow, oldest first (the query lists the newest first).
fn last_of<'a>(all: &'a [Past], key: &str) -> Vec<&'a Past> {
    let mut runs: Vec<&Past> = all.iter().filter(|p| p.key == key).take(RUNS).collect();
    runs.reverse();
    runs
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(values[n / 2]),
        _ => Some((values[n / 2 - 1] + values[n / 2]) / 2.0),
    }
}

fn arrow(now: f64, typical: f64) -> &'static str {
    match now / typical.max(f64::MIN_POSITIVE) {
        r if typical <= 0.0 || (1.0 - BAND..=1.0 + BAND).contains(&r) => "→",
        r if r > 1.0 => "↑",
        _ => "↓",
    }
}

fn versus(now: f64, typical: Option<f64>) -> String {
    match typical {
        Some(m) => format!("{} {} (median {})", span(now.round() as i64), arrow(now, m), span(m.round() as i64)),
        None => format!("{} (no history)", span(now.round() as i64)),
    }
}

/// The job of the critical path that ran longest and its longest step.
fn longest(profile: &RunProfile, jobs: &[&Value]) -> Option<String> {
    let (job, secs) = profile.critical_path_job.split(" > ").filter_map(|name| jobs.iter().find(|j| text(j, "name") == name)).filter_map(|j| Some((*j, span_secs(j)?))).max_by_key(|(_, s)| *s)?;
    let step = slowest_of(job).map_or(String::new(), |(name, s)| format!(", its slowest step `{}` {}", cell(&name), span(s)));
    Some(format!("**Longest:** critical path job `{}` {}{step}.\n", cell(&text(job, "name")), span(secs)))
}

fn charts(all: &[Past], first: Option<&str>) -> String {
    let mut order: Vec<&str> = first.into_iter().collect();
    order.extend(WORKFLOWS.iter().copied().filter(|w| Some(*w) != first));
    let mut out = String::new();
    for key in order.into_iter().filter(|k| WORKFLOWS.contains(k)).filter(|k| last_of(all, k).len() >= MIN_CHART).take(MAX_CHARTS) {
        let runs = last_of(all, key);
        let labels: Vec<String> = (1..=runs.len()).map(|i| i.to_string()).collect();
        let minutes: Vec<f64> = runs.iter().map(|p| p.wall / 60.0).collect();
        out.push_str(&format!("\n**{key}**, last {} successful runs, oldest first\n{}", runs.len(), line_chart(&format!("{key} wall time"), "minutes", &labels, &minutes)));
    }
    out
}

/// Empty for a run whose profile is not known yet, a workflow that is not a hosted one, an unreadable database and a database with no history.
pub(crate) fn section(env: &Env, profile: Option<&RunProfile>, jobs: &[&Value]) -> String {
    let Some(profile) = profile.filter(|p| p.workflow.contains('+')) else { return String::new() };
    let Ok(all) = history(&env.run) else { return String::new() };
    let key = key_of(&profile.workflow);
    let past = key.map(|k| last_of(&all, k)).unwrap_or_default();
    let chart = charts(&all, key);
    if chart.is_empty() && past.is_empty() {
        return String::new();
    }
    let typical = |pick: &dyn Fn(&&Past) -> f64| median(past.iter().map(pick).collect());
    let mut out = String::from("\n### Trend\n");
    if let Some(key) = key {
        out.push_str(&format!(
            "\n| This run | Wall | Critical path | Queue |\n|---|---|---|---|\n| {key} | {} | {} | {} |\n",
            versus(profile.wall_s, typical(&|p| p.wall)),
            versus(profile.critical_path_s, typical(&|p| p.critical)),
            versus(profile.queue_s_total, typical(&|p| p.queue)),
        ));
        out.push_str(&format!("\nMedian of the last {} successful runs; ↑ slower, ↓ faster, → within {:.0}%.\n", past.len(), BAND * 100.0));
    }
    if let Some(line) = longest(profile, jobs) {
        out.push('\n');
        out.push_str(&line);
    }
    out.push_str(&chart);
    out
}
