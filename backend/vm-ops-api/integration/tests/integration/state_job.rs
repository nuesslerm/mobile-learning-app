//! `POST /state/{verb}` and `GET /state/jobs/{job}` on the CI port: the fixed backup or verify script is landed and started on agent-shell like an exec
//! script, and only a job this service started can be read back.
use crate::{expect_json, with_engine, AGENT_SHELL};
use serde_json::json;
use vm_ops_api_integration::{case, Exec};

const FIND: &str = "find /tmp/exec-jobs -mindepth 1 -maxdepth 1 -mtime +1 -exec rm -rf {} + 2>/dev/null || true";
const STRANGER: &str = "0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab";

case!(a_state_job_starts_the_fixed_script_on_agent_shell_and_reads_back_only_its_own_jobs, ["POST /state/:verb", "GET /state/jobs/<jobId>"], {
    // Given agent-shell with its CI port open, where the clean-up, the job dir and the detached start each exit 0
    let mut w = with_engine(AGENT_SHELL);
    w.with_ci_port();
    w.fake_engine()
        .exec(Exec::exact(&["sh", "-c", FIND]).answer("", 0))
        .exec(Exec::prefix(&["mkdir", "-p", "-m", "700"], r"^/tmp/exec-jobs/[0-9a-f-]{36}$").answer("", 0))
        .exec(Exec::prefix(&["setsid", "sh", "-c"], r" sh /tmp/exec-jobs/[0-9a-f-]{36} 1800$").answer("", 0));
    let svc = w.start();
    // When the CI port is posted a backup
    let started = svc.ci_signed("POST", "/state/backup", Some(""));
    // Then it is 202 with a job id, and the archive landed in the job dir holds the fixed backup script
    let job = started.json()["jobId"].as_str().unwrap_or_default().to_string();
    expect_json(&started, 202, &json!({ "jobId": job }));
    let archives = svc.world().fake_engine().archives();
    let landed = String::from_utf8_lossy(&archives[0].entries[0].bytes).to_string();
    assert!(landed.contains("vm state backup --local --apps-file"), "{landed}");
    // When a verb that is no state job is posted, Then it is 400 and the Engine is asked nothing more
    let before = svc.world().fake_engine().archives().len();
    expect_json(&svc.ci_signed("POST", "/state/restore", Some("")), 400, &json!({ "error": "the verb must be backup or verify" }));
    assert_eq!(svc.world().fake_engine().archives().len(), before);
    // When a job this service did not start is read, Then it is 404
    expect_json(&svc.ci_signed("GET", &format!("/state/jobs/{STRANGER}"), None), 404, &json!({ "error": "no such state job" }));
    // When the admin port is asked, Then the route is not served there
    assert_ne!(svc.signed("POST", "/state/backup", Some("")).status, 202);
});
