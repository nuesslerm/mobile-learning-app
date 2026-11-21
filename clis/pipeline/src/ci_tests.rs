//! The per-test rows (`CI_TESTS`) of a weighted stage, read from what its Nx run already prints: a libtest `test <name> ... ok` line (stable libtest prints no time, so the
//! time is the one since the previous result of the same project, or since its binary started) or a vitest result line that ends in its own milliseconds. Nothing is run
//! twice and no runner flag changes, so no task hash moves. The rows go to the job-local database like `CI_UNITS`; the report job's merge folds them into the sealed one.
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use wf_bucket::ci_db::{self, TestRow};
use wf_bucket::ci_metrics::Gauges;

/// The longest line kept whole: a line with no newline must not grow the buffer without limit.
const MAX_LINE: usize = 64 * 1024;

/// A test shorter than this (and passing) is not a row: a crate has thousands and the sealed database would carry all of them for nothing.
const ROW_MIN_MS: i64 = 250;
/// A test of this many seconds or more is also a gauge.
const GAUGE_MIN_S: f64 = 10.0;

/// One test result seen in the stream, before the stage names its unit.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seen {
    pub project: String,
    pub task: String,
    pub suite: String,
    pub test: String,
    pub ms: i64,
    pub passed: bool,
    pub reported: bool,
}

#[derive(Default)]
struct Lane {
    suite: String,
    /// When the previous result (or the binary's start) came; none when this lane has not seen one, so the first gap is unknown rather than since the epoch.
    last_ms: Option<i64>,
    /// Inside a failed test's captured output, whose `test x ... ok` lines are the test's own prints.
    in_failure: bool,
}

/// The state of a stream: per project, the binary it is in and when its last result came (lines of parallel projects interleave).
#[derive(Default)]
pub(crate) struct Capture {
    lanes: BTreeMap<String, Lane>,
    tasks: BTreeMap<String, String>,
    pub seen: Vec<Seen>,
}

fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

/// `crate-wf: rest` is project `crate-wf`; a line with no such prefix has an empty project.
fn split_prefix(line: &str) -> (&str, &str) {
    match line.split_once(": ") {
        Some((p, rest)) if !p.is_empty() && !p.contains(char::is_whitespace) && !p.contains("::") => (p, rest),
        _ => ("", line),
    }
}

/// `Running tests/flow.rs (target/debug/deps/flow-abc)` is suite `flow`; `Doc-tests x` is `doc`.
fn suite_of(rest: &str) -> String {
    if rest.starts_with("Doc-tests") {
        return "doc".to_string();
    }
    let file = rest.trim_start_matches("Running").trim().trim_start_matches("unittests").trim().split(" (").next().unwrap_or_default();
    std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or("tests").to_string()
}

/// The time libtest printed after the result (`<0.123s>` from `--report-time`, `(0.123s)`), when it did.
fn reported_ms(tail: &str) -> Option<i64> {
    let t = tail.trim().trim_start_matches(['<', '(']).trim_end_matches(['>', ')']).trim_end_matches('s');
    t.parse::<f64>().ok().map(|s| (s * 1000.0).round() as i64)
}

fn vitest_mark(rest: &str) -> Option<(bool, &str)> {
    let t = rest.trim_start();
    for (m, ok) in [("✓", true), ("✔", true), ("√", true), ("×", false), ("✗", false), ("✘", false)] {
        if let Some(r) = t.strip_prefix(m) {
            return Some((ok, r.trim()));
        }
    }
    None
}

impl Capture {
    /// Reads one complete output line that finished at `now_ms`.
    pub(crate) fn line(&mut self, now_ms: i64, raw: &str) {
        let clean = strip_ansi(raw);
        let trimmed = clean.trim_start_matches(|c: char| !c.is_alphanumeric()).trim_end();
        if let Some(task) = trimmed.strip_prefix("nx run ") {
            let task = task.split_whitespace().next().unwrap_or_default();
            self.tasks.insert(task.split(':').next().unwrap_or_default().to_string(), task.to_string());
            return;
        }
        let (prefixed, rest) = split_prefix(clean.trim_end());
        // a single-task run prints no prefix: its lines belong to the one task seen
        let project = match (prefixed, self.tasks.len()) {
            ("", 1) => self.tasks.keys().next().cloned().unwrap_or_default(),
            _ => prefixed.to_string(),
        };
        let rest = rest.trim();
        let lane = self.lanes.entry(project.clone()).or_default();
        if rest.starts_with("Running ") || rest.starts_with("Doc-tests ") {
            *lane = Lane { suite: suite_of(rest), last_ms: Some(now_ms), in_failure: false };
            return;
        }
        if rest.starts_with("running ") && (rest.ends_with(" tests") || rest.ends_with(" test")) {
            lane.last_ms = Some(now_ms);
            return;
        }
        if rest.starts_with("---- ") && rest.ends_with(" stdout ----") {
            lane.in_failure = true;
            return;
        }
        if rest == "failures:" || rest.starts_with("test result:") {
            lane.in_failure = false;
            return;
        }
        let task = self.tasks.get(&project).cloned().unwrap_or_default();
        if let Some(body) = rest.strip_prefix("test ") {
            if lane.in_failure {
                return;
            }
            let Some((name, result)) = body.rsplit_once(" ... ") else { return };
            let (passed, tail) = match (result.strip_prefix("ok"), result.strip_prefix("FAILED")) {
                (Some(t), _) => (true, t),
                (_, Some(t)) => (false, t),
                _ => {
                    lane.last_ms = Some(now_ms);
                    return;
                }
            };
            let exact = reported_ms(tail);
            let ms = exact.unwrap_or_else(|| lane.last_ms.map_or(0, |last| (now_ms - last).max(0)));
            lane.last_ms = Some(now_ms);
            self.seen.push(Seen { project, task, suite: lane.suite.clone(), test: name.trim().to_string(), ms, passed, reported: exact.is_some() });
            return;
        }
        if let Some((passed, body)) = vitest_mark(rest) {
            let Some((name, tail)) = body.rsplit_once(' ') else { return };
            let Some(ms) = tail.strip_suffix("ms").and_then(|n| n.parse::<f64>().ok()) else { return };
            let (suite, test) = match name.split_once(" > ") {
                Some((file, test)) => (file.trim().to_string(), test.trim().to_string()),
                None => (name.split(" (").next().unwrap_or(name).trim().to_string(), "(file)".to_string()),
            };
            self.seen.push(Seen { project, task, suite, test, ms: ms.round() as i64, passed, reported: true });
        }
    }
}

/// Copies what `out` carries to `sink` as it arrives and feeds every complete line, with the time it ended, to the capture.
pub(crate) fn tee<R: Read, W: Write>(mut out: R, mut sink: W, capture: &Mutex<Capture>) {
    let (mut buf, mut line) = ([0u8; 8192], Vec::new());
    loop {
        let n = match out.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let _ = sink.write_all(&buf[..n]);
        for &b in &buf[..n] {
            if b == b'\n' {
                let now = wf_kit::time::now_ms() as i64;
                capture.lock().unwrap_or_else(std::sync::PoisonError::into_inner).line(now, &String::from_utf8_lossy(&line));
                line.clear();
            } else if line.len() < MAX_LINE {
                line.push(b);
            }
        }
    }
}

/// A test runs alone when its target is one of the `--test-threads=1` suites (`tooling/nx/cargo-crates.js`): then the time since the previous result is its own.
fn serial(task: &str) -> bool {
    let target = crate::nx_ledger::target_of(task);
    ["integration", "e2e", "verify", "replay-"].iter().any(|k| target.contains(k))
}

/// The rows of a stage's stream: the slow tests and every failed one (and a passing one that replaces a failed row of an earlier call), each in the unit its Nx task
/// belongs to. A task Nx replayed from its cache printed old results: it has none. The same test twice keeps the failed one, else the longer.
pub(crate) fn rows(run: &str, stage: &str, target: &str, seen: &[Seen], replayed: &[&str], failed_before: &[(String, String, String)]) -> Vec<TestRow> {
    let job = crate::ci_units::job_of(stage);
    let first = target.split(',').next().unwrap_or_default();
    let mut out: Vec<TestRow> = Vec::new();
    let mut at: std::collections::HashMap<(String, String, String), usize> = std::collections::HashMap::new();
    for s in seen {
        let task = if s.task.is_empty() { format!("{}:{first}", s.project) } else { s.task.clone() };
        if replayed.contains(&task.as_str()) {
            continue;
        }
        let unit = crate::shard_weights::unit_of(stage, &task).unwrap_or_else(|| s.project.clone());
        let key = (unit.clone(), s.suite.clone(), s.test.clone());
        if s.ms < ROW_MIN_MS && s.passed && !failed_before.contains(&key) {
            continue;
        }
        let basis = if s.reported { "reported" } else if serial(&task) { "serial" } else { "gap" };
        let row = TestRow { run: run.to_string(), job: job.clone(), stage: stage.to_string(), unit, suite: s.suite.clone(), test: s.test.clone(), ms: s.ms, outcome: if s.passed { "ok" } else { "failed" }.to_string(), basis: basis.to_string() };
        match at.get(&key) {
            Some(&i) if out[i].outcome == "failed" && row.outcome == "ok" => {}
            Some(&i) if out[i].outcome == "ok" && row.outcome == "failed" || row.ms > out[i].ms => out[i] = row,
            Some(_) => {}
            None => {
                at.insert(key, out.len());
                out.push(row);
            }
        }
    }
    out
}

/// What the Nx run printed, as rows in the job-local database; `replayed` are the tasks Nx took from its cache.
pub(crate) fn persist(stage: &str, target: &str, seen: &[Seen], replayed: &[&str]) -> Vec<TestRow> {
    let Some(run) = std::env::var("GITHUB_RUN_ID").ok().filter(|v| !v.is_empty()).filter(|_| crate::shard_weights::weighted(stage)) else { return Vec::new() };
    let failed_before = ci_db::failed_tests_local(&run, &crate::ci_units::job_of(stage), stage);
    let rows = rows(&run, stage, target, seen, replayed, &failed_before);
    if rows.is_empty() {
        return rows;
    }
    if let Err(why) = ci_db::record_tests_local(&rows) {
        wf_kit::wf_err!("wf: ci tests not recorded: {why}");
    }
    rows
}

/// What the capture has seen so far.
pub(crate) fn snapshot(capture: &Mutex<Capture>) -> Vec<Seen> {
    capture.lock().unwrap_or_else(std::sync::PoisonError::into_inner).seen.clone()
}

/// `ci.test.duration_s{stage,unit::test}` of each test of 10 s or more whose time is its own: a `gap` row of a parallel binary may name a test that only finished late.
pub(crate) fn gauges(into: &mut Gauges, rows: &[TestRow]) {
    for t in rows.iter().filter(|t| t.basis != "gap") {
        let seconds = t.ms as f64 / 1000.0;
        if seconds >= GAUGE_MIN_S {
            into.set("ci.test.duration_s", &t.stage, &format!("{}::{}", t.unit, t.test), seconds);
        }
    }
}

/// Whether a stage's Nx output is read at all: a CI job of a weighted stage.
pub(crate) fn capturing(stage: &str) -> bool {
    std::env::var("GITHUB_RUN_ID").is_ok_and(|v| !v.is_empty()) && crate::shard_weights::weighted(stage)
}

pub(crate) fn shared() -> Arc<Mutex<Capture>> {
    Arc::new(Mutex::new(Capture::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[(i64, &str)]) -> Vec<Seen> {
        let mut c = Capture::default();
        for (t, l) in lines {
            c.line(*t, l);
        }
        c.seen
    }

    #[test]
    fn given_libtest_lines_when_read_then_each_result_is_the_time_since_the_one_before_it() {
        let seen = feed(&[
            (1000, "crate-wf:      Running tests/flow.rs (clis/target/debug/deps/flow-8cb0)"),
            (1100, "crate-wf: running 3 tests"),
            (1600, "crate-wf: test a::fast ... ok"),
            (9600, "crate-wf: test a::slow ... ok"),
            (9700, "crate-wf: test a::bad ... FAILED"),
            (9800, "crate-wf: test a::skipped ... ignored"),
        ]);
        assert_eq!(seen.iter().map(|s| (s.suite.as_str(), s.test.as_str(), s.ms, s.passed)).collect::<Vec<_>>(), [("flow", "a::fast", 500, true), ("flow", "a::slow", 8000, true), ("flow", "a::bad", 100, false)]);
    }

    #[test]
    fn given_two_projects_interleaved_when_read_then_each_keeps_its_own_clock() {
        let seen = feed(&[(0, "p1: running 1 test"), (0, "p2: running 1 test"), (5000, "p2: test b ... ok"), (9000, "p1: test a ... ok")]);
        assert_eq!(seen.iter().map(|s| (s.project.as_str(), s.ms)).collect::<Vec<_>>(), [("p2", 5000), ("p1", 9000)]);
    }

    #[test]
    fn given_a_time_libtest_printed_when_read_then_it_wins_over_the_gap() {
        let seen = feed(&[(0, "running 1 test"), (9000, "test a ... ok <1.250s>")]);
        assert_eq!((seen[0].ms, seen[0].reported), (1250, true));
    }

    #[test]
    fn given_an_nx_header_when_a_test_follows_then_the_row_names_that_task_and_the_serial_targets_are_exact() {
        let seen = feed(&[(0, "> nx run crate-x:cargo-integration"), (0, "crate-x: running 1 test"), (3000, "crate-x: test t ... ok")]);
        let rows = rows("1", "integration-tests", "cargo-integration", &seen, &[], &[]);
        assert_eq!((rows[0].unit.as_str(), rows[0].basis.as_str(), rows[0].ms), ("crate-x", "serial", 3000));
    }

    #[test]
    fn given_vitest_lines_when_read_then_the_printed_milliseconds_are_the_time() {
        let seen = feed(&[(0, "app:  ✓ src/a.test.ts > suite > does it 12ms"), (0, "app:  ✓ src/b.test.ts (4 tests) 845ms"), (0, "app:  Duration  4.5s")]);
        assert_eq!(seen.iter().map(|s| (s.suite.as_str(), s.test.as_str(), s.ms)).collect::<Vec<_>>(), [("src/a.test.ts", "suite > does it", 12), ("src/b.test.ts", "(file)", 845)]);
    }

    #[test]
    fn given_fast_passing_tests_when_rows_are_made_then_only_slow_and_failed_ones_are_kept() {
        let s = |test: &str, ms, passed| Seen { project: "p".into(), task: String::new(), suite: "flow".into(), test: test.into(), ms, passed, reported: false };
        let rows = rows("1", "flow-tests", "cargo-flow", &[s("quick", 10, true), s("slow", 400, true), s("bad", 5, false)], &[], &[]);
        assert_eq!(rows.iter().map(|r| r.test.as_str()).collect::<Vec<_>>(), ["slow", "bad"]);
    }

    #[test]
    fn given_a_test_line_before_any_start_line_when_read_then_its_time_is_unknown_not_since_the_epoch() {
        let seen = feed(&[(1_700_000_000_000, "p: test a ... ok"), (1_700_000_002_000, "p: test b ... ok")]);
        assert_eq!(seen.iter().map(|s| s.ms).collect::<Vec<_>>(), [0, 2000]);
    }

    #[test]
    fn given_a_run_with_no_prefix_when_one_task_header_was_seen_then_its_lines_belong_to_that_task() {
        let seen = feed(&[(0, "> nx run app:cargo-integration  [local cache]"), (0, "running 1 test"), (3000, "test t ... ok")]);
        let rows = rows("1", "integration-tests", "cargo-integration", &seen, &[], &[]);
        assert_eq!((seen[0].project.as_str(), seen[0].task.as_str(), rows[0].unit.as_str(), rows[0].basis.as_str()), ("app", "app:cargo-integration", "app", "serial"));
    }

    #[test]
    fn given_a_failed_tests_captured_output_when_it_prints_result_lines_then_they_are_not_results() {
        let seen = feed(&[
            (0, "p: running 2 tests"),
            (1000, "p: test a ... FAILED"),
            (1000, "p: ---- a stdout ----"),
            (1100, "p: test echoed ... ok"),
            (1100, "p: failures:"),
            (1100, "p: test result: FAILED. 0 passed; 1 failed"),
        ]);
        assert_eq!(seen.iter().map(|s| s.test.as_str()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn given_a_task_nx_replayed_when_rows_are_made_then_its_old_output_is_not_a_row() {
        let seen = feed(&[(0, "> nx run app:test"), (0, "app:  ✓ src/b.test.ts (4 tests) 845ms")]);
        assert!(rows("1", "typecheck-lint-unit-tests", "test", &seen, &["app:test"], &[]).is_empty());
        assert_eq!(rows("1", "typecheck-lint-unit-tests", "test", &seen, &[], &[]).len(), 1);
    }

    #[test]
    fn given_the_same_vitest_title_twice_when_rows_are_made_then_the_longer_one_is_kept() {
        let seen = feed(&[(0, "app:  ✓ a.test.ts > same 300ms"), (0, "app:  ✓ a.test.ts > same 900ms"), (0, "app:  ✓ a.test.ts > same 400ms")]);
        let rows = rows("1", "flow-tests", "test", &seen, &[], &[]);
        assert_eq!(rows.iter().map(|r| r.ms).collect::<Vec<_>>(), [900]);
    }

    #[test]
    fn given_a_failed_row_of_an_earlier_call_when_the_retry_passes_fast_then_it_replaces_the_failed_row() {
        let seen = feed(&[(0, "p:   Running tests/flow.rs (x/flow-1)"), (50, "p: test a ... ok")]);
        let failed = [("p".to_string(), "flow".to_string(), "a".to_string())];
        let rows = rows("1", "flow-tests", "cargo-flow", &seen, &[], &failed);
        assert_eq!(rows.iter().map(|r| r.outcome.as_str()).collect::<Vec<_>>(), ["ok"]);
    }

    #[test]
    fn given_a_gap_row_over_ten_seconds_when_gauges_are_set_then_only_a_trusted_time_is_a_gauge() {
        let row = |basis: &str| TestRow { run: "1".into(), job: "j".into(), stage: "s".into(), unit: "u".into(), suite: "flow".into(), test: basis.into(), ms: 20_000, outcome: "ok".into(), basis: basis.into() };
        let mut g = Gauges::default();
        gauges(&mut g, &[row("gap"), row("serial")]);
        assert_eq!(g.readings().len(), 1);
    }

    struct Interrupting(u8);
    impl Read for Interrupting {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0 += 1;
            match self.0 {
                1 => Err(std::io::ErrorKind::Interrupted.into()),
                2 => {
                    buf[..18].copy_from_slice(b"p: test a ... ok\n\n");
                    Ok(18)
                }
                _ => Ok(0),
            }
        }
    }

    #[test]
    fn given_an_interrupted_read_when_teeing_then_the_rest_is_still_copied_and_read() {
        let capture = Mutex::new(Capture::default());
        let mut sink = Vec::new();
        tee(Interrupting(0), &mut sink, &capture);
        assert_eq!((sink.len(), snapshot(&capture).len()), (18, 1));
    }
}
