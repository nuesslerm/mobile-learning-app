//! Every generated script against its snapshot (`core/tests/snapshots`), and the helper-command static check.
use super::agent_shell_job::{AGENT_SHELL_JOB_CANCEL, AGENT_SHELL_JOB_STATUS, AGENT_SHELL_JOB_WRAPPER};
use super::redeploy::{append_history_args, app_redeploy_sibling_args, app_smoke_sibling_args, pair_sibling_args, retire_sibling_args, self_redeploy_sibling_args, self_smoke_sibling_args, smoke_stop_sibling_args, AppRedeploy, HistoryEntry, RedeployEnv};
use super::redeploy_status::redeploy_status_sibling_args;
use super::{HELPER_SUBCOMMANDS, IMAGE_REPO};
use std::path::PathBuf;

const VM_DIR: &str = "/home/ubuntu/vm-ops-api";
/// 2026-10-21T00:00:00.000Z
const HISTORY_AT: i64 = 1_792_540_800_000;

fn snapshot(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn env() -> RedeployEnv {
    RedeployEnv { host_label: "main".into(), image_namespace: "ns".into(), provision_token: "tok".into(), vm_dir: VM_DIR.into(), ..RedeployEnv::default() }
}

fn last(args: Vec<String>) -> String {
    args.last().cloned().unwrap_or_default()
}

fn app_redeploy() -> Vec<String> {
    let request = AppRedeploy { app: "remote-chat-agent", host_dir: "/home/ubuntu/rca", compose_file: "docker-compose.prod.yml", tag: "t1", reference: "dev", wipe_state: false, recreate_all: false, request_env: &[] };
    app_redeploy_sibling_args(&env(), &request)
}

fn app_smoke() -> Vec<String> {
    let request = AppRedeploy { app: "leetcode-auth-cookie-capture", host_dir: "/home/ubuntu/leetcode-auth-cookie-capture", compose_file: "docker-compose.capture.yml", tag: "t1", reference: "dev", wipe_state: false, recreate_all: false, request_env: &[] };
    app_smoke_sibling_args(&env(), &request).expect("an app with a smoke test")
}

fn history(image: &str) -> Vec<String> {
    let entry = HistoryEntry { key: "service-leetcode-backend", requested_by: "wf ship", tag: "t1", reference: "dev", outcome: "redeploy.triggered", exit_code: None };
    append_history_args(VM_DIR, image, &entry, HISTORY_AT)
}

/// Each snapshot, with the script this crate renders for it.
fn renders() -> Vec<(&'static str, String)> {
    vec![
        ("app-redeploy.sh", last(app_redeploy())),
        ("app-smoke.sh", last(app_smoke())),
        ("smoke-stop.sh", last(smoke_stop_sibling_args(&env(), "remote-chat-agent"))),
        ("self-smoke.sh", last(self_smoke_sibling_args(&env(), "t1", "dev"))),
        ("self-redeploy.sh", last(self_redeploy_sibling_args(&env(), "t1", "dev", "unknown"))),
        ("agent-shell-job-wrapper.sh", AGENT_SHELL_JOB_WRAPPER.to_string()),
        ("agent-shell-job-status.sh", AGENT_SHELL_JOB_STATUS.to_string()),
        ("agent-shell-job-cancel.sh", AGENT_SHELL_JOB_CANCEL.to_string()),
        ("redeploy-history.sh", last(history("ns/vm-ops-api:t1"))),
        ("pair.sh", last(pair_sibling_args(&env()))),
        ("retire.sh", last(retire_sibling_args(&env(), "old-app"))),
        ("redeploy-status.sh", last(redeploy_status_sibling_args(VM_DIR, "ns/vm-ops-api:t1", "last-redeploy.log"))),
    ]
}

#[test]
fn given_each_snapshot_when_its_builder_renders_then_the_script_is_that_snapshot() {
    // Given the snapshots, When each builder renders with the snapshot's parameters
    let differing: Vec<&str> = renders().into_iter().filter(|(name, script)| *script != snapshot(name)).map(|(name, _)| name).collect();

    // Then every script is its snapshot, whole
    assert_eq!(differing, Vec::<&str>::new());
}

#[test]
fn given_the_sibling_scripts_when_their_commands_are_read_then_every_helper_is_a_subcommand_of_the_binary_in_the_image_they_run() {
    // Given every sibling script that calls the binary, and every argv that starts a sibling
    let scripts = [last(app_redeploy()), last(self_smoke_sibling_args(&env(), "sha-1234567", "dev")), last(self_redeploy_sibling_args(&env(), "sha-1234567", "dev", "unknown"))];
    let argvs = [app_redeploy(), self_smoke_sibling_args(&env(), "t1", "dev"), self_redeploy_sibling_args(&env(), "t1", "dev", "unknown"), history(&env().sibling_image()), retire_sibling_args(&env(), "old-app"), pair_sibling_args(&env())];

    // When each `vm-ops-api-rs <word>` of a script is read, a flag of the binary (the smoke test's `--healthcheck`) being no helper
    for script in &scripts {
        let called: Vec<&str> = script.split("vm-ops-api-rs ").skip(1).filter_map(|rest| rest.split_whitespace().next()).filter(|word| !word.starts_with("--")).collect();

        // Then each word is a subcommand the binary has, all three are called, and no node helper is left
        assert!(called.iter().all(|word| HELPER_SUBCOMMANDS.contains(word)), "{called:?}");
        assert_eq!(called, HELPER_SUBCOMMANDS, "provision, then append, then verify");
        assert!(!script.contains("/app/dist/"), "a node helper is left");
    }
    // And each argv names exactly one image, this binary's, at the tag the sibling is to run
    for argv in &argvs {
        let images: Vec<&String> = argv[..argv.len() - 1].iter().filter(|arg| arg.starts_with("ns/")).collect();
        assert_eq!(images.len(), 1, "{argv:?}");
        assert!(images[0].starts_with(&format!("ns/{IMAGE_REPO}:")), "{argv:?}");
    }
}

#[test]
fn given_a_running_commit_tag_when_the_swap_renders_then_its_restore_runs_at_that_tag() {
    // Given this service running a commit tag, and one running `unknown`
    let running = RedeployEnv { image_tag: Some("sha-7654321".into()), ..env() };
    let unknown = RedeployEnv { image_tag: Some("unknown".into()), ..env() };

    // When the swap renders
    let pinned = last(self_redeploy_sibling_args(&running, "t1", "dev", "unknown"));
    let bare = last(self_redeploy_sibling_args(&unknown, "t1", "dev", "unknown"));

    // Then the first restores at that tag, and the second restores unpinned
    assert!(pinned.contains("IMAGE_TAG=sha-7654321 docker compose up -d )"));
    assert!(bare.contains("unset IMAGE_TAG && docker compose up -d )"));
}
