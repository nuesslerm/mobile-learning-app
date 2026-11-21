//! The shard runner's per-unit rows (`CI_UNITS`) through the built binary: a stage run in a job with a local database leaves a span per unit, the report job's
//! merge folds it into the sealed database, and `wf ci progress` on that runner names the unit a job is in. Nx, pnpm and the Actions API are stubs.
use crate::pipeline::Workspace;
use crate::support::otlp::gauges;
use crate::support::{last_json, MockReply, MockServer};
use serde_json::{json, Value};

const KEY: &str = "0707070707070707070707070707070707070707070707070707070707070707";

fn rows(db: &std::path::Path, sql: &str) -> Vec<Value> {
    let conn = rusqlite::Connection::open(db).unwrap();
    wf_bucket::ci_db::query(&conn, sql).unwrap()
}

#[test]
fn a_stage_run_in_a_job_leaves_one_span_per_unit_the_merge_carries_into_the_sealed_database_and_a_gauge_per_slow_unit() {
    // Given a planned typecheck stage whose Nx run leaves one task of the unit `app` (2 s), a job-local database and a stand-in metrics source
    let w = Workspace::new("wf-flow-units-");
    w.plan("HEAD~1");
    let dir = tempfile::tempdir().unwrap();
    let (local, sealed) = (dir.path().join("rows.sqlite"), dir.path().join("ci.sealed"));
    let source = MockServer::start(None);
    let env = |extra: &[(&'static str, String)]| {
        let mut all = vec![("WF_CI_DB", local.display().to_string()), ("CI_DB_KEY", KEY.to_string()), ("GITHUB_RUN_ID", "777".to_string()), ("GITHUB_SHA", "abc1234".to_string()), ("GITHUB_JOB", "typecheck-lint-unit-tests".to_string()), ("WF_CI_METRICS_BASE", format!("http://127.0.0.1:{}", source.port))];
        all.extend(extra.iter().cloned());
        all
    };

    // When the stage runs
    let r = w.wf(&["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", &w.path("plan.json")], &env(&[]));
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);

    // Then the unit has one row: ran, its 2 s, its interval, and no unit is left running (`lib` was started and ran no task)
    let units = rows(&local, "SELECT STAGE, UNIT, OUTCOME, MS, STARTED_MS, ENDED_MS FROM CI_UNITS ORDER BY UNIT");
    assert_eq!(units.iter().filter(|u| u["OUTCOME"] == "running").count(), 0, "{units:?}");
    let app = units.iter().find(|u| u["UNIT"] == "app").unwrap_or_else(|| panic!("no span for app: {units:?}"));
    assert_eq!((app["STAGE"].clone(), app["OUTCOME"].clone(), app["MS"].clone()), (json!("typecheck-lint-unit-tests"), json!("ran"), json!(2000)));
    assert_eq!(app["ENDED_MS"].as_i64().unwrap() - app["STARTED_MS"].as_i64().unwrap(), 2000);

    // And the source received the unit's seconds, and the same reading is a row of CI_METRICS
    let seen = gauges(&source.requests());
    assert!(seen.iter().any(|g| g.metric == "ci.unit.duration_s" && g.scope == "typecheck-lint-unit-tests" && g.name == "app" && g.value == 2.0), "{seen:?}");
    assert_eq!(rows(&local, "SELECT COUNT(*) AS n FROM CI_METRICS WHERE METRIC = 'ci.unit.duration_s'")[0]["n"], json!(1));

    // And the report job's merge folds the rows into the sealed database, where a query reads them
    let merged = w.wf(&["ci", "db", "merge", "--sealed-file", &sealed.display().to_string(), &local.display().to_string()], &env(&[]));
    assert_eq!(merged.exit_code, 0, "{}{}", merged.stdout, merged.stderr);
    let q = w.wf(&["ci", "db", "query", "--sealed-file", &sealed.display().to_string(), "SELECT UNIT, OUTCOME, MS FROM CI_UNITS WHERE UNIT = 'app'"], &env(&[]));
    assert_eq!(last_json(&q)["rows"], json!([{"UNIT": "app", "OUTCOME": "ran", "MS": 2000}]), "{}{}", q.stdout, q.stderr);
}

#[test]
fn a_stage_run_outside_a_job_with_a_database_writes_no_unit_rows_and_does_not_fail() {
    // Given the same planned stage and no local database (the kit sets WF_CI_DB=off)
    let w = Workspace::new("wf-flow-units-off-");
    w.plan("HEAD~1");

    // When the stage runs
    let r = w.wf(&["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", &w.path("plan.json")], &[("GITHUB_RUN_ID", "777".to_string()), ("GITHUB_SHA", "abc1234".to_string())]);

    // Then it fails only for the lint the stub fails, and says nothing about unit rows
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(!r.stderr.contains("ci units not recorded"), "{}", r.stderr);
}

#[test]
fn a_run_that_is_going_names_the_unit_its_job_is_in_when_asked_on_that_runner() {
    // Given a run with one running flow-tests job, and a job-local database that holds a unit started 90 s before and one that ended
    let run = json!({"id": 77, "status": "in_progress", "conclusion": null, "created_at": "2026-11-19T10:00:00Z", "head_sha": "abc", "workflow_id": 5, "head_branch": "dev"});
    let running = json!({"id": 1, "name": "run / flow-tests (3/6)", "status": "in_progress", "conclusion": null, "created_at": "2026-11-19T10:00:00Z", "started_at": "2026-11-19T10:00:02Z", "completed_at": null,
        "steps": [{"name": "Flow tests", "status": "in_progress", "conclusion": null, "started_at": "2026-11-19T10:03:03Z", "completed_at": null}]});
    let server = MockServer::start(None);
    server.routes(&[
        ("GET /repos/o/r/actions/runs/77", MockReply::json(200, run)),
        ("GET /repos/o/r/actions/runs/77/jobs?per_page=100&page=1", MockReply::json(200, json!({"total_count": 1, "jobs": [running]}))),
        ("GET /repos/o/r/actions/workflows/5/runs?branch=dev&status=success&per_page=1", MockReply::json(200, json!({"workflow_runs": []}))),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("rows.sqlite");
    let conn = rusqlite::Connection::open(&db).unwrap();
    wf_bucket::ci_db::ensure_ledger(&conn).unwrap();
    let at = 1_795_082_400_000i64;
    let started = at + 8 * 60_000 - 90_000;
    conn.execute_batch(&format!(
        "INSERT INTO CI_UNITS VALUES ('77','flow-tests','flow-tests','crate-pipeline',{started},NULL,NULL,'running');
         INSERT INTO CI_UNITS VALUES ('77','flow-tests','flow-tests','crate-wf',{at},{at},1000,'ran');"
    ))
    .unwrap();
    drop(conn);

    // When the progress is read at 10:08:00 on that runner
    let wf = |extra: &[(&str, &str)]| {
        let mut env = vec![("GITHUB_TOKEN", "flowtesttoken-0123456789"), ("GITHUB_REPOSITORY", "o/r"), ("WF_CI_NOW", "2026-11-19T10:08:00Z"), ("NO_PROXY", "127.0.0.1"), ("no_proxy", "127.0.0.1")];
        env.extend(extra.iter().cloned());
        let api = format!("http://127.0.0.1:{}", server.port);
        env.push(("GITHUB_API_URL", &api));
        let mut opts = crate::support::RunOpts::default();
        for (k, v) in env {
            opts = opts.env(k, v);
        }
        crate::support::run_wf(&["ci", "progress", "--run", "77"], &opts)
    };
    let r = wf(&[("WF_CI_DB", &db.display().to_string())]);

    // Then the view lists the running unit with how long it has run, and the job row carries it; the finished unit is not listed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let p = last_json(&r)["progress"].clone();
    assert_eq!(p["running_units"].as_array().unwrap().len(), 1, "{p}");
    assert_eq!((p["running_units"][0]["unit"].clone(), p["running_units"][0]["running_s"].clone()), (json!("crate-pipeline"), json!(90)), "{p}");
    assert_eq!(p["open"][0]["unit"], json!("crate-pipeline"), "{p}");

    // And on a machine with no job database the view has no running units
    let elsewhere = wf(&[("WF_CI_DB", "off")]);
    assert_eq!(last_json(&elsewhere)["progress"]["running_units"], json!([]), "{}", elsewhere.stdout);
}
