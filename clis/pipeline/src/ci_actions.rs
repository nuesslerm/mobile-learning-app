//! What the CI comment says to do about its own numbers: the "Where the idle time is" block (the steps of the run with the most idle seconds, each in a fixed
//! category, and the overhead share against the median of the last runs) and one action line per signal that is amber or red. The thresholds, the categories and the
//! actions are the constants below, so a test reads the same table the comment is built from; no action is free text.
use crate::ci_comment::{cell, text};
use crate::ci_insights::span;
use crate::ci_telemetry::Telemetry;
use crate::ci_trend::{arrow, key_of, last_of, median, Sealed};
use wf_bucket::ci_db::{JobResources, RunProfile, StepResources};

/// The steps the block lists.
const TOP_STEPS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Signal {
    Wall,
    Overhead,
    Idle,
    Efficiency,
    Queue,
    Replay,
    Iowait,
    Pressure,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Level {
    Green,
    Amber,
    Red,
}

/// One signal: how it is measured, where it turns amber and red (a value beyond the threshold, in the direction `higher_is_better` gives) and what to do about it.
pub(crate) struct Rule {
    pub signal: Signal,
    pub label: &'static str,
    pub unit: &'static str,
    pub amber: f64,
    pub red: f64,
    pub higher_is_better: bool,
    pub action: &'static str,
}

pub(crate) const RULES: [Rule; 8] = [
    Rule {
        signal: Signal::Wall,
        label: "Wall time",
        unit: "% slower than the median",
        amber: 10.0,
        red: 25.0,
        higher_is_better: false,
        action: "Start with the longest job of the critical path and its slowest step; a shard far longer than its siblings means the weights need re-measuring (`wf ci plan shard-weights --report`).",
    },
    Rule {
        signal: Signal::Overhead,
        label: "Overhead share",
        unit: "% of step seconds",
        amber: 30.0,
        red: 45.0,
        higher_is_better: false,
        action: "Fixed cost eats the runners: merge jobs that each pay setup and install, and give every job only the work that needs its own runner.",
    },
    Rule {
        signal: Signal::Idle,
        label: "Idle share",
        unit: "% of sampled seconds",
        amber: 35.0,
        red: 55.0,
        higher_is_better: false,
        action: "The CPUs wait: overlap the steps that idle (start the next stage while a wait runs) and run them on fewer, smaller runners.",
    },
    Rule {
        signal: Signal::Efficiency,
        label: "Parallel efficiency",
        unit: "",
        amber: 0.75,
        red: 0.6,
        higher_is_better: true,
        action: "The jobs run in a chain or one shard dominates: let independent jobs start together and rebalance the shards.",
    },
    Rule {
        signal: Signal::Queue,
        label: "Queue wait in total",
        unit: " s",
        amber: 20.0,
        red: 60.0,
        higher_is_better: false,
        action: "Jobs wait for runners: fewer, longer jobs ask less of the account's runner cap; the runner pool line says how much headroom there was.",
    },
    Rule {
        signal: Signal::Replay,
        label: "Nx replay",
        unit: " points under the median of the last runs",
        amber: 10.0,
        red: 30.0,
        higher_is_better: false,
        action: "Fewer tasks were replayed than this stage reached before: find the input whose hash changed; for a shard with nothing to run, skip the runner.",
    },
    Rule {
        signal: Signal::Iowait,
        label: "I/O wait average",
        unit: "%",
        amber: 10.0,
        red: 20.0,
        higher_is_better: false,
        action: "The disk is the limit: cut what the step writes (cache entry size, docker layers) or work on tmpfs.",
    },
    Rule {
        signal: Signal::Pressure,
        label: "Pressure peak",
        unit: "%",
        amber: 40.0,
        red: 60.0,
        higher_is_better: false,
        action: "The kernel stalls on CPU, memory or I/O: run fewer Nx tasks at once in that job or move it to a bigger runner.",
    },
];

pub(crate) fn rule(signal: Signal) -> &'static Rule {
    RULES.iter().find(|r| r.signal == signal).unwrap_or(&RULES[0])
}

/// A value exactly on a threshold is still the better level.
pub(crate) fn level(signal: Signal, value: f64) -> Level {
    let r = rule(signal);
    let past = |limit: f64| if r.higher_is_better { value < limit } else { value > limit };
    if past(r.red) {
        Level::Red
    } else if past(r.amber) {
        Level::Amber
    } else {
        Level::Green
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Category {
    Setup,
    Install,
    Cache,
    TestWait,
    E2eBoot,
    Teardown,
    Other,
}

impl Category {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Category::Setup => "setup",
            Category::Install => "install",
            Category::Cache => "cache",
            Category::TestWait => "test wait",
            Category::E2eBoot => "e2e boot",
            Category::Teardown => "teardown",
            Category::Other => "other",
        }
    }

    /// The lever of a category, said once for the idle block and the action lines.
    pub(crate) fn lever(self) -> &'static str {
        match self {
            Category::Setup => "setup is paid once per runner: merge the jobs that each run it, or keep the toolchain in the runner image",
            Category::Install => "cache the dependency install or merge shards that each install it",
            Category::Cache => "a cache step over 60 s is not paying for itself: check the entry size",
            Category::TestWait => "the machine waits inside the test or build step: run more tasks at once in it, or split what is serial",
            Category::E2eBoot => "start the device or the browser while the build runs, not after it",
            Category::Teardown => "teardown is the runner stopping: the only lever is fewer runners, so fewer, longer jobs",
            Category::Other => "look at what the step does while the CPU waits",
        }
    }

    /// Setup, install and teardown (which includes the runner's start and stop) are the overhead share.
    fn is_overhead(self) -> bool {
        matches!(self, Category::Setup | Category::Install | Category::Teardown)
    }
}

/// The category of a step by its name, from the fixed lists below; the first that matches wins.
pub(crate) fn classify(step: &str) -> Category {
    let n = step.to_lowercase();
    let has = |names: &[&str]| names.iter().any(|w| n.contains(w));
    let word = |names: &[&str]| n.split(|c: char| !c.is_alphanumeric()).any(|t| names.contains(&t));
    if n.starts_with("post ") || n == "complete job" {
        Category::Teardown
    } else if n == "set up job" || has(&["setup-job"]) {
        Category::Setup
    } else if has(&["-cache", "cache restore", "cache save", "restore cache", "save cache", "actions/cache"]) {
        Category::Cache
    } else if has(&["install-test-deps", "install-wf", "install wf"]) || word(&["install", "pnpm", "bun", "apt", "playwright"]) {
        Category::Install
    } else if ["how many", "which ", "the test plan", "point ", "hand ", "publish", "upload", "download"].iter().any(|p| n.starts_with(p)) {
        Category::Other
    } else if has(&["android", "emulator", "avd", "device", "browser"]) && word(&["start", "boot", "launch", "up", "bring", "wait", "prepare", "lease"]) {
        Category::E2eBoot
    } else if word(&["test", "tests", "build", "flow", "e2e", "integration", "lint", "typecheck", "verify"]) {
        Category::TestWait
    } else {
        Category::Other
    }
}

/// The share of step seconds that is overhead, of `(step name, seconds)` pairs; `None` without seconds.
pub(crate) fn overhead_share<'a>(steps: impl Iterator<Item = (&'a str, f64)>) -> Option<f64> {
    let (mut overhead, mut all) = (0.0, 0.0);
    for (name, secs) in steps {
        all += secs;
        if classify(name).is_overhead() {
            overhead += secs;
        }
    }
    (all > 0.0).then(|| overhead * 100.0 / all)
}

pub(crate) fn quoted(runs: &[&str]) -> String {
    runs.iter().map(|r| format!("'{}'", r.replace('\'', "''"))).collect::<Vec<_>>().join(", ")
}

/// The overhead share of each of the runs, from their `CI_STEPS` rows in the sealed database.
fn past_overheads(sealed: &Sealed, runs: &[&str]) -> Vec<f64> {
    if runs.is_empty() {
        return Vec::new();
    }
    let rows = sealed.rows(&format!("SELECT RUN_ID AS run, STEP AS step, DURATION_S AS secs FROM CI_STEPS WHERE DURATION_S IS NOT NULL AND RUN_ID IN ({})", quoted(runs)));
    runs.iter().filter_map(|run| overhead_share(rows.iter().filter(|r| text(r, "run") == *run).map(|r| (r["step"].as_str().unwrap_or_default(), r["secs"].as_f64().unwrap_or(0.0))))).collect()
}

/// A job's history run with fewer tasks than this says nothing about how much of it a cache can replay.
const MIN_CEILING_TASKS: i64 = 3;

/// The median replay share each job reached in those runs, from the Nx ledgers in the sealed database; a run with few tasks is left out, so one lucky or
/// docs-only run does not set the bar.
fn replay_ceilings(sealed: &Sealed, runs: &[&str]) -> Vec<(String, f64)> {
    if runs.is_empty() {
        return Vec::new();
    }
    let rows = sealed.rows(&format!(
        "SELECT JOB AS job, 1.0 * hit / n AS share FROM (SELECT RUN_ID, JOB, SUM(CASE WHEN OUTCOME = 'hit' THEN 1 ELSE 0 END) AS hit, COUNT(*) AS n FROM NX_TASKS WHERE RUN_ID IN ({}) GROUP BY RUN_ID, JOB) WHERE n >= {MIN_CEILING_TASKS}",
        quoted(runs)
    ));
    let mut by_job: Vec<(String, Vec<f64>)> = Vec::new();
    for r in &rows {
        let Some(share) = r["share"].as_f64() else { continue };
        let job = text(r, "job");
        match by_job.iter_mut().find(|e| e.0 == job) {
            Some(e) => e.1.push(share),
            None => by_job.push((job, vec![share])),
        }
    }
    by_job.into_iter().filter_map(|(job, shares)| Some((job, median(shares)?))).collect()
}

struct Reading {
    signal: Signal,
    value: f64,
    shown: String,
    /// The part of the run to look at: the job that is worst, or the lever of the biggest category.
    hint: String,
}

fn share(part: f64, whole: f64) -> f64 {
    if whole > 0.0 { part * 100.0 / whole } else { 0.0 }
}

/// What the biggest of these `(category, seconds)` is, as its lever.
fn biggest(mut by_category: Vec<(Category, f64)>) -> Option<Category> {
    by_category.sort_by(|a, b| b.1.total_cmp(&a.1));
    by_category.first().filter(|c| c.1 > 0.0).map(|c| c.0)
}

fn readings(profile: &RunProfile, resources: &[JobResources], steps: &[StepResources], overhead: Option<f64>, wall_median: Option<f64>, replay: &[(String, f64)], t: &Telemetry) -> Vec<Reading> {
    let mut out = Vec::new();
    let named = |job: &str| cell(wf_bucket::job_name::job_name(job));
    if let Some(m) = wall_median.filter(|m| *m > 0.0) {
        let slower = (profile.wall_s / m - 1.0) * 100.0;
        out.push(Reading { signal: Signal::Wall, value: slower, shown: format!("{slower:+.0}%"), hint: String::new() });
    }
    if let Some(v) = overhead {
        let mut by: Vec<(Category, f64)> = Vec::new();
        for r in t.step_rows.iter().filter(|r| classify(&r.step).is_overhead()) {
            let c = classify(&r.step);
            match by.iter_mut().find(|e| e.0 == c) {
                Some(e) => e.1 += r.duration_s.unwrap_or(0) as f64,
                None => by.push((c, r.duration_s.unwrap_or(0) as f64)),
            }
        }
        out.push(Reading { signal: Signal::Overhead, value: v, shown: format!("{v:.0}%"), hint: biggest(by).map(|c| format!("Biggest part is {}: {}.", c.name(), c.lever())).unwrap_or_default() });
    }
    let (idle, sampled): (f64, f64) = resources.iter().fold((0.0, 0.0), |a, r| (a.0 + r.idle_s, a.1 + r.duration_s));
    if sampled > 0.0 {
        let mut by: Vec<(Category, f64)> = Vec::new();
        for r in steps {
            let c = classify(&r.step);
            match by.iter_mut().find(|e| e.0 == c) {
                Some(e) => e.1 += r.idle_s,
                None => by.push((c, r.idle_s)),
            }
        }
        let v = share(idle, sampled);
        out.push(Reading { signal: Signal::Idle, value: v, shown: format!("{v:.0}%"), hint: biggest(by).map(|c| format!("Most idle is in {} steps: {}.", c.name(), c.lever())).unwrap_or_default() });
    }
    if profile.parallel_efficiency > 0.0 {
        out.push(Reading { signal: Signal::Efficiency, value: profile.parallel_efficiency, shown: format!("{:.0}%", profile.parallel_efficiency * 100.0), hint: String::new() });
    }
    out.push(Reading { signal: Signal::Queue, value: profile.queue_s_total, shown: span(profile.queue_s_total as i64), hint: String::new() });
    if let Some((job, points)) = replay.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
        out.push(Reading { signal: Signal::Replay, value: *points, shown: format!("{points:.0} points under"), hint: format!("Worst: {}.", named(job)) });
    }
    if let Some(r) = resources.iter().filter(|r| r.samples > 0).max_by(|a, b| a.iowait_avg.total_cmp(&b.iowait_avg)) {
        out.push(Reading { signal: Signal::Iowait, value: r.iowait_avg, shown: format!("{:.0}%", r.iowait_avg), hint: format!("Worst: {}.", named(&r.job)) });
    }
    let pressure = |r: &JobResources| r.psi_cpu_max.max(r.psi_mem_max).max(r.psi_io_max);
    if let Some(r) = resources.iter().filter(|r| r.samples > 0).max_by(|a, b| pressure(a).total_cmp(&pressure(b))) {
        out.push(Reading { signal: Signal::Pressure, value: pressure(r), shown: format!("{:.0}%", pressure(r)), hint: format!("Worst: {}.", named(&r.job)) });
    }
    out
}

/// One line for a reading that is amber or red; none for green.
fn action_line(r: &Reading) -> Option<String> {
    let lvl = level(r.signal, r.value);
    let rule = rule(r.signal);
    let (mark, word) = match lvl {
        Level::Green => return None,
        Level::Amber => ("🟠", "amber"),
        Level::Red => ("🔴", "red"),
    };
    let way = if rule.higher_is_better { "under" } else { "over" };
    let percent = |v: f64| if r.signal == Signal::Efficiency { format!("{:.0}%", v * 100.0) } else { v.to_string() };
    let limits = format!("{word}; amber {way} {}, red {way} {}{}", percent(rule.amber), percent(rule.red), if rule.unit.is_empty() { String::new() } else { format!(" ({})", rule.unit.trim()) });
    let hint = if r.hint.is_empty() { String::new() } else { format!(" {}", r.hint) };
    Some(format!("- {mark} **{} {}** ({limits}): {}{hint}\n", rule.label, r.shown, rule.action))
}

/// Empty without a finished run's profile (the lines are about the whole run). Otherwise one line per amber or red signal.
fn action_block(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    format!("\n### What to do\n\n{}", lines.concat())
}

fn idle_block(steps: &[StepResources], overhead: Option<f64>, overhead_median: Option<(f64, usize)>) -> String {
    let total: f64 = steps.iter().map(|s| s.idle_s).sum();
    let mut top: Vec<&StepResources> = steps.iter().filter(|s| s.idle_s > 0.0).collect();
    top.sort_by(|a, b| b.idle_s.total_cmp(&a.idle_s).then_with(|| a.job.cmp(&b.job)).then(a.step_no.cmp(&b.step_no)));
    if top.is_empty() && overhead.is_none() {
        return String::new();
    }
    let mut out = String::from("\n### Where the idle time is\n\n");
    if !top.is_empty() {
        out.push_str("The five steps of this run with the most idle seconds (CPU under 25% of the samples over the step).\n\n| Step | Job | Idle | Share of the run's idle | Category |\n|---|---|---|---|---|\n");
        for s in top.iter().take(TOP_STEPS) {
            out.push_str(&format!("| {} | {} | {} | {:.0}% | {} |\n", cell(&s.step), cell(wf_bucket::job_name::job_name(&s.job)), span(s.idle_s.round() as i64), share(s.idle_s, total), classify(&s.step).name()));
        }
        out.push('\n');
    }
    if let Some(v) = overhead {
        let against = overhead_median.map_or(String::new(), |(m, n)| format!(" {} (median {m:.0}% of the last {n} successful runs)", arrow(v, m)));
        out.push_str(&format!("**Overhead** (setup, install, runner start and stop, teardown): {v:.0}% of all step seconds{against}.\n"));
    }
    out
}

/// The block and the action lines for a comment; empty when the run has no step with samples over it, no profile and no signal to act on. `sealed` is the
/// database of earlier runs (`None` when it cannot be read: the median and the ceilings are then left out and nothing else changes).
pub(crate) fn section(sealed: Option<&Sealed>, t: &Telemetry) -> String {
    let key = t.profile.as_ref().and_then(|p| key_of(&p.workflow));
    let past = match (sealed, key) {
        (Some(s), Some(k)) => last_of(&s.past, k),
        _ => Vec::new(),
    };
    let runs: Vec<&str> = past.iter().map(|p| p.run.as_str()).collect();
    let overhead = overhead_share(t.step_rows.iter().filter_map(|r| Some((r.step.as_str(), r.duration_s? as f64))));
    let past_overhead = sealed.map(|s| past_overheads(s, &runs)).unwrap_or_default();
    let overhead_median = median(past_overhead.clone()).map(|m| (m, past_overhead.len()));
    let mut out = idle_block(&t.steps, overhead, overhead_median);
    let Some(profile) = &t.profile else { return out };
    let ceilings = sealed.map(|s| replay_ceilings(s, &runs)).unwrap_or_default();
    let replay: Vec<(String, f64)> = t
        .replay
        .iter()
        .filter(|(_, ran, hit)| ran + hit > 0)
        .filter_map(|(job, ran, hit)| {
            let ceiling = ceilings.iter().find(|c| c.0 == *job)?.1;
            Some((job.clone(), (ceiling - *hit as f64 / (*ran + *hit) as f64).max(0.0) * 100.0))
        })
        .collect();
    let wall_median = median(past.iter().map(|p| p.wall).collect());
    let all = readings(profile, &t.resources, &t.steps, overhead, wall_median, &replay, t);
    let lines: Vec<String> = all.iter().filter_map(action_line).collect();
    out.push_str(&action_block(&lines));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_owners_thresholds_when_a_value_is_leveled_then_it_is_amber_or_red_past_them_in_the_signals_direction_and_green_on_them() {
        // Given the table as the owner saw it
        let table: Vec<(Signal, f64, f64)> = RULES.iter().map(|r| (r.signal, r.amber, r.red)).collect();
        assert_eq!(
            table,
            [
                (Signal::Wall, 10.0, 25.0),
                (Signal::Overhead, 30.0, 45.0),
                (Signal::Idle, 35.0, 55.0),
                (Signal::Efficiency, 0.75, 0.6),
                (Signal::Queue, 20.0, 60.0),
                (Signal::Replay, 10.0, 30.0),
                (Signal::Iowait, 10.0, 20.0),
                (Signal::Pressure, 40.0, 60.0),
            ]
        );
        // When values on, between and past them are leveled
        let at = |s, v| level(s, v);
        // Then a lower-is-better signal is green on its amber line and a higher-is-better one turns amber and red below its lines
        assert_eq!([at(Signal::Idle, 35.0), at(Signal::Idle, 35.1), at(Signal::Idle, 55.1)], [Level::Green, Level::Amber, Level::Red]);
        assert_eq!([at(Signal::Efficiency, 0.75), at(Signal::Efficiency, 0.7), at(Signal::Efficiency, 0.5)], [Level::Green, Level::Amber, Level::Red]);
        assert_eq!([at(Signal::Queue, 15.0), at(Signal::Queue, 30.0), at(Signal::Queue, 90.0)], [Level::Green, Level::Amber, Level::Red]);
    }

    #[test]
    fn given_every_signal_when_its_action_is_read_then_each_has_one_distinct_action_and_each_lever_is_said() {
        // Given the table
        // When the actions are collected
        let mut actions: Vec<&str> = RULES.iter().map(|r| r.action).collect();
        actions.sort_unstable();
        actions.dedup();
        // Then every signal has its own non-empty action, and the levers the owner named are in the table
        assert_eq!(actions.len(), RULES.len());
        assert!(actions.iter().all(|a| !a.is_empty()));
        assert!(Category::Install.lever().contains("cache the dependency install or merge shards that each install it"));
        assert!(Category::Cache.lever().contains("a cache step over 60 s is not paying for itself: check the entry size"));
        assert!(rule(Signal::Replay).action.contains("skip the runner"));
    }

    #[test]
    fn given_step_names_when_they_are_classified_then_each_falls_in_the_category_of_the_fixed_lists() {
        // Given the names of the steps a hosted job has
        let names = [
            ("Set up job", Category::Setup),
            ("Run ./.github/actions/setup-job", Category::Setup),
            ("Run ./.github/actions/install-test-deps", Category::Install),
            ("Run pnpm install --frozen-lockfile", Category::Install),
            ("Install wf", Category::Install),
            ("Run ./.github/actions/install-wf", Category::Install),
            ("Run apt-get install -y jq", Category::Install),
            ("Install Playwright browsers", Category::Install),
            ("Run ./.github/actions/nx-cache", Category::Cache),
            ("Run ./.github/actions/sccache-cache", Category::Cache),
            ("Run ./.github/actions/playwright-cache", Category::Cache),
            ("Run ./.github/actions/metro-cache", Category::Cache),
            ("Install ffmpeg", Category::Install),
            ("Start the Chromium install for the browser harness", Category::Install),
            ("How many machines per test stage", Category::Other),
            ("Which tests, by stage", Category::Other),
            ("Which e2e tests", Category::Other),
            ("Which crates each integration shard runs", Category::Other),
            ("The test plan", Category::Other),
            ("Point the tests at the downloaded EAS pointers", Category::Other),
            ("Hand the EAS pointers to the e2e jobs", Category::Other),
            ("Publish the flow run bundle", Category::Other),
            ("Upload the integration run bundles", Category::Other),
            ("Publish the EAS Update the Android e2e tests load", Category::Other),
            ("Restore cache", Category::Cache),
            ("Flow tests", Category::TestWait),
            ("Build the images", Category::TestWait),
            ("Start the Android emulator", Category::E2eBoot),
            ("Start browser", Category::E2eBoot),
            ("Post Run ./.github/actions/nx-cache", Category::Teardown),
            ("Complete job", Category::Teardown),
            ("Run actions/checkout@v4", Category::Other),
            ("Capture the adapter", Category::Other),
        ];
        // When each is classified, then the category is the listed one (a word that only contains `apt` is not an install)
        for (name, expected) in names {
            assert_eq!(classify(name), expected, "{name}");
        }
    }

    #[test]
    fn given_steps_with_overhead_when_the_share_is_taken_then_setup_install_and_teardown_count_and_tests_and_caches_do_not() {
        // Given 10 s of setup, 20 s of install, 10 s of teardown, 50 s of tests and 10 s of cache
        let steps = [("Set up job", 10.0), ("Run pnpm install", 20.0), ("Complete job", 10.0), ("Flow tests", 50.0), ("Run ./.github/actions/nx-cache", 10.0)];
        // When the share is taken, then 40 of 100 s are overhead, and no seconds give none
        assert_eq!(overhead_share(steps.into_iter()), Some(40.0));
        assert_eq!(overhead_share(std::iter::empty()), None);
    }

    #[test]
    fn given_no_step_with_idle_seconds_when_the_block_is_made_then_the_overhead_line_is_still_said_with_its_median() {
        // Given a run whose steps have no idle seconds and an overhead of 40% against a median of 20%
        // When the block is made, then the overhead line is there and no table is
        let block = idle_block(&[], Some(40.0), Some((20.0, 5)));
        assert!(block.contains("**Overhead**") && block.contains("40% of all step seconds") && block.contains("median 20% of the last 5 successful runs"), "{block}");
        assert!(!block.contains("| Step |"), "{block}");
        // And with neither idle steps nor an overhead share the block is empty
        assert_eq!(idle_block(&[], None, None), "");
    }
}
