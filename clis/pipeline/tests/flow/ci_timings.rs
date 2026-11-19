//! `wf ci timings` and `wf ci progress` through the built binary, against a loopback fake of the Actions API: the run, its jobs with their steps and
//! the last green run of the branch.
use crate::support::{run_wf, MockReply, MockServer, RunOpts, RunResult};
use serde_json::{json, Value};

const TOKEN: &str = "flowtesttoken-0123456789";

fn step(name: &str, from: &str, to: &str) -> Value {
    json!({"name": name, "status": "completed", "conclusion": "success", "started_at": format!("2026-11-19T{from}Z"), "completed_at": format!("2026-11-19T{to}Z")})
}

fn job(name: &str, created: &str, started: &str, completed: &str, steps: Vec<Value>) -> Value {
    json!({"id": 1, "name": name, "status": "completed", "conclusion": "success", "created_at": format!("2026-11-19T{created}Z"), "started_at": format!("2026-11-19T{started}Z"), "completed_at": format!("2026-11-19T{completed}Z"), "steps": steps})
}

fn served(routes: &[(&str, Value)]) -> MockServer {
    let server = MockServer::start(None);
    let table: Vec<(String, MockReply)> = routes.iter().map(|(p, b)| (format!("GET {p}"), MockReply::json(200, b.clone()))).collect();
    server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());
    server
}

fn wf(server: &MockServer, args: &[&str], extra: &[(&str, &str)]) -> RunResult {
    let mut argv = vec!["ci"];
    argv.extend(args);
    let mut opts = RunOpts::default()
        .env("GITHUB_TOKEN", TOKEN)
        .env("GITHUB_REPOSITORY", "o/r")
        .env("GITHUB_API_URL", &format!("http://127.0.0.1:{}", server.port))
        .env("GITHUB_REF_NAME", "dev")
        .env("WF_CI_METRICS", "off")
        .env("NO_PROXY", "127.0.0.1")
        .env("no_proxy", "127.0.0.1");
    for (k, v) in extra {
        opts = opts.env(k, v);
    }
    run_wf(&argv, &opts)
}

fn line(r: &RunResult) -> Value {
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
