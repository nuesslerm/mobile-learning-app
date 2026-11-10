//! The write routes through the router: agent-shell's job start, status and cancel and its credential files, the hosted cooldown
//! ledger and the opencode run log. Given/When/Then.
use crate::harness::{world, world_with, Doubles, NOON};
use serde_json::{json, Value};
use vm_ops_api_core::application::ports::{ContainersError, Event, ExecResult};
use vm_ops_api_core::application::scripts::agent_shell_job::AGENT_SHELL_JOB_CANCEL;
use vm_ops_api_core::domain::agent_shell::{EXIT_MARKER, JOBS_CLEANUP, JOB_STATUS_SCRIPT};
use vm_ops_api_core::domain::containers::DockerContainer;
use vm_ops_api_core::testing::{ContainersCall, FakeContainers};

const JOB: &str = "00000000-0000-4000-8000-000000000001";

fn agent_shell() -> DockerContainer {
    DockerContainer { id: "as1".to_string(), names: Some(vec!["/agent-shell".to_string()]), state: Some("running".to_string()), ..Default::default() }
}

fn exits(code: i64, output: &str) -> Result<ExecResult, ContainersError> {
    Ok(ExecResult::attached(Some(code), output.to_string()))
}

#[tokio::test]
async fn given_agent_shell_when_a_script_is_started_read_and_cancelled_then_each_step_is_the_reply_node_gives_and_the_job_runs_detached() {
    // Given agent-shell, whose job reads as running once and then as exited 0
    let containers = FakeContainers::new()
        .with_app_containers("agent-shell", vec![agent_shell()])
        .with_exec(&["sh", "-c", JOBS_CLEANUP], vec![exits(0, "")])
        .with_exec(&["mkdir"], vec![exits(0, "")])
        .with_exec(&["setsid"], vec![exits(0, "")])
        .with_exec(&["sh", "-c", JOB_STATUS_SCRIPT], vec![exits(0, &format!("step 1{EXIT_MARKER}")), exits(0, &format!("step 1\nstep 2{EXIT_MARKER}0\n"))])
        .with_exec(&["sh", "-c", AGENT_SHELL_JOB_CANCEL], vec![exits(1, "")]);
    let world = world_with(false, Doubles { containers, ..Doubles::default() });
    // When a script is started, read twice and cancelled
    let started = world.signed("POST", "/services/agent-shell/exec-script", b"echo hi\n").await;
    let path = format!("/services/agent-shell/exec-script/{JOB}");
    let (running, done) = (world.signed("GET", &path, b"").await, world.signed("GET", &path, b"").await);
    let cancelled = world.signed("POST", &format!("{path}/cancel"), b"").await;
    // Then the start is 202 with the job id, the reads are running and then exited 0 with the output, and the cancel is ok
    assert_eq!((started.status, started.body.as_str()), (202, format!(r#"{{"jobId":"{JOB}"}}"#).as_str()));
    assert_eq!(running.body, format!(r#"{{"jobId":"{JOB}","running":true,"exitCode":null,"output":"step 1"}}"#));
    assert_eq!(done.body, format!(r#"{{"jobId":"{JOB}","running":false,"exitCode":0,"output":"step 1\nstep 2"}}"#));
    assert_eq!((cancelled.status, cancelled.body.as_str()), (200, r#"{"ok":true}"#));
    // And the job's wrapper was started detached after its script landed by tar in the job's dir
    let calls = world.containers.calls();
    let put = calls.iter().position(|call| matches!(call, ContainersCall::PutFile { target_dir, .. } if *target_dir == format!("/tmp/exec-jobs/{JOB}")));
    let detached = calls.iter().position(|call| matches!(call, ContainersCall::Exec { options, .. } if options.detach));
    assert!(put.is_some() && detached > put, "{calls:?}");
    // And a job id that is no UUID is 400 before agent-shell is asked
    let before = world.containers.calls().len();
    let refused = world.signed("GET", "/services/agent-shell/exec-script/not-a-uuid", b"").await;
    assert_eq!((refused.status, refused.body.as_str(), world.containers.calls().len()), (400, r#"{"error":"jobId must be a UUID"}"#, before));
}

#[tokio::test]
async fn given_a_service_whose_only_container_is_exited_when_a_script_is_started_then_it_is_503_and_nothing_is_exec_d() {
    // Given agent-shell whose only container is exited
    let exited = DockerContainer { state: Some("exited".to_string()), ..agent_shell() };
    let world = world_with(false, Doubles { containers: FakeContainers::new().with_app_containers("agent-shell", vec![exited]), ..Doubles::default() });
    // When a script is started
    let started = world.signed("POST", "/services/agent-shell/exec-script", b"echo hi\n").await;
    // Then it is 503 not running, and the Engine was only asked to list
    assert_eq!((started.status, started.body.as_str()), (503, r#"{"error":"agent-shell container is not running"}"#));
    assert_eq!(world.containers.calls(), [ContainersCall::ListByApp("agent-shell".to_string())]);
}

#[tokio::test]
async fn given_a_credential_post_when_it_is_written_then_the_file_lands_in_the_tools_dir_and_a_bad_post_never_asks_agent_shell() {
    // Given agent-shell, whose mkdir and chown exit 0
    let containers = FakeContainers::new().with_app_containers("agent-shell", vec![agent_shell()]).with_exec(&["mkdir"], vec![exits(0, "")]).with_exec(&["chown"], vec![exits(0, "")]);
    let world = world_with(false, Doubles { containers, ..Doubles::default() });
    // When a bad post and then claude's credentials are made
    let bad = world.signed("POST", "/agent-shell/credentials", br#"{"tool":"vim","content":"aGk="}"#).await;
    let asked = world.containers.calls().len();
    let written = world.signed("POST", "/agent-shell/credentials", br#"{"tool":"claude","content":"aGk="}"#).await;
    // Then the bad one is 400 with the known tools and asked nothing, and the good one is 200 with the file's path and size
    assert_eq!((bad.status, asked), (400, 0));
    assert_eq!(bad.body, r#"{"error":"unknown tool","knownTools":["codex","claude","gcloud","gemini","opencode"]}"#);
    assert_eq!(written.body, r#"{"ok":true,"tool":"claude","path":"/workspace/home/.claude/.credentials.json","bytes":2}"#);
    // And the file went into that dir by tar and the event names the tool
    let put = world.containers.calls().into_iter().find_map(|call| if let ContainersCall::PutFile { target_dir, .. } = call { Some(target_dir) } else { None });
    assert_eq!(put.as_deref(), Some("/workspace/home/.claude"));
    assert_eq!(world.log.events(), [Event::info("agent_shell_credentials.written").with("tool", "claude")]);
}

#[tokio::test]
async fn given_the_cooldown_ledger_when_posted_at_its_version_then_it_is_stored_and_a_second_post_at_the_old_version_is_409_stale() {
    // Given the service, whose ledger is empty at version 0
    let world = world(false);
    // When a ledger is posted at version 0, posted again at version 0, posted malformed, and read
    let stored = world.signed("POST", "/cooldowns", br#"{"ledger":{"minimax":{"until":1700000000000}},"version":0}"#).await;
    let stale = world.signed("POST", "/cooldowns", br#"{"ledger":{},"version":0}"#).await;
    let bad = world.signed("POST", "/cooldowns", br#"{"ledger":[],"version":0}"#).await;
    let read = world.signed("GET", "/cooldowns", b"").await;
    // Then the first is 200 with version 1, the second 409 stale with version 1, the malformed one 400, and the read has the first ledger
    assert_eq!((stored.status, stored.body.as_str()), (200, r#"{"version":1}"#));
    assert_eq!((stale.status, stale.body.as_str()), (409, r#"{"error":"stale","version":1}"#));
    assert_eq!((bad.status, bad.body.as_str()), (400, r#"{"error":"expected { ledger, version }"}"#));
    assert_eq!(read.body, r#"{"ledger":{"minimax":{"until":1700000000000}},"version":1}"#);
}

#[tokio::test]
async fn given_a_run_when_started_given_lines_and_finished_then_the_public_routes_read_it_back_finished_with_its_lines() {
    // Given the service at noon
    let world = world(false);
    // When a run is started, given two lines (one of them no string) and finished 5 s later, and a body that is no JSON is posted
    let started = world.signed("POST", "/opencode-log/run-1/start", br#"{"prompt":"Fix it","caller":"implementer","model":"m"}"#).await;
    let lines = world.signed("POST", "/opencode-log/run-1/line", br#"{"lines":["a",5,"b"]}"#).await;
    world.clock.set(NOON + 5_000);
    let finished = world.signed("POST", "/opencode-log/run-1/finish", br#"{"ok":true,"tokens":{"total":3,"input":2,"output":1},"cost":0.5,"durationMs":5000}"#).await;
    let bad = world.signed("POST", "/opencode-log/run-1/line", b"{oops").await;
    let detail = world.send("GET", "/opencode-log/api/runs/run-1", &[], b"").await;
    // Then each write is 200 ok, the bad one is 400, and the run reads back finished with its model, agent and string lines
    for reply in [&started, &lines, &finished] {
        assert_eq!((reply.status, reply.body.as_str()), (200, r#"{"ok":true}"#));
    }
    assert_eq!((bad.status, bad.body.as_str()), (400, r#"{"error":"invalid JSON body"}"#));
    let run: Value = serde_json::from_str(&detail.body).expect("json");
    assert_eq!((run["status"].as_str(), run["agent"].as_str(), run["model"].as_str()), (Some("finished"), Some("implementer"), Some("m")));
    assert_eq!((run["startedAt"].as_str(), run["finishedAt"].as_str()), (Some("2026-10-04T12:00:00.000Z"), Some("2026-10-04T12:00:05.000Z")));
    assert_eq!(run["lines"], json!(["a", "b"]));
}
