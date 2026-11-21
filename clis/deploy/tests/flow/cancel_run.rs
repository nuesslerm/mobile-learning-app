//! `wf vm ops cancel-run`, run on agent-shell (which holds the PAT): one named CI run, or every run still waiting for a runner, cancelled
//! through a fake GitHub; the token and every reply body stay out of the output.
use crate::support::{run_wf, MockReply, MockServer, RunOpts};
use serde_json::{json, Value};

const PAT: &str = "github_pat_11AAAAAAA0zzzzzzzzzzzz_secretsecretsecret";
const WORKFLOWS: [&str; 6] = ["ci-hosted-backend-core.yml", "ci-hosted-mobile-core.yml", "ci-hosted-cli-core.yml", "ci-hosted-cli-build.yml", "ci-hosted-backend-build.yml", "ci-hosted-mobile-build.yml"];

fn list(workflow: &str) -> String {
    format!("GET /repos/nuesslerm/mobile-learning-app/actions/workflows/{workflow}/runs?branch=dev&per_page=100")
}

fn cancel_route(id: u64) -> String {
    format!("POST /repos/nuesslerm/mobile-learning-app/actions/runs/{id}/cancel")
}

/// Runs spread over two of the six workflows (the other four list none): ci-hosted-backend-core.yml holds a pending, a running and a finished run,
/// ci-hosted-cli-core.yml a queued one.
fn github(cancel_status: u16) -> MockServer {
    let server = MockServer::start(None);
    let backend = json!({ "workflow_runs": [
        { "id": 9003, "run_number": 1230, "status": "pending", "display_title": "title-secret-xyz" },
        { "id": 9001, "run_number": 1228, "status": "in_progress" },
        { "id": 9000, "run_number": 1227, "status": "completed" },
    ] });
    let cli = json!({ "workflow_runs": [{ "id": 9002, "run_number": 1229, "status": "queued" }] });
    let mut table: Vec<(String, MockReply)> = WORKFLOWS.iter().map(|w| (list(w), MockReply::json(200, json!({ "workflow_runs": [] })))).collect();
    table.retain(|(k, _)| *k != list("ci-hosted-backend-core.yml") && *k != list("ci-hosted-cli-core.yml"));
    table.push((list("ci-hosted-backend-core.yml"), MockReply::json(200, backend)));
    table.push((list("ci-hosted-cli-core.yml"), MockReply::json(200, cli)));
    for id in [9003, 9002, 9001] {
        table.push((cancel_route(id), MockReply::json(cancel_status, if id == 9003 { json!({ "message": "reply-body-xyz" }) } else { json!({}) })));
    }
    server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());
    server
}

fn run(server: &MockServer, args: &[&str], pat: Option<&str>) -> crate::support::RunResult {
    let mut opts = RunOpts::default().clean_env().env("WF_GITHUB_API", &format!("http://127.0.0.1:{}", server.port));
    if let Some(pat) = pat {
        opts = opts.env("VM_PROVISION_GITHUB_TOKEN", pat);
    }
    let mut argv = vec!["vm", "ops", "cancel-run"];
    argv.extend_from_slice(args);
    run_wf(&argv, &opts)
}

fn posts(server: &MockServer) -> Vec<String> {
    server.requests().into_iter().filter(|q| q.method == "POST").map(|q| q.url).collect()
}

#[test]
fn queued_cancels_every_waiting_run_and_leaves_the_running_and_finished_ones_alone() {
    // Given a CI with one pending, one queued, one running and one finished run on dev
    let server = github(202);

    // When the verb runs with --queued and the PAT in its environment, as on agent-shell
    let r = run(&server, &["--queued"], Some(PAT));

    // Then it exits 0 and reports the two waiting runs, each accepted
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(report["ok"], json!(true));
    assert_eq!(report["cancelled"], json!([
        { "number": 1230, "id": 9003, "statusBefore": "pending", "http": 202 },
        { "number": 1229, "id": 9002, "statusBefore": "queued", "http": 202 },
    ]));

    // and only those two were cancelled, each POST carrying the PAT
    assert_eq!(posts(&server), [cancel_route(9003), cancel_route(9002)].map(|p| p.trim_start_matches("POST ").to_string()));
    assert!(server.requests().iter().all(|q| q.auth.as_deref() == Some(format!("Bearer {PAT}").as_str())));

    // and neither the token nor a reply body is in the output
    let everything = format!("{}{}", r.stdout, r.stderr);
    for secret in [PAT, "github_pat_", "title-secret-xyz", "reply-body-xyz"] {
        assert!(!everything.contains(secret), "{secret} leaked: {everything}");
    }
}

#[test]
fn run_cancels_the_named_run_by_number_and_refuses_a_finished_or_unknown_one() {
    // Given the same CI
    let server = github(202);

    // When one running run is named by its number
    let r = run(&server, &["--run", "1228"], Some(PAT));
    // Then exactly that run is cancelled
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(posts(&server), ["/repos/nuesslerm/mobile-learning-app/actions/runs/9001/cancel"]);

    // When a finished run is named, Then nothing is cancelled and the problem is said
    let finished = github(202);
    let r = run(&finished, &["--run", "1227"], Some(PAT));
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!((r.exit_code, report["ok"].clone(), posts(&finished).len()), (1, json!(false), 0));
    assert!(report["problem"].as_str().unwrap().contains("already completed"), "{report}");

    // When a number that is not among the runs is named, Then it exits 1 saying so
    let unknown = github(202);
    let r = run(&unknown, &["--run", "42"], Some(PAT));
    assert_eq!((r.exit_code, posts(&unknown).len()), (1, 0));
    assert!(r.stdout.contains("no CI run 42"), "{}", r.stdout);
}

#[test]
fn a_refused_cancel_fails_the_verb_and_a_missing_token_asks_github_nothing() {
    // Given a GitHub that refuses the cancel (the token lacks Actions: write)
    let refusing = github(403);
    // When --queued runs, Then it exits 1 with the status per run
    let r = run(&refusing, &["--queued"], Some(PAT));
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!((r.exit_code, report["ok"].clone(), report["cancelled"][0]["http"].clone()), (1, json!(false), json!(403)));

    // Given no token in the environment, When the verb runs, Then it names the key, asks GitHub nothing and exits 1
    let idle = github(202);
    let r = run(&idle, &["--queued"], None);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("VM_PROVISION_GITHUB_TOKEN is not set here") && idle.requests().is_empty(), "{}", r.stderr);

    // and neither flag, or both, is a usage error that asks GitHub nothing
    let none = github(202);
    assert_ne!(run(&none, &[], Some(PAT)).exit_code, 0);
    assert_ne!(run(&none, &["--queued", "--run", "1"], Some(PAT)).exit_code, 0);
    assert!(none.requests().is_empty());
}

#[test]
fn all_cancels_every_unfinished_run() {
    // Given waiting and running runs of the CI and a finished one
    let server = github(202);

    // When the verb runs with --all and the PAT in its environment
    let r = run(&server, &["--all"], Some(PAT));

    // Then every run that has not finished is cancelled and the finished one is left alone
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(report["ok"], json!(true));
    let mut posted = posts(&server);
    posted.sort();
    let mut want: Vec<String> = [9003, 9002, 9001].map(|id| cancel_route(id).trim_start_matches("POST ").to_string()).to_vec();
    want.sort();
    assert_eq!(posted, want);
}

#[test]
fn runs_queued_in_two_workflows_are_both_cancelled_and_the_running_ones_of_either_are_left_alone() {
    // Given a queued run in ci-hosted-mobile-core.yml and one in ci-hosted-mobile-build.yml, and a running run in ci-hosted-backend-build.yml
    let server = MockServer::start(None);
    let mut table: Vec<(String, MockReply)> = WORKFLOWS.iter().map(|w| (list(w), MockReply::json(200, json!({ "workflow_runs": [] })))).collect();
    let set = |table: &mut Vec<(String, MockReply)>, workflow: &str, runs: Value| {
        table.retain(|(k, _)| *k != list(workflow));
        table.push((list(workflow), MockReply::json(200, json!({ "workflow_runs": runs }))));
    };
    set(&mut table, "ci-hosted-mobile-core.yml", json!([{ "id": 7001, "run_number": 11, "status": "queued" }]));
    set(&mut table, "ci-hosted-mobile-build.yml", json!([{ "id": 7002, "run_number": 5, "status": "waiting" }]));
    set(&mut table, "ci-hosted-backend-build.yml", json!([{ "id": 7003, "run_number": 9, "status": "in_progress" }]));
    for id in [7001, 7002, 7003] {
        table.push((cancel_route(id), MockReply::json(202, json!({}))));
    }
    server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());

    // When the verb runs with --queued
    let r = run(&server, &["--queued"], Some(PAT));

    // Then both queued runs are cancelled, the running one is not, and every one of the six workflows was listed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let mut posted = posts(&server);
    posted.sort();
    assert_eq!(posted, ["/repos/nuesslerm/mobile-learning-app/actions/runs/7001/cancel", "/repos/nuesslerm/mobile-learning-app/actions/runs/7002/cancel"]);
    let listed = server.requests().iter().filter(|q| q.method == "GET").count();
    assert_eq!(listed, 6);
}

#[test]
fn a_run_number_shared_by_two_workflows_cancels_the_unfinished_run_not_the_finished_one() {
    // Given run number 50 finished in ci-hosted-backend-core.yml and still running in ci-hosted-cli-core.yml
    let server = MockServer::start(None);
    let mut table: Vec<(String, MockReply)> = WORKFLOWS.iter().filter(|w| !["ci-hosted-backend-core.yml", "ci-hosted-cli-core.yml"].contains(w)).map(|w| (list(w), MockReply::json(200, json!({ "workflow_runs": [] })))).collect();
    table.push((list("ci-hosted-backend-core.yml"), MockReply::json(200, json!({ "workflow_runs": [{ "id": 8001, "run_number": 50, "status": "completed" }] }))));
    table.push((list("ci-hosted-cli-core.yml"), MockReply::json(200, json!({ "workflow_runs": [{ "id": 8002, "run_number": 50, "status": "in_progress" }] }))));
    table.push((cancel_route(8002), MockReply::json(202, json!({}))));
    server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());

    // When run 50 is named
    let r = run(&server, &["--run", "50"], Some(PAT));

    // Then only the unfinished one is cancelled
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(posts(&server), ["/repos/nuesslerm/mobile-learning-app/actions/runs/8002/cancel"]);
}

#[test]
fn a_workflow_github_cannot_list_does_not_stop_the_cancel_of_the_others() {
    // Given ci-hosted-cli-core.yml answers 404 (renamed or disabled) and ci-hosted-backend-core.yml holds a queued run
    let server = MockServer::start(None);
    let mut table: Vec<(String, MockReply)> = WORKFLOWS.iter().filter(|w| !["ci-hosted-backend-core.yml", "ci-hosted-cli-core.yml"].contains(w)).map(|w| (list(w), MockReply::json(200, json!({ "workflow_runs": [] })))).collect();
    table.push((list("ci-hosted-backend-core.yml"), MockReply::json(200, json!({ "workflow_runs": [{ "id": 6001, "run_number": 3, "status": "queued" }] }))));
    table.push((list("ci-hosted-cli-core.yml"), MockReply::json(404, json!({ "message": "Not Found" }))));
    table.push((cancel_route(6001), MockReply::json(202, json!({}))));
    server.routes(&table.iter().map(|(k, v)| (k.as_str(), v.clone())).collect::<Vec<_>>());

    // When the verb runs with --all
    let r = run(&server, &["--all"], Some(PAT));

    // Then the queued run is cancelled and the exit code is 0
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(posts(&server), ["/repos/nuesslerm/mobile-learning-app/actions/runs/6001/cancel"]);
}
