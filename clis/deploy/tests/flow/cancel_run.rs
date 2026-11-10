//! `wf vm ops cancel-run`, run on agent-shell (which holds the PAT): one named CI run, or every run still waiting for a runner, cancelled
//! through a fake GitHub; the token and every reply body stay out of the output.
use crate::support::{run_wf, MockReply, MockServer, RunOpts};
use serde_json::{json, Value};

const PAT: &str = "github_pat_11AAAAAAA0zzzzzzzzzzzz_secretsecretsecret";
const LIST: &str = "GET /repos/nuesslerm/mobile-learning-app/actions/workflows/deploy.yml/runs?branch=dev&per_page=100";

fn cancel_route(id: u64) -> String {
    format!("POST /repos/nuesslerm/mobile-learning-app/actions/runs/{id}/cancel")
}

fn github(cancel_status: u16) -> MockServer {
    let server = MockServer::start(None);
    let runs = json!({ "workflow_runs": [
        { "id": 9003, "run_number": 1230, "status": "pending", "display_title": "title-secret-xyz" },
        { "id": 9002, "run_number": 1229, "status": "queued" },
        { "id": 9001, "run_number": 1228, "status": "in_progress" },
        { "id": 9000, "run_number": 1227, "status": "completed" },
    ] });
    server.routes(&[
        (LIST, MockReply::json(200, runs)),
        (&cancel_route(9003), MockReply::json(cancel_status, json!({ "message": "reply-body-xyz" }))),
        (&cancel_route(9002), MockReply::json(cancel_status, json!({}))),
        (&cancel_route(9001), MockReply::json(cancel_status, json!({}))),
    ]);
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
    assert!(r.stderr.contains("VM_PROVISION_GITHUB_TOKEN is not set") && idle.requests().is_empty(), "{}", r.stderr);

    // and neither flag, or both, is a usage error that asks GitHub nothing
    let none = github(202);
    assert_ne!(run(&none, &[], Some(PAT)).exit_code, 0);
    assert_ne!(run(&none, &["--queued", "--run", "1"], Some(PAT)).exit_code, 0);
    assert!(none.requests().is_empty());
}
