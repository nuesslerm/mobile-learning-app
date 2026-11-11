//! The device lab on the CI port: three fixed verbs and the status of the jobs they start. Given/When/Then, through the routers `main`
//! serves.
use crate::harness::{world_with, Doubles};
use vm_ops_api_core::application::ports::{ContainersError, ExecResult};
use vm_ops_api_core::domain::agent_shell::{EXIT_MARKER, JOBS_CLEANUP, JOB_STATUS_SCRIPT};
use vm_ops_api_core::domain::containers::DockerContainer;
use vm_ops_api_core::testing::{ContainersCall, FakeContainers};

const JOB: &str = "00000000-0000-4000-8000-000000000001";
const FOREIGN_JOB: &str = "0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab";

fn agent_shell() -> DockerContainer {
    DockerContainer { id: "as1".to_string(), names: Some(vec!["/agent-shell".to_string()]), state: Some("running".to_string()), ..Default::default() }
}

fn exits(code: i64, output: &str) -> Result<ExecResult, ContainersError> {
    Ok(ExecResult::attached(Some(code), output.to_string()))
}

fn lab_world() -> crate::harness::World {
    let containers = FakeContainers::new()
        .with_app_containers("agent-shell", vec![agent_shell()])
        .with_exec(&["sh", "-c", JOBS_CLEANUP], vec![exits(0, "")])
        .with_exec(&["mkdir"], vec![exits(0, "")])
        .with_exec(&["setsid"], vec![exits(0, "")])
        .with_exec(&["sh", "-c", JOB_STATUS_SCRIPT], vec![exits(0, &format!("redroid up{EXIT_MARKER}0\n"))]);
    world_with(false, Doubles { containers, ..Doubles::default() })
}

fn landed_script(world: &crate::harness::World) -> String {
    let tar = world.containers.calls().into_iter().find_map(|call| match call {
        ContainersCall::PutFile { tar, .. } => Some(tar),
        _ => None,
    });
    String::from_utf8_lossy(&tar.unwrap_or_default()).to_string()
}

#[tokio::test]
async fn given_the_ci_port_when_the_device_is_started_with_expo_go_then_a_job_runs_wf_s_own_verb_and_its_status_is_readable_by_that_job_only() {
    // Given agent-shell, whose job exits 0
    let world = lab_world();

    // When a CI-signed request starts the device with Expo Go, and then reads that job, a job the lab never started, and the job on the admin path
    let started = world.ci_signed("POST", "/android/device/start?expoGo=1", b"").await;
    let status = world.ci_signed("GET", &format!("/android/device/jobs/{JOB}"), b"").await;
    let foreign = world.ci_signed("GET", &format!("/android/device/jobs/{FOREIGN_JOB}"), b"").await;

    // Then the start is 202 with the job id, the job reads as exited 0 with its output, and the foreign job is a 404 that never reached agent-shell's status
    assert_eq!((started.status, started.body.as_str()), (202, format!(r#"{{"jobId":"{JOB}"}}"#).as_str()), "{}", started.body);
    assert_eq!(status.body, format!(r#"{{"jobId":"{JOB}","running":false,"exitCode":0,"output":"redroid up"}}"#));
    assert_eq!((foreign.status, foreign.body.as_str()), (404, r#"{"error":"not a device-lab job"}"#));
    // And the script that landed in the job is wf's fixed lifecycle verb, whatever the request carried
    assert!(landed_script(&world).contains("exec wf android start-device --local --expo-go"), "{}", landed_script(&world));
}

#[tokio::test]
async fn given_a_request_that_tries_to_shape_the_script_when_it_starts_the_device_then_the_job_is_the_fixed_one() {
    // Given agent-shell
    let world = lab_world();

    // When the start carries a body and a query that look like shell
    let started = world.ci_signed("POST", "/android/device/start?expoGo=1;rm%20-rf%20/", b"id > /tmp/x").await;

    // Then the job is the plain start verb (only expoGo=1 turns Expo Go on) with nothing of the request in it
    assert_eq!(started.status, 202, "{}", started.body);
    let script = landed_script(&world);
    assert!(script.contains("exec wf android start-device --local\n") && !script.contains("rm -rf") && !script.contains("id > /tmp/x"), "{script}");
}

#[tokio::test]
async fn given_the_admin_port_when_a_lab_route_is_called_there_then_it_is_a_404_and_agent_shell_is_never_asked() {
    // Given agent-shell
    let world = lab_world();

    // When the lab start is posted on the admin port with the admin seed
    let reply = world.signed("POST", "/android/device/start", b"").await;

    // Then it is a 404 and nothing ran
    assert_eq!(reply.status, 404, "{}", reply.body);
    assert_eq!(world.containers.calls(), []);
}
