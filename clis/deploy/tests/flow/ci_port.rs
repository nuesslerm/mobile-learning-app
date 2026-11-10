//! `wf vm ops ci-port`: the deployment check that a CI job's port answers /health with the CI seed and refuses an admin-only route.
//! vm-ops-api is a fake behind a fake tailnet.
use crate::support::run_wf;
use serde_json::{json, Value};
use wf_vmops::testing::{answers, FakeVm, CI_VM_SEED};

const LOGS: &str = "/services/agent-shell/logs?tail=1";

fn as_ci_job(vm: &FakeVm) -> crate::support::RunOpts {
    vm.opts().env("WF_VM_CI_PORT", "8081").env("VM_OPS_API_CI_AUTH_TOKEN", CI_VM_SEED)
}

#[test]
fn a_ci_port_that_answers_health_and_refuses_the_admin_route_passes_with_both_calls_signed_with_the_ci_seed() {
    // Given a CI job whose port serves /health and answers 404 for the logs route
    let vm = FakeVm::start();
    vm.replies(&[answers::reply(200, &json!({ "ok": true })), answers::reply(404, &json!({ "error": "not found" }))]);

    // When the check runs
    let r = run_wf(&["vm", "ops", "ci-port"], &as_ci_job(&vm));

    // Then it passes, both calls went to the CI port with the CI seed, and the line names the statuses
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let line: Value = serde_json::from_str(r.stdout.trim()).expect("one json line");
    assert_eq!((line["ok"].clone(), line["port"].clone()), (json!(true), json!(8081)));
    assert_eq!(line["checks"][0]["status"], json!(200));
    assert_eq!(line["checks"][1]["status"], json!(404));
    assert_eq!(vm.tailnet.destinations(), ["vm-ops-api-main:8081", "vm-ops-api-main:8081"]);
    let sent = vm.requests();
    vm.assert_signed_with(CI_VM_SEED, &sent[0], "GET", "/health", "");
    vm.assert_signed_with(CI_VM_SEED, &sent[1], "GET", LOGS, "");
}

#[test]
fn a_port_that_serves_the_admin_route_or_refuses_health_fails_the_check() {
    // Given a CI port that serves the logs route (it should not)
    let vm = FakeVm::start();
    vm.replies(&[answers::reply(200, &json!({ "ok": true })), answers::reply(200, &json!({ "logs": {} }))]);

    // When the check runs, Then it fails and names the route that answered 200
    let r = run_wf(&["vm", "ops", "ci-port"], &as_ci_job(&vm));
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let line: Value = serde_json::from_str(r.stdout.trim()).expect("one json line");
    assert_eq!((line["ok"].clone(), line["checks"][1]["ok"].clone(), line["checks"][1]["status"].clone()), (json!(false), json!(false), json!(200)));

    // Given a CI port that refuses the CI seed (401 on /health)
    vm.replies(&[answers::reply(401, &json!({ "error": "unauthorized" })), answers::reply(401, &json!({ "error": "unauthorized" }))]);

    // When the check runs, Then it fails on both routes
    let r = run_wf(&["vm", "ops", "ci-port"], &as_ci_job(&vm));
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let line: Value = serde_json::from_str(r.stdout.trim()).expect("one json line");
    assert_eq!((line["checks"][0]["ok"].clone(), line["checks"][1]["ok"].clone()), (json!(false), json!(false)));
}

#[test]
fn a_process_without_a_ci_port_is_refused_before_any_call() {
    // Given a process with no WF_VM_CI_PORT (agent-shell, the sandbox)
    let vm = FakeVm::start();
    vm.replies(&[answers::reply(200, &json!({ "ok": true }))]);

    // When the check runs, Then it says so and sends nothing
    let r = run_wf(&["vm", "ops", "ci-port"], &vm.opts());
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("WF_VM_CI_PORT is not set"), "{}", r.stderr);
    assert!(vm.requests().is_empty());
}
