//! `wf ci timings` and `wf ci progress` through the built binary, against a loopback fake of the Actions API: the run, its jobs with their steps and
//! the last green run of the branch.
use crate::support::otlp::gauges;
use crate::support::{run_wf, MockReply, MockServer, RunOpts, RunResult};
use serde_json::{json, Value};

const TOKEN: &str = "flowtesttoken-0123456789";

pub(crate) fn step(name: &str, from: &str, to: &str) -> Value {
    json!({"name": name, "status": "completed", "conclusion": "success", "started_at": format!("2026-11-19T{from}Z"), "completed_at": format!("2026-11-19T{to}Z")})
}

pub(crate) fn job(name: &str, created: &str, started: &str, completed: &str, steps: Vec<Value>) -> Value {
    json!({"id": 1, "name": name, "status": "completed", "conclusion": "success", "created_at": format!("2026-11-19T{created}Z"), "started_at": format!("2026-11-19T{started}Z"), "completed_at": format!("2026-11-19T{completed}Z"), "steps": steps})
}

pub(crate) fn served(routes: &[(&str, Value)]) -> MockServer {
    let server = MockServer::start(None);
    let table: Vec<(String, MockReply)> = routes.iter().map(|(p, b)| (format!("GET {p}"), MockReply::json(200, b.clone()))).collect();
    server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());
    server
}

pub(crate) fn wf(server: &MockServer, args: &[&str], extra: &[(&str, &str)]) -> RunResult {
    let mut argv = vec!["ci"];
    argv.extend(args);
    let mut opts = RunOpts::default()
        .env("GITHUB_TOKEN", TOKEN)
        .env("GITHUB_REPOSITORY", "o/r")
        .env("GITHUB_API_URL", &format!("http://127.0.0.1:{}", server.port))
        .env("GITHUB_REF_NAME", "dev")
        .env("WF_CI_METRICS", "off")
        .env("WF_CI_LOGS", "off")
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");
    for (k, v) in extra {
        opts = opts.env(k, v);
    }
    run_wf(&argv, &opts)
}

pub(crate) fn line(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}{}", r.stdout, r.stderr))
}

#[test]
fn given_a_finished_run_when_its_timings_are_read_then_each_job_its_waits_its_long_steps_the_stage_spreads_and_the_wall_time_are_one_json_line() {
    // Given a run with two shards of one stage, one slower by five minutes, and a single job
    let run = json!({"id": 77, "status": "completed", "conclusion": "success", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "abc", "workflow_id": 5, "head_branch": "dev"});
    let jobs = json!({"total_count": 3, "jobs": [
        job("run / flow-tests (1/6)", "10:00:00", "10:00:02", "10:10:02", vec![step("Set up job", "10:00:02", "10:00:03"), step("Run ./.github/actions/install-test-deps", "10:00:03", "10:03:03"), step("Flow tests", "10:03:03", "10:10:00")]),
        job("run / flow-tests (2/6)", "10:00:00", "10:00:30", "10:15:30", vec![step("Run ./.github/actions/install-test-deps", "10:00:31", "10:08:31"), step("Flow tests", "10:08:31", "10:15:30")]),
        job("run / wf-build", "10:00:00", "10:00:01", "10:02:01", vec![step("Build wf", "10:00:02", "10:02:00")]),
    ]});
    let server = served(&[("/repos/o/r/actions/runs/77", run), ("/repos/o/r/actions/runs/77/jobs?per_page=100&page=1", jobs)]);
    // When the timings are read for that run
    let r = wf(&server, &["timings", "--run", "77"], &[]);
    // Then each job has its duration and the time it waited for a runner, and steps under five seconds are left out
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = line(&r);
    let t = &out["timings"];
    let flow1 = t["jobs"].as_array().unwrap().iter().find(|j| j["job"] == "flow-tests (1/6)").unwrap();
    assert_eq!((flow1["stage"].clone(), flow1["shard"].clone(), flow1["duration_s"].clone(), flow1["queue_s"].clone()), (json!("flow-tests"), json!("1/6"), json!(600), json!(2)));
    assert_eq!(flow1["steps"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Run ./.github/actions/install-test-deps", "Flow tests"]);
    // And the stage's spread is the slowest shard less the fastest, its steps keep the slowest shard's time, and the run's wall time ends at the last job
    assert_eq!(t["spreads"], json!([{"stage": "flow-tests", "shards": 2, "spread_s": 300, "slowest_s": 900}]));
    let install = t["steps"].as_array().unwrap().iter().find(|s| s["step"] == "Run ./.github/actions/install-test-deps").unwrap();
    assert_eq!(install["slowest_s"], json!(480));
    assert_eq!(t["wall_s"], json!(930));
    assert_eq!(out["sent"], json!(false));
}

#[test]
fn given_a_run_that_is_going_when_its_progress_is_read_then_the_jobs_not_done_show_their_step_their_time_and_what_the_last_green_run_took() {
    // Given a run with one job done, one in its second step and one still waiting for a runner, and a green run before it
    let running = json!({"id": 1, "name": "run / flow-tests (3/6)", "status": "in_progress", "conclusion": null, "created_at": "2026-11-19T10:00:00Z", "started_at": "2026-11-19T10:00:02Z", "completed_at": null,
        "steps": [step("Set up job", "10:00:02", "10:00:03"), {"name": "Flow tests", "status": "in_progress", "conclusion": null, "started_at": "2026-11-19T10:03:03Z", "completed_at": null}]});
    let waiting = json!({"id": 2, "name": "run / android-e2e-tests", "status": "queued", "conclusion": null, "created_at": "2026-11-19T10:05:00Z", "started_at": null, "completed_at": null, "steps": []});
    let done = job("run / wf-build", "10:00:00", "10:00:01", "10:02:01", vec![]);
    let run = json!({"id": 77, "status": "in_progress", "conclusion": null, "created_at": "2026-11-19T10:00:00Z", "head_sha": "abc", "workflow_id": 5, "head_branch": "dev"});
    let prior = json!({"total_count": 1, "jobs": [job("run / flow-tests (3/6)", "09:00:00", "09:00:02", "09:12:02", vec![])]});
    let server = served(&[
        ("/repos/o/r/actions/runs/77", run),
        ("/repos/o/r/actions/runs/77/jobs?per_page=100&page=1", json!({"total_count": 3, "jobs": [done, running, waiting]})),
        ("/repos/o/r/actions/workflows/5/runs?branch=dev&status=success&per_page=1", json!({"workflow_runs": [{"id": 70}]})),
        ("/repos/o/r/actions/runs/70/jobs?per_page=100&page=1", prior),
    ]);
    // When the progress is read at 10:08:00
    let r = wf(&server, &["progress", "--run", "77"], &[("WF_CI_NOW", "2026-11-19T10:08:00Z")]);
    // Then the counts are right and each open job says where it is
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let p = line(&r)["progress"].clone();
    assert_eq!((p["done"].clone(), p["running"].clone(), p["not_started"].clone()), (json!(1), json!(1), json!(1)));
    let open = p["open"].as_array().unwrap();
    let flow = open.iter().find(|j| j["job"] == "flow-tests (3/6)").unwrap();
    assert_eq!((flow["step"].clone(), flow["elapsed_s"].clone(), flow["step_elapsed_s"].clone(), flow["expected_s"].clone()), (json!("Flow tests"), json!(478), json!(297), json!(720)));
    let android = open.iter().find(|j| j["job"] == "android-e2e-tests").unwrap();
    assert_eq!((android["waiting_s"].clone(), android["elapsed_s"].clone()), (json!(180), json!(null)));
}

#[test]
fn given_no_run_and_no_token_when_timings_are_asked_then_it_says_what_to_set_and_exits_1() {
    let server = MockServer::start(None);
    let r = run_wf(&["ci", "timings"], &RunOpts::default().env("GITHUB_TOKEN", "").env("GITHUB_RUN_ID", "").env("GITHUB_API_URL", &format!("http://127.0.0.1:{}", server.port)).env("PATH", "/usr/bin:/bin"));
    let out = line(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (1, json!(false)), "{}{}", r.stdout, r.stderr);
    assert!(out["next"].as_str().is_some_and(|n| n.contains("GITHUB_TOKEN")), "{out}");
}

#[test]
fn given_a_finished_run_when_its_timings_are_read_in_a_job_with_a_local_database_then_each_job_is_a_row_the_merge_carries_into_ci_jobs() {
    // Given a run of two jobs, one that waited 30 s for a runner, and a job-local database
    let run = json!({"id": 78, "status": "completed", "conclusion": "success", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "feedbeef"});
    let jobs = json!({"total_count": 2, "jobs": [
        job("run / flow-tests (1/6)", "10:00:00", "10:00:30", "10:10:30", vec![]),
        job("run / wf-build", "10:00:00", "10:00:01", "10:02:01", vec![]),
    ]});
    let server = served(&[("/repos/o/r/actions/runs/78", run), ("/repos/o/r/actions/runs/78/jobs?per_page=100&page=1", jobs)]);
    let dir = tempfile::tempdir().unwrap();
    let (local, sealed) = (dir.path().join("rows.sqlite"), dir.path().join("ci.sealed"));
    let key = "0707070707070707070707070707070707070707070707070707070707070707";

    // When the timings are read and the report job merges the local database
    let r = wf(&server, &["timings", "--run", "78"], &[("WF_CI_DB", &local.display().to_string())]);
    let merged = wf(&server, &["db", "merge", "--sealed-file", &sealed.display().to_string(), &local.display().to_string()], &[("CI_DB_KEY", key)]);

    // Then the sealed database holds one CI_JOBS row per job with its seconds and its wait
    assert_eq!((r.exit_code, merged.exit_code), (0, 0), "{}{}{}{}", r.stdout, r.stderr, merged.stdout, merged.stderr);
    let q = wf(&server, &["db", "query", "--sealed-file", &sealed.display().to_string(), "SELECT KIND, JOB, SECS, QUEUED, SHA, CONCLUSION, GROUP_KEY FROM CI_JOBS ORDER BY JOB"], &[("CI_DB_KEY", key)]);
    assert_eq!(
        line(&q)["rows"],
        json!([{"KIND": "flow-tests", "JOB": "flow-tests (1/6)", "SECS": 600, "QUEUED": 30, "SHA": "feedbeef", "CONCLUSION": "success", "GROUP_KEY": null}, {"KIND": "wf-build", "JOB": "wf-build", "SECS": 120, "QUEUED": 1, "SHA": "feedbeef", "CONCLUSION": "success", "GROUP_KEY": null}]),
        "{}{}",
        q.stdout,
        q.stderr
    );
}

#[test]
fn given_a_running_job_with_a_red_step_when_failed_is_asked_on_its_runner_then_the_step_is_named_while_the_job_still_runs() {
    // Given a run whose job on runner "GitHub Actions 7" is still running with its test step red and a later step pending
    let run = json!({"id": 79, "status": "in_progress", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "abc"});
    let jobs = json!({"total_count": 2, "jobs": [
        {"id": 1, "name": "run / flow-tests (1/6)", "status": "in_progress", "conclusion": null, "runner_name": "GitHub Actions 7", "html_url": "https://github.test/job/1",
         "steps": [{"name": "Set up job", "status": "completed", "conclusion": "success"}, {"name": "Flow tests", "status": "completed", "conclusion": "failure"}, {"name": "Upload", "status": "pending", "conclusion": null}]},
        {"id": 2, "name": "run / flow-tests (2/6)", "status": "in_progress", "conclusion": null, "runner_name": "GitHub Actions 8", "html_url": "https://github.test/job/2",
         "steps": [{"name": "Flow tests", "status": "in_progress", "conclusion": null}]},
    ]});
    let server = served(&[("/repos/o/r/actions/runs/79", run), ("/repos/o/r/actions/runs/79/jobs?per_page=100&page=1", jobs)]);

    // When the job on runner 7 asks which of its steps failed
    let r = wf(&server, &["failed", "--run", "79", "--runner", "GitHub Actions 7"], &[]);

    // Then it names its own job and the red step, not the other runner's job, and exits 0
    let out = line(&r);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!((out["ok"].clone(), out["job"].clone(), out["failed_steps"].clone()), (json!(true), json!("flow-tests (1/6)"), json!(["Flow tests"])), "{out}");
}

#[test]
fn given_a_shard_replaced_while_pending_when_the_timings_are_kept_then_it_is_a_cancelled_row_that_never_started_with_its_group_and_its_wait() {
    // Given an integration run whose shard 1 ran and whose shard 2 was replaced by a newer commit's job while it was still pending
    let run = json!({"id": 80, "status": "completed", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "cafe"});
    let pending = json!({"id": 2, "name": "run / integration-tests (2/2)", "status": "completed", "conclusion": "cancelled", "created_at": "2026-11-19T10:00:00Z", "started_at": null, "completed_at": "2026-11-19T10:04:00Z", "steps": []});
    let jobs = json!({"total_count": 2, "jobs": [job("run / integration-tests (1/2)", "10:00:00", "10:00:05", "10:09:05", vec![]), pending]});
    let server = served(&[("/repos/o/r/actions/runs/80", run), ("/repos/o/r/actions/runs/80/jobs?per_page=100&page=1", jobs)]);
    let dir = tempfile::tempdir().unwrap();
    let (local, sealed) = (dir.path().join("rows.sqlite"), dir.path().join("ci.sealed"));
    let key = "0707070707070707070707070707070707070707070707070707070707070707";

    // When the report job keeps the timings with the plan's shard keys and merges
    let r = wf(&server, &["timings", "--run", "80"], &[("WF_CI_DB", &local.display().to_string()), ("WF_CI_SHARD_KEYS", r#"["k-one","k-two"]"#), ("GITHUB_WORKFLOW", "CI core")]);
    let merged = wf(&server, &["db", "merge", "--sealed-file", &sealed.display().to_string(), &local.display().to_string()], &[("CI_DB_KEY", key)]);

    // Then the replaced shard is a cancelled row with no start, the 240 s it waited and its group, and the shard that ran has its own group
    assert_eq!((r.exit_code, merged.exit_code), (0, 0), "{}{}{}{}", r.stdout, r.stderr, merged.stdout, merged.stderr);
    let q = wf(&server, &["db", "query", "--sealed-file", &sealed.display().to_string(), "SELECT JOB, CONCLUSION, STARTED > 0 AS started, SECS, QUEUED, GROUP_KEY FROM CI_JOBS ORDER BY JOB"], &[("CI_DB_KEY", key)]);
    assert_eq!(
        line(&q)["rows"],
        json!([{"JOB": "integration-tests (1/2)", "CONCLUSION": "success", "started": 1, "SECS": 540, "QUEUED": 5, "GROUP_KEY": "hosted-integration-CI core-k-one"}, {"JOB": "integration-tests (2/2)", "CONCLUSION": "cancelled", "started": 0, "SECS": 0, "QUEUED": 240, "GROUP_KEY": "hosted-integration-CI core-k-two"}]),
        "{}{}",
        q.stdout,
        q.stderr
    );
}

#[test]
fn given_a_failed_job_when_the_timings_are_kept_then_the_failed_step_and_the_lines_that_say_why_are_in_ci_jobs() {
    // Given a run whose test job failed in its "Flow tests" step with a panic in its log
    let run = json!({"id": 81, "status": "completed", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "dead"});
    let mut red = job("run / flow-tests (1/6)", "10:00:00", "10:00:02", "10:03:02", vec![step("Flow tests", "10:00:03", "10:03:00")]);
    red["id"] = json!(501);
    red["conclusion"] = json!("failure");
    red["steps"][0]["conclusion"] = json!("failure");
    let jobs = json!({"total_count": 1, "jobs": [red]});
    let server = MockServer::start(None);
    server.routes(&[
        ("GET /repos/o/r/actions/runs/81", MockReply::json(200, run)),
        ("GET /repos/o/r/actions/runs/81/jobs?per_page=100&page=1", MockReply::json(200, jobs)),
        ("GET /repos/o/r/actions/jobs/501/logs", MockReply::text(200, "2026-11-19T10:02:59.0Z thread 'tests::adds' panicked at src/lib.rs:7:5:\n2026-11-19T10:02:59.1Z assertion `left == right` failed\n2026-11-19T10:03:00.0Z error: test failed, to rerun pass `-p demo --lib`\n")),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let (local, sealed) = (dir.path().join("rows.sqlite"), dir.path().join("ci.sealed"));
    let key = "0707070707070707070707070707070707070707070707070707070707070707";

    // When the timings are kept and merged
    let r = wf(&server, &["timings", "--run", "81"], &[("WF_CI_DB", &local.display().to_string())]);
    let merged = wf(&server, &["db", "merge", "--sealed-file", &sealed.display().to_string(), &local.display().to_string()], &[("CI_DB_KEY", key)]);

    // Then the row names the red step and holds the panic line
    assert_eq!((r.exit_code, merged.exit_code), (0, 0), "{}{}{}{}", r.stdout, r.stderr, merged.stdout, merged.stderr);
    let q = wf(&server, &["db", "query", "--sealed-file", &sealed.display().to_string(), "SELECT CONCLUSION, STEP, EXCERPT FROM CI_JOBS"], &[("CI_DB_KEY", key)]);
    let row = &line(&q)["rows"][0];
    assert_eq!((row["CONCLUSION"].as_str(), row["STEP"].as_str()), (Some("failure"), Some("Flow tests")), "{row}");
    assert!(row["EXCERPT"].as_str().is_some_and(|e| e.contains("panicked at src/lib.rs")), "{row}");
}

/// A run whose job on runner "GitHub Actions 7" is running with its test step red; `annotations` is what the check run's annotations route answers (none: no route).
fn red_running_run(annotations: Option<Value>) -> MockServer {
    let run = json!({"id": 79, "status": "in_progress", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "abc"});
    let jobs = json!({"total_count": 1, "jobs": [
        {"id": 1, "name": "run / flow-tests (1/6)", "status": "in_progress", "conclusion": null, "runner_name": "GitHub Actions 7", "html_url": "https://github.test/job/1",
         "steps": [{"name": "Flow tests", "status": "completed", "conclusion": "failure"}, {"name": "Upload", "status": "pending", "conclusion": null}]},
    ]});
    let server = MockServer::start(None);
    let mut table = vec![
        ("GET /repos/o/r/actions/runs/79".to_string(), MockReply::json(200, run)),
        ("GET /repos/o/r/actions/runs/79/jobs?per_page=100&page=1".to_string(), MockReply::json(200, jobs)),
        ("GET /repos/o/r/pulls?head=o:dev&state=open&per_page=1".to_string(), MockReply::json(200, json!([]))),
    ];
    if let Some(list) = annotations {
        table.push(("GET /repos/o/r/check-runs/1/annotations".to_string(), MockReply::json(200, list)));
    }
    server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());
    server
}

/// The log lines a stand-in OTLP logs ingest received: (message, attributes by key).
fn logged(ingest: &MockServer) -> Vec<(String, std::collections::BTreeMap<String, String>)> {
    let mut out = Vec::new();
    for request in ingest.requests().iter().filter(|r| r.method == "POST" && r.url.ends_with("/v1/logs")) {
        let body: Value = serde_json::from_str(&request.body).unwrap_or(Value::Null);
        for record in body["resourceLogs"].as_array().into_iter().flatten().flat_map(|r| r["scopeLogs"].as_array().into_iter().flatten()).flat_map(|s| s["logRecords"].as_array().into_iter().flatten()) {
            let attrs = record["attributes"].as_array().into_iter().flatten().filter_map(|a| Some((a["key"].as_str()?.to_string(), a["value"]["stringValue"].as_str()?.to_string()))).collect();
            out.push((record["body"]["stringValue"].as_str().unwrap_or_default().to_string(), attrs));
        }
    }
    out
}

#[test]
fn given_a_job_with_a_red_step_and_an_annotation_when_failed_runs_then_the_log_line_carries_the_failure_and_the_annotation_count() {
    // Given a running job with a red step whose annotations the API shows (two), and a stand-in logs ingest
    let server = red_running_run(Some(json!([{"annotation_level": "failure"}, {"annotation_level": "failure"}])));
    let ingest = MockServer::start(None);
    ingest.queue(vec![MockReply::json(200, json!({}))], true);
    let base = format!("http://127.0.0.1:{}", ingest.port);

    // When the job's end step runs `wf ci failed`
    let r = wf(&server, &["failed", "--run", "79"], &[("WF_CI_LOGS", ""), ("WF_CI_LOGS_BASE", &base), ("GITHUB_RUN_ID", "79"), ("GITHUB_SHA", "abc1234"), ("RUNNER_NAME", "GitHub Actions 7")]);

    // Then the verb exits 0 and reports the count, and the ingest got one `ci.job.failed` line with run, job, step, sha and the count, marked scrubbed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = line(&r);
    assert_eq!((out["annotations"].clone(), out["logged"].clone()), (json!(2), json!({"sent": true})), "{out}");
    let lines = logged(&ingest);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let (message, attrs) = &lines[0];
    assert_eq!(message, "ci.job.failed");
    let get = |k: &str| attrs.get(k).map(String::as_str);
    assert_eq!((get("run"), get("job"), get("step"), get("sha"), get("scrubbed"), get("annotations")), (Some("79"), Some("flow-tests (1/6)"), Some("Flow tests"), Some("abc1234"), Some("env-secrets"), Some("2")), "{attrs:?}");
}

#[test]
fn given_a_job_whose_annotations_the_api_does_not_show_when_failed_runs_then_the_count_is_unreadable_in_the_json_and_in_the_line() {
    // Given the same running job, but no annotations route (the API answers 404)
    let server = red_running_run(None);
    let ingest = MockServer::start(None);
    ingest.queue(vec![MockReply::json(200, json!({}))], true);
    let base = format!("http://127.0.0.1:{}", ingest.port);

    // When failed runs
    let r = wf(&server, &["failed", "--run", "79"], &[("WF_CI_LOGS", ""), ("WF_CI_LOGS_BASE", &base), ("GITHUB_RUN_ID", "79"), ("GITHUB_SHA", "abc1234"), ("RUNNER_NAME", "GitHub Actions 7")]);

    // Then the JSON says null and the line says unreadable, and the step is still reported
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!((line(&r)["annotations"].clone(), line(&r)["failed_steps"].clone()), (json!(null), json!(["Flow tests"])));
    assert_eq!(logged(&ingest)[0].1.get("annotations").map(String::as_str), Some("unreadable"));
}

#[test]
fn given_a_failure_hook_when_failed_runs_with_notify_then_one_json_post_reaches_it_and_without_notify_or_a_url_nothing_is_posted() {
    // Given a red running job, a logs ingest that is down, and a hook that answers 200
    let server = red_running_run(Some(json!([])));
    let hook = MockServer::start(None);
    hook.queue(vec![MockReply::json(200, json!({}))], true);
    let url = format!("http://127.0.0.1:{}/hook", hook.port);
    let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let down = format!("http://127.0.0.1:{gone}");
    let env = [("WF_CI_LOGS", ""), ("WF_CI_LOGS_BASE", down.as_str()), ("GITHUB_RUN_ID", "79"), ("GITHUB_SHA", "abc1234"), ("RUNNER_NAME", "GitHub Actions 7"), ("CI_FAILURE_HOOK_URL", url.as_str())];

    // When failed runs without --notify, and then with it
    let quiet = wf(&server, &["failed", "--run", "79"], &env);
    assert!(hook.requests().is_empty(), "posted without --notify: {:?}", hook.requests());
    let r = wf(&server, &["failed", "--run", "79", "--notify"], &env);

    // Then the notified run exits 0 though the log ingest is down, and the hook got the run, job, step, task and sha as JSON
    assert_eq!((quiet.exit_code, r.exit_code), (0, 0), "{}{}", r.stdout, r.stderr);
    assert!(line(&r)["logged"]["error"].is_string(), "{}", line(&r));
    assert_eq!(line(&r)["notified"], json!({"status": 200}));
    let posts: Vec<Value> = hook.requests().iter().filter(|q| q.method == "POST" && q.url == "/hook").map(|q| serde_json::from_str(&q.body).unwrap()).collect();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert_eq!((posts[0]["event"].clone(), posts[0]["run"].clone(), posts[0]["job"].clone(), posts[0]["step"].clone(), posts[0]["sha"].clone()), (json!("ci.job.failed"), json!("79"), json!("flow-tests (1/6)"), json!("Flow tests"), json!("abc1234")), "{:?}", posts[0]);

    // And with no URL, --notify is a no-op that still exits 0
    let none = wf(&server, &["failed", "--run", "79", "--notify"], &[("WF_CI_LOGS", ""), ("WF_CI_LOGS_BASE", down.as_str()), ("GITHUB_RUN_ID", "79"), ("GITHUB_SHA", "abc1234"), ("RUNNER_NAME", "GitHub Actions 7"), ("CI_FAILURE_HOOK_URL", "")]);
    assert_eq!((none.exit_code, line(&none)["notified"].clone()), (0, json!(null)), "{}{}", none.stdout, none.stderr);
}

#[test]
fn given_a_finished_run_when_timings_are_sent_then_a_stand_in_source_receives_every_gauge_the_verb_documents() {
    // Given a run of two shards of a stage and a single job, and a stand-in OTLP metrics source
    let run = json!({"id": 77, "status": "completed", "conclusion": "success", "created_at": "2026-11-19T10:00:00Z", "run_started_at": "2026-11-19T10:00:00Z", "head_sha": "abc", "workflow_id": 5, "head_branch": "dev"});
    let jobs = json!({"total_count": 3, "jobs": [
        job("run / flow-tests (1/6)", "10:00:00", "10:00:02", "10:10:02", vec![step("Flow tests", "10:03:03", "10:10:00")]),
        job("run / flow-tests (2/6)", "10:00:00", "10:00:30", "10:15:30", vec![step("Flow tests", "10:08:31", "10:15:30")]),
        job("run / wf-build", "10:00:00", "10:00:01", "10:02:01", vec![]),
    ]});
    let server = served(&[("/repos/o/r/actions/runs/77", run), ("/repos/o/r/actions/runs/77/jobs?per_page=100&page=1", jobs)]);
    let source = MockServer::start(None);

    // When the timings are read with --send
    let r = wf(&server, &["timings", "--run", "77", "--send"], &[("WF_CI_METRICS", "on"), ("WF_CI_METRICS_BASE", &format!("http://127.0.0.1:{}", source.port))]);

    // Then the source got the job, queue, step, spread and wall gauges with their labels
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let seen = gauges(&source.requests());
    let value = |metric: &str, scope: &str, name: &str| seen.iter().find(|g| g.metric == metric && g.scope == scope && g.name == name).map(|g| g.value);
    assert_eq!(value("ci.job.duration_s", "flow-tests", "1/6"), Some(600.0), "{seen:?}");
    assert_eq!(value("ci.job.duration_s", "flow-tests", "2/6"), Some(900.0));
    assert_eq!(value("ci.job.queue_s", "flow-tests", "2/6"), Some(30.0));
    assert_eq!(value("ci.step.duration_s", "flow-tests", "Flow tests"), Some(419.0));
    assert_eq!(value("ci.stage.spread_s", "flow-tests", "all"), Some(300.0));
    assert_eq!(value("ci.run.wall_s", "77", "all"), Some(930.0));
}
