//! The trace of a chain run and the run record beside it, as CI's `$WF_TEST_OUTPUT_DIR` collects them: each test is a window on the tap per wire
//! call, its verdict comes from whether it panicked, and a run that did not finish never reads as a pass.
use exec_testkit::{Suite, Tap};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::Path;

/// A tap in front of a service nobody answers for: no call goes through it.
fn idle_tap() -> Tap {
    Tap::to(TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap())
}

fn read(dir: &Path, file: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))).unwrap()
}

fn passing(suite: &Suite, dir: &Path, expected: usize, name: &str, rows: &[&str]) {
    let tap = idle_tap();
    suite.case_in(Some(dir), Some(expected), name, |case| {
        assert_eq!(case.call(&tap, rows, || (true, 7)), 7);
    });
}

#[test]
fn given_a_test_that_passes_and_one_that_panics_when_the_run_is_recorded_then_trace_and_report_name_each_test_its_calls_and_its_verdict() {
    // Given a suite of two tests in a binary that has two, one passing and one that panics after a call the wire refused
    let (suite, out) = (Suite::new("kit-trace", "t3"), tempfile::tempdir().unwrap());
    passing(&suite, out.path(), 2, "chain::writes", &["rpc:projects.mutate", "mutation:project.create"]);
    let tap = idle_tap();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        suite.case_in(Some(out.path()), Some(2), "chain::refuses", |case| {
            case.call(&tap, &["rpc:projects.writeFile"], || (false, ()));
            panic!("the wire refused the write");
        });
    }));
    // When both are in, Then the panic went on after it was recorded
    assert!(failed.is_err());
    // And the trace is the documented shape with one entry per test, each call carrying its rows, whether the wire served it, and what the windows saw
    let trace = read(out.path(), "trace.json");
    assert_eq!((trace["schema"].as_str(), trace["app"].as_str(), trace["suite"].as_str(), trace["complete"].clone()), (Some("method-trace/1"), Some("t3"), Some("kit-trace"), json!(true)));
    let tests = trace["tests"].as_array().unwrap();
    assert_eq!(tests.iter().map(|t| (t["test"].as_str().unwrap(), t["verdict"].as_str().unwrap())).collect::<Vec<_>>(), [("chain::writes", "pass"), ("chain::refuses", "fail")]);
    assert_eq!(tests[0]["calls"], json!([{"rows": ["rpc:projects.mutate", "mutation:project.create"], "ok": true, "agent": [], "executor": []}]));
    assert_eq!(tests[1]["calls"][0]["ok"], json!(false));
    // And the run record is a failed integration run of the suite that names the failing test and its message
    let report = read(out.path(), "report.json");
    assert_eq!((report["schema"].clone(), report["platform"].as_str(), report["script"].as_str(), report["verdict"].as_str(), report["ok"].clone()), (json!(1), Some("integration"), Some("kit-trace"), Some("fail"), json!(false)));
    assert!(report["message"].as_str().unwrap().contains("chain::refuses: the wire refused the write"), "{report}");
    assert_eq!(report["cases"].as_array().unwrap().len(), 2);
}

#[test]
fn given_every_test_of_the_binary_passed_when_the_run_is_recorded_then_it_reads_pass_and_a_run_that_stopped_early_reads_error() {
    // Given a binary of two tests of which only the first has run
    let (suite, out) = (Suite::new("kit-trace", "t3"), tempfile::tempdir().unwrap());
    passing(&suite, out.path(), 2, "chain::first", &["http:auth.session"]);
    // When the record is read, Then it says error and names how far the run got, and the trace says it is not complete
    let early = read(out.path(), "report.json");
    assert_eq!((early["verdict"].as_str(), early["ok"].clone(), early["exitCode"].clone()), (Some("error"), json!(false), json!(1)));
    assert!(early["message"].as_str().unwrap().contains("stopped after 1 of 2"), "{early}");
    assert_eq!(read(out.path(), "trace.json")["complete"], json!(false));
    // And when the second test passes too, Then the record says pass with the exit code 0
    passing(&suite, out.path(), 2, "chain::second", &["http:auth.token"]);
    let done = read(out.path(), "report.json");
    assert_eq!((done["verdict"].as_str(), done["ok"].clone(), done["exitCode"].clone(), done["expectedCases"].clone()), (Some("pass"), json!(true), json!(0), json!(2)));
    assert_eq!(read(out.path(), "trace.json")["complete"], json!(true));
}

#[test]
fn given_a_binary_that_cannot_list_its_tests_when_the_run_is_recorded_then_the_record_says_so_in_a_warning() {
    let (suite, out) = (Suite::new("kit-trace", "t3"), tempfile::tempdir().unwrap());
    let tap = idle_tap();
    suite.case_in(Some(out.path()), None, "chain::only", |case| {
        case.call(&tap, &["http:auth.session"], || (true, ()));
    });
    let report = read(out.path(), "report.json");
    assert_eq!(report["verdict"].as_str(), Some("error"));
    assert!(report["warnings"][0].as_str().unwrap().contains("cannot be told complete"), "{report}");
}
