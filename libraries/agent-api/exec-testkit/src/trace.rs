//! The method-hit trace of an integration chain run, and the run record that `wf test publish` takes beside it.
//!
//! A mapper's chain test drives the phone's wire one call at a time. Each call is a window on the `Tap`: the agent API methods and the executor
//! tools that went through it while the call ran, each with whether it was answered ok. A test is the calls it made and its verdict.
//! `trace.json` (schema `method-trace/1`) is written after every test beside `report.json` (a harness run record, schema 1, platform
//! `integration`) in `$WF_TEST_OUTPUT_DIR/<suite>/`, and `wf parity coverage --traces` reads it:
//!
//! ```json
//! {"schema": "method-trace/1", "app": "t3", "suite": "t3-serve-integration", "sha": "<commit>", "complete": true,
//!  "tests": [{"test": "chain::a_message_...", "verdict": "pass", "ms": 812, "calls": [
//!    {"rows": ["rpc:orchestration.dispatchCommand", "command:message.dispatch"], "ok": true,
//!     "agent": [{"method": "thread.create", "ok": true}, {"method": "turn.start", "ok": true}],
//!     "executor": [{"tool": "write_file", "ok": true}]}]}]}
//! ```
//!
//! `rows` are the coverage rows the call exercised (`rpc:<tag>`, `command:<type>`, `mutation:<type>`, `http:<name>`), `ok` whether the wire
//! answered it as served (a refusal is `ok: false`), `agent` and `executor` what the windows saw (a `code` is added to a hit that failed).
use crate::tap::{Seen, Tap};
use serde_json::{json, Value};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

pub const SCHEMA: &str = "method-trace/1";

/// The calls of one test.
#[derive(Default)]
pub struct Case {
    calls: Vec<Value>,
}

impl Case {
    /// The calls recorded so far, each `{rows, ok, agent, executor}`: what a test asserts on to prove where a call went.
    pub fn calls(&self) -> &[Value] {
        &self.calls
    }

    /// Runs one wire call as a window on `tap`: `run` makes the call (and waits for what it starts to settle) and says whether the wire
    /// served it, and the calls that went through the tap meanwhile are recorded with the `rows` the call exercises.
    pub fn call<R>(&mut self, tap: &Tap, rows: &[&str], run: impl FnOnce() -> (bool, R)) -> R {
        let mark = tap.mark();
        let (ok, result) = run();
        let (mut agent, mut executor) = (Vec::new(), Vec::new());
        for seen in tap.since(mark) {
            match seen {
                Seen::Agent { method, ok, code } => agent.push(hit("method", &method, ok, code)),
                Seen::Executor { tool, ok, code } => executor.push(hit("tool", &tool, ok, code)),
            }
        }
        self.calls.push(json!({"rows": rows, "ok": ok, "agent": agent, "executor": executor}));
        result
    }
}

fn hit(key: &str, name: &str, ok: bool, code: Option<String>) -> Value {
    let mut hit = json!({ key: name, "ok": ok });
    if let Some(code) = code {
        hit["code"] = json!(code);
    }
    hit
}

struct Done {
    test: String,
    verdict: &'static str,
    ms: i64,
    message: Option<String>,
    calls: Vec<Value>,
}

struct Run {
    started_ms: i64,
    tests: Vec<Done>,
}

/// One suite's run of a test binary: the tests that finished, written out after each.
pub struct Suite {
    name: &'static str,
    app: &'static str,
    run: Mutex<Option<Run>>,
}

impl Suite {
    pub const fn new(name: &'static str, app: &'static str) -> Suite {
        Suite { name, app, run: Mutex::new(None) }
    }

    /// Runs one test body and records it, its calls and its verdict, in `$WF_TEST_OUTPUT_DIR/<suite>` when that is set; a panic is
    /// recorded as a failure and then goes on.
    pub fn case(&self, test: &str, body: impl FnOnce(&mut Case)) {
        let out = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join(self.name));
        self.case_in(out.as_deref(), expected_tests(), test, body);
    }

    /// `case` with where the record goes and how many tests the binary has: the seam the kit's own flow tests use.
    pub fn case_in(&self, dir: Option<&Path>, expected: Option<usize>, test: &str, body: impl FnOnce(&mut Case)) {
        let started = now_ms();
        let mut case = Case::default();
        let outcome = catch_unwind(AssertUnwindSafe(|| body(&mut case)));
        let done = Done {
            test: test.to_string(),
            verdict: if outcome.is_ok() { "pass" } else { "fail" },
            ms: now_ms() - started,
            message: outcome.as_ref().err().map(|p| panic_text(p.as_ref())),
            calls: case.calls,
        };
        {
            let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
            let run = run.get_or_insert_with(|| Run { started_ms: started, tests: Vec::new() });
            run.started_ms = run.started_ms.min(started);
            run.tests.push(done);
            if let Some(dir) = dir {
                if let Err(e) = self.write(dir, expected, run) {
                    eprintln!("{}: cannot write the run record to {}: {e}", self.name, dir.display());
                }
            }
        }
        if let Err(p) = outcome {
            resume_unwind(p);
        }
    }

    /// `report.json` and `trace.json` for what has run so far. Until every test of the binary is in, the record says `error`, so a run
    /// that was killed or stopped partway is never published as a pass.
    fn write(&self, dir: &Path, expected: Option<usize>, run: &Run) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let finished = now_ms();
        let complete = expected.is_some_and(|n| run.tests.len() >= n);
        let failed = run.tests.iter().find(|t| t.verdict == "fail");
        let (verdict, message) = match (failed, complete) {
            (Some(t), _) => ("fail", Some(format!("{}: {}", t.test, t.message.as_deref().unwrap_or("")))),
            (None, false) => ("error", Some(format!("the run stopped after {} of {} tests", run.tests.len(), expected.map_or("an unknown number of".to_string(), |n| n.to_string())))),
            (None, true) => ("pass", None),
        };
        let sha = git_head();
        let tests: Vec<Value> = run.tests.iter().map(|t| json!({"test": t.test, "verdict": t.verdict, "ms": t.ms, "calls": t.calls})).collect();
        let trace = json!({"schema": SCHEMA, "app": self.app, "suite": self.name, "sha": sha, "complete": complete, "tests": tests});
        let cases: Vec<Value> = run.tests.iter().map(|t| json!({"name": t.test, "verdict": t.verdict, "ms": t.ms, "message": t.message})).collect();
        let report = json!({
            "schema": 1,
            "verdict": verdict,
            "ok": verdict == "pass",
            "exitCode": i32::from(verdict != "pass"),
            "message": message,
            "expectedCases": expected,
            "script": self.name,
            "platform": "integration",
            "sha": sha,
            "startedMs": run.started_ms,
            "finishedMs": finished,
            "started": iso(run.started_ms),
            "finished": iso(finished),
            "durationS": ((finished - run.started_ms).max(0) as f64 / 100.0).round() / 10.0,
            "recordDir": dir.display().to_string(),
            "cases": cases,
            "warnings": if expected.is_none() { json!(["the binary could not list its tests, so the run cannot be told complete"]) } else { json!([]) },
        });
        write_atomically(&dir.join("trace.json"), &trace)?;
        write_atomically(&dir.join("report.json"), &report)
    }
}

fn write_atomically(path: &Path, value: &Value) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value)? + "\n")?;
    std::fs::rename(tmp, path)
}

/// `chain::a_test` from `integration::chain::a_test::__here`, the type name of a function declared inside the test: the name libtest lists.
pub fn test_name(type_name: &str) -> String {
    let path = type_name.strip_suffix("::__here").unwrap_or(type_name);
    path.split_once("::").map_or(path, |(_, rest)| rest).to_string()
}

/// Runs the body as the test it is written in: `case!(SUITE, |case| { ... })`.
#[macro_export]
macro_rules! case {
    ($suite:expr, $body:expr) => {{
        fn __here() {}
        $suite.case(&$crate::trace::test_name(::std::any::type_name_of_val(&__here)), $body)
    }};
}

/// How many tests this run runs: the binary lists itself with the same arguments (libtest's `--list`).
fn expected_tests() -> Option<usize> {
    static EXPECTED: OnceLock<Option<usize>> = OnceLock::new();
    *EXPECTED.get_or_init(|| {
        let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--list").collect();
        let out = Command::new(std::env::current_exe().ok()?).args(args).arg("--list").env_clear().output().ok()?;
        let n = String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.ends_with(": test")).count();
        (out.status.success() && n > 0).then_some(n)
    })
}

fn git_head() -> Option<String> {
    static HEAD: OnceLock<Option<String>> = OnceLock::new();
    HEAD.get_or_init(|| {
        let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(env!("CARGO_MANIFEST_DIR")).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    })
    .clone()
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "the test panicked".into())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `2026-11-04T07:00:00.000Z` for a Unix time in milliseconds.
fn iso(ms: i64) -> String {
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rest / 3_600_000, rest / 60_000 % 60, rest / 1_000 % 60, rest % 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_type_name_of_a_function_in_a_test_when_named_then_it_is_the_module_path_libtest_lists() {
        assert_eq!(test_name("integration::chain::a_message_runs::__here"), "chain::a_message_runs");
        assert_eq!(test_name("integration::a_test::__here"), "a_test");
    }

    #[test]
    fn given_unix_times_when_formatted_then_they_are_utc_iso_8601() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_709_251_199_999), "2024-02-29T23:59:59.999Z");
        assert_eq!(iso(1_793_775_141_000), "2026-11-04T06:52:21.000Z");
    }
}
