//! `wf vm ops drill`: the second-door drills of plan unit D2b against the one vm-ops-api on 8080. The fake service pins the requests
//! `wf` sends (method, path, signature) and the JSON the plan's done-whens read.
use crate::support::{run_wf, MockReply, MockServer, RunOpts, RunResult};
use wf_vmops::testing::{answers, FakeVm};
use serde_json::{json, Value};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn fast(vm: &FakeVm) -> RunOpts {
    vm.opts().env("WF_VM_POLL_MS", "10").env("WF_VM_DOWN_TIMEOUT_MS", "5000").env("WF_VM_RECOVER_TIMEOUT_MS", "5000")
}

fn said(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}{}", r.stdout, r.stderr))
}

/// A `/health` whose `time` is the VM's clock at `time`, so the drill knows which watchdog lines are new.
fn health_at(time: &str) -> String {
    let mut body = answers::health(COMMIT);
    body["time"] = json!(time);
    answers::reply(200, &body)
}

fn reply(status: u16, body: &Value) -> String {
    answers::reply(status, body)
}

fn routes(server: &MockServer, table: &[(&str, u16, Value)]) {
    server.clear();
    server.queue(vec![], false);
    server.routes(&table.iter().map(|(key, status, body)| (*key, MockReply::json(*status, body.clone()))).collect::<Vec<_>>());
}

fn posts(requests: &[crate::support::MockRequest]) -> Vec<String> {
    requests.iter().filter(|r| r.method == "POST").map(|r| r.url.clone()).collect()
}

#[test]
fn a_drill_takes_the_service_down_and_names_who_brought_it_back() {
    // Given a service that drops, then answers again
    let (before, container) = ("2026-10-23T10:00:00.000Z", "vm-ops-api-ops-api-1");
    let log = |at: &str| answers::vm_ops_api_logs(&answers::watchdog_log(&[(at, container, true)]));
    let vm = FakeVm::start();
    let script = |logs: &Value| vec![health_at(before), reply(202, &answers::drill_accepted("stop", container)), "drop".to_string(), "drop".to_string(), reply(200, &answers::health(COMMIT)), reply(200, logs)];
    vm.replies_then_last(&script(&log("2026-10-23T10:00:30.000000Z")));

    // When the owner runs the stop drill
    let r = run_wf(&["vm", "ops", "drill", "stop", "--json"], &fast(&vm));

    // Then the drill was one signed POST /drill?action=stop
    assert_eq!(r.exit_code, 0, "{}{}", r.stderr, r.stdout);
    let sent = vm.requests();
    assert_eq!(posts(&sent), ["/drill?action=stop"]);
    vm.assert_signed(sent.iter().find(|q| q.method == "POST").unwrap(), "POST", "/drill?action=stop", "");
    // And the JSON the done-when reads: recovered, by the watchdog, with its line as evidence
    let v = said(&r);
    assert_eq!((&v["recovered"], &v["by"], &v["container"], &v["down"]), (&json!(true), &json!("watchdog"), &json!(container), &json!(true)));
    assert_eq!((&v["event"]["container"], &v["commit"]), (&json!(container), &json!(COMMIT)));

    // Given a recreate line from before the drill began (a freeze this time)
    let mut freeze = script(&log("2026-10-23T09:00:00.000000Z"));
    freeze[1] = reply(202, &answers::drill_accepted("freeze", container));
    vm.replies_then_last(&freeze);
    // When the owner runs the freeze drill
    let r = run_wf(&["vm", "ops", "drill", "freeze", "--json"], &fast(&vm));
    // Then it recovered, but not by the watchdog, and the request carried the freeze action
    let v = said(&r);
    assert_eq!((r.exit_code, &v["recovered"], &v["by"], &v["event"]), (0, &json!(true), &json!("other"), &Value::Null));
    assert_eq!(posts(&vm.requests()), ["/drill?action=freeze"]);
}

#[test]
fn a_drill_whose_service_stays_down_fails_with_recovered_false_and_no_flip() {
    // Given a service that never comes back, and no time to wait
    let vm = FakeVm::start();
    vm.replies_then_last(&[reply(200, &answers::health(COMMIT)), reply(202, &answers::drill_accepted("stop", "vm-ops-api-ops-api-1")), "drop".to_string()]);

    // When the owner runs the stop drill
    let r = run_wf(&["vm", "ops", "drill", "stop", "--json"], &fast(&vm).env("WF_VM_RECOVER_TIMEOUT_MS", "0"));

    // Then the drill exits 1 with recovered false and no watchdog, after the one POST /drill and nothing else
    let v = said(&r);
    assert_eq!((r.exit_code, &v["ok"], &v["recovered"], &v["by"]), (1, &json!(false), &json!(false), &Value::Null));
    assert_eq!(posts(&vm.requests()), ["/drill?action=stop"]);
    assert!(v["error"].as_str().unwrap().contains("did not recover"), "{v}");
}

#[test]
fn a_drill_that_never_takes_the_service_down_fails_and_one_that_cannot_start_sends_nothing() {
    // Given a service that keeps answering after the drill was accepted
    let vm = FakeVm::start();
    routes(&vm.server, &[("GET /health", 200, answers::health(COMMIT)), ("POST /drill?action=stop", 202, answers::drill_accepted("stop", "vm-ops-api-ops-api-1"))]);
    // When the owner runs the stop drill
    let r = run_wf(&["vm", "ops", "drill", "stop", "--json"], &fast(&vm).env("WF_VM_DOWN_TIMEOUT_MS", "100"));
    // Then it fails saying /health never stopped, after the one POST /drill and nothing else
    let v = said(&r);
    assert_eq!((r.exit_code, &v["down"]), (1, &json!(false)));
    assert!(v["error"].as_str().unwrap().contains("never stopped answering"), "{v}");
    assert_eq!(posts(&vm.requests()), ["/drill?action=stop"]);

    // Given a service that does not answer /health at all
    vm.replies_then_last(&["drop".to_string()]);
    // When the owner runs the freeze drill
    let r = run_wf(&["vm", "ops", "drill", "freeze", "--json"], &fast(&vm));
    // Then it fails before any POST
    assert_eq!((r.exit_code, posts(&vm.requests()).len()), (1, 0));
    assert!(said(&r)["error"].as_str().unwrap().contains("nothing to drill"), "{}", r.stdout);

    // Given a drill that names no action, or one that does not exist
    for args in [vec!["drill"], vec!["drill", "reboot"]] {
        vm.server.clear();
        let mut argv = vec!["vm", "ops"];
        argv.extend(&args);
        // When the owner runs it
        let r = run_wf(&argv, &fast(&vm));
        // Then it exits 1 before any request
        assert_eq!((r.exit_code, vm.requests().len()), (1, 0), "{args:?}: {}", r.stderr);
    }
}
