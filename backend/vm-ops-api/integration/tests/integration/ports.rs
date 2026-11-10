//! The two ports of the real service (plan section "Two-port vm-ops-api"): the CI port serves only what a CI job needs and
//! takes its own seed alone; the admin port serves everything and takes the admin seed alone.
use crate::{engine_calls, expect_json, post, with_engine, AGENT_SHELL};
use serde_json::json;
use std::time::Duration;
use vm_ops_api_integration::{case, Exec, CI_SEED, SEED};

const EXEC_SCRIPT: &str = "/services/agent-shell/exec-script";
const FIND: &str = "find /tmp/exec-jobs -mindepth 1 -maxdepth 1 -mtime +1 -exec rm -rf {} + 2>/dev/null || true";
const NOT_FOUND: &str = r#"{"error":"not found"}"#;
const UNAUTHORIZED: &str = r#"{"error":"unauthorized"}"#;

case!(the_ci_port_serves_the_ci_routes_and_answers_404_for_the_admin_routes_and_the_admin_port_serves_the_exec_script, {
    // Given the service with both ports open, and agent-shell ready to run a script
    let mut w = with_engine(AGENT_SHELL);
    w.with_ci_port();
    w.fake_engine()
        .exec(Exec::exact(&["sh", "-c", FIND]).answer("", 0))
        .exec(Exec::prefix(&["mkdir", "-p", "-m", "700"], r"^/tmp/exec-jobs/[0-9a-f-]{36}$").answer("", 0))
        .exec(Exec::prefix(&["setsid", "sh", "-c"], r" sh /tmp/exec-jobs/[0-9a-f-]{36} 1800$").answer("", 0));
    let svc = w.start();
    // When health and a container list are read on the CI port with the CI seed
    // Then both answer
    assert_eq!(svc.ci_signed("GET", "/health", None).status, 200);
    assert_eq!(svc.ci_signed("GET", "/services/leetcode-backend/ps", None).status, 200);
    let before = engine_calls(&svc);
    // When the exec-script and a log read, admin routes, are requested on the CI port with the CI seed
    let exec = svc.ci_signed("POST", EXEC_SCRIPT, Some("#!/bin/sh\nexit 0\n"));
    let logs = svc.ci_signed("GET", "/services/agent-shell/logs", None);
    // Then each is 404, nothing reached agent-shell, and the service logged the port and the path of each
    assert_eq!([(exec.status, exec.body.as_str()), (logs.status, logs.body.as_str())], [(404, NOT_FOUND), (404, NOT_FOUND)]);
    assert_eq!(engine_calls(&svc), before);
    // the service writes a request's log line after its answer: wait for both
    let refused_events = || svc.events().into_iter().filter(|e| e["message"] == "http.route_not_on_port").collect::<Vec<_>>();
    let mut refused = refused_events();
    for _ in 0..50 {
        if refused.len() >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
        refused = refused_events();
    }
    let line = |method: &str, path: &str| json!({ "level": "warn", "message": "http.route_not_on_port", "port": "ci", "method": method, "path": path });
    let kept = |e: &serde_json::Value| json!({ "level": e["level"], "message": e["message"], "port": e["port"], "method": e["method"], "path": e["path"] });
    assert_eq!(refused.iter().map(kept).collect::<Vec<_>>(), [line("POST", EXEC_SCRIPT), line("GET", "/services/agent-shell/logs")]);
    // When the exec-script is posted on the admin port with the admin seed
    let started = post(&svc, EXEC_SCRIPT, b"#!/bin/sh\nexit 0\n");
    // Then it runs: 202 with a job id
    let job = started.json()["jobId"].as_str().unwrap_or_default().to_string();
    expect_json(&started, 202, &json!({ "jobId": job }));
    assert!(!job.is_empty() && engine_calls(&svc).iter().any(|call| call.starts_with("PUT /containers/as1/archive")));
});

case!(a_request_signed_with_the_other_ports_seed_is_401_on_either_port, {
    // Given the service with both ports open
    let mut w = with_engine(AGENT_SHELL);
    w.with_ci_port();
    let svc = w.start();
    // When each seed signs a /health read on the other port, and the CI seed signs the exec-script on the admin port
    let ci_seed_on_admin = svc.signed_with(CI_SEED, "GET", "/health", None);
    let admin_seed_on_ci = svc.signed_on_ci_with(SEED, "GET", "/health", None);
    let ci_seed_on_admin_exec = svc.signed_with(CI_SEED, "POST", EXEC_SCRIPT, Some("#!/bin/sh\n"));
    // Then each is 401 and nothing reached agent-shell
    for reply in [&ci_seed_on_admin, &admin_seed_on_ci, &ci_seed_on_admin_exec] {
        expect_json(reply, 401, &json!({ "error": "unauthorized" }));
        assert_eq!(reply.body, UNAUTHORIZED);
    }
    assert_eq!(engine_calls(&svc), Vec::<String>::new());
    // And each port still takes its own seed
    assert_eq!(svc.ci_signed("GET", "/health", None).status, 200);
    assert_eq!(svc.signed("GET", "/health", None).status, 200);
});

case!(the_service_refuses_to_start_with_a_ci_port_and_no_ci_seed_or_the_admin_seed_as_its_ci_seed, {
    for (what, seed) in [("no CI seed", None), ("the admin seed as the CI seed", Some(SEED))] {
        // Given a world that asks for the CI port, with the seed under test
        let mut w = with_engine(AGENT_SHELL);
        w.with_ci_port();
        match seed {
            None => w.unset("VM_OPS_API_CI_AUTH_TOKEN"),
            Some(seed) => w.set("VM_OPS_API_CI_AUTH_TOKEN", seed),
        };
        // When the service starts
        let exit = w.run_to_exit(Duration::from_secs(15));
        // Then it exits 1 and says why, so a CI port is never opened on the admin seed
        assert_eq!(exit.code, Some(1), "{what}: stdout {} stderr {}", exit.stdout, exit.stderr);
        assert!(format!("{}{}", exit.stdout, exit.stderr).contains("VM_OPS_API_CI_AUTH_TOKEN"), "{what}: stdout {} stderr {}", exit.stdout, exit.stderr);
    }
});

/// Every route `core/src/domain/exposure.rs` marks AdminOnly, with a concrete path; the flow test of the router walks the table itself,
/// this crate cannot see it, so the list is written out here.
const ADMIN_ONLY: [(&str, &str); 20] = [
    ("GET", "/services/leetcode-backend/logs"),
    ("POST", "/services/leetcode-backend/retire"),
    ("POST", "/services/orca-serve/pair"),
    ("POST", "/drill"),
    ("GET", "/cooldowns"),
    ("POST", "/cooldowns"),
    ("GET", "/opencode-log"),
    ("GET", "/opencode-log/api/runs"),
    ("GET", "/opencode-log/api/runs/run-1"),
    ("POST", "/opencode-log/run-1/start"),
    ("POST", "/opencode-log/run-1/line"),
    ("POST", "/opencode-log/run-1/finish"),
    ("POST", "/services/agent-shell/exec-script"),
    ("GET", "/services/agent-shell/exec-script/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab"),
    ("POST", "/services/agent-shell/exec-script/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab/cancel"),
    ("GET", "/services/agent-shell/vm-ledger/pool-cooldowns.json"),
    ("GET", "/services/agent-shell/artifact"),
    ("POST", "/agent-shell/credentials"),
    ("GET", "/services/agent-shell/logs"),
    ("POST", "/services/agent-shell/retire"),
];

case!(every_admin_only_route_is_404_on_the_ci_port_with_the_ci_seed_and_reaches_nothing, {
    // Given the service with both ports open
    let mut w = with_engine(AGENT_SHELL);
    w.with_ci_port();
    let svc = w.start();
    let before = engine_calls(&svc);
    // When each is requested on the CI port, signed with the CI seed
    let answers: Vec<(&str, &str, u16)> = ADMIN_ONLY.iter().map(|(method, path)| (*method, *path, svc.ci_signed(method, path, Some("{}")).status)).collect();
    // Then every one is the plain 404 and no engine call was made
    assert_eq!(answers.iter().filter(|(_, _, status)| *status != 404).collect::<Vec<_>>(), Vec::<&(&str, &str, u16)>::new());
    assert_eq!(engine_calls(&svc), before);
    // And a CI-signed request for such a route on the admin port is refused as unauthorized, not served (the public opencode-log reads
    // take no credentials at all there)
    for (method, path) in ADMIN_ONLY.into_iter().filter(|(method, path)| !(*method == "GET" && path.starts_with("/opencode-log"))) {
        let reply = svc.signed_with(CI_SEED, method, path, Some("{}"));
        assert_eq!((reply.status, reply.body.as_str()), (401, UNAUTHORIZED), "{method} {path}");
    }
});
