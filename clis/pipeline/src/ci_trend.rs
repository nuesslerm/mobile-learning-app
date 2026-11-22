//! The "Trend" section of the CI comment: the last successful runs of each hosted workflow from the sealed CI database, this run against their median,
//! and the job and step that made this run long. The section is left out whenever the database cannot be read.
use crate::ci_actions::quoted;
use crate::ci_comment::{cell, multi_line_chart, slowest_of, span_secs, text, var};
use crate::ci_insights::span;
use serde_json::Value;
use wf_bucket::ci_db::{self, RunProfile, Working};

const RUNS: usize = 20;
/// Fewer past runs than this make a line, not a trend.
const MIN_CHART: usize = 3;
/// Within this share of the median a run is neither slower nor faster.
const BAND: f64 = 0.10;
/// The longest jobs that get a line next to the total; more lines than the palette has colours would not be told apart.
const JOB_LINES: usize = 6;
/// One colour per line, the total first; mid-tone so they read on GitHub's light and dark themes, each with the emoji square that names it in the legend.
const PALETTE: [(&str, &str); JOB_LINES + 1] = [("#388bfd", "🟦"), ("#f0883e", "🟧"), ("#3fb950", "🟩"), ("#a371f7", "🟪"), ("#d4a72c", "🟨"), ("#f85149", "🟥"), ("#a0724a", "🟫")];
const WORKFLOWS: [&str; 6] = ["cli_core", "mobile_core", "backend_core", "cli_build", "mobile_build", "backend_build"];

/// A successful run: no job failed, was cancelled or has no conclusion. The table keeps no branch, and only dev's pipeline merges its rows.
const SQL: &str = "SELECT p.RUN_ID AS run, p.WORKFLOW AS workflow, p.WALL_S AS wall, p.CRITICAL_PATH_S AS critical, p.QUEUE_S_TOTAL AS queue, MIN(j.TS) AS ts \
    FROM CI_RUN_PROFILE p JOIN CI_JOBS j ON j.RUN_ID = p.RUN_ID WHERE p.WORKFLOW LIKE '%+%' GROUP BY p.RUN_ID \
    HAVING SUM(CASE WHEN j.CONCLUSION IS NULL OR j.CONCLUSION IN ('failure', 'cancelled') THEN 1 ELSE 0 END) = 0 ORDER BY ts DESC, p.RUN_ID DESC LIMIT 2000";

pub(crate) struct Past {
    pub run: String,
    pub key: &'static str,
    pub wall: f64,
    critical: f64,
    queue: f64,
}

/// The sealed CI database, read once for a comment: the successful runs before this one, newest first, and queries for what the other blocks need of them.
pub(crate) struct Sealed {
    work: Working,
    pub past: Vec<Past>,
}

impl Sealed {
    /// `WF_CI_SEALED_FILE` names a sealed file to read instead of the state ref (tests, offline replay).
    pub(crate) fn open(run: &str) -> Result<Sealed, String> {
        let work = match var("WF_CI_SEALED_FILE") {
            Some(path) => Working::from_sealed(ci_db::key()?, std::fs::read(path).ok())?,
            None => Working::fetch()?,
        };
        ci_db::ensure_ledger(&work.connect()?)?;
        let past = history(&work, run)?;
        Ok(Sealed { work, past })
    }

    /// The rows of a query, none when it fails.
    pub(crate) fn rows(&self, statement: &str) -> Vec<Value> {
        self.work.connect().and_then(|conn| ci_db::query(&conn, statement)).unwrap_or_default()
    }
}

fn history(work: &Working, run: &str) -> Result<Vec<Past>, String> {
    let rows = ci_db::query(&work.connect()?, SQL)?;
    Ok(rows
        .iter()
        .filter(|r| text(r, "run") != run)
        .filter_map(|r| {
            let key = key_of(&text(r, "workflow"))?;
            Some(Past { run: text(r, "run"), key, wall: r["wall"].as_f64()?, critical: r["critical"].as_f64().unwrap_or(0.0), queue: r["queue"].as_f64().unwrap_or(0.0) })
        })
        .collect())
}

pub(crate) fn key_of(workflow: &str) -> Option<&'static str> {
    let head = workflow.split('+').next()?;
    WORKFLOWS.iter().copied().find(|w| *w == head)
}

/// The newest `RUNS` of a workflow, oldest first (the query lists the newest first).
pub(crate) fn last_of<'a>(all: &'a [Past], key: &str) -> Vec<&'a Past> {
    let mut runs: Vec<&Past> = all.iter().filter(|p| p.key == key).take(RUNS).collect();
    runs.reverse();
    runs
}

pub(crate) fn median(mut values: Vec<f64>) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(values[n / 2]),
        _ => Some((values[n / 2 - 1] + values[n / 2]) / 2.0),
    }
}

pub(crate) fn arrow(now: f64, typical: f64) -> &'static str {
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

/// A job's minutes in each of the runs (0 where it did not run), its shards as one: the slowest shard, since the shards run side by side and the slowest one is
/// what the run waits for.
struct JobLine {
    name: String,
    minutes: Vec<f64>,
}

impl JobLine {
    fn typical(&self) -> f64 {
        median(self.minutes.iter().copied().filter(|m| *m > 0.0).collect()).unwrap_or(0.0)
    }
}

/// The longest jobs of these runs by their median, longest first; a job that ran in under half of them is left out, and so is the report itself. The count left out is returned.
fn job_lines(sealed: &Sealed, runs: &[&Past]) -> (Vec<JobLine>, usize) {
    let ids: Vec<&str> = runs.iter().map(|p| p.run.as_str()).collect();
    let rows = sealed.rows(&format!(
        "SELECT RUN_ID AS run, JOB AS job, SECS AS secs FROM CI_JOBS WHERE RUN_ID IN ({}) AND SECS > 0 AND COALESCE(CONCLUSION, '') <> 'skipped'",
        quoted(&ids)
    ));
    let mut lines: Vec<JobLine> = Vec::new();
    for r in &rows {
        let Some(at) = ids.iter().position(|id| *id == text(r, "run")) else { continue };
        let raw = text(r, "job");
        let name = wf_bucket::job_name::stage(&raw);
        if matches!(name, "report" | "budget") {
            continue;
        }
        let minutes = r["secs"].as_f64().unwrap_or(0.0) / 60.0;
        let line = match lines.iter().position(|l| l.name == name) {
            Some(i) => &mut lines[i],
            None => {
                lines.push(JobLine { name: name.to_string(), minutes: vec![0.0; ids.len()] });
                lines.last_mut().expect("just pushed")
            }
        };
        line.minutes[at] = line.minutes[at].max(minutes);
    }
    lines.retain(|l| l.minutes.iter().filter(|m| **m > 0.0).count() * 2 >= ids.len());
    lines.sort_by(|a, b| b.typical().total_cmp(&a.typical()).then_with(|| a.name.cmp(&b.name)));
    let omitted = lines.len().saturating_sub(JOB_LINES);
    lines.truncate(JOB_LINES);
    (lines, omitted)
}

/// One line chart of this workflow's last runs, minutes, oldest first: the total wall time, then its longest jobs, each in its own colour and named in the line under
/// the chart (Mermaid draws no legend). Empty below `MIN_CHART` runs.
fn chart(sealed: &Sealed, key: &str) -> String {
    let runs = last_of(&sealed.past, key);
    if runs.len() < MIN_CHART {
        return String::new();
    }
    let labels: Vec<String> = (1..=runs.len()).map(|i| i.to_string()).collect();
    let (jobs, omitted) = job_lines(sealed, &runs);
    let mut series = vec![runs.iter().map(|p| p.wall / 60.0).collect::<Vec<f64>>()];
    series.extend(jobs.iter().map(|j| j.minutes.clone()));
    let palette: Vec<&str> = PALETTE.iter().map(|(color, _)| *color).collect();
    let mut names = vec![format!("{} total wall time", PALETTE[0].1)];
    names.extend(jobs.iter().enumerate().map(|(i, j)| format!("{} `{}` (median {})", PALETTE[i + 1].1, cell(&j.name), span((j.typical() * 60.0).round() as i64))));
    let note = if omitted > 0 { format!(" {omitted} shorter jobs are left out.") } else { String::new() };
    format!(
        "\n**{key}**, minutes of the last {} successful runs, oldest first\n{}\n{}\n\nA matrix job is one line at its slowest shard.{note}\n",
        runs.len(),
        multi_line_chart(&format!("{key} wall time and its longest jobs"), "minutes", &labels, &series, &palette),
        names.join(" · ")
    )
}

/// What the run has shown so far, for the cells of the table that only a finished run fills.
pub(crate) struct Now<'a> {
    pub profile: Option<&'a RunProfile>,
    /// Seconds from the first job's start to now while the run goes on.
    pub wall_so_far: Option<i64>,
    pub running: bool,
    pub jobs: &'a [&'a Value],
}

/// A cell the finished run fills: this run's figure against the median, or `pending` with the median while the run goes on, or `none`.
fn cell_of(now: &Now, figure: Option<f64>, typical: Option<f64>) -> String {
    match (figure, now.running) {
        (Some(v), _) => versus(v, typical),
        (None, true) => typical.map_or("pending".to_string(), |m| format!("pending (median {})", span(m.round() as i64))),
        (None, false) => "none".to_string(),
    }
}

/// Empty for a workflow that is not a hosted one, an unreadable database and a database with no history: those are known before the run ends and stay so. Otherwise the
/// heading, the table and the chart are there from the first render (the chart is history), with this run's cells `pending` until its profile exists.
pub(crate) fn section(sealed: Option<&Sealed>, workflow: &str, now: &Now) -> String {
    if !workflow.contains('+') {
        return String::new();
    }
    let Some(all) = sealed.map(|s| &s.past) else { return String::new() };
    let key = key_of(workflow);
    let past = key.map(|k| last_of(all, k)).unwrap_or_default();
    let chart = key.zip(sealed).map(|(k, s)| chart(s, k)).unwrap_or_default();
    if chart.is_empty() && past.is_empty() {
        return String::new();
    }
    let typical = |pick: &dyn Fn(&&Past) -> f64| median(past.iter().map(pick).collect());
    let mut out = format!("\n{}\n", crate::ci_skeleton::TREND);
    if let Some(key) = key {
        let wall_typical = typical(&|p| p.wall);
        let wall = match (now.profile, now.wall_so_far) {
            (None, Some(so_far)) if now.running => format!("{} so far{}", span(so_far), wall_typical.map_or(String::new(), |m| format!(" (median {})", span(m.round() as i64)))),
            _ => cell_of(now, now.profile.map(|p| p.wall_s), wall_typical),
        };
        out.push_str(&format!(
            "\n| This run | Wall | Critical path | Queue |\n|---|---|---|---|\n| {key} | {wall} | {} | {} |\n",
            cell_of(now, now.profile.map(|p| p.critical_path_s), typical(&|p| p.critical)),
            cell_of(now, now.profile.map(|p| p.queue_s_total), typical(&|p| p.queue)),
        ));
        out.push_str(&format!("\nMedian of the last {} successful runs; ↑ slower, ↓ faster, → within {:.0}%.\n", past.len(), BAND * 100.0));
    }
    let longest = now.profile.and_then(|p| longest(p, now.jobs)).unwrap_or_else(|| {
        format!("**Longest:** {}\n", crate::ci_skeleton::pending(now.running, "the run to finish", "no job of the critical path has a recorded time"))
    });
    out.push('\n');
    out.push_str(&longest);
    out.push_str(&chart);
    out
}
