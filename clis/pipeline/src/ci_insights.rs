//! What a CI run's logs, jobs and the VM's samples say about where its time and resources went, for `wf ci comment`: the facts read out
//! of a job log (Nx task timings and cache counts, suite and test durations, deploy phases, sccache), the waits between jobs, and
//! the sections that lay them out so slow work, resource hogs and cache misses can be found.
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a valid pattern"))
}

/// One `nx run-many` summary a log carries.
#[derive(Debug, Clone, PartialEq)]
pub struct NxRun {
    pub duration: String,
    pub hits: u64,
    pub total: u64,
    pub critical: String,
    pub recoverable: String,
}

/// What one job's log says.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LogFacts {
    /// Nx tasks that ran, with their milliseconds.
    pub nx_tasks: Vec<(String, u64)>,
    pub nx_runs: Vec<NxRun>,
    /// Test suites (`cargo test` binaries) as (crate, seconds).
    pub suites: Vec<(String, f64)>,
    /// Tests whose start and result were printed on separate lines, as (crate, test, seconds).
    pub tests: Vec<(String, String, f64)>,
    /// Deploy phases (`phase build: 280 s`).
    pub phases: Vec<(String, u64)>,
    /// sccache's own hits, misses and write errors.
    pub sccache: Option<(u64, u64, u64)>,
    /// What each Nx stage cost this process tree: target, CPU seconds, bytes written, wall seconds.
    pub nx_cost: Vec<(String, f64, u64, f64)>,
}

/// Seconds since the epoch of a log line's timestamp, with its fraction.
fn stamp(line: &str) -> Option<(f64, &str)> {
    static STAMP: OnceLock<Regex> = OnceLock::new();
    let c = re(&STAMP, r"^\x{feff}?(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(\.\d+)?Z ?").captures(line)?;
    let whole = crate::ci_comment::epoch_of(&format!("{}Z", &c[1]))?;
    let fraction = c.get(2).and_then(|f| f.as_str().parse::<f64>().ok()).unwrap_or(0.0);
    Some((whole as f64 + fraction, &line[c.get(0)?.end()..]))
}

/// A line without Nx's `crate-x: ` label, and the label.
fn tagged(line: &str) -> (&str, &str) {
    static TAG: OnceLock<Regex> = OnceLock::new();
    match re(&TAG, r"^((?:crate-[A-Za-z0-9_-]+|@[A-Za-z0-9_/-]+)):\s*").captures(line) {
        Some(c) => (c.get(1).map_or("", |m| m.as_str()), &line[c.get(0).map_or(0, |m| m.end())..]),
        None => ("", line),
    }
}

pub fn facts(log: &str) -> LogFacts {
    static COLOUR: OnceLock<Regex> = OnceLock::new();
    static TASK: OnceLock<Regex> = OnceLock::new();
    static SUITE: OnceLock<Regex> = OnceLock::new();
    static TEST: OnceLock<Regex> = OnceLock::new();
    static PHASE: OnceLock<Regex> = OnceLock::new();
    static CACHE: OnceLock<Regex> = OnceLock::new();
    let colour = re(&COLOUR, r"\x1b\[[0-9;?]*[ -/]*[@-~]");
    let (task, suite, test, phase, cache) = (
        re(&TASK, r#"^"([^"]+)":\s*(\d+),?$"#),
        re(&SUITE, r"^test result: .*?finished in ([0-9.]+)s"),
        re(&TEST, r"^test (\S+) \.\.\. ?$"),
        re(&PHASE, r"^phase (.+?): (\d+) s$"),
        re(&CACHE, r"^(\d+)/(\d+) hit"),
    );
    let mut out = LogFacts::default();
    let (mut in_timings, mut last_tag) = (false, String::new());
    let mut pending: BTreeMap<String, (String, f64)> = BTreeMap::new();
    let (mut duration, mut hits, mut critical) = (None::<String>, None::<(u64, u64)>, None::<String>);
    for raw in log.split('\n') {
        let clean = colour.replace_all(raw, "");
        let (at, rest) = match stamp(&clean) {
            Some((at, rest)) => (Some(at), rest),
            None => (None, clean.as_ref()),
        };
        let (tag, bare) = tagged(rest.trim());
        if !tag.is_empty() {
            last_tag = tag.to_string();
        }
        if in_timings {
            if bare == "}" {
                in_timings = false;
            } else if let Some(c) = task.captures(bare) {
                out.nx_tasks.push((c[1].to_string(), c[2].parse().unwrap_or(0)));
            }
            continue;
        }
        if bare == "Task Execution Timings:" {
            in_timings = true;
        } else if let Some(v) = bare.strip_prefix("Run duration:") {
            duration = Some(v.trim().to_string());
        } else if let Some(c) = bare.strip_prefix("Cache:").and_then(|v| cache.captures(v.trim())) {
            hits = Some((c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0)));
        } else if let Some(v) = bare.strip_prefix("Critical path:") {
            critical = Some(v.trim().to_string());
        } else if let Some(v) = bare.strip_prefix("Recoverable time:") {
            if let (Some(duration), Some((hits, total)), Some(critical)) = (duration.take(), hits.take(), critical.take()) {
                out.nx_runs.push(NxRun { duration, hits, total, critical, recoverable: v.trim().to_string() });
            }
        } else if let Some(c) = suite.captures(bare) {
            out.suites.push((last_tag.clone(), c[1].parse().unwrap_or(0.0)));
        } else if let Some(v) = bare.strip_prefix("; finished in ").and_then(|v| v.strip_suffix('s')) {
            out.suites.push((last_tag.clone(), v.parse().unwrap_or(0.0)));
        } else if let (Some(c), Some(at)) = (test.captures(bare), at) {
            pending.insert(tag.to_string(), (c[1].to_string(), at));
        } else if matches!(bare, "ok" | "FAILED" | "ignored") {
            if let (Some((name, from)), Some(at)) = (pending.remove(tag), at) {
                out.tests.push((tag.to_string(), name, (at - from).max(0.0)));
            }
        } else if let Some(c) = phase.captures(bare) {
            out.phases.push((c[1].to_string(), c[2].parse().unwrap_or(0)));
        } else if let Some(json) = bare.strip_prefix("wf: nx cost ") {
            if let Ok(v) = serde_json::from_str::<Value>(json) {
                out.nx_cost.push((v["target"].as_str().unwrap_or("?").to_string(), v["cpuSeconds"].as_f64().unwrap_or(0.0), v["writtenBytes"].as_u64().unwrap_or(0), v["seconds"].as_f64().unwrap_or(0.0)));
            }
        } else if let Some(json) = bare.strip_prefix("wf: sccache ") {
            if let Ok(v) = serde_json::from_str::<Value>(json) {
                let n = |key: &str| v[key].as_str().and_then(|s| s.parse::<u64>().ok()).or_else(|| v[key].as_u64()).unwrap_or(0);
                let sum = out.sccache.unwrap_or((0, 0, 0));
                out.sccache = Some((sum.0 + n("hits"), sum.1 + n("misses"), sum.2 + n("writeErrors")));
            }
        }
    }
    out
}

/// "1h 02m", "4m 05s", "37s".
pub fn span(secs: i64) -> String {
    match secs {
        s if s >= 3600 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
        s if s >= 60 => format!("{}m {:02}s", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

pub fn bytes(value: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (value.max(0.0), 0);
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{v:.0} B") } else { format!("{v:.1} {}", UNITS[i]) }
}

/// A job as the time sections see it.
#[derive(Debug, Clone)]
pub struct JobTime {
    pub name: String,
    pub started: Option<i64>,
    pub ended: Option<i64>,
    /// Seconds between the job being created (its needs done) and a runner starting it.
    pub queued: Option<i64>,
    /// Every step of at least five seconds, longest first.
    pub steps: Vec<(String, i64)>,
}

impl JobTime {
    pub fn secs(&self) -> Option<i64> {
        self.started.zip(self.ended).filter(|(f, t)| t >= f).map(|(f, t)| t - f)
    }
}

/// The wall time of the run, the part of it any job was running, and the rest: the waits between jobs.
pub fn busy_and_idle(jobs: &[JobTime]) -> Option<(i64, i64, i64)> {
    let mut spans: Vec<(i64, i64)> = jobs.iter().filter_map(|j| j.started.zip(j.ended)).filter(|(f, t)| t >= f).collect();
    spans.sort_unstable();
    let (first, last) = (spans.first()?.0, spans.iter().map(|s| s.1).max()?);
    let (mut busy, mut reach) = (0, first);
    for (from, to) in spans {
        busy += (to - from.max(reach)).max(0);
        reach = reach.max(to);
    }
    Some((last - first, busy, last - first - busy))
}

fn top<T: Clone>(items: &[T], n: usize, key: impl Fn(&T) -> f64) -> Vec<T> {
    let mut v = items.to_vec();
    v.sort_by(|a, b| key(b).total_cmp(&key(a)));
    v.truncate(n);
    v
}

/// The first things to read: how the wall time splits into work and waiting, and the single biggest job, wait, step, suite, test and task.
pub fn where_time_went(jobs: &[JobTime], facts: &[(String, LogFacts)]) -> String {
    let Some((wall, busy, idle)) = busy_and_idle(jobs) else { return String::new() };
    let mut out = String::from("\n### ⏱️ Where the time went\n\n");
    out.push_str(&format!("- **{} wall** = {} of jobs running + **{} waiting between them** (the jobs run one at a time on one runner)\n", span(wall), span(busy), span(idle)));
    let slow = top(&jobs.iter().filter_map(|j| Some((j.name.clone(), j.secs()?))).collect::<Vec<_>>(), 3, |(_, s)| *s as f64);
    if !slow.is_empty() {
        let list = slow.iter().map(|(n, s)| format!("**{n}** {} ({}% of the run)", span(*s), (*s as f64 * 100.0 / wall.max(1) as f64).round())).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("- Longest jobs: {list}\n"));
    }
    let waits = top(&jobs.iter().filter_map(|j| Some((j.name.clone(), j.queued?))).filter(|(_, q)| *q >= 60).collect::<Vec<_>>(), 3, |(_, q)| *q as f64);
    if !waits.is_empty() {
        out.push_str(&format!("- Waited for the runner: {}\n", waits.iter().map(|(n, q)| format!("**{n}** {}", span(*q))).collect::<Vec<_>>().join(", ")));
    }
    let steps: Vec<(String, i64)> = jobs.iter().flat_map(|j| j.steps.iter().take(3).map(|(s, t)| (format!("{} › {s}", j.name), *t))).collect();
    let steps = top(&steps, 4, |(_, t)| *t as f64);
    if !steps.is_empty() {
        out.push_str(&format!("- Slowest steps: {}\n", steps.iter().map(|(s, t)| format!("{s} **{}**", span(*t))).collect::<Vec<_>>().join("; ")));
    }
    let tests: Vec<(String, f64)> = facts.iter().flat_map(|(_, f)| f.tests.iter().map(|(c, t, s)| (format!("{c} › {t}"), *s))).collect();
    let tests = top(&tests, 3, |(_, s)| *s);
    if !tests.is_empty() {
        out.push_str(&format!("- Slowest tests: {}\n", tests.iter().map(|(t, s)| format!("`{t}` **{}**", span(s.round() as i64))).collect::<Vec<_>>().join("; ")));
    }
    let suites: Vec<(String, f64)> = facts.iter().flat_map(|(_, f)| f.suites.clone()).collect();
    let suites = top(&suites, 3, |(_, s)| *s);
    if !suites.is_empty() {
        out.push_str(&format!("- Slowest test suites: {}\n", suites.iter().map(|(c, s)| format!("{c} **{}**", span(s.round() as i64))).collect::<Vec<_>>().join("; ")));
    }
    let tasks: Vec<(String, u64)> = facts.iter().flat_map(|(_, f)| f.nx_tasks.clone()).collect();
    let tasks = top(&tasks, 3, |(_, ms)| *ms as f64);
    if !tasks.is_empty() {
        out.push_str(&format!("- Slowest Nx tasks: {}\n", tasks.iter().map(|(t, ms)| format!("`{t}` **{:.1}s**", *ms as f64 / 1000.0)).collect::<Vec<_>>().join("; ")));
    }
    out
}

/// One job's cache figures.
#[derive(Debug, Default, Clone)]
pub struct CacheFacts {
    pub name: String,
    /// The remote build cache: hits, misses, puts, mean get milliseconds.
    pub build: Option<(u64, u64, u64, f64)>,
    /// Nx tasks replayed of those run.
    pub nx: Option<(u64, u64)>,
    pub sccache: Option<(u64, u64, u64)>,
    pub layers: Option<(u64, u64)>,
}

fn pct(part: u64, whole: u64) -> String {
    if whole == 0 { "-".into() } else { format!("{}% ({part}/{whole})", (part as f64 * 100.0 / whole as f64).round()) }
}

/// Every cache layer per job with the totals over the run, then the work that ran for lack of a hit, slowest first.
pub fn cache_section(rows: &[CacheFacts], facts: &[(String, LogFacts)]) -> String {
    if rows.iter().all(|r| r.build.is_none() && r.nx.is_none() && r.sccache.is_none() && r.layers.is_none()) {
        return String::new();
    }
    let sum2 = |pick: &dyn Fn(&CacheFacts) -> Option<(u64, u64)>| rows.iter().filter_map(pick).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    let build = sum2(&|r| r.build.map(|b| (b.0, b.0 + b.1)));
    let nx = sum2(&|r| r.nx);
    let scc = sum2(&|r| r.sccache.map(|s| (s.0, s.0 + s.1)));
    let layers = sum2(&|r| r.layers);
    let mut out = String::from("\n### 🗄️ Cache\n\n");
    out.push_str(&format!("**Whole run:** build cache {} · Nx tasks replayed {} · sccache {} · docker layers cached {}\n\n", pct(build.0, build.1), pct(nx.0, nx.1), pct(scc.0, scc.1), pct(layers.0, layers.1)));
    out.push_str("| Job | Nx tasks | Build cache | sccache | Layers |\n|---|---|---|---|---|\n");
    for r in rows {
        let cell = |v: Option<String>| v.unwrap_or_else(|| "-".into());
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            crate::ci_comment::cell(&r.name),
            cell(r.nx.map(|n| pct(n.0, n.1))),
            cell(r.build.map(|b| format!("{}<br><sub>get {:.0} ms · {} put</sub>", pct(b.0, b.0 + b.1), b.3, b.2))),
            cell(r.sccache.map(|s| format!("{}{}", pct(s.0, s.0 + s.1), if s.2 > 0 { format!(" ⚠️ {} write errors", s.2) } else { String::new() }))),
            cell(r.layers.map(|l| pct(l.0, l.1)))
        ));
    }
    let ran: Vec<(String, u64)> = facts.iter().flat_map(|(job, f)| f.nx_tasks.iter().map(move |(t, ms)| (format!("{t} ({job})"), *ms))).collect();
    let ran = top(&ran, 8, |(_, ms)| *ms as f64);
    if !ran.is_empty() {
        out.push_str("\nTasks that ran, slowest first (each is time a cache hit would have saved):\n\n");
        for (t, ms) in ran {
            out.push_str(&format!("- `{}` {:.1} s\n", crate::ci_comment::cell(&t), ms as f64 / 1000.0));
        }
    }
    out
}

/// The collapsed blocks: every slow step per job, the slowest tests and suites, the Nx runs and deploy phases.
pub fn detail_blocks(jobs: &[JobTime], facts: &[(String, LogFacts)]) -> String {
    let mut out = String::new();
    let rows: Vec<String> = jobs.iter().filter(|j| !j.steps.is_empty()).map(|j| format!("| {} | {} |\n", crate::ci_comment::cell(&j.name), j.steps.iter().take(6).map(|(s, t)| format!("{} {}", crate::ci_comment::cell(s), span(*t))).collect::<Vec<_>>().join("<br>"))).collect();
    if !rows.is_empty() {
        out.push_str(&format!("\n<details><summary>Slowest steps per job</summary>\n\n| Job | Steps (five seconds or more) |\n|---|---|\n{}\n</details>\n", rows.concat()));
    }
    let tests: Vec<(String, String, f64)> = facts.iter().flat_map(|(_, f)| f.tests.clone()).collect();
    let tests = top(&tests, 15, |t| t.2);
    if !tests.is_empty() {
        let body: String = tests.iter().map(|(c, t, s)| format!("| {} | `{}` | {} |\n", crate::ci_comment::cell(c), crate::ci_comment::cell(t), span(s.round() as i64))).collect();
        out.push_str(&format!("\n<details><summary>Slowest tests ({} timed)</summary>\n\n| Crate | Test | Took |\n|---|---|---|\n{body}\n</details>\n", facts.iter().map(|(_, f)| f.tests.len()).sum::<usize>()));
    }
    let suites: Vec<(String, f64)> = facts.iter().flat_map(|(_, f)| f.suites.clone()).collect();
    let suites = top(&suites, 12, |s| s.1);
    if !suites.is_empty() {
        let body: String = suites.iter().map(|(c, s)| format!("| {} | {} |\n", crate::ci_comment::cell(c), span(s.round() as i64))).collect();
        out.push_str(&format!("\n<details><summary>Test suites by time</summary>\n\n| Suite | Took |\n|---|---|\n{body}\n</details>\n"));
    }
    let runs: Vec<String> = facts.iter().flat_map(|(job, f)| f.nx_runs.iter().map(move |r| format!("| {} | {} | {}/{} | {} | {} |\n", crate::ci_comment::cell(job), r.duration, r.hits, r.total, r.critical, r.recoverable))).collect();
    if !runs.is_empty() {
        out.push_str(&format!(
            "\n<details><summary>Nx runs (the critical path is the longest chain of tasks; the rest of the run is overhead)</summary>\n\n| Job | Duration | Cache hits | Critical path | Recoverable |\n|---|---|---|---|---|\n{}\n</details>\n",
            runs.concat()
        ));
    }
    let phases: Vec<String> = facts.iter().filter(|(_, f)| !f.phases.is_empty()).map(|(job, f)| format!("| {} | {} |\n", crate::ci_comment::cell(job), f.phases.iter().map(|(p, s)| format!("{} {}", crate::ci_comment::cell(p), span(*s as i64))).collect::<Vec<_>>().join("<br>"))).collect();
    if !phases.is_empty() {
        out.push_str(&format!("\n<details><summary>Deploy phases</summary>\n\n| Leg | Phases |\n|---|---|\n{}\n</details>\n", phases.concat()));
    }
    out
}

/// What the Nx stages cost on the two-core VM: CPU seconds, the share of both cores they kept busy, and the bytes they wrote, per job.
pub fn cost_block(facts: &[(String, LogFacts)]) -> String {
    let rows: Vec<String> = facts
        .iter()
        .flat_map(|(job, f)| f.nx_cost.iter().map(move |(target, cpu, written, secs)| (job, target, *cpu, *written, *secs)))
        .map(|(job, target, cpu, written, secs)| format!("| {job} | {target} | {cpu:.0} s | {:.0}% | {} | {secs:.0} s |\n", cpu / (secs * 2.0).max(0.1) * 100.0, bytes(written as f64)))
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    format!("\n<details><summary>CPU and disk writes per Nx stage (children of the stage, 2 cores)</summary>\n\n| Job | Stage | CPU | Of both cores | Written | Wall |\n|---|---|---|---|---|---|\n{}\n</details>\n", rows.concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "2026-10-30T10:00:00.0000000Z crate-a: test slow::one ... \n2026-10-30T10:02:30.5000000Z crate-a: ok\n2026-10-30T10:02:31.0000000Z crate-a: test quick::two ... \n2026-10-30T10:02:32.0000000Z crate-a: FAILED\n2026-10-30T10:02:33.0000000Z crate-a: test result: FAILED. 1 passed; 1 failed; finished in 152.50s\n2026-10-30T10:02:34.0000000Z crate-b: test result: ok. 3 passed\n2026-10-30T10:02:34.5000000Z crate-b: ; finished in 9.25s\n2026-10-30T10:03:00.0000000Z Task Execution Timings:\n2026-10-30T10:03:00.0000000Z {\n2026-10-30T10:03:00.0000000Z   \"crate-a:cargo-flow\": 152000,\n2026-10-30T10:03:00.0000000Z   \"crate-b:cargo-flow\": 9200\n2026-10-30T10:03:00.0000000Z }\n2026-10-30T10:03:00.0000000Z   Run duration:      2m 35s\n2026-10-30T10:03:00.0000000Z   Cache:             1/3 hit (33%)\n2026-10-30T10:03:00.0000000Z   Critical path:     2m 32s (1 task)\n2026-10-30T10:03:00.0000000Z   Recoverable time:  3.0s (2% of the run)\n2026-10-30T10:03:01.0000000Z phase build: 280 s\n2026-10-30T10:03:02.0000000Z wf: sccache {\"hits\":\"4\",\"misses\":\"6\",\"writeErrors\":\"0\"}\n";

    #[test]
    fn given_a_log_when_read_then_tests_suites_tasks_runs_phases_and_sccache_come_out_with_their_numbers() {
        let f = facts(LOG);
        assert_eq!(f.tests.len(), 2);
        assert_eq!((f.tests[0].0.as_str(), f.tests[0].1.as_str(), f.tests[0].2), ("crate-a", "slow::one", 150.5));
        assert_eq!(f.suites, [("crate-a".to_string(), 152.5), ("crate-b".to_string(), 9.25)]);
        assert_eq!(f.nx_tasks, [("crate-a:cargo-flow".to_string(), 152_000), ("crate-b:cargo-flow".to_string(), 9_200)]);
        assert_eq!(f.nx_runs, [NxRun { duration: "2m 35s".into(), hits: 1, total: 3, critical: "2m 32s (1 task)".into(), recoverable: "3.0s (2% of the run)".into() }]);
        assert_eq!(f.phases, [("build".to_string(), 280)]);
        assert_eq!(f.sccache, Some((4, 6, 0)));
    }

    fn job(name: &str, started: i64, ended: i64, queued: i64) -> JobTime {
        JobTime { name: name.into(), started: Some(started), ended: Some(ended), queued: Some(queued), steps: vec![("Flow tests".into(), ended - started - 5)] }
    }

    #[test]
    fn given_jobs_run_one_after_another_when_the_time_is_split_then_the_gaps_between_them_are_the_waiting() {
        let jobs = [job("a", 0, 100, 0), job("b", 160, 400, 60), job("c", 400, 450, 0)];
        assert_eq!(busy_and_idle(&jobs), Some((450, 390, 60)));
        let text = where_time_went(&jobs, &[("b".to_string(), facts(LOG))]);
        assert!(text.contains("7m 30s wall") && text.contains("1m 00s waiting between them"), "{text}");
        assert!(text.contains("**b** 4m 00s") && text.contains("Waited for the runner: **b** 1m 00s"), "{text}");
        assert!(text.contains("slow::one") && text.contains("crate-a:cargo-flow"), "{text}");
    }

    #[test]
    fn given_cache_figures_when_laid_out_then_the_whole_run_and_each_job_show_their_hit_rates_and_the_slowest_tasks_that_ran() {
        let rows = [CacheFacts { name: "flow".into(), build: Some((9, 1, 1, 63.0)), nx: Some((1, 3)), sccache: Some((4, 6, 2)), layers: Some((3, 33)) }];
        let text = cache_section(&rows, &[("flow".to_string(), facts(LOG))]);
        assert!(text.contains("build cache 90% (9/10)") && text.contains("Nx tasks replayed 33% (1/3)") && text.contains("sccache 40% (4/10)"), "{text}");
        assert!(text.contains("2 write errors") && text.contains("`crate-a:cargo-flow (flow)` 152.0 s"), "{text}");
    }

    #[test]
    fn given_bytes_and_spans_when_shown_then_they_use_units_a_person_reads() {
        assert_eq!((bytes(512.0), bytes(1_500_000.0), bytes(2.5e9)), ("512 B".to_string(), "1.5 MB".to_string(), "2.5 GB".to_string()));
        assert_eq!((span(37), span(245), span(3725)), ("37s".to_string(), "4m 05s".to_string(), "1h 02m".to_string()));
    }
}
