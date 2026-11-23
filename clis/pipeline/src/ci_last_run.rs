//! The "Last run" halves of the CI comment: the sections that show a run (the idle time, what to do, the runners, the caches, the slowest steps) are also built for the
//! previous finished run of the same workflow, from that run's rows in the sealed CI database. The builders are the ones the current run uses; only their input differs:
//! rows read by run id instead of the live jobs, the sampler's files and the logs.
use crate::ci_actions::quoted;
use crate::ci_comment::{cell, text};
use crate::ci_insights::{CacheFacts, JobTime};
use crate::ci_telemetry::Telemetry;
use crate::ci_trend::{key_of, last_of, Past, Sealed};
use serde_json::Value;
use wf_bucket::ci_db::{JobResources, RunProfile, StepResources, StepRow};

/// A run's sections as the builders make them (each with its heading, empty when the database holds nothing for it), and the label that names the run.
pub(crate) struct LastRun {
    pub label: String,
    pub idle: String,
    pub todo: String,
    pub runners: String,
    pub cache: String,
    pub health: String,
    pub problems: String,
    pub steps: String,
}

/// The id of the newest finished run of the same workflow before `current`: the run ids only grow, and the database only holds the branch whose pipeline merges its
/// rows (dev), so this is the previous run of the workflow on that branch.
fn previous(sealed: &Sealed, workflow: &str, current: &str) -> Option<(String, String, i64)> {
    let key = key_of(workflow)?;
    let rows = sealed.rows("SELECT p.RUN_ID AS run, p.WORKFLOW AS workflow, MIN(j.TS) AS ts, MIN(j.SHA) AS sha FROM CI_RUN_PROFILE p JOIN CI_JOBS j ON j.RUN_ID = p.RUN_ID GROUP BY p.RUN_ID");
    let before = |run: &str| match (run.parse::<u64>(), current.parse::<u64>()) {
        (Ok(r), Ok(c)) => r < c,
        _ => run < current,
    };
    let id = |run: &str| run.parse::<u64>().unwrap_or(0);
    rows.iter()
        .filter(|r| key_of(&text(r, "workflow")) == Some(key) && before(&text(r, "run")))
        .max_by_key(|r| id(&text(r, "run")))
        .map(|r| (text(r, "run"), text(r, "sha"), r["ts"].as_i64().unwrap_or(0)))
}

/// "5m ago", "3h ago", "2d ago".
pub(crate) fn ago(secs: i64) -> String {
    match secs.max(0) {
        s if s >= 86_400 => format!("{}d ago", s / 86_400),
        s if s >= 3600 => format!("{}h ago", s / 3600),
        s if s >= 60 => format!("{}m ago", s / 60),
        _ => "just now".to_string(),
    }
}

fn num(r: &Value, key: &str) -> f64 {
    r[key].as_f64().unwrap_or(0.0)
}

fn profile_of(sealed: &Sealed, run: &str) -> Option<RunProfile> {
    let rows = sealed.rows(&format!(
        "SELECT WORKFLOW AS workflow, WALL_S AS wall, RUNNER_S AS runner, PEAK_JOBS AS peak, AVG_JOBS AS avg, QUEUE_S_TOTAL AS queue, QUEUE_S_MAX AS queue_max, JOBS AS jobs, \
         CRITICAL_PATH_JOB AS path, CRITICAL_PATH_S AS path_s, PARALLEL_EFFICIENCY AS efficiency FROM CI_RUN_PROFILE WHERE RUN_ID IN ({})",
        quoted(&[run])
    ));
    let r = rows.first()?;
    Some(RunProfile {
        run: run.to_string(),
        workflow: text(r, "workflow"),
        wall_s: num(r, "wall"),
        runner_s: num(r, "runner"),
        peak_jobs: r["peak"].as_i64().unwrap_or(0),
        avg_jobs: num(r, "avg"),
        queue_s_total: num(r, "queue"),
        queue_s_max: num(r, "queue_max"),
        jobs: r["jobs"].as_i64().unwrap_or(0),
        critical_path_job: text(r, "path"),
        critical_path_s: num(r, "path_s"),
        parallel_efficiency: num(r, "efficiency"),
    })
}

fn resources_of(sealed: &Sealed, run: &str) -> Vec<JobResources> {
    sealed
        .rows(&format!(
            "SELECT JOB AS job, COALESCE(WORKFLOW, '') AS workflow, COALESCE(RUNNER, '') AS runner, CORES AS cores, SAMPLES AS samples, CPU_AVG AS cpu_avg, CPU_P95 AS cpu_p95, CPU_MAX AS cpu_max, \
             IOWAIT_AVG AS iowait, MEM_AVG_MB AS mem_avg, MEM_PEAK_MB AS mem_peak, MEM_TOTAL_MB AS mem_total, SWAP_PEAK_MB AS swap, LOAD1_MAX AS load1, PSI_CPU_MAX AS psi_cpu, \
             PSI_MEM_MAX AS psi_mem, PSI_IO_MAX AS psi_io, READ_MB AS read_mb, WRITE_MB AS write_mb, DURATION_S AS duration, IDLE_S AS idle, SATURATED_S AS saturated \
             FROM CI_JOB_RESOURCES WHERE RUN_ID IN ({}) ORDER BY JOB",
            quoted(&[run])
        ))
        .iter()
        .map(|r| JobResources {
            run: run.to_string(),
            job: text(r, "job"),
            workflow: text(r, "workflow"),
            runner: text(r, "runner"),
            cores: r["cores"].as_i64().unwrap_or(0),
            samples: r["samples"].as_i64().unwrap_or(0),
            cpu_avg: num(r, "cpu_avg"),
            cpu_p95: num(r, "cpu_p95"),
            cpu_max: num(r, "cpu_max"),
            iowait_avg: num(r, "iowait"),
            mem_avg_mb: num(r, "mem_avg"),
            mem_peak_mb: num(r, "mem_peak"),
            mem_total_mb: num(r, "mem_total"),
            swap_peak_mb: num(r, "swap"),
            load1_max: num(r, "load1"),
            psi_cpu_max: num(r, "psi_cpu"),
            psi_mem_max: num(r, "psi_mem"),
            psi_io_max: num(r, "psi_io"),
            read_mb: num(r, "read_mb"),
            write_mb: num(r, "write_mb"),
            duration_s: num(r, "duration"),
            idle_s: num(r, "idle"),
            saturated_s: num(r, "saturated"),
        })
        .collect()
}

fn step_rows_of(sealed: &Sealed, run: &str) -> Vec<StepRow> {
    sealed
        .rows(&format!("SELECT JOB AS job, STEP_NO AS no, STEP AS step, STARTED AS started, COMPLETED AS completed, DURATION_S AS secs, CONCLUSION AS conclusion FROM CI_STEPS WHERE RUN_ID IN ({}) ORDER BY JOB, STEP_NO", quoted(&[run])))
        .iter()
        .map(|r| StepRow {
            run: run.to_string(),
            job: text(r, "job"),
            step_no: r["no"].as_i64().unwrap_or(0),
            step: text(r, "step"),
            started: r["started"].as_i64(),
            completed: r["completed"].as_i64(),
            duration_s: r["secs"].as_i64(),
            conclusion: r["conclusion"].as_str().map(str::to_string),
        })
        .collect()
}

fn step_resources_of(sealed: &Sealed, run: &str) -> Vec<StepResources> {
    sealed
        .rows(&format!("SELECT JOB AS job, STEP_NO AS no, STEP AS step, DURATION_S AS secs, IDLE_S AS idle, SATURATED_S AS saturated, IOWAIT_AVG AS iowait FROM CI_STEP_RESOURCES WHERE RUN_ID IN ({}) ORDER BY JOB, STEP_NO", quoted(&[run])))
        .iter()
        .map(|r| StepResources { run: run.to_string(), job: text(r, "job"), step_no: r["no"].as_i64().unwrap_or(0), step: text(r, "step"), duration_s: num(r, "secs"), idle_s: num(r, "idle"), saturated_s: num(r, "saturated"), iowait_avg: num(r, "iowait") })
        .collect()
}

/// (job, tasks that ran, tasks a cache replayed) from the run's Nx tasks.
fn replay_of(sealed: &Sealed, run: &str) -> Vec<(String, usize, usize)> {
    sealed
        .rows(&format!("SELECT JOB AS job, SUM(CASE WHEN OUTCOME = 'hit' THEN 1 ELSE 0 END) AS hit, COUNT(*) AS n FROM NX_TASKS WHERE RUN_ID IN ({}) GROUP BY JOB ORDER BY JOB", quoted(&[run])))
        .iter()
        .map(|r| {
            let (hit, n) = (r["hit"].as_i64().unwrap_or(0).max(0) as usize, r["n"].as_i64().unwrap_or(0).max(0) as usize);
            (text(r, "job"), n.saturating_sub(hit), hit)
        })
        .collect()
}

/// The slow steps of each job of the run (as `job_time` keeps them: no setup, post or complete steps, five seconds or more, longest first).
fn job_times(steps: &[StepRow]) -> Vec<JobTime> {
    let mut jobs: Vec<JobTime> = Vec::new();
    for s in steps {
        let at = match jobs.iter().position(|j| j.name == s.job) {
            Some(at) => at,
            None => {
                jobs.push(JobTime { name: s.job.clone(), started: None, ended: None, queued: None, steps: Vec::new() });
                jobs.len() - 1
            }
        };
        let own = !s.step.starts_with("Post ") && !s.step.starts_with("Set up") && !s.step.starts_with("Complete");
        if let Some(secs) = s.duration_s.filter(|secs| own && *secs >= 5) {
            jobs[at].steps.push((s.step.clone(), secs));
        }
    }
    for j in &mut jobs {
        j.steps.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    }
    jobs
}

/// The previous finished run of `workflow` before `current`, built section by section from its rows; `None` when the database holds no such run. `base` is the
/// repository's URL, for the commit link of the label.
pub(crate) fn read(sealed: &Sealed, workflow: &str, current: &str, base: &str) -> Option<LastRun> {
    let (run, sha, ts) = previous(sealed, workflow, current)?;
    let key = key_of(workflow)?;
    let profile = profile_of(sealed, &run);
    let t = Telemetry {
        workflow: profile.as_ref().map(|p| p.workflow.clone()).unwrap_or_default(),
        resources: resources_of(sealed, &run),
        step_rows: step_rows_of(sealed, &run),
        steps: step_resources_of(sealed, &run),
        replay: replay_of(sealed, &run),
        profile,
        ..Telemetry::default()
    };
    let id = |p: &Past| p.run.parse::<u64>().unwrap_or(0);
    let older: Vec<Past> = sealed.past.iter().filter(|p| id(p) < run.parse::<u64>().unwrap_or(0)).cloned().collect();
    let (idle, todo) = crate::ci_actions::section_over(Some(sealed), &t, &last_of(&older, key));
    let todo = if todo.is_empty() && t.profile.is_some() { format!("\n{}\n\n{}\n", crate::ci_skeleton::TODO, crate::ci_skeleton::none("no signal is amber or red")) } else { todo };

    let runners = if t.resources.is_empty() { String::new() } else { format!("\n{}\n\n{}", crate::ci_skeleton::RUNNERS, crate::ci_comment::runner_block(&crate::ci_comment::runner_rows_of(&t.resources))) };
    let cache_rows: Vec<CacheFacts> = t.replay.iter().map(|(job, ran, hit)| CacheFacts { name: job.clone(), nx: Some((*hit as u64, (ran + hit) as u64)), ..CacheFacts::default() }).collect();
    let cache = crate::ci_insights::cache_section(&cache_rows, &[]);

    let conn = sealed.conn();
    let events = conn.as_ref().ok().and_then(|c| wf_bucket::ci_db::cache_events_in(c, Some(&run)).ok()).unwrap_or_default();
    let replay: Vec<(String, u64, u64)> = t.replay.iter().map(|(job, ran, hit)| (job.clone(), *ran as u64, *hit as u64)).collect();
    let health = crate::ci_cache_health::section(&crate::ci_cache_health::assess_events(&events, &replay));
    let found = conn
        .and_then(|c| wf_bucket::miss_reason::classify_db(&c, Some(&run), false))
        .map(|misses| crate::ci_cache_misses::Findings { misses, rates: Vec::new(), events: events.clone() });
    let problems = crate::ci_cache_misses::problems_in_events(&events);
    let mut problems_text = crate::ci_cache_misses::problems_section(&problems, Some(&found), false);
    if problems_text.is_empty() && (!events.is_empty() || !t.replay.is_empty()) {
        problems_text = format!("\n{}\n\n{}\n", crate::ci_skeleton::CACHE_PROBLEMS, crate::ci_skeleton::none("no cache restore failed"));
    }
    let steps = crate::ci_insights::slowest_steps(&job_times(&t.step_rows));

    let short: String = sha.chars().take(7).collect();
    let when = ago(crate::ci_timings::now() - ts);
    let label = match (short.is_empty(), sha.len() >= 7) {
        (true, _) => format!("run {}, {when}", cell(&run)),
        (false, true) => format!("[`{short}`]({base}/commit/{sha}), {when}"),
        _ => format!("`{short}`, {when}"),
    };
    Some(LastRun { label, idle, todo, runners, cache, health, problems: problems_text, steps })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_ages_when_said_then_the_unit_is_the_biggest_whole_one() {
        // Given spans of seconds
        // When they are said as an age
        // Then each uses its biggest whole unit and a negative one is just now
        assert_eq!([ago(20), ago(300), ago(7300), ago(200_000), ago(-5)], ["just now", "5m ago", "2h ago", "2d ago", "just now"]);
    }

    #[test]
    fn given_the_steps_of_a_run_when_timed_then_setup_post_and_short_steps_are_left_out_and_the_slowest_comes_first() {
        // Given the steps of one job
        let row = |no, step: &str, secs| StepRow { run: "1".into(), job: "core-tests (1/3)".into(), step_no: no, step: step.into(), started: None, completed: None, duration_s: Some(secs), conclusion: None };
        let steps = [row(1, "Set up job", 90), row(2, "Install", 40), row(3, "Flow tests", 200), row(4, "Quick", 2), row(5, "Post Install", 30), row(6, "Complete job", 20)];
        // When they are timed
        let jobs = job_times(&steps);
        // Then only the job's own steps of five seconds or more stay, longest first
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].steps, [("Flow tests".to_string(), 200), ("Install".to_string(), 40)]);
    }
}
