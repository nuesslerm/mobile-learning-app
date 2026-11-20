//! The parallelism telemetry through the built binary: `wf ci resources` (runner samples and Nx task intervals), the run profile and the steps `wf ci timings`
//! keeps, and `wf ci concurrency`, against a loopback fake of the Actions API and job-local databases merged into a sealed file.
use crate::ci_timings::{job, line, served, step, wf};
use crate::support::otlp::gauges;
use crate::support::{run_wf, MockReply, MockServer, RunOpts};
use serde_json::{json, Value};
use std::path::Path;

const KEY: &str = "0707070707070707070707070707070707070707070707070707070707070707";

fn sample(t: i64, cpu: u32, mem: u32, psi_cpu: f64) -> String {
    let once = if t == 1005 { r#","cores":4,"runner":"GitHub Actions 7""# } else { "" };
    format!(r#"{{"t":{t},"cpu":{cpu},"memUsedMb":{mem},"memTotalMb":16000,"load1":1.5,"iowaitPct":2,"swapUsedMb":0,"procsRunning":3,"readBytes":1048576,"writeBytes":2097152,"psiCpu":{psi_cpu},"psiMem":0.5,"psiIo":1.0{once}}}"#)
}

fn write_stats(dir: &Path, job: &str, rows: &[String]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(format!("stats-{job}.jsonl")), rows.join("\n")).unwrap();
}

/// The sealed file the job-local database at `local` merges into, then one query on it.
fn query(local: &Path, sql: &str) -> Value {
    let dir = local.parent().unwrap();
    let sealed = dir.join("ci.sealed").display().to_string();
    let opts = RunOpts::default().env("CI_DB_KEY", KEY);
    let merged = run_wf(&["ci", "db", "merge", "--sealed-file", &sealed, &local.display().to_string()], &opts);
    assert_eq!(merged.exit_code, 0, "{}{}", merged.stdout, merged.stderr);
    let q = run_wf(&["ci", "db", "query", "--sealed-file", &sealed, sql], &opts);
    line(&q)["rows"].clone()
}

fn seed_tasks(path: &Path, tasks: &[(&str, &str, i64, i64, f64)]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE NX_TASKS (RUN_ID TEXT NOT NULL, SHA TEXT NOT NULL, TS INTEGER NOT NULL, JOB TEXT NOT NULL, TASK TEXT NOT NULL, HASH TEXT NOT NULL, OUTCOME TEXT NOT NULL, MS INTEGER NOT NULL,
           PASSED INTEGER NOT NULL, STARTED_MS INTEGER, ENDED_MS INTEGER, CPU_S REAL, PRIMARY KEY (RUN_ID, TASK, HASH));",
    )
    .unwrap();
    for (task, outcome, from, to, cpu) in tasks {
        conn.execute("INSERT INTO NX_TASKS VALUES ('9','abc',1,'core-tests',?1,?1,?2,?3,1,?4,?5,?6)", rusqlite::params![task, outcome, to - from, from, to, cpu]).unwrap();
    }
}

#[test]
fn given_a_jobs_runner_samples_when_its_resources_are_read_then_the_peaks_the_p95_and_the_seconds_idle_and_saturated_are_one_row_of_ci_job_resources() {
    // Given six samples 5 s apart: two idle, three over 90% and one under it with CPU pressure over 20
    let dir = tempfile::tempdir().unwrap();
    let stats = dir.path().join("runner-stats");
    let rows: Vec<String> = [(1005, 10, 1000, 0.0), (1010, 95, 4000, 5.0), (1015, 99, 6000, 8.0), (1020, 50, 3000, 30.0), (1025, 20, 2000, 0.0), (1030, 100, 5000, 12.0)].iter().map(|(t, c, m, p)| sample(*t, *c, *m, *p)).collect();
    write_stats(&stats, "core-tests", &rows);
    let local = dir.path().join("rows.sqlite");

    // When the resources are read and the report job merges its database
    let r = run_wf(&["ci", "resources", "--stats", &stats.display().to_string(), "--run", "9"], &RunOpts::default().env("WF_CI_DB", &local.display().to_string()).env("GITHUB_WORKFLOW", "CI core"));

    // Then the job's row has the runner, the cores, the peaks, the nearest-rank p95, the pressure peaks, the disk traffic and the seconds idle and saturated
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!((line(&r)["jobs"][0]["job"].clone(), line(&r)["tasks"].clone()), (json!("core-tests"), json!(0)));
    let sql = "SELECT JOB, WORKFLOW, RUNNER, CORES, SAMPLES, ROUND(CPU_AVG, 1) AS cpu_avg, CPU_P95, CPU_MAX, MEM_PEAK_MB, MEM_TOTAL_MB, PSI_CPU_MAX, PSI_IO_MAX, READ_MB, WRITE_MB, DURATION_S, IDLE_S, SATURATED_S FROM CI_JOB_RESOURCES";
    assert_eq!(
        query(&local, sql),
        json!([{"JOB": "core-tests", "WORKFLOW": "CI core", "RUNNER": "GitHub Actions 7", "CORES": 4, "SAMPLES": 6, "cpu_avg": 62.3, "CPU_P95": 100.0, "CPU_MAX": 100.0, "MEM_PEAK_MB": 6000.0, "MEM_TOTAL_MB": 16000.0,
            "PSI_CPU_MAX": 30.0, "PSI_IO_MAX": 1.0, "READ_MB": 6.0, "WRITE_MB": 12.0, "DURATION_S": 30.0, "IDLE_S": 10.0, "SATURATED_S": 20.0}])
    );
}

#[test]
fn given_nx_task_intervals_in_a_jobs_database_when_the_resources_are_read_then_each_task_that_ran_carries_its_cores_the_overlap_and_the_machine_it_saw() {
    // Given a job's samples, and its database with two overlapping tasks that ran and one that a cache replayed
    let dir = tempfile::tempdir().unwrap();
    let (stats, ledgers) = (dir.path().join("runner-stats"), dir.path().join("ci-db"));
    let rows: Vec<String> = [(1005, 20, 1000), (1010, 80, 2000), (1015, 100, 3000), (1020, 40, 2500), (1025, 10, 1500)].iter().map(|(t, c, m)| sample(*t, *c, *m, 0.0)).collect();
    write_stats(&stats, "core-tests", &rows);
    seed_tasks(&ledgers.join("ci-db-core-tests/rows.sqlite"), &[("app:test", "ran", 1_005_000, 1_015_000, 20.0), ("lib:test", "ran", 1_012_000, 1_022_000, 5.0), ("app:lint", "hit", 1_006_000, 1_006_100, 0.0)]);
    let local = dir.path().join("rows.sqlite");

    // When the resources are read
    let r = run_wf(&["ci", "resources", "--stats", &stats.display().to_string(), "--ledgers", &ledgers.display().to_string(), "--run", "9"], &RunOpts::default().env("WF_CI_DB", &local.display().to_string()));

    // Then only the tasks that ran have a row; each has its seconds, its share of the stage's CPU, the cores that makes, the two that overlapped, and the average CPU and memory peak of the samples over its interval
    assert_eq!((r.exit_code, line(&r)["tasks"].clone()), (0, json!(2)), "{}{}", r.stdout, r.stderr);
    assert_eq!(
        query(&local, "SELECT JOB, TASK, STARTED, ENDED, SECONDS, CPU_S, CORES_USED, CONCURRENT, CPU_AVG, MEM_PEAK_MB FROM CI_TASK_RESOURCES ORDER BY TASK"),
        json!([
            {"JOB": "core-tests", "TASK": "app:test", "STARTED": 1005, "ENDED": 1015, "SECONDS": 10.0, "CPU_S": 20.0, "CORES_USED": 2.0, "CONCURRENT": 2, "CPU_AVG": 90.0, "MEM_PEAK_MB": 3000.0},
            {"JOB": "core-tests", "TASK": "lib:test", "STARTED": 1012, "ENDED": 1022, "SECONDS": 10.0, "CPU_S": 5.0, "CORES_USED": 0.5, "CONCURRENT": 2, "CPU_AVG": 50.0, "MEM_PEAK_MB": 3000.0}
        ])
    );
}

fn timed(name: &str, created: &str, started: &str, completed: &str) -> Value {
    job(name, created, started, completed, vec![])
}

#[test]
fn given_a_run_of_jobs_in_a_chain_and_two_shards_when_its_timings_are_read_then_the_profile_has_the_peak_the_efficiency_the_waits_and_the_critical_path() {
    // Given plan, wf-build, two shards of different length that run together, a report, and a skipped job
    let run = json!({"id": 90, "name": "CI core", "status": "in_progress", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "abc", "workflow_id": 5, "head_branch": "dev"});
    let mut skipped = timed("run / deploy", "10:13:01", "10:13:01", "10:13:01");
    skipped["conclusion"] = json!("skipped");
    let jobs = json!({"total_count": 6, "jobs": [
        timed("run / plan", "10:00:00", "10:00:05", "10:01:00"),
        timed("run / wf-build", "10:01:00", "10:01:01", "10:03:00"),
        timed("run / core-tests (1/2)", "10:03:00", "10:03:01", "10:13:01"),
        timed("run / core-tests (2/2)", "10:03:00", "10:03:02", "10:08:02"),
        timed("run / report", "10:13:01", "10:13:02", "10:13:30"),
        skipped,
    ]});
    let server = served(&[("/repos/o/r/actions/runs/90", run), ("/repos/o/r/actions/runs/90/jobs?per_page=100&page=1", jobs)]);
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("rows.sqlite");

    // When the timings are read in a job with a local database
    let r = wf(&server, &["timings", "--run", "90"], &[("WF_CI_DB", &local.display().to_string())]);

    // Then 1102 runner seconds ran over 810 s of wall with at most two jobs at once: an efficiency of 1102 / (810 x 2), the waits sum to 10 s with 5 s the longest, and the critical path runs through the longer shard
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let p = &line(&r)["profile"];
    assert_eq!((p["jobs"].clone(), p["peak_jobs"].clone(), p["runner_s"].clone(), p["wall_s"].clone(), p["queue_s_total"].clone(), p["queue_s_max"].clone()), (json!(5), json!(2), json!(1102.0), json!(810.0), json!(10.0), json!(5.0)));
    assert!((p["parallel_efficiency"].as_f64().unwrap() - 1102.0 / 1620.0).abs() < 1e-9 && (p["avg_jobs"].as_f64().unwrap() - 1102.0 / 810.0).abs() < 1e-9, "{p}");
    assert_eq!((p["critical_path_job"].clone(), p["critical_path_s"].clone()), (json!("plan > wf-build > core-tests (1/2) > report"), json!(805.0)));
    // and the sealed database has the same row, keyed by the run
    let rows = query(&local, "SELECT RUN_ID, WORKFLOW, PEAK_JOBS, CRITICAL_PATH_S FROM CI_RUN_PROFILE");
    assert_eq!(rows, json!([{"RUN_ID": "90", "WORKFLOW": "CI core", "PEAK_JOBS": 2, "CRITICAL_PATH_S": 805.0}]));
}

#[test]
fn given_a_job_on_a_named_runner_with_setup_steps_when_its_timings_are_kept_then_ci_steps_has_every_step_and_ci_jobs_the_wait_to_start_and_the_runner() {
    // Given a job that waited 30 s, ran on a named runner, and has a setup step and a test step; and a job cancelled while pending
    let run = json!({"id": 91, "name": "CI core", "status": "completed", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "abc"});
    let mut on_runner = job("run / core-tests (1/2)", "10:00:00", "10:00:30", "10:10:30", vec![step("Set up job", "10:00:31", "10:00:35"), step("Checkout", "10:00:35", "10:00:55"), step("Core tests", "10:00:55", "10:10:30")]);
    on_runner["runner_name"] = json!("GitHub Actions 7");
    let mut pending = job("run / core-tests (2/2)", "10:00:00", "10:00:00", "10:04:00", vec![]);
    pending["conclusion"] = json!("cancelled");
    pending["started_at"] = Value::Null;
    let server = served(&[("/repos/o/r/actions/runs/91", run), ("/repos/o/r/actions/runs/91/jobs?per_page=100&page=1", json!({"total_count": 2, "jobs": [on_runner, pending]}))]);
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("rows.sqlite");

    // When the timings are kept
    let r = wf(&server, &["timings", "--run", "91"], &[("WF_CI_DB", &local.display().to_string())]);

    // Then every step is a row with its length, and the job row has the wait from creation to start and the runner; the job that never started has neither
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(
        query(&local, "SELECT STEP_NO, STEP, DURATION_S, CONCLUSION FROM CI_STEPS WHERE JOB = 'core-tests (1/2)' ORDER BY STEP_NO"),
        json!([{"STEP_NO": 1, "STEP": "Set up job", "DURATION_S": 4, "CONCLUSION": "success"}, {"STEP_NO": 2, "STEP": "Checkout", "DURATION_S": 20, "CONCLUSION": "success"}, {"STEP_NO": 3, "STEP": "Core tests", "DURATION_S": 575, "CONCLUSION": "success"}])
    );
    assert_eq!(
        query(&local, "SELECT JOB, QUEUED_S, RUNNER FROM CI_JOBS ORDER BY JOB"),
        json!([{"JOB": "core-tests (1/2)", "QUEUED_S": 30, "RUNNER": "GitHub Actions 7"}, {"JOB": "core-tests (2/2)", "QUEUED_S": null, "RUNNER": null}])
    );
}

fn running(name: &str, status: &str) -> Value {
    json!({"id": 1, "name": name, "status": status, "conclusion": null, "created_at": "2026-11-19T10:00:00Z", "started_at": null, "completed_at": null, "steps": []})
}

#[test]
fn given_runs_in_progress_and_queued_across_workflows_when_concurrency_is_read_then_busy_queued_runs_and_headroom_are_counted_per_workflow_and_in_all() {
    // Given two workflows with runs in progress and one queued run (the first run listed under both statuses), with 5 jobs running and 3 waiting
    let runs = |ids: &[(u64, &str)]| json!({"total_count": ids.len(), "workflow_runs": ids.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>()});
    let jobs = |list: Vec<Value>| json!({"total_count": list.len(), "jobs": list});
    let server = served(&[
        ("/repos/o/r/actions/runs?status=in_progress&per_page=100", runs(&[(1, "CI core"), (2, "CI mobile e2e")])),
        ("/repos/o/r/actions/runs?status=queued&per_page=100", runs(&[(3, "CI core"), (1, "CI core")])),
        ("/repos/o/r/actions/runs/1/jobs?per_page=100&page=1", jobs(vec![running("run / a", "in_progress"), running("run / b", "in_progress"), running("run / c", "in_progress"), running("run / d", "queued")])),
        ("/repos/o/r/actions/runs/2/jobs?per_page=100&page=1", jobs(vec![running("run / e", "in_progress"), running("run / f", "in_progress")])),
        ("/repos/o/r/actions/runs/3/jobs?per_page=100&page=1", jobs(vec![running("run / g", "queued"), running("run / h", "queued")])),
    ]);
    let source = MockServer::start(None);
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("rows.sqlite");

    // When the concurrency is read at the end of run 7
    let r = wf(&server, &["concurrency", "--phase", "end", "--run", "7"], &[("WF_CI_DB", &local.display().to_string()), ("WF_CI_METRICS", ""), ("WF_CI_METRICS_BASE", &format!("http://127.0.0.1:{}", source.port)), ("WF_CI_NOW", "2026-11-19T10:00:00Z")]);

    // Then the line is the account's: 5 busy of the cap of 20, 3 queued, 3 runs, headroom 15, and each workflow has its own counts
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = line(&r);
    assert_eq!((out["busy"].clone(), out["queued"].clone(), out["runs_active"].clone(), out["headroom"].clone(), out["cap"].clone()), (json!(5), json!(3), json!(3), json!(15), json!(20)));
    assert_eq!(out["workflows"], json!([{"workflow": "CI core", "busy": 3, "queued": 3, "runs_active": 2}, {"workflow": "CI mobile e2e", "busy": 2, "queued": 0, "runs_active": 1}]));
    // and the rows are in CI_CONCURRENCY with the phase and the account's headroom on each
    assert_eq!(
        query(&local, "SELECT RUN_ID, PHASE, WORKFLOW, BUSY, QUEUED, RUNS_ACTIVE, CAP, HEADROOM FROM CI_CONCURRENCY ORDER BY WORKFLOW"),
        json!([
            {"RUN_ID": "7", "PHASE": "end", "WORKFLOW": "CI core", "BUSY": 3, "QUEUED": 3, "RUNS_ACTIVE": 2, "CAP": 20, "HEADROOM": 15},
            {"RUN_ID": "7", "PHASE": "end", "WORKFLOW": "CI mobile e2e", "BUSY": 2, "QUEUED": 0, "RUNS_ACTIVE": 1, "CAP": 20, "HEADROOM": 15},
            {"RUN_ID": "7", "PHASE": "end", "WORKFLOW": "all", "BUSY": 5, "QUEUED": 3, "RUNS_ACTIVE": 3, "CAP": 20, "HEADROOM": 15}
        ])
    );
    // and the gauges went out with the workflow as scope and the phase as name
    let seen = gauges(&source.requests());
    let value = |metric: &str, scope: &str| seen.iter().find(|g| g.metric == metric && g.scope == scope && g.name == "end").map(|g| g.value);
    assert_eq!((value("ci.runners.busy", "all"), value("ci.runners.queued", "all"), value("ci.runners.headroom", "CI core")), (Some(5.0), Some(3.0), Some(15.0)), "{seen:?}");
}

#[test]
fn given_an_unreadable_actions_api_when_concurrency_is_read_then_it_says_why_and_still_exits_zero() {
    // Given an API that refuses the listing
    let server = MockServer::start(None);
    server.routes(&[("GET /repos/o/r/actions/runs?status=in_progress&per_page=100", MockReply::json(403, json!({"message": "forbidden"})))]);

    // When the concurrency is read
    let r = wf(&server, &["concurrency", "--run", "7"], &[]);

    // Then the line says it failed with a next step, and the job is not failed
    assert_eq!((r.exit_code, line(&r)["ok"].clone()), (0, json!(false)), "{}{}", r.stdout, r.stderr);
    assert!(line(&r)["next"].as_str().is_some_and(|n| n.contains("actions:read")), "{}", r.stdout);
}
