//! `wf vm ops runner-check`: the proof, from inside a CI job, that the job has the builder sidecar and nothing of the host. The docker CLI
//! is a stub, PID 1's environment a file under a temp /proc-like directory and the host docker socket a path under the same directory.
use crate::support::{create_stub, run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::Path;

struct Job {
    dir: tempfile::TempDir,
}

impl Job {
    /// A runner whose daemon answers as `builder-sidecar`, PID 1 holds `pid1` and no host socket exists.
    fn new(pid1: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("proc/1")).unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        std::fs::write(dir.path().join("proc/1/environ"), pid1.iter().map(|e| format!("{e}\0")).collect::<String>()).unwrap();
        create_stub(&dir.path().join("bin/docker"), "case \"$*\" in \"info --format {{.Name}}\") echo builder-sidecar;; *) exit 2;; esac");
        Self { dir }
    }

    fn sock(&self) -> String {
        self.dir.path().join("docker.sock").display().to_string()
    }

    fn run(&self, docker_host: Option<&str>, extra: &[(&str, &str)]) -> RunResult {
        let root = self.dir.path().join("proc");
        let mut opts: RunOpts = RunOpts::default().bin_dir(&self.dir.path().join("bin")).env("DOCKER_HOST", docker_host.unwrap_or(""));
        for (k, v) in extra {
            opts = opts.env(k, v);
        }
        run_wf(&["vm", "ops", "runner-check", "--json", "--proc-root", root.to_str().unwrap(), "--docker-sock-path", &self.sock()], &opts)
    }
}

fn line(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.trim()).unwrap_or_else(|_| panic!("one json line: {}{}", r.stdout, r.stderr))
}

const CLEAN: [&str; 4] = ["PATH=/usr/bin", "BWS_CI_OPEN_ACCESS_TOKEN=job-token-1", "BWS_CI_DEPLOY_ACCESS_TOKEN=job-token-2", "HOME=/root"];

#[test]
fn a_job_with_the_sidecar_no_host_socket_and_a_clean_pid1_passes_and_the_probe_without_an_address_is_said_to_be_skipped() {
    // Given a runner job as intended
    let job = Job::new(&CLEAN);
    // When the check runs
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[]);
    // Then it passes and the line names the daemon and says the dind probe was skipped
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let l = line(&r);
    assert_eq!((l["ok"].clone(), l["docker_host_answers"].clone(), l["docker_server"].clone()), (json!(true), json!(true), json!("builder-sidecar")));
    assert_eq!((l["host_docker_sock_absent"].clone(), l["pid1_env_clean"].clone(), l["agent_shell_dind_unreachable"].clone()), (json!(true), json!(true), Value::Null));
    assert!(l["agent_shell_dind_probe"].as_str().unwrap().starts_with("skipped"), "{l}");
}

#[test]
fn a_job_without_docker_host_or_with_a_host_socket_fails() {
    // Given a job with no DOCKER_HOST
    let job = Job::new(&CLEAN);
    // When the check runs, Then it fails on the daemon
    let r = job.run(None, &[]);
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(line(&r)["docker_host_answers"], json!(false));

    // Given a job where the host docker socket exists
    std::fs::write(job.sock(), "").unwrap();
    assert!(Path::new(&job.sock()).exists());
    // When the check runs, Then it fails on the socket
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[]);
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(line(&r)["host_docker_sock_absent"], json!(false));
}

#[test]
fn a_pid1_environment_holding_a_vault_or_provision_variable_fails_naming_only_the_variable_and_never_its_value() {
    for leak in ["ACCESS_TOKEN=leaked-value-1", "BWS_ACCESS_TOKEN=leaked-value-2", "VM_PROVISION_GITHUB_TOKEN=leaked-value-4", "BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN=leaked-value-5"] {
        // Given a PID 1 that holds one forbidden variable next to the two job tokens
        let mut env = CLEAN.to_vec();
        env.push(leak);
        let job = Job::new(&env);
        // When the check runs
        let r = job.run(Some("tcp://127.0.0.1:2376"), &[]);
        // Then it fails, names the variable and prints no value
        assert_eq!(r.exit_code, 1, "{leak}: {}{}", r.stdout, r.stderr);
        let l = line(&r);
        let name = leak.split('=').next().unwrap();
        assert_eq!((l["pid1_env_clean"].clone(), l["pid1_offending_names"].clone()), (json!(false), json!([name])), "{leak}");
        assert!(!r.stdout.contains("leaked-value") && !r.stderr.contains("leaked-value"), "{leak}");
    }
}

#[test]
fn the_vault_server_url_passes_next_to_the_job_tokens() {
    // Given a PID 1 that holds the vault server URL next to the job tokens
    let mut env = CLEAN.to_vec();
    env.push("BWS_SERVER_URL=https://vault.example");
    let job = Job::new(&env);
    // When the check runs
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[]);
    // Then it passes
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(line(&r)["pid1_env_clean"], json!(true));
}

#[test]
fn addresses_a_job_must_not_reach_pass_only_when_every_one_is_refused_and_an_unresolved_name_fails() {
    // Given a listener that accepts (what the admin port would be without the policy) and a port nothing listens on
    let open = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let open_addr = open.local_addr().unwrap().to_string();
    let closed = { let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap().to_string() };
    let job = Job::new(&CLEAN);

    // When the check must-not-reach list holds only the closed port
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[("RUNNER_CHECK_MUST_NOT_REACH", &closed)]);
    // Then it passes and says unreachable
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(line(&r)["must_not_reach"], json!([{ "addr": closed, "state": "unreachable" }]));

    // When the list also holds the open listener
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[("RUNNER_CHECK_MUST_NOT_REACH", &format!("{closed},{open_addr}"))]);
    // Then it fails and names that address as reachable
    assert_eq!(r.exit_code, 1, "{}", r.stdout);
    assert_eq!(line(&r)["must_not_reach"][1], json!({ "addr": open_addr, "state": "reachable" }));

    // When a name does not resolve, Then nothing was proven and it fails
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[("RUNNER_CHECK_MUST_NOT_REACH", "no-such-host.invalid:8080")]);
    assert_eq!(r.exit_code, 1, "{}", r.stdout);
    assert_eq!(line(&r)["must_not_reach"], json!([{ "addr": "no-such-host.invalid:8080", "state": "unresolved" }]));
}

#[test]
fn an_unreadable_pid1_environment_fails_because_nothing_was_proven() {
    // Given a proc root with no PID 1 environment
    let job = Job::new(&CLEAN);
    std::fs::remove_file(job.dir.path().join("proc/1/environ")).unwrap();
    // When the check runs, Then it fails
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[]);
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!((line(&r)["pid1_env_readable"].clone(), line(&r)["pid1_env_clean"].clone()), (json!(false), json!(false)));
}

#[test]
fn an_agent_shell_dind_address_that_accepts_a_connection_fails_and_one_that_refuses_passes() {
    // Given an address something listens on
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let open = listener.local_addr().unwrap().to_string();
    let job = Job::new(&CLEAN);
    // When the check runs, Then it fails: the job reaches agent-shell's daemon
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[("AGENT_SHELL_DIND_ADDR", &open)]);
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(line(&r)["agent_shell_dind_unreachable"], json!(false));

    // Given the listener is gone, so the connect is refused
    drop(listener);
    // When the check runs, Then it passes
    let r = job.run(Some("tcp://127.0.0.1:2376"), &[("AGENT_SHELL_DIND_ADDR", &open)]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(line(&r)["agent_shell_dind_unreachable"], json!(true));
}
