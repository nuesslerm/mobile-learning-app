//! `wf vm ops runner-check`: read-only proof, from inside a CI job on the self-hosted runner, that the job reaches the builder sidecar and
//! nothing of the host. Prints names and booleans, never a value.
use serde_json::{json, Value};
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::process::Command;
use std::time::Duration;

/// The two job tokens that are job env on purpose; any other BWS_ name in PID 1's environment is a leak.
const JOB_TOKENS: [&str; 2] = ["BWS_CI_OPEN_ACCESS_TOKEN", "BWS_CI_DEPLOY_ACCESS_TOKEN"];
/// Config, not a credential.
const CONFIG_NAMES: [&str; 1] = ["BWS_SERVER_URL"];
fn forbidden(name: &str) -> bool {
    name == "ACCESS_TOKEN"
        || name == "VM_PROVISION_GITHUB_TOKEN"
        || (name.starts_with("BWS_") && !JOB_TOKENS.contains(&name) && !CONFIG_NAMES.contains(&name))
}

/// The names (never the values) of PID 1's environment, or None when it cannot be read.
fn pid1_names(proc_root: &str) -> Option<Vec<String>> {
    let mut raw = Vec::new();
    std::fs::File::open(format!("{proc_root}/1/environ")).ok()?.read_to_end(&mut raw).ok()?;
    Some(raw.split(|b| *b == 0).filter(|entry| !entry.is_empty()).map(|entry| String::from_utf8_lossy(entry.split(|b| *b == b'=').next().unwrap_or_default()).into_owned()).collect())
}

/// The daemon's name through DOCKER_HOST, None when it is unset or the daemon does not answer.
fn docker_server_name() -> Option<String> {
    std::env::var("DOCKER_HOST").ok().filter(|h| !h.is_empty())?;
    let out = Command::new("docker").args(["info", "--format", "{{.Name}}"]).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|name| !name.is_empty())
}

/// Whether nothing answers at `addr` (refused, unreachable or timed out), which is what a runner job must see.
fn unreachable(addr: &str) -> bool {
    let Some(target) = addr.to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return true;
    };
    TcpStream::connect_timeout(&target, Duration::from_secs(3)).is_err()
}

/// What a job sees at `addr`: "reachable" (a connection opened: a failure), "unresolved" (the name did not resolve, so nothing was proven) or
/// "unreachable" (refused, unreachable or timed out: what the tailnet policy must give).
fn reach_state(addr: &str) -> &'static str {
    let Some(target) = addr.to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return "unresolved";
    };
    if TcpStream::connect_timeout(&target, Duration::from_secs(3)).is_ok() {
        "reachable"
    } else {
        "unreachable"
    }
}

/// `proc_root` and `docker_sock` default to `/proc` and `/var/run/docker.sock`; the tests inject others.
pub fn run(proc_root: Option<&str>, docker_sock: Option<&str>) -> Option<i32> {
    let proc_root = proc_root.unwrap_or("/proc");
    let docker_sock = docker_sock.unwrap_or("/var/run/docker.sock");
    let server = docker_server_name();
    let names = pid1_names(proc_root);
    let offending: Vec<String> = names.iter().flatten().filter(|n| forbidden(n)).cloned().collect();
    let dind = std::env::var("AGENT_SHELL_DIND_ADDR").ok().filter(|a| !a.trim().is_empty());
    let mut report = json!({
        "docker_host_answers": server.is_some(),
        "docker_server": server,
        "host_docker_sock_absent": !std::path::Path::new(docker_sock).exists(),
        "pid1_env_readable": names.is_some(),
        "pid1_env_clean": names.is_some() && offending.is_empty(),
    });
    if !offending.is_empty() {
        report["pid1_offending_names"] = json!(offending);
    }
    let dind_ok = match &dind {
        Some(addr) => {
            let ok = unreachable(addr);
            report["agent_shell_dind_unreachable"] = json!(ok);
            ok
        }
        None => {
            report["agent_shell_dind_unreachable"] = Value::Null;
            report["agent_shell_dind_probe"] = json!("skipped: no agent-shell dind address configured (AGENT_SHELL_DIND_ADDR is unset)");
            true
        }
    };
    // Addresses a runner job must NOT reach (the admin port, agent-shell's ssh): a name that does not resolve proves nothing and fails.
    let forbidden_addrs: Vec<String> = std::env::var("RUNNER_CHECK_MUST_NOT_REACH").unwrap_or_default().split(',').map(str::trim).filter(|a| !a.is_empty()).map(String::from).collect();
    let probed: Vec<(String, &'static str)> = forbidden_addrs.iter().map(|a| (a.clone(), reach_state(a))).collect();
    if !probed.is_empty() {
        report["must_not_reach"] = json!(probed.iter().map(|(a, state)| json!({ "addr": a, "state": state })).collect::<Vec<_>>());
    }
    let denied = probed.iter().all(|(_, state)| *state == "unreachable");
    let ok = report["docker_host_answers"] == json!(true) && report["host_docker_sock_absent"] == json!(true) && report["pid1_env_clean"] == json!(true) && dind_ok && denied;
    report["ok"] = json!(ok);
    println!("{report}");
    Some(if ok { 0 } else { 1 })
}
