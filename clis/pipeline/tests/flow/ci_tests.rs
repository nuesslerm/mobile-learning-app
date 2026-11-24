//! The per-test rows (`CI_TESTS`) through the built binary: a stage whose Nx run prints libtest results leaves a row per slow or failed test, with the time between
//! results as its duration (or the time libtest reported), a gauge per test of 10 s or more, and the report job's merge carries them into the sealed database. The
//! stub below is the test binary: Nx itself prints what the tests print, prefixed with the project.
use crate::pipeline::Workspace;
use crate::support::otlp::gauges;
use crate::support::{last_json, MockServer};
use serde_json::{json, Value};

const KEY: &str = "0707070707070707070707070707070707070707070707070707070707070707";

/// `nx run-many -t typecheck -p app`: a libtest binary of four tests, one of them 1 s after the one before, one with a reported time, one failing.
const NX_WITH_TESTS: &str = r#"[ "$3" = typecheck ] || exit 0
echo "> nx run app:typecheck"
echo "app:      Running tests/flow.rs (target/debug/deps/flow-0a1b)"
echo "app: running 4 tests"
echo "app: test suite::quick ... ok"
sleep 1
echo "app: test suite::waited ... ok"
echo "app: test suite::reported ... ok <12.500s>"
echo "app: test suite::broken ... FAILED"
mkdir -p "$HOME/.nx/ws/cache"
printf '{"tasks":[{"taskId":"app:typecheck","hash":"42","startTime":"2026-10-30T10:00:00.000Z","endTime":"2026-10-30T10:00:15.000Z","cacheStatus":"cache-miss","status":0}]}' > "$HOME/.nx/ws/cache/run.json"
exit 0"#;

fn rows(db: &std::path::Path, sql: &str) -> Vec<Value> {
    let conn = rusqlite::Connection::open(db).unwrap();
    wf_bucket::ci_db::query(&conn, sql).unwrap()
}

#[test]
fn a_stage_whose_tests_print_results_leaves_a_row_per_slow_or_failed_test_a_gauge_per_test_over_ten_seconds_and_the_merge_carries_them() {
    // Given a planned stage whose Nx run prints four libtest results (one after a 1 s wait), a job-local database and a stand-in metrics source
    let w = Workspace::new("wf-flow-tests-");
    w.s.stub("nx", NX_WITH_TESTS);
    w.plan("HEAD~1");
    let dir = tempfile::tempdir().unwrap();
    let (local, sealed) = (dir.path().join("rows.sqlite"), dir.path().join("ci.sealed"));
    let source = MockServer::start(None);
    // its own HOME: the flow binary's shared one holds the run.json other tests' stubs write
    let home = dir.path().join("home").display().to_string();
    let env = || {
        vec![("HOME", home.clone()), ("WF_CI_DB", local.display().to_string()), ("CI_DB_KEY", KEY.to_string()), ("GITHUB_RUN_ID", "888".to_string()), ("GITHUB_SHA", "abc1234".to_string()), ("GITHUB_JOB", "typecheck-lint-unit-tests".to_string()), ("WF_CI_METRICS_BASE", format!("http://127.0.0.1:{}", source.port))]
    };

    // When the stage runs
    let r = w.wf(&["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", &w.path("plan.json")], &env());

    // Then the run's output still reaches stderr as it was printed
    assert!(r.stderr.contains("app: test suite::quick ... ok"), "{}{}", r.stdout, r.stderr);

    // And the failed test, the one that waited 1 s and the one libtest timed are rows of the unit, and the quick one is not
    let tests = rows(&local, "SELECT STAGE, UNIT, SUITE, TEST, MS, OUTCOME, BASIS FROM CI_TESTS ORDER BY TEST");
    assert_eq!(tests.iter().map(|t| t["TEST"].as_str().unwrap()).collect::<Vec<_>>(), ["suite::broken", "suite::reported", "suite::waited"], "{tests:?}");
    assert!(tests.iter().all(|t| t["STAGE"] == "typecheck-lint-unit-tests" && t["UNIT"] == "app" && t["SUITE"] == "flow"), "{tests:?}");
    assert_eq!((tests[0]["OUTCOME"].clone(), tests[0]["BASIS"].clone()), (json!("failed"), json!("gap")));
    assert_eq!((tests[1]["MS"].clone(), tests[1]["BASIS"].clone()), (json!(12500), json!("reported")));
    let waited = tests[2]["MS"].as_i64().unwrap();
    assert!(waited >= 950, "waited {waited} ms: {tests:?}");

    // And only the test of 10 s or more is a gauge, and the same reading is a row of CI_METRICS
    let seen = gauges(&source.requests());
    let named: Vec<_> = seen.iter().filter(|g| g.metric == "ci.test.duration_s").collect();
    assert_eq!(named.len(), 1, "{seen:?}");
    assert_eq!((named[0].scope.as_str(), named[0].name.as_str(), named[0].value), ("typecheck-lint-unit-tests", "app::suite::reported", 12.5));
    assert_eq!(rows(&local, "SELECT COUNT(*) AS n FROM CI_METRICS WHERE METRIC = 'ci.test.duration_s'")[0]["n"], json!(1));

    // And the report job's merge folds the rows into the sealed database, where a query names the slowest test first
    let merged = w.wf(&["ci", "db", "merge", "--sealed-file", &sealed.display().to_string(), &local.display().to_string()], &env());
    assert_eq!(merged.exit_code, 0, "{}{}", merged.stdout, merged.stderr);
    let q = w.wf(&["ci", "db", "query", "--sealed-file", &sealed.display().to_string(), "SELECT UNIT, TEST, MS FROM CI_TESTS ORDER BY MS DESC LIMIT 1"], &env());
    assert_eq!(last_json(&q)["rows"], json!([{"UNIT": "app", "TEST": "suite::reported", "MS": 12500}]), "{}{}", q.stdout, q.stderr);
}

#[test]
fn a_stage_run_outside_a_job_with_a_database_writes_no_test_rows_and_still_prints_the_tests() {
    // Given the same planned stage and no local database (the kit sets WF_CI_DB=off)
    let w = Workspace::new("wf-flow-tests-off-");
    w.s.stub("nx", NX_WITH_TESTS);
    w.plan("HEAD~1");
    let home = tempfile::tempdir().unwrap();

    // When the stage runs
    let r = w.wf(&["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", &w.path("plan.json")], &[("HOME", home.path().display().to_string()), ("GITHUB_RUN_ID", "888".to_string()), ("GITHUB_SHA", "abc1234".to_string())]);

    // Then the tests are on stderr and nothing is said about rows
    assert!(r.stderr.contains("app: test suite::waited ... ok"), "{}{}", r.stdout, r.stderr);
    assert!(!r.stderr.contains("ci tests not recorded"), "{}", r.stderr);
}
