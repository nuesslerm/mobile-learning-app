//! `wf ci db` through the built binary: two jobs' local databases are merged into a sealed file, a row both hold is kept once, the
//! sealed file opens only with its key, and the queries read what was merged. No state ref is touched (`--sealed-file`).
use crate::support::{last_json, run_wf, RunOpts};
use serde_json::json;

const KEY: &str = "0707070707070707070707070707070707070707070707070707070707070707";

fn opts(db: &std::path::Path) -> RunOpts {
    RunOpts::default().env("CI_DB_KEY", KEY).env("WF_CI_DB", &db.display().to_string())
}

#[test]
fn given_two_jobs_rows_when_merged_into_the_sealed_database_then_each_row_is_kept_once_and_the_queries_read_them() {
    // Given two jobs that each recorded rows into their own local database, one row in both
    let dir = tempfile::tempdir().unwrap();
    let (a, b, sealed) = (dir.path().join("a.sqlite"), dir.path().join("b.sqlite"), dir.path().join("ci.sealed"));
    for (db, table, row) in [(&a, "TEST_RUNS", r#"{"suite":"flow","ok":true}"#), (&a, "TEST_RUNS", r#"{"suite":"flow","ok":true}"#), (&b, "TEST_RUNS", r#"{"suite":"flow","ok":true}"#), (&b, "WF_USAGE", r#"{"command":"test run","exit_code":0}"#)] {
        let r = run_wf(&["ci", "db", "record", table, row], &opts(db));
        assert_eq!((r.exit_code, last_json(&r)["recorded"].clone()), (0, json!(true)), "{}{}", r.stdout, r.stderr);
    }
    // When both are merged into a sealed file
    let sealed_arg = sealed.display().to_string();
    let merged = run_wf(&["ci", "db", "merge", "--sealed-file", &sealed_arg, &a.display().to_string(), &b.display().to_string()], &opts(&a));
    // Then two distinct rows went in, and the file is sealed (no SQLite header readable)
    assert_eq!((merged.exit_code, last_json(&merged)["merged"].clone()), (0, json!(2)), "{}{}", merged.stdout, merged.stderr);
    assert!(std::fs::read(&sealed).unwrap().starts_with(b"WFCIDB1\n"));
    // And a query reads each table's row once, by its JSON fields
    let q = run_wf(&["ci", "db", "query", "--sealed-file", &sealed_arg, "SELECT (SELECT COUNT(*) FROM TEST_RUNS) AS runs, (SELECT json_extract(ROW, '$.command') FROM WF_USAGE) AS cmd"], &opts(&a));
    assert_eq!(last_json(&q)["rows"], json!([{"runs": 1, "cmd": "test run"}]), "{}{}", q.stdout, q.stderr);
    // And merging the same file again adds nothing
    let again = run_wf(&["ci", "db", "merge", "--sealed-file", &sealed_arg, &b.display().to_string()], &opts(&a));
    assert_eq!(last_json(&again)["merged"], json!(0));
    // And another key cannot open it, and an unknown table is refused before anything is written
    let wrong = run_wf(&["ci", "db", "query", "--sealed-file", &sealed_arg, "SELECT 1"], &opts(&a).env("CI_DB_KEY", &"08".repeat(32)));
    assert_eq!((wrong.exit_code, last_json(&wrong)["ok"].clone()), (1, json!(false)));
    assert!(last_json(&wrong)["error"].as_str().unwrap().contains("does not open"), "{}", wrong.stdout);
    let refused = run_wf(&["ci", "db", "record", "USERS", "{}"], &opts(&a));
    assert!(last_json(&refused)["error"].as_str().unwrap().contains("is not one of"), "{}", refused.stdout);
}

#[test]
fn given_no_local_database_when_a_row_is_recorded_then_nothing_is_written_and_the_command_still_succeeds() {
    // Given a run outside a hosted job (the kit sets WF_CI_DB=off)
    // When a row is recorded
    let r = run_wf(&["ci", "db", "record", "TEST_RUNS", r#"{"suite":"flow"}"#], &RunOpts::default());
    // Then nothing was kept and the command did not fail
    assert_eq!((r.exit_code, last_json(&r)["recorded"].clone()), (0, json!(false)), "{}{}", r.stdout, r.stderr);
}
